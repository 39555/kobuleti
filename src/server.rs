use std::io::ErrorKind;
use std::str;
use futures::executor;
use std::collections::HashMap;
use anyhow::anyhow;
use anyhow::{Context};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc};
use std::net::SocketAddr;
use tracing::{trace, debug, info, warn, error};
use std::sync::{Arc, Mutex};
use tokio_stream::StreamExt;
use futures::{future, Sink, SinkExt};
use std::future::Future;
use tokio_util::codec::{LinesCodec, Framed, FramedRead, FramedWrite};
use crate::protocol::{AsyncMessageReceiver, GameContextId, MessageReceiver, MessageDecoder, encode_message};
use crate::protocol::{server, client, ToContext, TryNextContext};
use crate::protocol::server::{ServerGameContext, Intro, Home, SelectRole, Game};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
/// Shorthand for the transmit half of the message channel.
type Tx = mpsc::UnboundedSender<String>;
/// Shorthand for the receive half of the message channel.
type Rx = mpsc::UnboundedReceiver<String>;
type Answer<T> = oneshot::Sender<T>;
use async_trait::async_trait;
use crate::protocol::{DataForNextContext};
use crate::game::{AbilityDeck, HealthDeck, Deckable, Deck, MonsterDeck, Card, Rank, Suit, Role};
use crate::protocol::server::{ServerNextContextData, ServerStartGameData, LoginStatus};
use crate::protocol::client::{ClientNextContextData, ClientStartGameData};
use crate::protocol::MessageError;

pub mod peer;
pub mod details;
pub mod session;

use crate::protocol::server::{Msg, ChatLine, IntroEvent, HomeEvent, SelectRoleEvent, GameEvent};
use peer::{Peer, PeerHandle, Connection};
use session::{GameSessionHandle, GameSessionState, ToSession, GameSession};
use tokio::sync::oneshot;



pub async fn listen(addr: SocketAddr, shutdown: impl Future) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr)
        .await
        .context(format!("Failed to bind a socket to {}", addr))?;
    info!("Listening on: {}", addr);
    trace!("Spawn a server actor");
    let (to_server, mut server_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut state  = Room::default();
        let mut server = Server::default();
        trace!("Start incoming messages processing for server actor");
        loop {
            if let Some(command) = server_rx.recv().await {
                if let Err(e) = server.message(command, &mut state).await {
                    error!("failed to process an \
                           internal command on the server: {}", e);
                }
            };
        }
    });
    let server_handle =  ServerHandle::for_tx(to_server);

    trace!("Listen for new connections..");
    tokio::select!{
        _ = async {  
            loop {
                match listener.accept().await {
                    Err(e) => { 
                        error!("failed to accept a new connection {}", e); 
                        continue;
                    },
                    Ok((stream, addr)) => {
                        info!("{} has connected", addr);
                        let server_handle_for_peer = server_handle.clone();
                        trace!("start a task for process connection");
                        tokio::spawn(async move {
                            if let Err(e) = process_connection(stream, 
                                                               server_handle_for_peer)
                                .await {
                                    error!("an error occurred; error = {:?}", e);
                            }
                            info!("{} has disconnected", addr);
                        });
                     }
                }
         } 
        } => Ok(()),
        _ = shutdown => {
            info!("The shutdown signal has been received. Server is shutting down");
            Ok(())
        }
    }
}

