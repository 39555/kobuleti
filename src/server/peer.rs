use anyhow::Context as _;
use futures::SinkExt;
use tokio::{
    net::TcpStream,
    sync::{
        mpsc::channel,
        oneshot::{self, error::RecvError},
    },
};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use tracing::{debug, error, info, info_span, trace, warn, Instrument};

use super::{details::actor_api, states, Answer, Handle, Tx, MPSC_CHANNEL_CAPACITY};
use crate::{
    game::{Card, Rank, Role, Suit},
    protocol::{client, encode_message, AsyncMessageReceiver, MessageDecoder, Msg, Username},
};

pub type PeerHandle<T> = Handle<Msg<self::SharedCmd, T>>;

actor_api! { // Shared
    impl<M>  Handle<Msg<SharedCmd, M>>{
        pub async fn ping(&self) -> Result<(), RecvError>;
        pub async fn get_username(&self) -> Result<Username, RecvError>;
        pub async fn close(&self);

    }
}

actor_api! { // Intro
    impl  Handle<Msg<SharedCmd, IntroCmd>>{
        pub async fn set_username(&self, username: Username);
        pub async fn send_tcp(&self, msg: Msg<server::SharedMsg, server::IntroMsg>);
        pub async fn enter_game(&self, server: states::HomeHandle) -> Result<HomeHandle, RecvError>;
        pub async fn reconnect_roles(&self, server: states::RolesHandle, old_peer: RolesHandle) -> Result<RolesHandle, RecvError>;
        pub async fn reconnect_game(&self, server: states::GameHandle, old_peer: GameHandle) -> Result<GameHandle, RecvError>;

    }
}
actor_api! { // Home
    impl  Handle<Msg<SharedCmd, HomeCmd>>{
        pub async fn send_tcp(&self, msg: Msg<server::SharedMsg, server::HomeMsg>);
        pub async fn start_roles(&self, server_handle: states::RolesHandle) -> Result<RolesHandle, RecvError>;


    }
}
actor_api! { // Roles
    impl  Handle<Msg<SharedCmd, RolesCmd>>{
        pub async fn take_peer(&self) ->  Result<Peer<Roles>, RecvError>;
        pub async fn send_tcp(&self, msg: Msg<server::SharedMsg, server::RolesMsg>)   ;
        pub async fn start_game(&self, server_handle: states::GameHandle) ->  Result<GameHandle, RecvError>;
        pub async fn get_role(&self) ->  Result<Option<Role>, RecvError>;
        pub async fn select_role(&self, role: Role) ->  Result<(), RecvError>;


    }
}

actor_api! { // Game
    impl  Handle<Msg<SharedCmd, GameCmd>>{
        pub async fn take_peer(&self) -> Result<Peer<Game>, RecvError>;
        pub async fn send_tcp(&self, msg: Msg<server::SharedMsg, server::GameMsg>)   ;
        pub async fn get_abilities(&self) -> Result<[Option<Rank>; 3], RecvError>;
        pub async fn select_ability(&self, ability: Rank) ->  Result<(), RecvError>;
        pub async fn drop_ability(&self, ability: Rank) ->  Result<(), RecvError>;
        pub async fn attack(&self, monster: Card) ->  Result<(), RecvError>;
        pub async fn continue_game(&self) ->  Result<(), RecvError>;
        pub async fn sync_with_client(&self);

    }
}
use crate::protocol::With;
macro_rules! with {
    ($($src:ident,)*) => {
        $(
            impl With<$src, Msg<SharedCmd, $src>> for Msg<SharedCmd, $src>{
                #[inline]
                fn with(value: $src) ->  Msg<SharedCmd, $src> {
                    Msg::State(value)
                }
            }
        )*
    }
}
with! {IntroCmd, HomeCmd, RolesCmd, GameCmd,}
impl<M> With<SharedCmd, Msg<SharedCmd, M>> for Msg<SharedCmd, M> {
    #[inline]
    fn with(value: SharedCmd) -> Msg<SharedCmd, M> {
        Msg::Shared(value)
    }
}

pub type IntroHandle = Handle<Msg<SharedCmd, IntroCmd>>;
pub type HomeHandle = Handle<Msg<SharedCmd, HomeCmd>>;
pub type RolesHandle = Handle<Msg<SharedCmd, RolesCmd>>;
pub type GameHandle = Handle<Msg<SharedCmd, GameCmd>>;

