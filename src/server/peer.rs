use anyhow::anyhow;
use anyhow::Context as _;
use std::net::SocketAddr;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn, trace, debug};
use async_trait::async_trait;
use crate::{ protocol::{
                client::{
                    self, IntroMsg, HomeMsg, SelectRoleMsg, GameMsg
                },
                server::{self, 
                    Msg,
                    ServerGameContext,
                    ServerNextContextData,
                    LoginStatus,
                    Intro,
                    Home,
                    SelectRole,
                    Game,
                    TurnResult,
                    PlayerId
                },
                GameContext,
                GameContextKind,
                AsyncMessageReceiver,
                ToContext,



            },
             server::{ServerCmd, Answer, ServerHandle},
             game::{Role, Rank, Card}

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
            C::Intro(i)      => i.name.as_ref()
                .expect("if peer is logged, and a handle of this peer \
allows for other actors e.g. if a server room has a peer handle to this peer, \
this peer must has a username"),
            C::Home(h)       => &h.name,
            C::SelectRole(r) => &r.name, 
            C::Game(g)       => &g.name,
        }.clone()
    }
}


#[derive(Clone, Debug)]   
pub struct Connection {
    pub addr     : SocketAddr,
    // can be None for close a socket connection but 
    // wait until the connection sends all messages
    // and will close by EOF
    pub socket: Option<UnboundedSender<Msg>>,
    pub server: ServerHandle,
}

impl Connection {
    pub fn new(addr: SocketAddr, socket_tx: UnboundedSender<Msg>, world_handle: ServerHandle) -> Self {
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
            SelectRole(Role, Answer<()>),
            GetRole(Answer<Option<Role>>),
        },
        #[derive(Debug)]
        pub enum GameCmd {
            GetActivePlayer(Answer<PlayerId>),
            GetAbilities(Answer<[Option<Rank>; 3]>),
            DropAbility(Rank, Answer<()>),
            SelectAbility(Rank, Answer<()>),
            Attack(Card, Answer<()>),
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
    GetContextId       (Answer<GameContextKind>),
    GetUsername        (Answer<String>),
    Close              ,
    NextContext        (ServerNextContextData, Answer<()>), 
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
    pub async fn next_context(&self, for_server: ServerNextContextData) {
        send_oneshot_and_wait(&self.tx, |to| PeerCmd::NextContext(for_server, to)).await
    }
    pub fn send_tcp(&self, msg: server::Msg){
        let _ = self.tx.send(PeerCmd::SendTcp(msg));
    }
    pub async fn get_username(&self) -> String {
        send_oneshot_and_wait(&self.tx, PeerCmd::GetUsername).await
    } 
    pub async fn get_context_id(&self) -> GameContextKind {
        send_oneshot_and_wait(&self.tx, PeerCmd::GetContextId).await
    }
    
    
    
}




#[derive(Debug)]
pub struct IntroHandle<'a>(pub &'a mut PeerHandle);
impl<'a> IntroHandle<'_>{
    pub fn set_username(&self, username: String){
        let _ = self.0.tx.send(
            PeerCmd::from(ContextCmd::from(IntroCmd::SetUsername(username))));
    }
}
#[derive(Debug, Clone)]
pub struct HomeHandle<'a>(pub &'a PeerHandle);
#[derive(Debug, Clone)]
pub struct SelectRoleHandle<'a>(pub &'a PeerHandle);
impl SelectRoleHandle<'_>{
    pub async fn select_role(&self,role: Role){
         send_oneshot_and_wait(&self.0.tx, 
            |to| PeerCmd::from(ContextCmd::from(SelectRoleCmd::SelectRole(role, to)))).await;
    }
    pub async fn get_role(&self) -> Option<Role>{
        send_oneshot_and_wait(&self.0.tx, 
            |to| PeerCmd::from(ContextCmd::from(SelectRoleCmd::GetRole(to)))).await
    }
}

