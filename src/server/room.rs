use anyhow::anyhow;
use anyhow::Context as _;
use crate::protocol::{AsyncMessageReceiver, GameContextId, MessageReceiver, MessageDecoder, encode_message};
use crate::protocol::{server, client, ToContext, TryNextContext};
use crate::protocol::server::{ServerGameContext, Intro, Home, SelectRole, Game};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
/// Shorthand for the transmit half of the message channel.
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
/// Shorthand for the receive half of the message channel.
type Rx = tokio::sync::mpsc::UnboundedReceiver<String>;
type Answer<T> = oneshot::Sender<T>;
use async_trait::async_trait;
use crate::protocol::{DataForNextContext};
use crate::game::{AbilityDeck, HealthDeck, Deckable, Deck, MonsterDeck, Card, Rank, Suit, Role};
use crate::protocol::server::{ServerNextContextData, ServerStartGameData, LoginStatus, SelectRoleStatus};
use crate::protocol::client::{ClientNextContextData, ClientStartGameData};
use crate::protocol::MessageError;

use crate::protocol::server::{Msg, ChatLine, IntroMsg, HomeMsg, SelectRoleMsg, GameMsg};
use crate::server::peer::{Peer, PeerHandle, Connection, ServerGameContextHandle, ContextCmd,
                IntroCmd, HomeCmd, SelectRoleCmd, GameCmd,
                IntroHandle, SelectRoleHandle, HomeHandle, GameHandle};
use crate::server::session::{GameSessionHandle, GameSessionState, SessionCmd, GameSession};
use tokio::sync::oneshot;
use std::net::SocketAddr;                           
use tracing::{info, debug, warn, trace, error};
use tokio::sync::mpsc;
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
    DropPlayer          (SocketAddr),
    AppendChat          (server::ChatLine),
    GetChatLog          (Answer<Vec<server::ChatLine>>),
    RequestNextContextAfter  (SocketAddr, /*current*/ GameContextId),
    SelectRole(SocketAddr, Role),
}

use crate::server::details::fn_send;
use crate::server::details::fn_send_and_wait_responce;


impl ServerHandle {
    pub fn for_tx(tx: UnboundedSender<ToServer>) -> Self{
        ServerHandle{to_world: tx}
    }
    fn_send!(
        ToServer => to_world  =>
            pub broadcast(sender: SocketAddr, message: server::Msg);
            pub drop_player(who: SocketAddr);
            pub append_chat(line: server::ChatLine);
            pub request_next_context_after(sender: SocketAddr, current: GameContextId);
            pub select_role(sender: SocketAddr, role: Role);
    );
    fn_send_and_wait_responce!(
         ToServer => to_world =>
            pub get_chat_log() -> Vec<server::ChatLine>;
            pub add_player(addr: SocketAddr, username: String, peer: PeerHandle) -> LoginStatus;
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
            ToServer::AddPlayer(addr, username, peer_handle, tx) => { 
                let _ = tx.send(room.add_player(addr, username, peer_handle).await); 
            },
            ToServer::DropPlayer(addr)   => { 
                room.drop_player(addr);  
            },
            ToServer::AppendChat(line)   => { 
                room.chat.push(line); 
            },
            ToServer::GetChatLog(tx)     => { 
                let _ = tx.send(room.chat.clone());
            },
            ToServer::SelectRole(addr, role) => {
                // panic if invalid context, 
                // because it will be a server side error from a ctx handle
               room.get_peer(addr).expect("Peer must be exists").1
                    .send(server::Msg::from(SelectRoleMsg::SelectedStatus(
                                    room.select_role_for_peer(addr, role).await
               )));
            }
            ToServer::RequestNextContextAfter(addr, current) => {
                info!("A next context was requested by peer {} 
                      (current: {:?})", addr, current);
                let p = &room.get_peer(addr)
                    .expect("failed to find the peer in the world storage").1;
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
                            HomeMsg::Chat(ChatLine::Connection(
                                p.get_username().await)))).await;
                    },
                    Id::SelectRole(_) => {
                        if room.is_full() 
                           && { 
                               // check for the same context 
                               let mut all_have_same_ctx = true;
                               for other in room.peer_iter()
                                   .filter(|peer| addr != peer.0 ) {
                                       all_have_same_ctx &= other.1.get_context_id().await == current ;
                                       if !all_have_same_ctx { break };
                                       
                                }
                                all_have_same_ctx
                           } {
                            info!("A game ready to start: next context SelectRole");
                            for p in room.peer_iter(){
                                p.1.next_context(ServerNextContextData::SelectRole(())); 
                            };
                        } 
                        else {
                            info!("Attempt to start a game. A game does not ready to start..")
                        }
                    } ,
                    Id::Game(_) => {
                        if room.are_all_have_roles().await {
                            if room.session.is_none(){
                                info!("Start a new game session");
                                let (to_session, mut session_rx) = mpsc::unbounded_channel::<SessionCmd>();
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

    fn get_peer(&self, addr: SocketAddr) -> anyhow::Result<&(SocketAddr, PeerHandle)> {
        for p in self.peer_iter(){
            if p.0 == addr {
               return  Ok(&p);
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
    async fn select_role_for_peer(&self, sender: SocketAddr, role: Role) -> SelectRoleStatus {
          for (p_addr, p_handle) in self.peer_iter() {
                let ctx_handle = Into::<ServerGameContextHandle>::into(p_handle.get_context_id().await);
                let select_role = <&SelectRoleHandle>::try_from(&ctx_handle)
                    .expect("Unexpected context"); 
                let p_role = select_role.get_role(&p_handle.tx).await;
                if p_role.is_some() && p_role.unwrap() == role {
                    return  if *p_addr != sender { SelectRoleStatus::Busy } 
                            else { SelectRoleStatus::AlreadySelected }
                }
        }
        let p = &self.get_peer(sender).expect("must be exists").1 ;
        <&SelectRoleHandle>::try_from(&Into::<ServerGameContextHandle>::into(p.get_context_id().await))
            .map_err(|e| anyhow!(e))
            .expect("Unexpected context") 
            .select_role(&p.tx, role);
        SelectRoleStatus::Ok(role)
    }
  
    fn is_full(&self) -> bool {
        self.peers.iter().position(|p| p.is_none()).is_none()
    }
    async fn add_player(&mut self, sender: SocketAddr, username: String, player: PeerHandle) -> LoginStatus {
        info!("Try login a player {}", sender);
        for p in self.peer_iter().filter(|p| p.0 != sender) {
            if p.1.get_username().await == username {
                return LoginStatus::AlreadyLogged;
            }
        }
        if let Some(it) = self.peers.iter_mut().position(|x| x.is_none() ){
            <&IntroHandle>::try_from(&Into::<ServerGameContextHandle>::into(player.get_context_id().await))
                .expect("Must be Intro").set_username(&player.tx, username);
            self.peers[it] = Some((sender, player));
            LoginStatus::Logged
        }  else {
            LoginStatus::PlayerLimit
        }

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
    // TODO if different ctxts
    async fn are_all_have_roles(&self) -> bool {
        for p in self.peer_iter() {
             let ctx = Into::<ServerGameContextHandle>::into(p.1.get_context_id().await);
             let ctx = <&SelectRoleHandle>::try_from(&ctx);
             if ctx.is_err() || ctx.unwrap().get_role(&p.1.tx).await.is_none() { 
                return false;
            }
         }
        true
    }

} 
                                                                    