use crate::protocol::details::impl_GameContextKind_from_state;
impl_GameContextKind_from_state! {IntroHandle => Intro, HomeHandle => Home, RolesHandle => Roles, GameHandle => Game,}
impl_GameContextKind_from_state! {Intro => Intro, Home => Home, Roles => Roles, Game => Game,}

pub struct Peer<State>
where
    State: AssociatedServerHandle + AssociatedHandle,
{
    pub username: Username,
    pub state: State,
}

#[derive(Default)]
pub struct Intro {
    username: Option<Username>,
}

impl<T> Peer<T>
where
    T: AssociatedServerHandle + AssociatedHandle,
{
    fn get_username(&self) -> &Username {
        &self.username
    }
}
impl Intro {
    fn get_username(&self) -> &Username {
        &self.username.as_ref().unwrap()
    }
}

pub struct Home;

#[derive(Default)]
pub struct Roles {
    pub selected_role: Option<Role>,
}

use crate::{
    game::{AbilityDeck, Deckable},
    protocol::server::ABILITY_COUNT,
    server::details::Stateble,
};

pub struct Game {
    pub abilities: Stateble<AbilityDeck, ABILITY_COUNT>,
    pub selected_ability: Option<usize>,
    pub health: u16,
}
impl Game {
    pub fn new(role: Suit) -> Self {
        let mut abilities = AbilityDeck::new(role);
        abilities.shuffle();
        Game {
            abilities: Stateble::with_items(abilities),
            health: 36,
            selected_ability: None,
        }
    }
    pub fn get_role(&self) -> Suit {
        self.abilities.items.suit
    }
}

impl From<Intro> for Peer<Home> {
    fn from(intro: Intro) -> Self {
        Peer {
            username: intro.username.unwrap(),
            state: Home,
        }
    }
}

impl From<Peer<Home>> for Peer<Roles> {
    fn from(value: Peer<Home>) -> Self {
        Peer {
            username: value.username,
            state: Roles::default(),
        }
    }
}
impl From<Peer<Roles>> for Peer<Game> {
    fn from(value: Peer<Roles>) -> Self {
        Peer {
            username: value.username,
            state: Game::new(Suit::from(
                value.state.selected_role.expect("Role must be selected"),
            )),
        }
    }
}

use crate::protocol::IncomingSocketMessage;

impl IncomingSocketMessage for IntroHandle {
    type Msg = client::IntroMsg;
}
impl IncomingSocketMessage for HomeHandle {
    type Msg = client::HomeMsg;
}
impl IncomingSocketMessage for RolesHandle {
    type Msg = client::RolesMsg;
}
impl IncomingSocketMessage for GameHandle {
    type Msg = client::GameMsg;
}

use crate::protocol::{server, SendSocketMessage};
impl SendSocketMessage for IntroHandle {
    type Msg = Msg<server::SharedMsg, server::IntroMsg>;
}
impl SendSocketMessage for HomeHandle {
    type Msg = Msg<server::SharedMsg, server::HomeMsg>;
}
impl SendSocketMessage for RolesHandle {
    type Msg = Msg<server::SharedMsg, server::RolesMsg>;
}
impl SendSocketMessage for GameHandle {
    type Msg = Msg<server::SharedMsg, server::GameMsg>;
}
impl SendSocketMessage for states::IntroHandle {
    type Msg = Msg<server::SharedMsg, server::IntroMsg>;
}
impl SendSocketMessage for states::HomeHandle {
    type Msg = Msg<server::SharedMsg, server::HomeMsg>;
}
impl SendSocketMessage for states::RolesHandle {
    type Msg = Msg<server::SharedMsg, server::RolesMsg>;
}
impl SendSocketMessage for states::GameHandle {
    type Msg = Msg<server::SharedMsg, server::GameMsg>;
}
impl SendSocketMessage for Intro {
    type Msg = Msg<server::SharedMsg, server::IntroMsg>;
}
impl SendSocketMessage for Home {
    type Msg = Msg<server::SharedMsg, server::HomeMsg>;
}
impl SendSocketMessage for Roles {
    type Msg = Msg<server::SharedMsg, server::RolesMsg>;
}
impl SendSocketMessage for Game {
    type Msg = Msg<server::SharedMsg, server::GameMsg>;
}