#[derive(Debug, Clone)]
pub struct GameHandle<'a>(pub &'a PeerHandle);
impl GameHandle<'_>{
    pub async fn get_abilities(&self) -> [Option<Rank>; 3]{
        send_oneshot_and_wait(&self.0.tx, 
            |to| PeerCmd::from(ContextCmd::from(GameCmd::GetAbilities(to)))).await
    } 
    pub async fn drop_ability(&self, ability: Rank){
        send_oneshot_and_wait(&self.0.tx, 
            |to| PeerCmd::from(ContextCmd::from(GameCmd::DropAbility(ability, to)))).await
    }
    pub async fn select_ability(&self, ability: Rank){
        send_oneshot_and_wait(&self.0.tx, 
            |to| PeerCmd::from(ContextCmd::from(GameCmd::SelectAbility(ability, to)))).await
    }
    pub async fn attack(&self, monster: Card){
        send_oneshot_and_wait(&self.0.tx, 
            |to| PeerCmd::from(ContextCmd::from(GameCmd::Attack(monster, to)))).await
    }
    
}



pub type ServerGameContextHandle<'a> = GameContext<
     self::IntroHandle<'a>       , 
     self::HomeHandle<'a>         , 
     self::SelectRoleHandle<'a>   , 
     self::GameHandle<'a>       ,
>;

impl<'a> TryFrom<&'a ServerGameContextHandle<'a>> for &'a SelectRoleHandle<'a> {
    type Error = &'static str;
    fn try_from(value: &'a ServerGameContextHandle<'a>) -> Result<Self, Self::Error> {
        match value {
            GameContext::SelectRole(s) => Ok(s),
            _ => Err("Unexpected Context")
        }

    }
}
impl<'a> TryFrom<&'a ServerGameContextHandle<'a>> for &'a IntroHandle<'a> {
    type Error = &'static str;
    fn try_from(value: &'a ServerGameContextHandle<'a>) -> Result<Self, Self::Error> {
        match value {
            GameContext::Intro(s) => Ok(s),
            _ => Err("Unexpected Context")
        }

    }
}
/*
impl<'a> From<(GameContextKind, &'a PeerHandle)> for ServerGameContextHandle<'a> {
    fn from(value: (GameContextKind, &'a PeerHandle)) -> Self {
        match value.0 {
            GameContext::Intro(_) => ServerGameContextHandle::Intro(IntroHandle(value.1)),
            GameContext::Home(_) => ServerGameContextHandle::Home(HomeHandle(value.1)),
            GameContext::SelectRole(_) => ServerGameContextHandle::SelectRole(SelectRoleHandle(value.1)),
            GameContext::Game(_) => ServerGameContextHandle::Game(GameHandle(value.1)),
        }
    }
}
*/

impl From<&ServerGameContextHandle<'_>> for GameContextKind {
    fn from(value: &ServerGameContextHandle) -> Self {
        match value {
            GameContext::Intro(_) => Self::Intro(()),
            GameContext::Home(_) => Self::Home(()),
            GameContext::SelectRole(_) => Self::SelectRole(()),
            GameContext::Game(_) => Self::Game(()),
        }
    }
} 


