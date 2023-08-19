use std::net::SocketAddr;

use anyhow::{anyhow, Context as _};
use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, info, trace, warn};

use crate::{
    game::{Card, Rank, Role},
    protocol::{
        Username,
        client::{self, GameMsg, HomeMsg, IntroMsg, RolesMsg},
        server::{
            self, Game, Home, Intro, LoginStatus, Msg, PlayerId, Roles, ServerGameContext,
            NextContext, TurnResult,
        },
        AsyncMessageReceiver, GameContext, GameContextKind, ToContext,
    },
    server::{
        Handle,
        session::{GameSession, GameSessionHandle, GameSessionState, SessionCmd},
        Answer, ServerCmd, ServerHandle,
        details::api
    },
};



api! {
    impl Handle<PeerCmd> {
        pub async fn ping(&self) -> ();
        pub async fn next_context(&self, for_server: NextContext)  -> ();
        pub fn send_tcp(&self, msg: server::Msg)   ;
        pub async fn get_username(&self) -> Username ;
        pub async fn get_context_id(&self) -> GameContextKind ;
        pub async fn get_addr(&self) -> SocketAddr ;
        pub fn close(&self);
        pub fn context_cmd(&self, cmd: ContextCmd);
        pub async fn sync_reconnection(&self, new_connection: Connection) -> ();

    }
}
pub type PeerHandle = Handle<PeerCmd>;

pub type ServerGameContextHandle2 = Handle<ContextCmd>;





pub struct Peer {
    pub context: ServerGameContext,
}
impl Peer {
    pub fn new(start_context: ServerGameContext) -> Peer {
        Peer {
            context: start_context,
        }
    }

    pub fn get_username(&self) -> Username {
        use GameContext as C;
        match self.context.as_inner() {
            C::Intro(i) => i.name.as_ref().expect(
                "if peer is logged, and a handle of this peer \
allows for other actors e.g. if a server room has a peer handle to this peer, \
this peer must has a username",
            ),
            C::Home(h) => &h.name,
            C::Roles(r) => &r.name,
            C::Game(g) => &g.name,
        }
        .clone()
    }
}

#[derive(Clone, std::fmt::Debug)]
pub struct Connection {
    pub addr: SocketAddr,
    // can be None for close a socket connection but
    // wait until the connection sends all messages
    // and will close by EOF
    pub socket: Option<UnboundedSender<Msg>>,
    pub server: ServerHandle,
}

impl Connection {
    pub fn new(
        addr: SocketAddr,
        socket_tx: UnboundedSender<Msg>,
        world_handle: ServerHandle,
    ) -> Self {
        Connection {
            addr,
            socket: Some(socket_tx),
            server: world_handle,
        }
    }
    pub fn close_socket(&mut self) {
        self.socket = None;
    }
}