pub trait AssociatedHandle
where
    Self::Handle: SendSocketMessage,
{
    type Handle;
}
impl AssociatedHandle for Intro {
    type Handle = IntroHandle;
}
impl AssociatedHandle for Home {
    type Handle = HomeHandle;
}
impl AssociatedHandle for Roles {
    type Handle = RolesHandle;
}
impl AssociatedHandle for Game {
    type Handle = GameHandle;
}
pub trait AssociatedServerHandle
where
    Self::Handle: SendSocketMessage,
{
    type Handle;
}

impl AssociatedServerHandle for IntroHandle {
    type Handle = states::IntroHandle;
}
impl AssociatedServerHandle for HomeHandle {
    type Handle = states::HomeHandle;
}
impl AssociatedServerHandle for RolesHandle {
    type Handle = states::RolesHandle;
}
impl AssociatedServerHandle for GameHandle {
    type Handle = states::GameHandle;
}
impl AssociatedServerHandle for Intro {
    type Handle = <<Intro as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle;
}
impl AssociatedServerHandle for Home {
    type Handle = <<Home as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle;
}
impl AssociatedServerHandle for Roles {
    type Handle = <<Roles as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle;
}
impl AssociatedServerHandle for Game {
    type Handle = <<Game as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle;
}
pub trait IncomingCommand
where
    for<'a> Self::Cmd: core::fmt::Debug + Send,
{
    type Cmd;
}
impl IncomingCommand for Intro {
    type Cmd = IntroCmd;
}
impl IncomingCommand for Home {
    type Cmd = HomeCmd;
}
impl IncomingCommand for Roles {
    type Cmd = RolesCmd;
}
impl IncomingCommand for Game {
    type Cmd = GameCmd;
}
impl IncomingCommand for states::IntroHandle {
    type Cmd = states::IntroCmd;
}
impl IncomingCommand for states::HomeHandle {
    type Cmd = Msg<states::SharedCmd, states::HomeCmd>;
}
impl IncomingCommand for states::RolesHandle {
    type Cmd = Msg<states::SharedCmd, states::RolesCmd>;
}
impl IncomingCommand for states::GameHandle {
    type Cmd = Msg<states::SharedCmd, states::GameCmd>;
}

use std::net::SocketAddr;

pub struct Connection<M>
where
    Handle<M>: SendSocketMessage,
{
    addr: SocketAddr,
    server: Handle<M>,
    // can be None for close a socket connection but
    // wait until the connection sends all messages
    // and will close by EOF
    socket: Option<Tx<<Handle<M> as SendSocketMessage>::Msg>>,
}
impl<T> Clone for Connection<T>
where
    Handle<T>: SendSocketMessage,
{
    fn clone(&self) -> Self {
        Connection {
            addr: self.addr,
            server: self.server.clone(),
            socket: self.socket.clone(),
        }
    }
}

impl<M> Connection<M>
where
    Handle<M>: SendSocketMessage,
{
    pub fn new(
        addr: SocketAddr,
        server: Handle<M>,
        socket: Tx<<Handle<M> as SendSocketMessage>::Msg>,
    ) -> Self {
        Connection {
            addr,
            server,
            socket: Some(socket),
        }
    }
    pub fn close_socket(&mut self) {
        self.socket = None;
    }
}

macro_rules! done {
    ($option:expr) => {
        match $option {
            None => return Ok(()),
            Some(x) => x,
        }
    };
}

struct NotifyServer<State, PeerHandle>(pub State, pub Answer<PeerHandle>);

