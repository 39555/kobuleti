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
                MessageError,
                encode_message,
                ToContext,
                Tx,



            },
             server::{Answer, ServerHandle},
             game::Role

};


pub struct Peer {
    pub context: ServerGameContext,
}
impl Peer {
    pub fn new(start_context: ServerGameContext) -> Peer {
        Peer{ context: start_context }
    }
}


    
pub struct Connection {
    pub addr     : SocketAddr,
    pub to_socket: Tx,
    pub server: ServerHandle,
}

impl Connection {
    pub fn new(addr: SocketAddr, socket_tx: Tx, world_handle: ServerHandle) -> Self {
        Connection{
         addr, to_socket: socket_tx, server: world_handle}
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        //self.server.drop_player(self.addr);
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

#[derive(Debug)]
pub enum PeerCmd {
    Ping (Answer<()>),
    Send               (server::Msg),
    GetAddr            (Answer<SocketAddr>),
    GetContextId       (Answer<GameContextId>),
    GetUsername        (Answer<String>),
    Close              (String),
    NextContext        (ServerNextContextData), 
    ContextCmd         (ContextCmd),
}
impl From<ContextCmd> for PeerCmd {
    fn from(cmd: ContextCmd) -> Self{
        PeerCmd::ContextCmd(cmd)
    }
}

use crate::server::details::oneshot_send_and_wait;
#[derive(Debug, Clone)]
pub struct PeerHandle{
    pub tx: UnboundedSender<PeerCmd>,
}
impl PeerHandle {
    pub fn new(tx: UnboundedSender<PeerCmd>, context: GameContextId) -> Self {
        PeerHandle{tx}
    }
    pub async fn ping(&self) -> (){
        oneshot_send_and_wait(&self.tx, |to| PeerCmd::Ping(to)).await
    }
    pub fn next_context(&self, for_server: ServerNextContextData) {
        let _ = self.tx.send(PeerCmd::NextContext(for_server));
    }
    pub fn send(&self, msg: server::Msg){
        let _ = self.tx.send(PeerCmd::Send(msg));
    }
    pub async fn get_username(&self) -> String {
        oneshot_send_and_wait(&self.tx, |to| PeerCmd::GetUsername(to)).await
    } 
    pub async fn get_context_id(&self) -> GameContextId {
        oneshot_send_and_wait(&self.tx, |to| PeerCmd::GetContextId(to)).await
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
        oneshot_send_and_wait(to_peer, 
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




// TODO internal commands by contexts?
#[async_trait]
impl<'a> AsyncMessageReceiver<PeerCmd, &'a Connection> for Peer {
    async fn message(&mut self, msg: PeerCmd, state:  &'a Connection) -> Result<(), MessageError>{
        match msg {
            PeerCmd::Ping(to) => {
                let _ = to.send(());
            }
            PeerCmd::Close(reason)  => {
                // TODO thiserror errorkind 
                // //self.world_handle.broadcast(self.addr, server::Msg::(ChatLine::Disconnection(
        //                    self.username)));
            }
            PeerCmd::Send(msg) => {
                let _ = state.to_socket.send(encode_message(msg));
            },
            PeerCmd::GetAddr(to) => {
                let _ = to.send(state.addr);
            },
            PeerCmd::GetContextId(to) => {
                let _ = to.send(GameContextId::from(&self.context));
            },
            PeerCmd::GetUsername(to) => { 
                use ServerGameContext as C;
                let n = match &self.context {
                    C::Intro(i)      => i.username.as_ref()
                        .expect("if peer is logged, and a handle of this peer \
allows for other actors e.g. if a server room has a peer, this peer must has a username"),
                    C::Home(h)       => &h.username,
                    C::SelectRole(r) => &r.username, 
                    C::Game(g)       => &g.username,
                };
                let _ = to.send(n.clone());
            },
            PeerCmd::NextContext(data_for_next_context) => {
                 let next_ctx_id = GameContextId::from(&data_for_next_context);
                 self.context.to(data_for_next_context, state)
                     .or_else(
                     |e| 
                     Err(MessageError::NextContextRequestError{
                        next: next_ctx_id,
                        current: GameContextId::from(&self.context),
                        reason: e.to_string()
                     })
                     )?;

            },
            PeerCmd::ContextCmd(msg) => {
                self.context.message(msg, state).await.unwrap();
            }
            _ => (),
            
        }
       Ok(())
    }
}

#[async_trait]
impl<'a> AsyncMessageReceiver<IntroCmd, &'a Connection> for Intro {
    async fn message(&mut self, msg: IntroCmd, state:  &'a Connection) -> Result<(), MessageError>{
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
impl<'a> AsyncMessageReceiver<HomeCmd, &'a Connection> for Home {
    async fn message(&mut self, msg: HomeCmd, state:  &'a Connection) -> Result<(), MessageError>{
        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<SelectRoleCmd, &'a Connection> for SelectRole {
    async fn message(&mut self, msg: SelectRoleCmd, state:  &'a Connection) -> Result<(), MessageError>{
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
impl<'a> AsyncMessageReceiver<GameCmd, &'a Connection> for Game {
    async fn message(&mut self, msg: GameCmd, state:  &'a Connection) -> Result<(), MessageError>{
        Ok(())
    }
}






#[async_trait]
impl<'a> AsyncMessageReceiver<client::Msg, &'a Connection> for PeerHandle {
    async fn message(&mut self, msg: client::Msg, state: &'a Connection)-> Result<(), MessageError>{
         match msg {
            client::Msg::App(e) => {
                match e {
                    client::AppMsg::Ping => {
                        trace!("Ping the client-peer connection {}", state.addr);
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        // TODO cast SendError to MessageError
                        self.tx.send(PeerCmd::Ping(tx))
                            .map_err(|e| MessageError::BrokenPipe(
                                    format!("Peer Actor not responding: {e}")))?;

                        match rx.await {
                            Ok(()) => {
                                info!("Pong to {}", state.addr);
                                let _ = state.to_socket.send(encode_message(
                                        server::Msg::from(server::AppMsg::Pong)));
                            }
                            Err(e) => {
                                return Err(MessageError::BrokenPipe(
                                        format!("Ping command not received from the Peer actor: {}", e)))
                            }
                        }
                    }
                    client::AppMsg::Logout =>  {
                        let _ = state.to_socket.
                            send(encode_message(server::Msg::from(server::AppMsg::Logout)));
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


#[async_trait]
impl<'a> AsyncMessageReceiver<client::IntroMsg, (&'a PeerHandle ,&'a Connection)> for IntroHandle {
    async fn message(&mut self, msg: client::IntroMsg,
                     (peer_handle, state):  (&'a PeerHandle ,&'a Connection)) -> Result<(), MessageError>{
        use client::IntroMsg;
        match msg {
            IntroMsg::AddPlayer(username) =>  {
                info!("{} is trying to connect to the game as {}",
                      state.addr , &username); 
                let status = state.server.add_player(state.addr, 
                                        username, 
                                        peer_handle.clone()).await;
                trace!("Connection status: {:?}", status);
                let _ = state.to_socket.send(encode_message(Msg::from(
                    server::IntroMsg::LoginStatus(status))));
                if status != LoginStatus::Logged {
                    return Err(MessageError::LoginRejected{
                        reason: format!("{:?}", status)
                    });
                }
            },
            IntroMsg::GetChatLog => {
                // peer_handle.get_login_status().await;
                // TODO check logging?
                //if self.username.is_none() {
                //    warn!("Client not logged but the ChatLog was requested");
                //    return Err(MessageError::NotLogged);
                //}
                info!("Send a chat history to the client");
                let _ = state.to_socket.send(encode_message(server::Msg::Intro(
                    server::IntroMsg::ChatLog(state.server.get_chat_log().await))));
            }
        }

        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<client::HomeMsg, (&'a PeerHandle ,&'a Connection)> for HomeHandle {
    async fn message(&mut self, msg: client::HomeMsg, 
                     (peer_handle, state):  (&'a PeerHandle ,&'a Connection))-> Result<(), MessageError>{
        use client::HomeMsg;
        match msg {
            HomeMsg::Chat(msg) => {
                let msg = server::ChatLine::Text(
                    format!("{}: {}", peer_handle.get_username().await , msg));
                state.server.append_chat(msg.clone());
                state.server.broadcast(state.addr, Msg::Home(server::HomeMsg::Chat(msg)));
            },
            _ => (),
        }

        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<client::SelectRoleMsg, (&'a PeerHandle ,&'a Connection)> for SelectRoleHandle {
    async fn message(&mut self, msg: client::SelectRoleMsg, 
                     (peer_handle, state):   (&'a PeerHandle ,&'a Connection))-> Result<(), MessageError>{
        use client::SelectRoleMsg;
        match msg {
            SelectRoleMsg::Chat(msg) => {
                let msg = server::ChatLine::Text(
                    format!("{}: {}", peer_handle.get_username().await, msg));
                state.server.append_chat(msg.clone());
                state.server.broadcast(state.addr, server::Msg::SelectRole(server::SelectRoleMsg::Chat(msg)));
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
impl<'a> AsyncMessageReceiver<client::GameMsg, (&'a PeerHandle ,&'a Connection)> for GameHandle {
    async fn message(&mut self, msg: client::GameMsg, 
                     (peer_handle, state):  (&'a PeerHandle ,&'a Connection))-> Result<(), MessageError>{

        use client::GameMsg;
        match msg {
            GameMsg::Chat(msg) => {
                let msg = server::ChatLine::Text(
                    format!("{}: {}", peer_handle.get_username().await , msg));
                state.server.append_chat(msg.clone());
                state.server.broadcast(state.addr, server::Msg::Game(server::GameMsg::Chat(msg)));
            },
        }

        Ok(())
    }
}

