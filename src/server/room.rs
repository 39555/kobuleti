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
    pub tx: UnboundedSender<ServerCmd>,
}   

pub enum ServerCmd {
    Ping (Answer<()>),
    AddPlayer           (SocketAddr, /*username*/ String,
                         PeerHandle, Answer<LoginStatus>),
    Broadcast           (SocketAddr, server::Msg ),
    GetPeerHandle (SocketAddr, Answer<PeerHandle>),
    DropPeer          (SocketAddr),
    AppendChat          (server::ChatLine),
    GetChatLog          (Answer<Vec<server::ChatLine>>),
    RequestNextContextAfter  (SocketAddr, /*current*/ GameContextId),
    SelectRole(SocketAddr, Role),
    Shutdown(Answer<()>)
}

use crate::server::details::fn_send;
use crate::server::details::fn_send_and_wait_responce;
use crate::server::details::send_oneshot_and_wait;

impl ServerHandle {
    pub fn for_tx(tx: UnboundedSender<ServerCmd>) -> Self{
        ServerHandle{tx}
    }
    pub async fn shutdown(&self) {
        send_oneshot_and_wait(&self.tx, 
            |to| ServerCmd::Shutdown(to)).await
    }
    pub fn drop_player(&self, whom: SocketAddr){
        let _ = self.tx.send(ServerCmd::DropPeer(whom));
    }
    pub async fn get_peer_handle(&self, whom: SocketAddr) -> PeerHandle { 
        send_oneshot_and_wait(&self.tx, 
            |to| ServerCmd::GetPeerHandle(whom, to)).await
    }
    fn_send!(
        ServerCmd => tx  =>
            pub broadcast(sender: SocketAddr, message: server::Msg);
            //pub drop_player(who: SocketAddr);
            pub append_chat(line: server::ChatLine);
            pub request_next_context_after(sender: SocketAddr, current: GameContextId);
            pub select_role(sender: SocketAddr, role: Role);
    );
    fn_send_and_wait_responce!(
         ServerCmd => tx =>
            pub get_chat_log() -> Vec<server::ChatLine>;
            pub add_player(addr: SocketAddr, username: String, peer: PeerHandle) -> LoginStatus;
        );
}                  