#[tracing::instrument(skip_all, name="Peer", fields(p = %socket.peer_addr().unwrap()))]
pub async fn accept_connection(
    socket: &mut TcpStream,
    intro_server: states::IntroHandle,
) -> anyhow::Result<()> {
    let addr = socket.peer_addr()?;
    macro_rules! run_state {
        ($visitor:expr, $connection:expr, $peer_rx:expr ) => {
            async {
                let (done, mut done_rx) = oneshot::channel();
                let mut state = ReduceState {
                    connection: $connection,
                    done: Some(done),
                };
                loop {
                    tokio::select! {
                        new_state = &mut done_rx => {
                            return Ok::<Option<_>, anyhow::Error>(Some(($visitor, new_state?)))
                        }
                        cmd = $peer_rx.recv() => match cmd {
                            Some(peer_cmd) => {
                                debug!(?peer_cmd);
                                match peer_cmd {
                                    Msg::Shared(cmd) =>  match cmd {
                                        SharedCmd::Ping(tx) => {
                                             let _ = tx.send(());
                                        }
                                        SharedCmd::Close() => {
                                                state.connection.close_socket();
                                                break;

                                        }
                                        SharedCmd::GetUsername(tx) => {
                                            let _ = tx.send($visitor.get_username().clone());
                                        }

                                    },
                                    Msg::State(cmd) => {
                                        if let Err(e) = $visitor.reduce(cmd, &mut state).await {
                                            error!(cause = %e, "Failed to process");
                                            break;
                                        }
                                    }
                                }
                            }
                            None => {
                                // EOF. The last PeerHandle has been dropped
                                info!(addr = ?state.connection.addr, "Drop peer actor");
                                break
                            }
                        }
                    }
                }
                Ok(None)
            }.instrument(tracing::info_span!("PeerActor", ?addr))
        };
    }
    let (r, w) = socket.split();
    let mut writer = FramedWrite::new(w, LinesCodec::new());
    let mut reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));

    macro_rules! run_state_handle {
        ($handle:expr, $connection:expr, $socket:expr ) => {
            async {
                loop {
                    tokio::select! {
                        msg = $socket.recv() => match msg {
                            Some(tcp_msg) => {
                               debug!(?tcp_msg);
                               writer.send(encode_message(tcp_msg)).await
                                    .context("Failed to send to the socket")?;
                            }
                            None => {
                                info!("Socket rx EOF");
                                break;
                            }
                        },

                        msg = reader.next::<Msg<client::SharedMsg, _>>() => match msg {
                            Some(client_msg) => {
                                let client_msg = client_msg?;
                                debug!(?client_msg);
                                match client_msg {
                                    Msg::Shared(msg) => match msg {
                                        client::SharedMsg::Ping => {
                                            $handle.ping().await.context("Peer Actor not responding")?;
                                            $connection.server.ping().await.context("Server Actor not responding")?;
                                            let _ = $connection
                                                .socket
                                                .as_ref()
                                                .map(|s| s.send(Msg::with(server::SharedMsg::Pong)));

                                        }
                                        client::SharedMsg::Logout => {
                                            let _ = $connection
                                                    .socket
                                                    .as_ref()
                                                    .map(|s| s.send(Msg::Shared(server::SharedMsg::Logout)));
                                                break;
                                        }
                                    }
                                    Msg::State(msg) => {
                                        $handle.reduce(
                                            msg,
                                           &mut $connection).await?;
                                    }
                                }
                            },
                            None => {
                                info!("Connection aborted..");
                                break
                            }
                        }
                    }
                }
                Ok::<(), anyhow::Error>(())
            }
        };
    }

    macro_rules! run_peer {
        ($start_block:block, $visitor:expr, $server:expr $(,$notify_server:expr)?) => {
            async {
                let (to_peer, mut peer_rx) = channel(MPSC_CHANNEL_CAPACITY);
                let mut handle = Handle::for_tx(to_peer);
                $(let _ = $notify_server.send(handle.clone());)?
                let (to_socket, mut socket_rx) = channel(MPSC_CHANNEL_CAPACITY);
                $start_block;
                let mut visitor = $visitor;
                let mut connection = Connection::new(addr, $server, to_socket);
                let peer = tokio::spawn({
                    let connection = connection.clone();
                    async move {
                        return run_state!(visitor, connection, peer_rx).await;
                    }
                });
                tokio::select!{
                    result =  run_state_handle!(handle, connection,  socket_rx)  => {
                        connection.server.drop_peer(addr).await;
                        result.context("PeerHandle error")?;

                        Ok(None)
                    },
                    new_state = peer => {
                        new_state.context("tokio::join Error")?
                    }
                }
            }
        };

    }

    let (intro, start_state) = done!(
        run_peer!({}, Intro::default(), intro_server.clone())
            .instrument(info_span!("Intro"))
            .await?
    );
    let result = async {
        match start_state {
            DoneByConnectionType::Reconnection(start_state) => match start_state {
                GameContext::Roles((old_peer_handle, NotifyServer(server, tx))) => {
                    let roles = old_peer_handle.take_peer().await?;
                    drop(old_peer_handle); // ! important
                    let (roles, NotifyServer(server, tx)) = done!(
                        run_peer!(
                            {
                                writer
                                    .send(encode_message(
                                        Msg::<server::SharedMsg, server::IntroMsg>::State(
                                            server::IntroMsg::ReconnectRoles(
                                                roles.state.selected_role,
                                            ),
                                        ),
                                    ))
                                    .await?;
                            },
                            roles,
                            server,
                            tx
                        )
                        .instrument(info_span!("Roles"))
                        .await?
                    );
                    // TODO it repeats
                    let game = Peer::<Game>::from(roles);
                    done!(run_peer!({
                            writer
                                .send(encode_message(
                                    Msg::<server::SharedMsg, server::RolesMsg>::State(
                                        server::RolesMsg::StartGame(
                                            crate::protocol::client::StartGame {
                                                abilities: game
                                                    .state
                                                    .abilities
                                                    .active_items()
                                                    .map(|i| i.map(|i| *i)),
                                                monsters: server.get_monsters().await?,
                                                role: game.state.get_role(),
                                            },
                                        ),
                                    ),
                                ))
                                .await?;

                    }, game, server, tx).await?);
                }
                GameContext::Game((old_peer_handle, NotifyServer(server, tx))) => {
                    let game = old_peer_handle.take_peer().await?;
                    drop(old_peer_handle); // ! important
                    done!(
                        run_peer!(
                            {
                                writer
                                    .send(encode_message(
                                        Msg::<server::SharedMsg, server::IntroMsg>::State(
                                            server::IntroMsg::ReconnectGame(
                                                crate::protocol::client::StartGame {
                                                    abilities: game
                                                        .state
                                                        .abilities
                                                        .active_items()
                                                        .map(|i| i.map(|i| *i)),
                                                    monsters: server.get_monsters().await?,
                                                    role: game.state.get_role(),
                                                },
                                            ),
                                        ),
                                    ))
                                    .await?;
                            },
                            game,
                            server,
                            tx
                        )
                        .instrument(info_span!("Game"))
                        .await?
                    );
                }
                _ => unreachable!("Reconnection in this context not allowed"),
            },
            DoneByConnectionType::New(NotifyServer(server, tx)) => {
                let (home, NotifyServer(server, tx)) = done!(
                    run_peer!(
                        {
                            writer
                                .send(encode_message(
                                    Msg::<server::SharedMsg, server::IntroMsg>::State(
                                        server::IntroMsg::StartHome,
                                    ),
                                ))
                                .await?;
                        },
                        Peer::<Home>::from(intro),
                        server,
                        tx
                    )
                    .instrument(info_span!("Home"))
                    .await?
                );

                let (roles, NotifyServer(server, tx)) = done!(
                    run_peer!(
                        {
                            writer
                                .send(encode_message(
                                    Msg::<server::SharedMsg, server::HomeMsg>::State(
                                        server::HomeMsg::StartRoles(None),
                                    ),
                                ))
                                .await?;
                        },
                        Peer::<Roles>::from(home),
                        server,
                        tx
                    )
                    .instrument(info_span!("Roles"))
                    .await?
                );

                let game = Peer::<Game>::from(roles);
                done!(
                    run_peer!(
                        {
                            writer
                                .send(encode_message(
                                    Msg::<server::SharedMsg, server::RolesMsg>::State(
                                        server::RolesMsg::StartGame(
                                            crate::protocol::client::StartGame {
                                                abilities: game
                                                    .state
                                                    .abilities
                                                    .active_items()
                                                    .map(|i| i.map(|i| *i)),
                                                monsters: server.get_monsters().await?,
                                                role: game.state.get_role(),
                                            },
                                        ),
                                    ),
                                ))
                                .await?;
                        },
                        game,
                        server,
                        tx
                    )
                    .instrument(info_span!("Game"))
                    .await?
                );
            }
        };
        Ok::<(), anyhow::Error>(())
    }
    .await;
    // remove peer_slot from the intro server after disconnection
    intro_server.drop_peer(addr).await;
    result
}