macro_rules! nested {
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



nested! {
pub type ContextCmd = GameContext <
        #[derive(Debug)]
        pub enum IntroCmd {
            SetUsername(Username),

        },
        #[derive(Debug)]
        pub enum HomeCmd {

        },
        #[derive(derive_more::Debug)]
        pub enum RolesCmd {
            Roles(
                Role,
                #[debug(skip)]
                Answer<()>
                       ),
            GetRole(
                #[debug(skip)]
                Answer<Option<Role>>
                ),
            SpawnSession([PlayerId; crate::protocol::server::MAX_PLAYER_COUNT],#[debug(skip)] Answer<GameSessionHandle>),
        },
        #[derive(derive_more::Debug)]
        pub enum GameCmd {
            GetActivePlayer(
                #[debug(skip)]
                Answer<PlayerId>
            ),
            GetAbilities(
                #[debug(skip)]
                Answer<[Option<Rank>; 3]>
            ),
            DropAbility(
                Rank,
                #[debug(skip)]
                Answer<()>
                        ),
            SelectAbility(Rank,
                #[debug(skip)]
                Answer<()>),
            Attack(Card,
                #[debug(skip)]
                Answer<()>),
            Continue(#[debug(skip)]
                Answer<()>),
            UpdateClientState,
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
    RolesCmd  => Roles,
    GameCmd        => Game,
    => ContextCmd
}


impl From<ContextCmd> for PeerCmd {
    fn from(cmd: ContextCmd) -> Self {
        PeerCmd::ContextCmd(cmd)
    }
}

use crate::server::details::send_oneshot_and_wait;



use std::marker::PhantomData;
struct ContextHandle<'a, T>(pub &'a mut PeerHandle, PhantomData<T>);
impl<'a> ContextHandle<'a, IntroCmd> {
    pub fn set_username(&self, username: Username) {
        let _ = self
            .0
            .tx
            .send(PeerCmd::from(ContextCmd::from(IntroCmd::SetUsername(
                username,
        ))));
    }
}

#[derive(Debug)]
pub struct IntroHandle<'a>(pub &'a mut PeerHandle);



impl IntroHandle<'_> {
    pub fn set_username(&self, username: Username) {
        let _ = self
            .0
            .tx
            .send(PeerCmd::from(ContextCmd::from(IntroCmd::SetUsername(
                username,
            ))));
    }
}
#[derive(Debug, Clone)]
pub struct HomeHandle<'a>(pub &'a PeerHandle);

#[derive(Debug, Clone)]
pub struct RolesHandle<'a>(pub &'a PeerHandle);
impl RolesHandle<'_> {
    pub async fn select_role(&self, role: Role) {
        send_oneshot_and_wait(&self.0.tx, |to| {
            PeerCmd::from(ContextCmd::from(RolesCmd::Roles(role, to)))
        })
        .await;
    }
    pub async fn get_role(&self) -> Option<Role> {
        send_oneshot_and_wait(&self.0.tx, |to| {
            PeerCmd::from(ContextCmd::from(RolesCmd::GetRole(to)))
        })
        .await
    }
    pub async fn spawn_session(&self, players: [PlayerId; crate::protocol::server::MAX_PLAYER_COUNT]) -> GameSessionHandle {
        send_oneshot_and_wait(&self.0.tx, |to| {
            PeerCmd::from(ContextCmd::from(RolesCmd::SpawnSession(players, to)))
        })
        .await
    }
}

#[derive(Debug, Clone)]
pub struct GameHandle<'a>(pub &'a PeerHandle);
impl GameHandle<'_> {
    pub async fn get_abilities(&self) -> [Option<Rank>; 3] {
        send_oneshot_and_wait(&self.0.tx, |to| {
            PeerCmd::from(ContextCmd::from(GameCmd::GetAbilities(to)))
        })
        .await
    }
    pub async fn drop_ability(&self, ability: Rank) {
        send_oneshot_and_wait(&self.0.tx, |to| {
            PeerCmd::from(ContextCmd::from(GameCmd::DropAbility(ability, to)))
        })
        .await
    }
    pub async fn select_ability(&self, ability: Rank) {
        send_oneshot_and_wait(&self.0.tx, |to| {
            PeerCmd::from(ContextCmd::from(GameCmd::SelectAbility(ability, to)))
        })
        .await
    }
    pub async fn attack(&self, monster: Card) {
        send_oneshot_and_wait(&self.0.tx, |to| {
            PeerCmd::from(ContextCmd::from(GameCmd::Attack(monster, to)))
        })
        .await
    }
    pub async fn continue_game(&self) {
        send_oneshot_and_wait(&self.0.tx, |to| {
            PeerCmd::from(ContextCmd::from(GameCmd::Continue(to)))
        })
        .await
    }
    fn update_game_state_for_client(&self){
        let _ = self.0.tx.send(PeerCmd::from(ContextCmd::from(GameCmd::UpdateClientState)));

    }
    async fn active_player(&self) -> PlayerId {
        send_oneshot_and_wait(&self.0.tx, |to| {
            PeerCmd::from(ContextCmd::from(GameCmd::GetActivePlayer(to)))
        })
        .await
    }
}