#[async_trait]
impl<'a> AsyncMessageReceiver<PeerCmd, &'a mut Connection> for Peer {
    async fn message(&mut self, msg: PeerCmd, state:  &'a mut Connection) -> anyhow::Result<()>{
        use crate::protocol::client::{ClientNextContextData, ClientStartGameData};
        match msg {
            PeerCmd::Ping(to) => {
                let _ = to.send(());
            }
            PeerCmd::SyncReconnection(new_connection, tx) => {
                trace!("Sync reconnection for {}", state.addr);
                *state = new_connection;
                let socket = state.socket.as_ref().expect("A socket must be opened");
                match &mut self.context {
                    ServerGameContext::SelectRole(r) => {
                        socket
                            .send(
                                Msg::from(
                            server::AppMsg::NextContext(ClientNextContextData::SelectRole(r.role))))
                            // prevent dev error = new peer should be with the open connection
                                .expect("Must be opened");
                        let _ = socket.send(Msg::from(server::SelectRoleMsg::AvailableRoles(
                                    state.server.get_available_roles().await
                                        ))
                            );
                    }, 
                    ServerGameContext::Game( g) => {
                       // let mut abilities :[Option<Rank>; 3] = Default::default();
                        //g.abilities.ranks[..3].iter()
                         // .map(|r| Some(r) ).zip(abilities.iter_mut()).for_each(|(r, a)| *a = r.copied() );
                         socket.send(
                                 Msg::App(
                            server::AppMsg::NextContext(ClientNextContextData::Game(
                                ClientStartGameData{
                                    abilities: g.take_abilities(), 
                                    monsters: g.session.get_monsters().await,
                                    role: g.get_role(),
                                }
                        ))))
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
                let _ = state.socket.as_ref().map(|s| s.send(msg));
            },
            PeerCmd::GetAddr(to) => {
                let _ = to.send(state.addr);
            },
            PeerCmd::GetContextId(to) => {
                let _ = to.send(GameContextKind::from(&self.context));
            },
            PeerCmd::GetUsername(to) => { 
                
                let _ = to.send(self.get_username());
            },
            PeerCmd::NextContext(data_for_next_context, to ) => {
                 let next_ctx_id = GameContextKind::from(&data_for_next_context);
                 // sends to socket inside
                 self.context.to(data_for_next_context, state).with_context(|| format!(
                    "Failed to request a next context ({:?} for {:?})",
                        next_ctx_id, GameContextKind::from(&self.context), 
                     ))?;
                 let _ = to.send(());

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
                self.name = Some(username);
            }
        };
        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<HomeCmd, &'a mut Connection> for Home {
    async fn message(&mut self, _msg: HomeCmd, _state:  &'a mut Connection) -> anyhow::Result<()>{
        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<SelectRoleCmd, &'a mut Connection> for SelectRole {
    async fn message(&mut self, msg: SelectRoleCmd, state:  &'a mut Connection) -> anyhow::Result<()>{
        match msg {
            SelectRoleCmd::SelectRole(role, tx) => {
                debug!("Select role {:?} for peer {}", role, state.addr);
                self.role = Some(role);
                let _ = tx.send(());
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
    async fn message(&mut self, msg: GameCmd, _state:  &'a mut Connection) -> anyhow::Result<()>{
        match msg {
            GameCmd::GetActivePlayer(to) => {
                let _ = to.send(self.session.get_active_player().await);

            }
            GameCmd::GetAbilities(to) => {
                let _ = to.send(self.take_abilities());

            }
            GameCmd::DropAbility(ability, to) => {
                use crate::protocol::server::AbilityStatus;
                let i = self.abilities.ranks.iter()
                        .position(|i| *i == ability).ok_or_else(||
                        anyhow::anyhow!("Bad ability to drop {:?}", ability))?;
                *self.active_abilities.iter_mut()
                    .nth(i).expect("Must exists") = AbilityStatus::Dropped;
                let _ = to.send(());
            }
            GameCmd::SelectAbility(ability, to) => {
                self.selected_ability = Some(self.abilities.ranks.iter().position(|i| *i == ability)
                    .ok_or_else(||
                                anyhow!("This ability not exists {:?}", ability)

                )?);
                let _ = to.send(());
            }

            GameCmd::Attack(monster, to) => {
                let ability = self.selected_ability
                    .ok_or_else(|| anyhow!("Ability is not selected"))?;
                // TODO responce send to client
                if monster.rank as u16 > self.abilities.ranks[ability] as u16 {
                    return Err(anyhow!("Wrong monster to attack = {:?}, ability = {:?}", monster.rank, self.abilities.ranks[ability]));
                } else {
                    self.session.drop_monster(monster).await?
                }
                let _ = to.send(());
            }

        }
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
                        let _ = state.socket.as_ref().map(|s| s.send(
                                    server::Msg::from(server::AppMsg::Pong)));
                    }
                    client::AppMsg::Logout =>  {
                        let _ = state.socket.as_ref().map(|s| 
                            s.send(server::Msg::from(server::AppMsg::Logout)));
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
                match ctx {
                    GameContextKind::Intro(_) => IntroHandle(self).message(IntroMsg::try_from(msg).unwrap(),  state).await?,
                    GameContextKind::Home(_) => HomeHandle(&*self).message(HomeMsg::try_from(msg).unwrap(),  state).await?,
                    GameContextKind::SelectRole(_) => SelectRoleHandle(&*self).message(SelectRoleMsg::try_from(msg).unwrap(),  state).await?,
                    GameContextKind::Game(_) => GameHandle(&*self).message(GameMsg::try_from(msg).unwrap(),  state).await?,

                }
                 //Into::<ServerGameContextHandle>::into((ctx, &*self))
                 //    .message(msg,  state).await?;
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
impl<'a> AsyncMessageReceiver<client::IntroMsg, &'a mut Connection> for IntroHandle<'_> {
    async fn message(&mut self, msg: client::IntroMsg,
                      state:   &'a mut Connection) -> anyhow::Result<()>{
        use client::IntroMsg;
        match msg {
            IntroMsg::AddPlayer(username) =>  {
                info!("{} is trying to login as {}",
                      state.addr , &username); 
                let status = state.server.add_player(state.addr, 
                                        username, 
                                        self.0.clone()).await;
                trace!("Connection status: {:?}", status);
                let _ = state.socket.as_ref().unwrap().send(Msg::from(
                    server::IntroMsg::LoginStatus(status)));
                match status {
                    LoginStatus::Logged => (), 
                    LoginStatus::Reconnected => {
                        // this get handle to previous peer actor and drop the current handle,
                        // so new actor will shutdown
                        *self.0 = state.server.get_peer_handle(state.addr).await;
                        send_oneshot_and_wait(&self.0.tx, 
                                              |oneshot| PeerCmd::SyncReconnection(state.clone(), oneshot)).await;
                        let _ = state.socket.as_ref().unwrap().send(server::Msg::from(
                            server::AppMsg::ChatLog(state.server.get_chat_log().await)));
                    }
                    _ => { // connection fail
                        warn!("Login attempt rejected = {:?}", status);
                        close_peer(state, self.0).await;
                    }
                }
            },
            IntroMsg::GetChatLog => {
                if state.server.is_peer_connected(state.addr).await{
                    info!("Send a chat history to the client");
                    if let Some(s) = state.socket.as_ref() {
                        let _ = s.send(server::Msg::from(
                        server::AppMsg::ChatLog(state.server.get_chat_log().await)));
                    }
                } else {
                    warn!("Client not logged but the ChatLog was requested");
                    close_peer(state, self.0).await;
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
impl<'a> AsyncMessageReceiver<client::HomeMsg, &'a mut Connection> for HomeHandle<'_> {
    async fn message(&mut self, msg: client::HomeMsg, 
                     state:  &'a mut Connection) -> anyhow::Result<()>{
        use client::HomeMsg;
        match msg {
            HomeMsg::Chat(msg) => {
               broadcast_chat(state, self.0, msg).await
            },
            _ => (),
        }

        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<client::SelectRoleMsg,  &'a mut Connection> for SelectRoleHandle<'_> {
    async fn message(&mut self, msg: client::SelectRoleMsg, 
                     state:   &'a mut Connection) -> anyhow::Result<()> {
        use client::SelectRoleMsg;
        match msg {
            SelectRoleMsg::Chat(msg) => {
               broadcast_chat(state, self.0, msg).await
            },
            SelectRoleMsg::Select(role) => {
                info!("select role request {:?}", role);
                state.server.select_role(state.addr, role);
            }
        }
        Ok(())
    }
}

async fn active_player(to_peer: &UnboundedSender<PeerCmd>) -> PlayerId {
    send_oneshot_and_wait(to_peer, 
        |to| PeerCmd::from(ContextCmd::from(GameCmd::GetActivePlayer(to)))).await
}

#[async_trait]
impl<'a> AsyncMessageReceiver<client::GameMsg, &'a mut Connection> for GameHandle<'_> {
    async fn message(&mut self, msg: client::GameMsg, 
                     state:  &'a mut Connection) -> anyhow::Result<()>{
        use client::GameMsg;
        macro_rules! turn {
            ($($msg:ident)::*($ok:expr)) => {
               state.socket.as_ref().expect("Must be opened")
                    .send(server::Msg::from( $($msg)::*( {
                    let active_player = active_player(&self.0.tx).await;
                    if active_player == state.addr {
                        state.server.make_turn(state.addr).await;
                        TurnResult::Ok($ok)
                    } else {
                        TurnResult::Err(state.server.get_peer_username(active_player).await)
                    }
                })))?
            }
        }
        match msg {
            GameMsg::Chat(msg) => {
               broadcast_chat(state, self.0, msg).await
            }, 
            GameMsg::DropAbility(rank)   => {
                turn!(server::GameMsg::DropAbility({
                    
                    rank

                }));
            },
            GameMsg::SelectAbility(rank) => {
                turn!(server::GameMsg::SelectAbility(rank));
            },
            GameMsg::Attack(card)        => {
                turn!(server::GameMsg::Attack(card));
            },
            GameMsg::Continue => {
                state.server.make_turn(state.addr).await;

            }
        }


        Ok(())
    }
}