async fn close_peer<S, M>(state: &mut Connection<S>, peer: &Handle<Msg<SharedCmd, M>>)
where
    Handle<S>: SendSocketMessage,
{
    // should close but wait the socket writer EOF,
    // so it just drops socket tx
    let _ = peer.tx.send(Msg::Shared(SharedCmd::Close()));
    trace!("Close the socket tx on the PeerHandle side");
    state.socket = None;
}

#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<client::IntroMsg, &'a mut Connection<states::IntroCmd>>
    for IntroHandle
{
    async fn reduce(
        &mut self,
        msg: client::IntroMsg,
        state: &'a mut Connection<states::IntroCmd>,
    ) -> anyhow::Result<()> {
        use client::IntroMsg;

        use crate::protocol::server::LoginStatus;
        match msg {
            IntroMsg::Login(username) => {
                let status = state
                    .server
                    .login_player(state.addr, username, self.clone())
                    .await
                    .context("Login failed")?;
                info!(?status);
                state
                    .socket
                    .as_ref()
                    .unwrap()
                    .send(Msg::with(server::IntroMsg::LoginStatus(status)))
                    .await?;
                match status {
                    LoginStatus::Logged => (),
                    LoginStatus::Reconnected => {
                        state
                            .socket
                            .as_ref()
                            .expect("Must be open in this context")
                            .send(Msg::with(server::SharedMsg::ChatLog(
                                state.server.get_chat_log().await?.unwrap(),
                            )))
                            .await?;
                    }
                    _ => {
                        // connection fail
                        warn!(?status, "Login attempt rejected");
                        close_peer(state, self).await;
                    }
                }
            }
            IntroMsg::GetChatLog => {
                if state.server.is_peer_connected(state.addr).await? {
                    if let Some(s) = state.socket.as_ref() {
                        // TODO get_chat_log if log only in other servers
                        state
                            .server
                            .get_chat_log()
                            .await
                            .map(|log| async {
                                if let Some(log) = log {
                                    s.send(Msg::with(server::SharedMsg::ChatLog(log))).await
                                } else {
                                    Ok(())
                                }
                            })?
                            .await?;
                    }
                } else {
                    warn!("Client not logged but the ChatLog was requested");
                    close_peer(state, self).await;
                }
            }
            IntroMsg::Continue => {
                state.server.enter_game(state.addr).await;
            }
        }

        Ok(())
    }
}
macro_rules! broadcast_chat {
    ($sender:expr, $peer:expr, $server:expr, $msg:expr) => {{
        let msg = server::ChatLine::Text(format!("{}: {}", $peer.get_username().await?, $msg));
        $server.append_chat(msg.clone()).await;
        $server
            .broadcast($sender, Msg::with(server::SharedMsg::Chat(msg)))
            .await?;
    }};
}

