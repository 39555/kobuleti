use anyhow::Context as _;
use std::net::SocketAddr;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn, error, trace, debug};
use async_trait::async_trait;
use crate::{ protocol::{
                client,
                server::{self, 
                    Msg,
                    ServerGameContext,
                    ServerNextContextData,
                    LoginStatus,
                    Intro,
                    Home,
                    SelectRole,
                    Game,
                },
                GameContext,
                GameContextId,
                AsyncMessageReceiver,
                encode_message,
                ToContext,
                Tx,



            },
             server::{ServerCmd, Answer, ServerHandle},
             game::Role

};


pub struct Peer {
    pub context: ServerGameContext,
}
impl Peer {
    pub fn new(start_context: ServerGameContext) -> Peer {
        Peer{ context: start_context }
    }
    pub fn get_username(&self) -> String {
        use ServerGameContext as C;
        match &self.context {
            C::Intro(i)      => i.username.as_ref()
                .expect("if peer is logged, and a handle of this peer \
allows for other actors e.g. if a server room has a peer handle to this peer, \
this peer must has a username"),
            C::Home(h)       => &h.username,
            C::SelectRole(r) => &r.username, 
            C::Game(g)       => &g.username,
        }.clone()
    }
}


#[derive(Clone, Debug)]   
pub struct Connection {
    pub addr     : SocketAddr,
    // can be None for close a socket connection but 
    // wait until the connection sends all messages
    // and will close by EOF
    pub socket: Option<Tx>,
    pub server: ServerHandle,
}

impl Connection {
    pub fn new(addr: SocketAddr, socket_tx: Tx, world_handle: ServerHandle) -> Self {
        Connection{
         addr, socket: Some(socket_tx), server: world_handle}
    }
    pub fn close_socket(&mut self){
        self.socket = None;
    }
}