async fn process_connection(mut socket: TcpStream, 
                            server: ServerHandle ) -> anyhow::Result<()> {
    let addr    = socket.peer_addr()?;
    let (r, w)  = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    trace!("Spawn a Peer actor for {}", addr);
    let (to_peer, mut peer_rx) = mpsc::unbounded_channel();
    let connection = Connection::new(addr, tx, server);
    let mut peer = Peer::new(
        ServerGameContext::from(
            Intro::new(
                PeerHandle::for_tx(to_peer))));
    let mut socket_writer = FramedWrite::new(w, LinesCodec::new());
    let mut socket_reader = MessageDecoder::new(
                                FramedRead::new(r, LinesCodec::new()));
    loop {
        tokio::select! { 
            Some(msg) = rx.recv() => {
                trace!("Message was receiver from a peer. 
                       Send to associated client {}", addr);
                socket_writer.send(&msg).await
                    .context("Failed to send a message to the socket")?;
            }

            Some(command) = peer_rx.recv() => {
                trace!("Peer {} received a server internal command.", addr);
                if let Err(e) = peer.message(command, &connection).await{
                    error!("Failed to process internal commands by Peer: {}", e);
                    break;
                }
            }  

            msg = socket_reader.next::<client::Msg>() => match msg {
                Ok(msg) => { 
                    trace!("Process a new message from {}", addr);
                    if let Err(e) = peer.message(msg, &connection).await {
                        error!("Failed to process client messages by Peer {}", e);
                        break;
                    }
                },
                Err(e) => { 
                    if e.kind() == ErrorKind::ConnectionAborted {
                        info!("Connection {} aborted..", addr);
                    } else {
                        error!("Failed to receive a socket message from the client:
                          {}", e); 
                    }
                    break
                }
            }
        }
    };
    info!("Disconnect client {}", addr);
    Ok(())
}



//                           
// interface for Server actor
//

//
#[derive(Clone)]
pub struct ServerHandle {
    pub to_world: UnboundedSender<ToServer>,
}   

pub enum ToServer {
    AddPlayer           (SocketAddr, /*username*/ String,
                         PeerHandle, Answer<LoginStatus>),
    Broadcast           (SocketAddr, server::Msg ),
    IsServerFull        (Answer<bool>),
    IsUserExists        (String, Answer<bool>),
    DropPlayer          (SocketAddr),
    AppendChat          (server::ChatLine),
    GetChatLog          (Answer<Vec<server::ChatLine>>),
    RequestNextContext  (SocketAddr, GameContextId),
}

use crate::server::details::fn_send;
use crate::server::details::fn_send_and_wait_responce;


impl ServerHandle {
    fn for_tx(tx: UnboundedSender<ToServer>) -> Self{
        ServerHandle{to_world: tx}
    }
    fn_send!(
        ToServer => to_world  =>
            broadcast(sender: SocketAddr, message: server::Msg);
            drop_player(who: SocketAddr);
            append_chat(line: server::ChatLine);
            request_next_context(sender: SocketAddr, current: GameContextId);
    );
    fn_send_and_wait_responce!(
         ToServer => to_world =>
            //is_server_full() -> bool ;
            //is_user_exists(username: String ) -> bool ;
            get_chat_log() -> Vec<server::ChatLine>;
            add_player(addr: SocketAddr, username: String, peer: PeerHandle) -> LoginStatus;
        );
}                  


// server actor
#[derive(Default)]
pub struct Server {}
#[async_trait]
impl<'a> AsyncMessageReceiver<ToServer, &'a mut Room> for Server {
    async fn message(&mut self, msg: ToServer, room:  &'a mut Room)-> Result<(), MessageError>{
        match msg {
            ToServer::Broadcast(sender,  message) => { 
                room.broadcast(sender, message).await 
            },
            ToServer::IsServerFull(tx)   => { 
                let _ = tx.send(room.is_full());
            } ,
            ToServer::AddPlayer(addr, username, peer_handle, tx) => { 
                let _ = tx.send(room.add_player(addr, &username, peer_handle)); 
            },
            ToServer::DropPlayer(addr)   => { 
                room.drop_player(addr);  
            },
            ToServer::IsUserExists(username, tx) => {
                let _ = tx.send(room.is_user_exists(&username).await).unwrap();
            }
            ToServer::AppendChat(line)   => { 
                room.chat.push(line); 
            },
            ToServer::GetChatLog(tx)     => { 
                let _ = tx.send(room.chat.clone());
            },
            ToServer::RequestNextContext(addr, current) => {
                info!("A next context was requested by peer {} 
                      (current: {:?})", addr, current);
                let p = room.get_peer(addr)
                    .await
                    .expect("failed to find the peer in the world storage");
                use GameContextId as Id;
                let next = GameContextId::try_next_context(current)
                        .map_err(|e| MessageError::ContextError(e.to_string()))?;
                match next {
                    Id::Intro(_) => { 
                        p.next_context(ServerNextContextData::Intro(()));
                    },
                    Id::Home(_) => {
                        p.next_context(ServerNextContextData::Home(()));
                        info!("Player {} was connected to the game", addr);
                        // only other players in the home context 
                        // need this chat message.
                        room.broadcast(addr, Msg::from(
                            HomeEvent::Chat(ChatLine::Connection(
                                p.get_username().await)))).await;
                    },
                    Id::SelectRole(_) => {
                        if room.is_full() 
                           && { 
                               // check for the same context 
                               let mut all_have_same_ctx = true;
                               for other in room.peer_iter()
                                   .filter(|peer| addr != peer.0 ) {
                                       all_have_same_ctx &=  other.1.get_context_id().await != current ;
                                       if !all_have_same_ctx { break };
                                       
                                }
                                all_have_same_ctx
                           } {
                            info!("a game ready for start: next context SelectRole");
                            for p in room.peer_iter(){
                                p.1.next_context(ServerNextContextData::SelectRole(())); 
                            };
                        } 
                    } ,
                    Id::Game(_) => {
                        if room.are_all_have_roles().await {
                            if room.session.is_none(){
                                info!("Start a new game session");
                                let (to_session, mut session_rx) = mpsc::unbounded_channel::<ToSession>();
                                tokio::spawn(async move {
                                    let mut state = GameSessionState::new();
                                    let mut session = GameSession{};
                                    loop {
                                        if let Some(cmd) = session_rx.recv().await {
                                            if let Err(e) = session.message(cmd, &mut state).await {
                                                error!("failed to process internal commands 
                                                       by the game session: {}", e);
                                            }
                                        }
                                    };
                                 });                       
                                room.session = Some(GameSessionHandle::for_tx(to_session));
                            }

                            for p in room.peer_iter(){
                                p.1.next_context(ServerNextContextData::Game(
                                        ServerStartGameData{
                                            session:  room.session.as_ref().unwrap()
                                                .clone(), 
                                            monsters: room.session.as_ref().unwrap()
                                                .get_monsters().await
                                        }
                                ));
                            };
                        }       

                    }
                };
            }
        }
        Ok(())
    }
}




// server state for 2 players
#[derive(Default)]
pub struct Room {
    session: Option<GameSessionHandle> ,
    peers: [Option<(SocketAddr, PeerHandle)>; 2] ,
    chat: Vec<server::ChatLine>  ,
}


impl Room {

    

    fn peer_iter(&self) -> impl Iterator<Item=&(SocketAddr, PeerHandle)>{
        self.peers.iter().filter(|p| p.is_some()).map(move |p| p.as_ref().unwrap())
    }

    async fn get_peer(&self, addr: SocketAddr) -> anyhow::Result<&PeerHandle> {
        for p in self.peer_iter(){
            if p.0 == addr {
               return  Ok(&p.1);
            }
        }
        Err(anyhow!("peer not found with addr {}", addr))
    }
    async fn broadcast(&self, sender: SocketAddr, message: server::Msg){
        trace!("broadcast message {:?} to other clients", message);
        for peer in self.peers.iter().filter(|p| p.is_some()) {
            let p = peer.as_ref().unwrap();
            if p.0 != sender {
                // ignore message from other contexts
                if matches!(&message, server::Msg::App(_)) 
                    || ( p.1.get_context_id().await == GameContextId::from(&message) ) {
                    info!("send");
                    let _ = p.1.send(message.clone());
                }
            }
        }
    }
    
    async fn is_user_exists(&self, username: &String) -> bool {
        for p in self.peer_iter() {
            if p.1.get_username().await[..] == username[..] { return true; }
        }
        false
    }
  
    fn is_full(&self) -> bool {
        self.peers.iter().position(|p| p.is_none()).is_none()
    }
    fn add_player(&mut self, addr: SocketAddr, player_username: &String , player: PeerHandle) -> LoginStatus {
        
        //self.peers.iter_mut().find(|p| p.0)
        player.get_username();
        let it = self.peers.iter_mut().position(|x| x.is_none() )
        .expect("failed to find an empty game slot for a player");
        self.peers[it] = Some((addr, player));
        LoginStatus::Logged
    }
    fn drop_player(&mut self, who: SocketAddr) {
        for p in self.peers.iter_mut().filter(|p| p.is_some()) {
            info!("try {}", p.as_ref().unwrap().0);
           if p.as_ref().unwrap().0 == who {  
              *p = None;
              return;
           }
        }
        panic!("failed to find a player for disconnect from SharedState");
    }
    async fn are_all_have_roles(&self) -> bool {
        for p in self.peer_iter() {
                 if p.1.get_role().await.is_none(){ 
                    return false;
                }
         }
        true
    }

} 








#[cfg(test)]
mod tests {
    use super::*;

   
}