#[async_trait::async_trait]
impl<'a>
    AsyncMessageReceiver<
        client::HomeMsg,
        &'a mut Connection<Msg<states::SharedCmd, states::HomeCmd>>,
    > for HomeHandle
{
    async fn reduce(
        &mut self,
        msg: client::HomeMsg,
        state: &'a mut Connection<Msg<states::SharedCmd, states::HomeCmd>>,
    ) -> anyhow::Result<()> {
        use client::HomeMsg;
        match msg {
            HomeMsg::Chat(msg) => broadcast_chat!(state.addr, self, state.server, msg),
            HomeMsg::EnterRoles => {
                state.server.start_roles(state.addr).await;
            }
        }
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a>
    AsyncMessageReceiver<
        client::RolesMsg,
        &'a mut Connection<Msg<states::SharedCmd, states::RolesCmd>>,
    > for RolesHandle
{
    async fn reduce(
        &mut self,
        msg: client::RolesMsg,
        state: &'a mut Connection<Msg<states::SharedCmd, states::RolesCmd>>,
    ) -> anyhow::Result<()> {
        use client::RolesMsg;
        match msg {
            RolesMsg::Chat(msg) => broadcast_chat!(state.addr, self, state.server, msg),
            RolesMsg::Select(role) => {
                state.server.select_role(state.addr, role).await;
            }
            RolesMsg::StartGame => {
                state.server.start_game(state.addr).await;
            }
        }
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a>
    AsyncMessageReceiver<
        client::GameMsg,
        &'a mut Connection<Msg<states::SharedCmd, states::GameCmd>>,
    > for GameHandle
{
    async fn reduce(
        &mut self,
        msg: client::GameMsg,
        state: &'a mut Connection<Msg<states::SharedCmd, states::GameCmd>>,
    ) -> anyhow::Result<()> {
        use crate::protocol::server::TurnResult;
        macro_rules! turn {
            ($($msg:ident)::*($ok:expr)) => {
               state.socket.as_ref()
                   .expect("Must be opened")
                   .send(Msg::with( $($msg)::*( {
                    let active_player =  state.server.get_active_player().await?;
                    if active_player == state.addr {
                        TurnResult::Ok($ok)
                    } else {
                        TurnResult::Err(state.server.get_peer_username(active_player).await?)
                    }
                }))).await?;
                state.server.broadcast_game_state(state.addr).await;
                self.sync_with_client().await;
            }
        }
        use client::GameMsg;
        match msg {
            GameMsg::Chat(msg) => broadcast_chat!(state.addr, self, state.server, msg),
            GameMsg::DropAbility(rank) => {
                turn!(server::GameMsg::DropAbility({
                    self.drop_ability(rank).await?;
                    rank
                }));
            }
            GameMsg::SelectAbility(rank) => {
                turn!(server::GameMsg::SelectAbility({
                    self.select_ability(rank).await?;
                    rank
                }));
            }
            GameMsg::Attack(card) => {
                turn!(server::GameMsg::Attack({
                    self.attack(card).await?;
                    card
                }));
            }
            GameMsg::Continue => {
                turn!(server::GameMsg::Continue({
                    self.continue_game().await?;
                }));
            }
        }
        Ok(())
    }
}

use crate::protocol::{GameContext, NextState};
impl NextState for Intro {
    type Next = GameContext<(), Home, Roles, Game>;
}
impl NextState for Home {
    type Next = Roles;
}
impl NextState for Roles {
    type Next = Game;
}
impl NextState for Game {
    type Next = Game;
}

trait DoneType {
    type Type;
}

enum DoneByConnectionType {
    New(NotifyServer<states::HomeHandle, HomeHandle>),
    Reconnection(
        GameContext<
            (),
            (),
            (RolesHandle, NotifyServer<states::RolesHandle, RolesHandle>),
            (GameHandle, NotifyServer<states::GameHandle, GameHandle>),
        >,
    ),
}

impl DoneType for Intro {
    type Type = DoneByConnectionType;
}
macro_rules! done_type {
    ($($state:ident,)*) => {
        $(
            impl DoneType for $state{
                type Type =  NotifyServer<
                            <<$state as NextState>::Next as AssociatedServerHandle>::Handle,
                            <<$state as NextState>::Next as AssociatedHandle>::Handle,
                        >;

            }
        )*

    }
}
done_type! {Home, Roles, Game,}

struct ReduceState<ContextState>
where
    ContextState: AssociatedServerHandle + DoneType,
    <ContextState as AssociatedServerHandle>::Handle: IncomingCommand,
    Handle<<<ContextState as AssociatedServerHandle>::Handle as IncomingCommand>::Cmd>:
        SendSocketMessage,
{
    connection:
        Connection<<<ContextState as AssociatedServerHandle>::Handle as IncomingCommand>::Cmd>,
    done: Option<oneshot::Sender<<ContextState as DoneType>::Type>>,
}

#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<IntroCmd, &'a mut ReduceState<Intro>> for Intro {
    async fn reduce(
        &mut self,
        msg: IntroCmd,
        state: &'a mut ReduceState<Intro>,
    ) -> anyhow::Result<()> {
        match msg {
            IntroCmd::EnterGame(server, tx) => {
                state
                    .done
                    .take()
                    .unwrap()
                    .send(DoneByConnectionType::New(NotifyServer(server, tx)))
                    .map_err(|_| anyhow::anyhow!("Failed to cancel"))?;
            }
            IntroCmd::ReconnectRoles(server, old_peer, tx) => {
                let _ = state
                    .done
                    .take()
                    .unwrap()
                    .send(DoneByConnectionType::Reconnection(GameContext::Roles((
                        old_peer,
                        NotifyServer(server, tx),
                    ))));
            }
            IntroCmd::ReconnectGame(server, old_peer, tx) => {
                let _ = state
                    .done
                    .take()
                    .unwrap()
                    .send(DoneByConnectionType::Reconnection(GameContext::Game((
                        old_peer,
                        NotifyServer(server, tx),
                    ))));
            }
            IntroCmd::SendTcp(msg) => {
                if let Some(s) = state.connection.socket.as_ref() {
                    s.send(msg).await?;
                }
            }
            IntroCmd::SetUsername(name) => {
                self.username = Some(name);
            }
        }
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<HomeCmd, &'a mut ReduceState<Home>> for Peer<Home> {
    async fn reduce(
        &mut self,
        msg: HomeCmd,
        state: &'a mut ReduceState<Home>,
    ) -> anyhow::Result<()> {
        match msg {
            HomeCmd::SendTcp(msg) => {
                state
                    .connection
                    .socket
                    .as_ref()
                    .expect("Open here")
                    .send(msg)
                    .await?;
            }
            HomeCmd::StartRoles(server, tx) => {
                let _ = state.done.take().unwrap().send(NotifyServer(server, tx));
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<RolesCmd, &'a mut ReduceState<Roles>> for Peer<Roles> {
    async fn reduce(
        &mut self,
        msg: RolesCmd,
        state: &'a mut ReduceState<Roles>,
    ) -> anyhow::Result<()> {
        match msg {
            RolesCmd::TakePeer(tx) => {
                take_mut::take(self, |this| {
                    let _ = tx.send(this);
                    Peer::<Roles> {
                        username: Username::default(),
                        state: Roles::default(),
                    }
                });
            }
            RolesCmd::SendTcp(msg) => {
                state
                    .connection
                    .socket
                    .as_ref()
                    .expect("Open here")
                    .send(msg)
                    .await?;
            }
            RolesCmd::StartGame(server, tx) => {
                let _ = state.done.take().unwrap().send(NotifyServer(server, tx));
            }
            RolesCmd::GetRole(tx) => {
                let _ = tx.send(self.state.selected_role);
            }
            RolesCmd::SelectRole(role, tx) => {
                self.state.selected_role = Some(role);
                let _ = tx.send(());
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<GameCmd, &'a mut ReduceState<Game>> for Peer<Game> {
    async fn reduce(
        &mut self,
        msg: GameCmd,
        state: &'a mut ReduceState<Game>,
    ) -> anyhow::Result<()> {
        match msg {
            GameCmd::TakePeer(tx) => {
                take_mut::take(self, |this| {
                    let _ = tx.send(this);
                    Peer::<Game> {
                        username: Username::default(),
                        state: Game {
                            selected_ability: None,
                            abilities: Stateble::with_items(AbilityDeck::new(Suit::Clubs)),
                            health: 0,
                        },
                    }
                });
            }
            GameCmd::SendTcp(msg) => {
                state
                    .connection
                    .socket
                    .as_ref()
                    .expect("Open here")
                    .send(msg)
                    .await?;
            }
            GameCmd::GetAbilities(tx) => {
                let _ = tx.send(self.state.abilities.active_items().map(|i| i.map(|i| *i)));
            }
            GameCmd::DropAbility(ability, tx) => {
                let i = self
                    .state
                    .abilities
                    .items
                    .ranks
                    .iter()
                    .position(|i| *i == ability)
                    .ok_or_else(|| anyhow::anyhow!("Bad ability to drop {:?}", ability))?;
                //let i = *self.state.abilities.items.ranks.iter().nth(i).unwrap();
                self.state.abilities.deactivate_item_by_index(i)?;

                state.connection.server.switch_to_next_player().await?;
                let _ = tx.send(());
            }
            GameCmd::SelectAbility(ability, tx) => {
                self.state.selected_ability = Some(
                    self.state
                        .abilities
                        .items
                        .ranks
                        .iter()
                        .position(|i| *i == ability)
                        .ok_or_else(|| anyhow::anyhow!("This ability not exists {:?}", ability))?,
                );
                state.connection.server.switch_to_next_player().await?;
                let _ = tx.send(());
            }

            GameCmd::Attack(monster, tx) => {
                #[allow(unused)]
                let ability = self
                    .state
                    .selected_ability
                    .ok_or_else(|| anyhow::anyhow!("Ability is not selected"))?;
                // TODO responce send to client
                // if monster.rank as u16 > self.abilities.items.ranks[ability] as u16 {
                //     return Err(anyhow!(
                //         "Wrong monster to attack = {:?}, ability = {:?}",
                //         monster.rank,
                //        self.abilities.items.ranks[ability]
                //     ));
                //} else {
                state.connection.server.drop_monster(monster).await??;
                trace!(
                    "Drop monster {:?}, all now {:?}",
                    monster,
                    state.connection.server.get_monsters().await
                );
                //}
                state.connection.server.switch_to_next_player().await?;
                let _ = tx.send(());
            }
            GameCmd::ContinueGame(tx) => {
                // TODO end of deck, handle game finish
                let _ = self.state.abilities.next_actives();
                state.connection.server.continue_game_cycle().await?;
                state.connection.server.switch_to_next_player().await?;
                let _ = tx.send(());
            }
            GameCmd::SyncWithClient() => {
                state
                    .connection
                    .socket
                    .as_ref()
                    .unwrap()
                    .send(Msg::State(server::GameMsg::UpdateGameData((
                        state.connection.server.get_monsters().await?,
                        self.state.abilities.active_items().map(|i| i.map(|i| *i)),
                    ))))
                    .await?;
            }
        }
        Ok(())
    }
}