macro_rules! nested_contexts {
    (
        pub type $type:ident = GameContext <
        $(
            $( #[$meta:meta] )*
            $vis:vis enum $name:ident {
                $($tt:tt)*
            },
        )*
        >

    ) => {
        $(
            $( #[$meta] )*
            $vis enum $name {
                $($tt)*
            }
        )*

        pub type $type = GameContext <
            $($name,)*

        >;

    }
}
nested_contexts!{
pub type ContextCmd = GameContext <
        #[derive(Debug)]
        pub enum IntroCmd {
            SetUsername(String),

        },
        #[derive(Debug)]
        pub enum HomeCmd {

        },
        #[derive(Debug)]
        pub enum SelectRoleCmd {
            SelectRole(Role),
            GetRole(Answer<Option<Role>>),
        },
        #[derive(Debug)]
        pub enum GameCmd {

        },
    >
}
macro_rules! impl_from_inner_command {
($( $src: ident => $dst_pat: ident $(,)?)+ => $dst: ty) => {
    $(
    impl From<$src> for $dst {
        fn from(src: $src) -> Self {
            Self::$dst_pat(src)
        }
    }
    )*
    };
}
impl_from_inner_command! {
    IntroCmd       => Intro ,
    HomeCmd        => Home,
    SelectRoleCmd  => SelectRole, 
    GameCmd        => Game,
    => ContextCmd
}
use ascension_macro::DisplayOnlyIdents;
use std::fmt::Display;
#[derive(Debug, DisplayOnlyIdents)]
pub enum PeerCmd {
    Ping (Answer<()>),
    SendTcp               (server::Msg),
    GetAddr            (Answer<SocketAddr>),
    GetContextId       (Answer<GameContextId>),
    GetUsername        (Answer<String>),
    Close              ,
    NextContext        (ServerNextContextData), 
    ContextCmd         (ContextCmd),
    SyncReconnection (Connection, Answer<()>),
}
impl From<ContextCmd> for PeerCmd {
    fn from(cmd: ContextCmd) -> Self{
        PeerCmd::ContextCmd(cmd)
    }
}

use crate::server::details::send_oneshot_and_wait;
#[derive(Debug, Clone)]
pub struct PeerHandle{
    pub tx: UnboundedSender<PeerCmd>,
}
impl PeerHandle {
    pub fn for_tx(tx: UnboundedSender<PeerCmd>) -> Self {
        PeerHandle{tx}
    }
    pub fn next_context(&self, for_server: ServerNextContextData) {
        let _ = self.tx.send(PeerCmd::NextContext(for_server));
    }
    pub fn send_tcp(&self, msg: server::Msg){
        let _ = self.tx.send(PeerCmd::SendTcp(msg));
    }
    pub async fn get_username(&self) -> String {
        send_oneshot_and_wait(&self.tx, |to| PeerCmd::GetUsername(to)).await
    } 
    pub async fn get_context_id(&self) -> GameContextId {
        send_oneshot_and_wait(&self.tx, |to| PeerCmd::GetContextId(to)).await
    }
    
}




#[derive(Debug, Clone)]
pub struct IntroHandle;
impl IntroHandle{
    pub fn set_username(&self, to_peer: &UnboundedSender<PeerCmd>, username: String){
        let _ = to_peer.send(
            PeerCmd::from(ContextCmd::from(IntroCmd::SetUsername(username))));
    }
}
#[derive(Debug, Clone)]
pub struct HomeHandle;
#[derive(Debug, Clone)]
pub struct SelectRoleHandle;
impl SelectRoleHandle{
    pub fn select_role(&self, to_peer: &UnboundedSender<PeerCmd>, role: Role){
         let _ = to_peer.send(
            PeerCmd::from(ContextCmd::from(SelectRoleCmd::SelectRole(role))));
    }
    pub async fn get_role(&self, to_peer: &UnboundedSender<PeerCmd>,) -> Option<Role>{
        send_oneshot_and_wait(to_peer, 
            |to| PeerCmd::from(ContextCmd::from(SelectRoleCmd::GetRole(to)))).await
    }
}

#[derive(Debug, Clone)]
pub struct GameHandle;




use crate::details::impl_try_from_for_inner;
impl_try_from_for_inner!{
    pub type ServerGameContextHandle = GameContext<
         self::IntroHandle          => Intro, 
         self::HomeHandle           => Home, 
         self::SelectRoleHandle     => SelectRole, 
         self::GameHandle           => Game,
    >;
}
// GameContextIf -> ServerGameContextHandle
use crate::details::impl_from;
impl_from!{ impl From () GameContext<(), (), (), () >  for ServerGameContextHandle {
                       Intro(_)      => Intro(IntroHandle{})
                       Home(_)       => Home(HomeHandle{})
                       SelectRole(_) => SelectRole(SelectRoleHandle{})
                       Game(_)       => Game(GameHandle{})
        }
}




#[async_trait]
impl<'a> AsyncMessageReceiver<PeerCmd, &'a mut Connection> for Peer {
    async fn message(&mut self, msg: PeerCmd, state:  &'a mut Connection) -> anyhow::Result<()>{
        use crate::protocol::client::{ClientNextContextData, ClientStartGameData};
        use crate::game::Rank;
        use crate::protocol::encode_message;
        match msg {
            PeerCmd::Ping(to) => {
                let _ = to.send(());
            }
            PeerCmd::SyncReconnection(new_connection, tx) => {
                trace!("Sync reconnection for {}", state.addr);
                *state = new_connection;
                let socket = state.socket.as_ref().expect("A socket must be opened");
                match &self.context {
                    ServerGameContext::SelectRole(r) => {
                        socket
                            .send(
                                encode_message(Msg::App(
                            server::AppMsg::NextContext(ClientNextContextData::SelectRole(r.role)))))
                            // prevent dev error = new peer should be with the open connection
                                .expect("Must be opened");
                    }, 
                    ServerGameContext::Game(g) => {
                        let mut abilities :[Option<Rank>; 3] = Default::default();
                        g.ability_deck.ranks[..3].iter()
                          .map(|r| Some(r) ).zip(abilities.iter_mut()).for_each(|(r, a)| *a = r.copied() );
                         socket.send(
                                 encode_message(Msg::App(
                            server::AppMsg::NextContext(ClientNextContextData::Game(
                                ClientStartGameData{
                                    abilities, 
                                    monsters: g.to_session.get_monsters().await,
                                    role: g.role
                                }
                        )))))
                            .expect("Must be opened");
                    }
                    _ => unreachable!("Reconnection not allowed for the Intro or Home contexts"),
                
                };
                state.server.broadcast_to_all(Msg::from(
                            server::AppMsg::Chat(server::ChatLine::Reconnection(
                                self.get_username())))).await;
                let _ = tx.send(());
            }
            PeerCmd::Close  => {
                state.server.drop_peer(state.addr);
                debug!("Close the socket tx on the Peer actor side");
                state.close_socket();
            }
            PeerCmd::SendTcp(msg) => {
                let _ = state.socket.as_ref().map(|s| s.send(encode_message(msg)));
            },
            PeerCmd::GetAddr(to) => {
                let _ = to.send(state.addr);
            },
            PeerCmd::GetContextId(to) => {
                let _ = to.send(GameContextId::from(&self.context));
            },
            PeerCmd::GetUsername(to) => { 
                
                let _ = to.send(self.get_username());
            },
            PeerCmd::NextContext(data_for_next_context) => {
                 let next_ctx_id = GameContextId::from(&data_for_next_context);
                 // sends to socket inside
                 self.context.to(data_for_next_context, state).with_context(|| format!(
                    "Failed to request a next context ({:?} for {:?})",
                        next_ctx_id, GameContextId::from(&self.context), 
                     ))?;

            },
            PeerCmd::ContextCmd(msg) => {
                self.context.message(msg, state).await?;
            }
            
        }
       Ok(())
    }
}

#[async_trait]
impl<'a> AsyncMessageReceiver<IntroCmd, &'a mut Connection> for Intro {
    async fn message(&mut self, msg: IntroCmd, state:  &'a mut Connection) -> anyhow::Result<()>{
        match msg {
            IntroCmd::SetUsername(username) => {
                trace!("Set username {} for {}", username, state.addr);
                self.username = Some(username);
            }
        };
        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<HomeCmd, &'a mut Connection> for Home {
    async fn message(&mut self, msg: HomeCmd, state:  &'a mut Connection) -> anyhow::Result<()>{
        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<SelectRoleCmd, &'a mut Connection> for SelectRole {
    async fn message(&mut self, msg: SelectRoleCmd, state:  &'a mut Connection) -> anyhow::Result<()>{
        match msg {
            SelectRoleCmd::SelectRole(role) => {
                self.role = Some(role);
            }
            SelectRoleCmd::GetRole(tx) => {
                let _ = tx.send(self.role);
            }
        }
        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<GameCmd, &'a mut Connection> for Game {
    async fn message(&mut self, msg: GameCmd, state:  &'a mut Connection) -> anyhow::Result<()>{
        Ok(())
    }
}






#[async_trait]
impl<'a> AsyncMessageReceiver<client::Msg, &'a mut Connection> for PeerHandle {
    async fn message(&mut self, msg: client::Msg, state: &'a mut Connection) -> anyhow::Result<()>{
        debug!("New message from {}: {:?}", state.addr, msg);
        match msg {
            client::Msg::App(e) => {
                match e {
                    client::AppMsg::Ping => {
                        trace!("Ping the client-peer connection {}", state.addr);
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        // TODO cast SendError to MessageError
                        let _ = self.tx.send(PeerCmd::Ping(tx));
                        rx.await.context("Peer Actor not responding")?;
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        let _ = state.server.tx.send(ServerCmd::Ping(tx)); 
                        rx.await.context("Server Actor not responding")?;
                        info!("Pong to {}", state.addr);
                        let _ = state.socket.as_ref().map(|s| s.send(encode_message(
                                    server::Msg::from(server::AppMsg::Pong))));
                    }
                    client::AppMsg::Logout =>  {
                        let _ = state.socket.as_ref().map(|s| 
                            s.send(encode_message(server::Msg::from(server::AppMsg::Logout))));
                        info!("Logout");
                        // TODO 
                        //return Err(MessageError::Logout);
                    },
                    client::AppMsg::NextContext => {
                       state.server.request_next_context_after(state.addr    
                            , self.get_context_id().await);
                    },
                }
            },
            _ => {
                let ctx = self.get_context_id().await;
                 Into::<ServerGameContextHandle>::into(ctx)
                     .message(msg, (self, state)).await?;
            }
        }
        Ok(())
    }
}

async fn close_peer(state: &mut Connection, peer : &PeerHandle){
    // should close but wait the socket writer EOF,
    // so it just drops socket tx
    let _ = peer.tx.send(PeerCmd::Close);
    trace!("Close the socket tx on the PeerHandle side");
    state.socket = None;
}

#[async_trait]
impl<'a> AsyncMessageReceiver<client::IntroMsg, (&'a mut  PeerHandle ,&'a mut Connection)> for IntroHandle {
    async fn message(&mut self, msg: client::IntroMsg,
                     (peer_handle, state):  (&'a mut  PeerHandle ,&'a mut Connection)) -> anyhow::Result<()>{
        use client::IntroMsg;
        match msg {
            IntroMsg::AddPlayer(username) =>  {
                info!("{} is trying to login as {}",
                      state.addr , &username); 
                let status = state.server.add_player(state.addr, 
                                        username, 
                                        peer_handle.clone()).await;
                trace!("Connection status: {:?}", status);
                let _ = state.socket.as_ref().unwrap().send(encode_message(Msg::from(
                    server::IntroMsg::LoginStatus(status))));
                match status {
                    LoginStatus::Logged => (), 
                    LoginStatus::Reconnected => {
                        // this get handle to previous peer actor and drop the current handle,
                        // so new actor will shutdown
                        *peer_handle = state.server.get_peer_handle(state.addr).await;
                        send_oneshot_and_wait(&peer_handle.tx, 
                                              |oneshot| PeerCmd::SyncReconnection(state.clone(), oneshot)).await;
                        let _ = state.socket.as_ref().unwrap().send(encode_message(server::Msg::from(
                            server::AppMsg::ChatLog(state.server.get_chat_log().await))));
                    }
                    _ => { // connection fail
                        warn!("Login attempt rejected = {:?}", status);
                        close_peer(state, peer_handle).await;
                    }
                }
            },
            IntroMsg::GetChatLog => {
                if state.server.is_peer_connected(state.addr).await{
                    info!("Send a chat history to the client");
                    if let Some(s) = state.socket.as_ref() {
                        let _ = s.send(encode_message(server::Msg::from(
                        server::AppMsg::ChatLog(state.server.get_chat_log().await))));
                    }
                } else {
                    warn!("Client not logged but the ChatLog was requested");
                    close_peer(state, peer_handle).await;
                }
                
               
            }
        }

        Ok(())
    }
}

async fn broadcast_chat(state: &Connection, peer : &PeerHandle, msg: String ) {
    let msg = server::ChatLine::Text(
                    format!("{}: {}", peer.get_username().await , msg));
    state.server.append_chat(msg.clone());
    state.server.broadcast(state.addr, Msg::from(server::AppMsg::Chat(msg)));
}

#[async_trait]
impl<'a> AsyncMessageReceiver<client::HomeMsg, (&'a mut  PeerHandle ,&'a mut Connection)> for HomeHandle {
    async fn message(&mut self, msg: client::HomeMsg, 
                     (peer_handle, state):  (&'a  mut PeerHandle ,&'a mut Connection)) -> anyhow::Result<()>{
        use client::HomeMsg;
        match msg {
            HomeMsg::Chat(msg) => {
               broadcast_chat(state, peer_handle, msg).await
            },
            _ => (),
        }

        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<client::SelectRoleMsg, (&'a mut  PeerHandle ,&'a mut Connection)> for SelectRoleHandle {
    async fn message(&mut self, msg: client::SelectRoleMsg, 
                     (peer_handle, state):   (&'a mut  PeerHandle ,&'a mut Connection)) -> anyhow::Result<()> {
        use client::SelectRoleMsg;
        match msg {
            SelectRoleMsg::Chat(msg) => {
               broadcast_chat(state, peer_handle, msg).await
            },
            SelectRoleMsg::Select(role) => {
                info!("select role request {:?}", role);
                state.server.select_role(state.addr, role);
            }
        }
        Ok(())
    }
}

#[async_trait]
impl<'a> AsyncMessageReceiver<client::GameMsg, (&'a mut  PeerHandle ,&'a mut Connection)> for GameHandle {
    async fn message(&mut self, msg: client::GameMsg, 
                     (peer_handle, state):  (&'a mut  PeerHandle ,&'a mut Connection)) -> anyhow::Result<()>{

        use client::GameMsg;
        match msg {
            GameMsg::Chat(msg) => {
               broadcast_chat(state, peer_handle, msg).await
            },
        }

        Ok(())
    }
}