#[derive(Debug)]
#[repr(transparent)]
pub struct ServerGameContextHandle<'a>(
    pub GameContext<
     self::IntroHandle<'a>,
     self::HomeHandle<'a>,
     self::RolesHandle<'a>,
     self::GameHandle<'a>,
>);

impl<'a> ServerGameContextHandle<'a> {
    pub fn as_inner(&'a self) -> &'a GameContext<IntroHandle ,HomeHandle, RolesHandle ,GameHandle,>{
        &self.0
    }
    pub fn as_inner_mut(&'a mut self) -> &'a mut GameContext<IntroHandle ,HomeHandle, RolesHandle ,GameHandle,>{
        &mut self.0
    }
}
impl From<&ServerGameContextHandle<'_>> for GameContextKind{
    #[inline]
    fn from(value: &ServerGameContextHandle<'_>) -> Self {
        GameContextKind::from(&value.0)
    }
}
impl<'a> TryFrom<&'a ServerGameContextHandle<'a>> for &'a RolesHandle<'a> {
    type Error = &'static str;
    fn try_from(value: &'a ServerGameContextHandle<'a>) -> Result<Self, Self::Error> {
        match value.as_inner() {
            GameContext::Roles(s) => Ok(s),
            _ => Err("Unexpected Context"),
        }
    }
}
impl<'a> TryFrom<&'a ServerGameContextHandle<'a>> for &'a IntroHandle<'a> {
    type Error = &'static str;
    fn try_from(value: &'a ServerGameContextHandle<'a>) -> Result<Self, Self::Error> {
        match value.as_inner() {
            GameContext::Intro(s) => Ok(s),
            _ => Err("Unexpected Context"),
        }
    }
}