// server actor
#[derive(Default)]
pub struct Server {}
#[async_trait]
impl<'a> AsyncMessageReceiver<ServerCmd, &'a mut Room> for Server {
    async fn message(&mut self, msg: ServerCmd, room:  &'a mut Room) -> anyhow::Result<()>{
        match msg {
            ServerCmd::Shutdown(to) => {
                info!("Shutting down the server...");
                room.shutdown();
                let _ = to.send(());
            }
            ServerCmd::Ping(to) => {
                debug!("Pong from the server actor");
                let _ = to.send(());
            }, 
            ServerCmd::Broadcast(sender,  message) => { 
                room.broadcast(sender, message).await 
            },
            ServerCmd::AddPlayer(addr, username, peer_handle, tx) => {
                let _ = tx.send(room.add_player(addr, username, peer_handle).await); 
            },
            ServerCmd::GetPeerHandle(addr, to) => {
                let _ = to.send(room.get_peer(addr)?.peer.clone());
            }
            ServerCmd::DropPeer(addr)   => {
                if let Ok(_) = room.get_peer(addr) {
                    trace!("Drop a peer handle {}", addr);
                    room.drop_peer_handle(addr).await
                        .expect("Must drop if peer is logged"); 
                }
            },
            ServerCmd::AppendChat(line)   => { 
                room.chat.push(line); 
            },
            ServerCmd::GetChatLog(tx)     => { 
                let _ = tx.send(room.chat.clone());
            },
            ServerCmd::SelectRole(addr, role) => {
                // panic if invalid context, 
                // because it will be a server side error from a ctx handle
               room.get_peer(addr).expect("Peer must be exists").peer
                    .send(server::Msg::from(SelectRoleMsg::SelectedStatus(
                                    room.select_role_for_peer(addr, role).await
               )));
            }
            ServerCmd::RequestNextContextAfter(addr, current) => {
                info!("A next context was requested by peer {} 
                      (current: {:?})", addr, current);
                let p = &room.get_peer(addr)
                    .expect("failed to find the peer in the world storage").peer;
                use GameContextId as Id;
                let next = GameContextId::try_next_context(current)?;
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
                                   .filter(|peer| addr != peer.addr ) {
                                       all_have_same_ctx &= other.peer.get_context_id().await == current ;
                                       if !all_have_same_ctx { break };
                                       
                                }
                                all_have_same_ctx
                           } {
                            info!("A game ready to start: next context SelectRole");
                            for p in room.peer_iter(){
                                p.peer.next_context(ServerNextContextData::SelectRole(())); 
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
                                p.peer.next_context(ServerNextContextData::Game(
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerStatus {
    Connected,
    WaitReconnection,
}

pub struct PeerSlot {
    addr: SocketAddr,
    status: PeerStatus,
    peer: PeerHandle,
}
impl PeerSlot{
    pub fn new(addr: SocketAddr, peer: PeerHandle) -> Self {
        PeerSlot{addr, status: PeerStatus::Connected, peer}
    }
}

// server state for 2 players
#[derive(Default)]
pub struct Room {
    session: Option<GameSessionHandle> ,
    peers: [Option<PeerSlot>; 2] ,
    chat: Vec<server::ChatLine>  ,
}


impl Room {

    fn peer_iter(&self) -> impl Iterator<Item=&PeerSlot>{
        self.peers.iter().filter(|p| p.is_some()).map(move |p| p.as_ref().unwrap())
    } 
    fn peer_iter_mut(&mut self) -> impl Iterator<Item=&mut PeerSlot>{
        self.peers.iter_mut().filter(|p| p.is_some()).map(move |p| p.as_mut().unwrap())
    }
    fn get_peer(&self, addr: SocketAddr) 
        -> anyhow::Result<&PeerSlot> {
        self.peer_iter().find(|p| p.addr == addr)
            .ok_or(anyhow!("peer with addr {} not found", addr)
        )
    }
    fn get_peer_slot_mut(&mut self, addr: SocketAddr) 
        -> anyhow::Result<<&mut [Option<PeerSlot>] as IntoIterator>::Item> {
        self.peers.iter_mut().filter(|p| p.is_some()).find(|p| p.as_ref().unwrap().addr == addr)
            .ok_or(anyhow!("peer with addr {} not found", addr)
        )
    }
    async fn broadcast(&self, sender: SocketAddr, message: server::Msg){
        trace!("broadcast message {:?} to other clients", message);
        for peer in self.peers.iter().filter(|p| p.is_some()) {
            let p = peer.as_ref().unwrap();
            if p.addr != sender {
                // ignore message from other contexts
                if matches!(&message, server::Msg::App(_)) 
                    || ( p.peer.get_context_id().await == GameContextId::from(&message) ) {
                    info!("send");
                    let _ = p.peer.send(message.clone());
                }
            }
        }
    }
    async fn select_role_for_peer(&self, sender: SocketAddr, role: Role) -> SelectRoleStatus {
          for p in self.peer_iter() {
                let ctx_handle = Into::<ServerGameContextHandle>::into(p.peer.get_context_id().await);
                let select_role = <&SelectRoleHandle>::try_from(&ctx_handle)
                    .expect("Unexpected context"); 
                let p_role = select_role.get_role(&p.peer.tx).await;
                if p_role.is_some() && p_role.unwrap() == role {
                    return  if p.addr != sender { SelectRoleStatus::Busy } 
                            else { SelectRoleStatus::AlreadySelected }
                }
        }
        let p = &self.get_peer(sender).expect("must be exists").peer ;
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
        info!("Try login a player {} as {}", sender, username);
        for p in self.peer_iter_mut() {
            if p.peer.get_username().await == username {
                if p.status == PeerStatus::WaitReconnection {
                    // reconnection here
                    p.addr = sender;
                    p.status = PeerStatus::Connected;
                    return LoginStatus::Reconnected;

                } else {
                    return LoginStatus::AlreadyLogged;
                }
            }
        }
        // new player
        if let Some(it) = self.peers.iter_mut().position(|x| x.is_none() ){
            <&IntroHandle>::try_from(&Into::<ServerGameContextHandle>::into(player.get_context_id().await))
                .expect("Must be Intro").set_username(&player.tx, username);
            self.peers[it] = Some(PeerSlot::new(sender, player));
            LoginStatus::Logged
        }  else {
            LoginStatus::PlayerLimit
        }

    }
    async fn drop_peer_handle(&mut self, whom: SocketAddr) -> anyhow::Result<()> {
        let p = self.get_peer_slot_mut(whom)?;
        use GameContextId as Id;
        match p.as_ref().unwrap().peer.get_context_id().await {
            Id::Intro(_) | Id::Home(_) => {
                *p = None;
            },
            _ => {
                // keep connected in the game
                // for reconnection in 'Room::add_player'
                p.as_mut().unwrap().status = PeerStatus::WaitReconnection;
            },
        }
        Ok(())
        
    }
    fn shutdown(&mut self) { 
        trace!("Drop all peers");
        // if it is a last handle, peer actor will shutdown
        for p in self.peers.iter_mut(){
            *p = None;
        }
    }

    // TODO if different ctxts
    async fn are_all_have_roles(&self) -> bool {
        for p in self.peer_iter() {
             let ctx = Into::<ServerGameContextHandle>::into(p.peer.get_context_id().await);
             let ctx = <&SelectRoleHandle>::try_from(&ctx);
             if ctx.is_err() || ctx.unwrap().get_role(&p.peer.tx).await.is_none() { 
                return false;
            }
         }
        true
    }

} 
                                                                    