#[async_trait]
impl<'a> AsyncMessageReceiver<PeerCmd, &'a mut Connection> for Peer {
    async fn reduce(&mut self, msg: PeerCmd, state: &'a mut Connection) -> anyhow::Result<()> {
        use crate::protocol::client::{NextContext, StartGame};
        match msg {
            PeerCmd::Ping(to) => {
                let _ = to.send(());
            }
            PeerCmd::SyncReconnection(new_connection, tx) => {
                trace!("Sync reconnection for {}", state.addr);
                *state = new_connection;
                let socket = state.socket.as_ref()
                    .expect("A socket of the new peer must be connected");
                match &mut self.context.as_inner() {
                    GameContext::Roles(r) => {
                        socket
                            .send(Msg::from(server::AppMsg::NextContext(
                                NextContext::Roles(r.role),
                            )))
                            // prevent dev error
                            .expect("New peer should be connected");
                        let _ = socket.send(Msg::from(server::RolesMsg::AvailableRoles(
                            state.server.get_available_roles().await,
                        )));
                    }
                    GameContext::Game(g) => {
                        socket
                            .send(Msg::App(server::AppMsg::NextContext(
                                NextContext::Game(StartGame {
                                    abilities: g.abilities.active_items(),
                                    monsters: g.session.get_monsters().await,
                                    role: g.get_role(),
                                }),
                            )))
                            .expect("Must be opened");
                    }
                    _ => unreachable!("Reconnection not allowed for the Intro or Home contexts"),
                };
                state
                    .server
                    .broadcast_to_all(Msg::from(server::AppMsg::Chat(
                        server::ChatLine::Reconnection(self.get_username()),
                    )))
                    .await;
                let _ = tx.send(());
            }
            PeerCmd::Close() => {
                state.server.drop_peer(state.addr);
                debug!("Close the socket tx on the Peer actor side");
                state.close_socket();
            }
            // TODO socket closed error return
            PeerCmd::SendTcp(msg) => {
                let _ = state.socket.as_ref().map(|s| s.send(msg));
            }
            PeerCmd::GetAddr(to) => {
                let _ = to.send(state.addr);
            }
            PeerCmd::GetContextId(to) => {
                let _ = to.send(GameContextKind::from(self.context.as_inner()));
            }
            PeerCmd::GetUsername(to) => {
                let _ = to.send(self.get_username());
            }
            PeerCmd::NextContext(data_for_next_context, to) => {
                let next_ctx_id = GameContextKind::from(&data_for_next_context);
                // sends to socket inside
                use crate::protocol::ContextConverter;
                take_mut::take_or_recover(&mut self.context, || ServerGameContext::default() , |this| {
                    ServerGameContext::try_from((ContextConverter(this, data_for_next_context), &*state))
                        .expect("Should switch")
                });
                let _ = to.send(());
            }
            PeerCmd::ContextCmd(msg) => {
                self.context.reduce(msg, state).await?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl<'a> AsyncMessageReceiver<IntroCmd, &'a mut Connection> for Intro {
    async fn reduce(&mut self, msg: IntroCmd, state: &'a mut Connection) -> anyhow::Result<()> {
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
    async fn reduce(&mut self, _msg: HomeCmd, _state: &'a mut Connection) -> anyhow::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl<'a> AsyncMessageReceiver<RolesCmd, &'a mut Connection> for Roles {
    async fn reduce(
        &mut self,
        msg: RolesCmd,
        state: &'a mut Connection,
    ) -> anyhow::Result<()> {
        use RolesCmd as Cmd;
        match msg {
            Cmd::Roles(role, tx) => {
                debug!("Select role {:?} for peer {}", role, state.addr);
                self.role = Some(role);
                let _ = tx.send(());
            }
            Cmd::GetRole(tx) => {
                let _ = tx.send(self.role);
            }
            Cmd::SpawnSession(players, tx) => {
                info!("Start a new game session");
                let _ = tx.send(crate::server::session::spawn_session(players, state.server.clone()));
            }
        }
        Ok(())
    }
}

async fn send_active_status(game: &mut Game, state: &mut Connection, next_player: PlayerId) {
    use crate::protocol::TurnStatus;
    state.server.broadcast(
        next_player,
        Msg::from(server::GameMsg::Turn(TurnStatus::Wait)),
    );
    state
        .server
        .send_to_player(
            next_player,
            Msg::from(server::GameMsg::Turn(TurnStatus::Ready(
                game.session.get_game_phase().await,
            ))),
        )
        .await;
}

#[async_trait]
impl<'a> AsyncMessageReceiver<GameCmd, &'a mut Connection> for Game {
    async fn reduce(&mut self, msg: GameCmd, state: &'a mut Connection) -> anyhow::Result<()> {
        match msg {
            GameCmd::GetActivePlayer(tx) => {
                let _ = tx.send(self.session.get_active_player().await);
            }
            GameCmd::GetAbilities(tx) => {
                let _ = tx.send(self.abilities.active_items());
            }
            GameCmd::DropAbility(ability, tx) => {
                let i = self
                    .abilities.items
                    .ranks
                    .iter()
                    .position(|i| *i == ability)
                    .ok_or_else(|| anyhow::anyhow!("Bad ability to drop {:?}", ability))?;
                self.abilities.drop_item(*self.abilities.items.ranks.iter().nth(i).unwrap())?;
                send_active_status(self, state, self.session.switch_to_next_player().await).await;
                let _ = tx.send(());
            }
            GameCmd::SelectAbility(ability, tx) => {
                self.selected_ability = Some(
                    self.abilities.items
                        .ranks
                        .iter()
                        .position(|i| *i == ability)
                        .ok_or_else(|| anyhow!("This ability not exists {:?}", ability))?,
                );
                send_active_status(self, state, self.session.switch_to_next_player().await).await;
                let _ = tx.send(());
            }

            GameCmd::Attack(monster, tx) => {
                let ability = self
                    .selected_ability
                    .ok_or_else(|| anyhow!("Ability is not selected"))?;
                // TODO responce send to client
               // if monster.rank as u16 > self.abilities.items.ranks[ability] as u16 {
               //     return Err(anyhow!(
               //         "Wrong monster to attack = {:?}, ability = {:?}",
               //         monster.rank,
                //        self.abilities.items.ranks[ability]
               //     ));
                //} else {
                self.session.drop_monster(monster).await?;
                trace!("Drop monster {:?}, all now {:?}", monster, self.session.get_monsters().await);
                //}
                send_active_status(self, state, self.session.switch_to_next_player().await).await;
                let _ = tx.send(());
            }
            GameCmd::Continue(tx) => {
                // TODO end of deck, handle game finish
                let _ = self.abilities.next_actives();
                self.session.continue_game_cycle().await;
                send_active_status(self, state, self.session.switch_to_next_player().await).await;
                let _ = tx.send(());
            }
            GameCmd::UpdateClientState => {
                state.socket.as_ref().unwrap().send(Msg::from(server::GameMsg::UpdateGameData((
                    self.session.get_monsters().await,
                    self.abilities.active_items(),
                ))))?;
            }

        }
        Ok(())
    }
}

#[async_trait]
impl<'a> AsyncMessageReceiver<client::Msg, &'a mut Connection> for PeerHandle {
    async fn reduce(&mut self, msg: client::Msg, state: &'a mut Connection) -> anyhow::Result<()> {
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
                        let _ = state
                            .socket
                            .as_ref()
                            .map(|s| s.send(server::Msg::from(server::AppMsg::Pong)));
                    }
                    client::AppMsg::Logout => {
                        let _ = state
                            .socket
                            .as_ref()
                            .map(|s| s.send(server::Msg::from(server::AppMsg::Logout)));
                        info!("Logout");
                        // TODO
                        //return Err(MessageError::Logout);
                    }
                    client::AppMsg::NextContext => {
                        state
                            .server
                            .request_next_context_after(state.addr, self.get_context_id().await);
                    }
                }
            }
            _ => {
                
                // validation message context
                //
                let ctx = self.get_context_id().await;
                macro_rules! context {
                    ( $(
                            $context:ident => $context_handle:ident => $context_msg:ident,
                        )*
                        ) => {
                        match ctx {
                            $(
                                GameContextKind::$context => {
                                    $context_handle(&*self)
                                        .reduce($context_msg::try_from(msg)
                                                .map_err(|e| anyhow!(concat!("Must be", 
                                                                stringify!($context_msg),
                                                                "here, found = {:?}"), e) )?
                                                , state)
                                        .await?;
                                }
                            )*
                            _ => unreachable!(),
                        }
                    }
                }
                match ctx {
                    GameContextKind::Intro => {
                        IntroHandle(self)
                            .reduce(IntroMsg::try_from(msg)
                                    .map_err(|e| 
                                anyhow!("Must be 'Intro' here, found = {:?}", e))?, state)
                            .await?
                    }
                    _ => {
                        context!{
                            Home => HomeHandle => HomeMsg,
                            Roles => RolesHandle => RolesMsg, 
                            Game => GameHandle => GameMsg,
                        }
                    }
                    
                }
            }
        }
        Ok(())
    }
}

async fn close_peer(state: &mut Connection, peer: &PeerHandle) {
    // should close but wait the socket writer EOF,
    // so it just drops socket tx
    let _ = peer.tx.send(PeerCmd::Close());
    trace!("Close the socket tx on the PeerHandle side");
    state.socket = None;
}

#[async_trait]
impl<'a> AsyncMessageReceiver<client::IntroMsg, &'a mut Connection> for IntroHandle<'_> {
    async fn reduce(
        &mut self,
        msg: client::IntroMsg,
        state: &'a mut Connection,
    ) -> anyhow::Result<()> {
        use client::IntroMsg;
        match msg {
            IntroMsg::AddPlayer(username) => {
                info!("{} is trying to login as {}", state.addr, &username);
                let status = state
                    .server
                    .add_player(state.addr, username, self.0.clone())
                    .await;
                trace!("Connection status: {:?}", status);
                let _ = state
                    .socket
                    .as_ref()
                    .unwrap()
                    .send(Msg::from(server::IntroMsg::LoginStatus(status)));
                match status {
                    LoginStatus::Logged => (),
                    LoginStatus::Reconnected => {
                        // this get handle to previous peer actor and drop the current handle,
                        // so new actor will shutdown
                        *self.0 = state.server.get_peer_handle(state.addr).await;
                        send_oneshot_and_wait(&self.0.tx, |oneshot| {
                            PeerCmd::SyncReconnection(state.clone(), oneshot)
                        })
                        .await;
                        let _ = state.socket.as_ref().unwrap().send(server::Msg::from(
                            server::AppMsg::ChatLog(state.server.get_chat_log().await),
                        ));
                    }
                    _ => {
                        // connection fail
                        warn!("Login attempt rejected = {:?}", status);
                        close_peer(state, self.0).await;
                    }
                }
            }
            IntroMsg::GetChatLog => {
                if state.server.is_peer_connected(state.addr).await {
                    info!("Send a chat history to the client");
                    if let Some(s) = state.socket.as_ref() {
                        let _ = s.send(server::Msg::from(server::AppMsg::ChatLog(
                            state.server.get_chat_log().await,
                        )));
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

async fn broadcast_chat(state: &Connection, peer: &PeerHandle, msg: String) {
    let msg = server::ChatLine::Text(format!("{}: {}", peer.get_username().await, msg));
    state.server.append_chat(msg.clone());
    state
        .server
        .broadcast(state.addr, Msg::from(server::AppMsg::Chat(msg)));
}

#[async_trait]
impl<'a> AsyncMessageReceiver<client::HomeMsg, &'a mut Connection> for HomeHandle<'_> {
    async fn reduce(
        &mut self,
        msg: client::HomeMsg,
        state: &'a mut Connection,
    ) -> anyhow::Result<()> {
        use client::HomeMsg;
        match msg {
            HomeMsg::Chat(msg) => broadcast_chat(state, self.0, msg).await,
            _ => (),
        }

        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<client::RolesMsg, &'a mut Connection> for RolesHandle<'_> {
    async fn reduce(
        &mut self,
        msg: client::RolesMsg,
        state: &'a mut Connection,
    ) -> anyhow::Result<()> {
        use client::RolesMsg;
        match msg {
            RolesMsg::Chat(msg) => broadcast_chat(state, self.0, msg).await,
            RolesMsg::Select(role) => {
                info!("select role request {:?}", role);
                state.server.select_role(state.addr, role);
            }
        }
        Ok(())
    }
}




#[async_trait]
impl<'a> AsyncMessageReceiver<client::GameMsg, &'a mut Connection> for GameHandle<'_> {
    async fn reduce(
        &mut self,
        msg: client::GameMsg,
        state: &'a mut Connection,
    ) -> anyhow::Result<()> {
        use client::GameMsg;
        macro_rules! turn {
            ($($msg:ident)::*($ok:expr)) => {
               state.socket.as_ref()
                   .expect("Must be opened")
                   .send(server::Msg::from( $($msg)::*( {
                    let active_player = self.active_player().await;
                    if active_player == state.addr {
                        TurnResult::Ok($ok)
                    } else {
                        TurnResult::Err(state.server.get_peer_username(active_player).await)
                    }
                })))?;
                state.server.broadcast_game_state(state.addr);
                self.update_game_state_for_client();
            }
        }
        match msg {
            GameMsg::Chat(msg) => broadcast_chat(state, self.0, msg).await,
            GameMsg::DropAbility(rank) => {
                turn!(server::GameMsg::DropAbility({
                    self.drop_ability(rank).await;
                    rank
                }));
            }
            GameMsg::SelectAbility(rank) => {
                turn!(server::GameMsg::SelectAbility({
                    self.select_ability(rank).await;
                    rank
                }));
            }
            GameMsg::Attack(card) => {
                turn!(server::GameMsg::Attack({
                    self.attack(card).await;
                    card
                }));
            }
            GameMsg::Continue => {
                turn!(server::GameMsg::Continue({
                    self.continue_game().await;
                }));
            }
        }

        Ok(())
    }
}
