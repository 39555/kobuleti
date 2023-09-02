use std::net::SocketAddr;

use arrayvec::ArrayVec;
use futures::stream::StreamExt;
use tokio::sync::{mpsc, mpsc::channel, oneshot};
use tracing::{debug, error, info, info_span, trace, Instrument};

use super::{
    details::{Stateble, StatebleItem},
    peer,
    peer::PeerHandle,
    Answer, Handle, Rx, Tx, MPSC_CHANNEL_CAPACITY,
};
use crate::{
    game::{Card, Deck, Role},
    protocol::{
        client, server,
        server::{ChatLine, LoginStatus, PlayerId, SharedMsg, MAX_PLAYER_COUNT},
        AsyncMessageReceiver, GameContext, GameContextKind, GamePhaseKind, Msg, SendSocketMessage,
        Username,
    },
};

pub struct StartServer<S, Rx> {
    server: S,
    server_rx: Rx,
}
impl<S, Rx> StartServer<S, Rx> {
    pub fn new(server: S, rx: Rx) -> Self {
        StartServer {
            server,
            server_rx: rx,
        }
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

macro_rules! recv {
    ($result:expr) => {
        $result.expect("Peer actor is connected")
    };
}

#[tracing::instrument(skip_all, name = "Intro")]
pub async fn run_intro_server(
    StartServer {
        server: intro,
        server_rx: intro_rx,
    }: &mut StartServer<IntroServer, Rx<IntroCmd>>,
) -> anyhow::Result<()> {
    let (notify_new_server, mut rx) = channel::<ServerHandleByContext>(1);
    loop {
        let (cancel, mut cancel_rx) = oneshot::channel();
        let mut state = ServerState::new(cancel);
        tokio::select! {
            new_server = rx.recv() => {
                debug!(server = ?new_server.as_ref().map(|s| GameContextKind::from(&s.0))
                       , "Set new state");
                intro.game_server = new_server;
            }
            // run intro server
            next = async {
                loop{
                    tokio::select! {
                        new =  &mut cancel_rx => {
                            return Ok::<Option<SenderPeerId>, anyhow::Error>(Some(new?))
                        }
                        cmd = intro_rx.recv() => match cmd {
                            Some(cmd) => {
                                if let Err(e) = intro.reduce(cmd, &mut state).await {
                                    error!(cause = %e, "Failed to reduce IntroCmd");
                                }
                            }
                            None => {
                                info!("Server rx EOF");
                                break Ok(None);
                            }
                        }
                    }
                }
            } => {

                let sender = done!(next?);
                // start a new state machine
                if intro.game_server.is_none() {
                    trace!(?sender, "Start new server = Home");
                    let home = StartServer::async_from((sender, &mut *intro)).await;
                    tokio::spawn({
                        let notify_new_server = notify_new_server.clone();
                        async move {
                            if let Err(e) = run_server(home, notify_new_server).await {
                                error!("State server error = {:#}", e)
                            }
                        }
                    });
                // connect to existing state machine
                } else {
                    let server = intro.game_server.as_ref().unwrap();
                    trace!(?sender, server = ?GameContextKind::from(&server.0), "Connect to new server state");
                    let peer_slot = intro
                        .peers
                        .0
                        .iter_mut()
                        .find(|p| p.addr == sender)
                        .ok_or(PeerNotFound(sender))?;

                    // uses by Roles and Game
                    macro_rules! reconnection {
                        ($server:expr, $peer_slot:expr, $reconnect_function:ident) => {{
                            let name = recv!($peer_slot.peer.as_ref().unwrap().get_username().await);
                            let PeerSlot {
                                addr,
                                peer: old_handle,
                            } = recv!($server.get_peer_handle_by_username(name.clone()).await).expect(&format!(
                                "Peer with name = {} must exists until reconnection",
                                name
                            ));
                            let new_peer_handle = recv!(
                                $peer_slot
                                    .peer
                                    .as_ref()
                                    .unwrap()
                                    .$reconnect_function($server.clone(), old_handle.clone())
                                    .await
                            );
                            drop(old_handle);
                            recv!(
                                $server
                                    .reconnect_peer(addr, ($peer_slot.addr, new_peer_handle))
                                    .await
                            );
                        }};
                    }
                    match &server.0 {
                        GameContext::Home(h) => {
                            let handle =
                                recv!(peer_slot.peer.as_ref().unwrap().enter_game(h.clone()).await);
                            recv!(h.add_peer(peer_slot.addr, handle)
                                .await)
                                .expect("Peer slot must be empty");
                        }
                        // Reconnection here
                        GameContext::Roles(r) => {
                            reconnection!(r, peer_slot, reconnect_roles);
                        }
                        GameContext::Game(g) => {
                            reconnection!(g, peer_slot, reconnect_game);
                        }
                        s => unimplemented!("Connecting to state = {:?}"
                                            , GameContextKind::from(s)),
                    };
                    // !important; Disable the peer handle in Intro, but keep a peer_slot
                    peer_slot.peer = None;
                };
            }
        }
    }
}

use tokio::sync::oneshot::error::RecvError;

use super::details::actor_api;
actor_api! { // Shared
    impl<M> Handle<Msg<SharedCmd, M>> {
        pub async fn ping(&self) -> Result<(), RecvError>;
        pub async fn get_chat_log(&self) -> Result<Vec<server::ChatLine>, RecvError>;
        pub async fn append_chat(&self, line: server::ChatLine);
        pub async fn get_peer_id_by_name(&self, username: Username) -> Result<Option<PlayerId>, RecvError>;
        pub async fn get_peer_username(&self, whom: PlayerId) -> Result<Username, RecvError> ;
        pub async fn drop_peer(&self, whom: PlayerId) ;
        pub async fn shutdown(&self) -> Result<(), RecvError>;
    }
}

pub type IntroHandle = Handle<IntroCmd>;
pub type HomeHandle = Handle<Msg<SharedCmd, HomeCmd>>;
pub type RolesHandle = Handle<Msg<SharedCmd, RolesCmd>>;
pub type GameHandle = Handle<Msg<SharedCmd, GameCmd>>;

actor_api! { // Intro
    impl Handle<IntroCmd> {
        pub async fn ping(&self) -> Result<(), RecvError>;
        pub async fn login_player(&self, sender: SocketAddr, name: Username, handle: peer::IntroHandle) -> Result<LoginStatus, RecvError>;
        pub async fn is_peer_connected(&self, who: PlayerId) -> Result<bool, RecvError>;
        pub async fn enter_game(&self, who: PlayerId);
        pub async fn get_chat_log(&self) -> Result<Option<Vec<server::ChatLine>>, RecvError>;
        pub async fn drop_peer(&self, whom: PlayerId) ;
        pub async fn shutdown(&self) -> Result<(), RecvError>;

    }
}

actor_api! { // Home
    impl Handle<Msg<SharedCmd, HomeCmd>> {
        pub async fn add_peer(&self, id: PlayerId, handle: peer::HomeHandle) -> Result<Result<(), PeersCapacityError>, RecvError>;
        pub async fn broadcast(&self, sender: SocketAddr, message: Msg<SharedMsg, server::HomeMsg>) -> Result<(), RecvError>;
        pub async fn broadcast_to_all(&self, msg:  Msg<SharedMsg, server::HomeMsg>) -> Result<(), RecvError> ;
        pub async fn start_roles(&self, sender: PlayerId);

    }
}

actor_api! { // Roles
    impl Handle<Msg<SharedCmd, RolesCmd>> {
        pub async fn get_available_roles(&self) -> Result<[RoleStatus; Role::count()], RecvError>;
        pub async fn select_role(&self, sender: PlayerId, role: Role);
        pub async fn broadcast(&self, sender: SocketAddr, message:  Msg<SharedMsg, server::RolesMsg>) -> Result<(), RecvError>;
        pub async fn broadcast_to_all(&self, msg:  Msg<SharedMsg, server::RolesMsg>) -> Result<(), RecvError> ;
        pub async fn is_peer_connected(&self, who: PlayerId) -> Result<bool, RecvError> ;
        pub async fn start_game(&self, sender: PlayerId);
        pub async fn get_peer_handle_by_username(&self, whom: Username) -> Result<Option<PeerSlot<peer::RolesHandle>>, RecvError>;
        pub async fn reconnect_peer(&self, whom: PlayerId, new: (PlayerId, peer::RolesHandle)) -> Result<(), RecvError> ;
    }
}

actor_api! { // Game
    impl Handle<Msg<SharedCmd, GameCmd>> {
        pub async fn get_peer_handle_by_username(&self, whom: Username) -> Result<Option<PeerSlot<peer::GameHandle>>, RecvError>;
        pub async fn is_peer_connected(&self, who: PlayerId) -> Result<bool, RecvError> ;
        pub async fn broadcast(&self, sender: SocketAddr, message: Msg<SharedMsg, server::GameMsg>) -> Result<(), RecvError>;
        pub async fn broadcast_to_all(&self, msg:  Msg<SharedMsg, server::GameMsg>) -> Result<(), RecvError> ;
        pub async fn get_monsters(&self)          -> Result<[Option<Card>; 2], RecvError>;
        pub async fn get_active_player(&self)     -> Result<PlayerId, RecvError>;
        pub async fn get_game_phase(&self)        -> Result<GamePhaseKind, RecvError> ;
        pub async fn drop_monster(&self, monster: Card) -> Result<Result<(), super::details::DeactivateItemError>, RecvError> ;
        pub async fn switch_to_next_player(&self) -> Result<PlayerId, RecvError> ;
        pub async fn next_monsters(&self)         -> Result<Result<(), super::details::EndOfItems>, RecvError>;
        pub async fn continue_game_cycle(&self)   -> Result<(), RecvError>;
        pub async fn broadcast_game_state(&self, sender: PlayerId);
        pub async fn reconnect_peer(&self, whom: PlayerId, new: (PlayerId, peer::GameHandle))  -> Result<(), RecvError> ;
    }
}

use crate::protocol::With;
macro_rules! with {
    ($($src:ident,)*) => {
        $(
            impl With<$src, Msg<SharedCmd, $src>> for Msg<SharedCmd, $src>{
                #[inline]
                fn with(value: $src) -> Msg<SharedCmd, $src> {
                    Msg::State(value)
                }
            }
        )*
    }
}
with! {HomeCmd, RolesCmd, GameCmd,}
impl<M> With<SharedCmd, Msg<SharedCmd, M>> for Msg<SharedCmd, M> {
    #[inline]
    fn with(value: SharedCmd) -> Msg<SharedCmd, M> {
        Msg::Shared(value)
    }
}

impl With<IntroCmd, IntroCmd> for Msg<SharedCmd, IntroCmd> {
    #[inline]
    fn with(value: IntroCmd) -> IntroCmd {
        value
    }
}

async fn run_server(
    mut start_home: StartServer<HomeServer, Rx<Msg<SharedCmd, HomeCmd>>>,
    intro: Tx<ServerHandleByContext>,
) -> anyhow::Result<()> {
    {
        let (cancel, cancel_rx) = oneshot::channel();
        let state = ServerState::new(cancel);
        let _ = done!(
            run_state(&mut start_home, state, cancel_rx)
                .instrument(info_span!("Home"))
                .await?
        );
    }

    let mut start_roles =
        StartServer::async_from(ServerConverter::new(start_home.server, &intro)).await;
    {
        let (cancel, cancel_rx) = oneshot::channel();
        let state = ServerState::new(cancel);
        let _ = done!(
            run_state(&mut start_roles, state, cancel_rx)
                .instrument(info_span!("Roles"))
                .await?
        );
    }
    let mut start_game =
        StartServer::async_from(ServerConverter::new(start_roles.server, &intro)).await;
    {
        let (cancel, cancel_rx) = oneshot::channel();
        let state = ServerState::new(cancel);
        let _ = done!(
            run_state(&mut start_game, state, cancel_rx)
                .instrument(info_span!("Game"))
                .await?
        );
    }
    Ok(())
}

async fn run_state<Server, M, State>(
    StartServer {
        server: visitor,
        server_rx: rx,
    }: &mut StartServer<Server, Rx<Msg<SharedCmd, M>>>,
    mut state: State,
    mut cancel: oneshot::Receiver<SenderPeerId>,
) -> anyhow::Result<Option<SenderPeerId>>
where
    for<'a> Server: AsyncMessageReceiver<M, &'a mut State> + Send + ReceiveSharedCmd,
    M: Send + Sync + 'static,
    for<'a> GameContextKind: From<&'a Server>,
{
    loop {
        tokio::select! {
            new =  &mut cancel => {
                return Ok(Some(new?))

            }
            cmd = rx.recv() => match cmd {
                Some(cmd) => {
                    match cmd {
                        Msg::Shared(msg) => {
                           if let Err(e) = visitor.reduce_shared_cmd(msg).await {
                                error!(cause = %e, "Failed to reduce SharedCmd");
                            }
                        },
                        Msg::State(msg) => {
                            if let Err(e) = visitor.reduce(msg, &mut state).await {
                                error!(cause = %e, "Failed to reduce StateCmd");
                            }
                        }
                    }
                }
                None => {
                    info!("Server rx EOF");
                    break
                }
            }
        }
    }

    Ok(None)
}

struct ServerHandleByContext(GameContext<(), HomeHandle, RolesHandle, GameHandle>);
#[derive(Default)]
pub struct IntroServer {
    peers: Room<Option<peer::IntroHandle>>,
    game_server: Option<ServerHandleByContext>,
}

#[derive(Debug)]
struct StateServer<T> {
    chat: Vec<ChatLine>,
    peers: T,
}

type HomeServer = StateServer<Room<peer::HomeHandle>>;
type RolesServer = StateServer<Room<(PeerStatus, peer::RolesHandle)>>;

use crate::protocol::server::MONSTERS_PER_LINE_COUNT;
type GameState = StateServer<Stateble<Room<(PeerStatus, peer::GameHandle)>, 1>>;
struct GameServer {
    state: GameState,
    monsters: Stateble<Deck, MONSTERS_PER_LINE_COUNT>,
    phase: GamePhaseKind,
}

impl StatebleItem for Room<(PeerStatus, peer::GameHandle)> {
    type Item = PeerSlot<(PeerStatus, peer::GameHandle)>;
}
impl AsRef<[PeerSlot<(PeerStatus, peer::GameHandle)>]> for Room<(PeerStatus, peer::GameHandle)> {
    fn as_ref(&self) -> &[PeerSlot<(PeerStatus, peer::GameHandle)>] {
        self.0.as_ref()
    }
}
impl PartialEq for PeerSlot<(PeerStatus, peer::GameHandle)> {
    fn eq(&self, other: &Self) -> bool {
        self.addr == other.addr
    }
}
impl Eq for PeerSlot<(PeerStatus, peer::GameHandle)> {}

impl AsRef<[Card]> for Deck {
    fn as_ref(&self) -> &[Card] {
        &self.cards
    }
}
impl StatebleItem for Deck {
    type Item = Card;
}

use crate::protocol::details::impl_GameContextKind_from_state;
impl_GameContextKind_from_state! {IntroHandle => Intro, HomeHandle => Home, RolesHandle => Roles, GameHandle => Game,}
impl_GameContextKind_from_state! {IntroServer => Intro, HomeServer => Home, RolesServer => Roles, GameServer => Game,}
impl_GameContextKind_from_state! {StateServer<Stateble<Room<(PeerStatus, peer::GameHandle)>, 1>> => Game,}

impl IntroServer {
    #[tracing::instrument(skip_all, fields(p = %sender, name = %username))]
    async fn login_player(
        &mut self,
        sender: SocketAddr,
        username: Username,
        handle: peer::IntroHandle,
    ) -> LoginStatus {
        if self.peers.0.is_full() {
            info!("PlayerLimit");
            return LoginStatus::PlayerLimit;
        }

        if futures::stream::iter(
            self.peers
                .0
                .iter_mut()
                .filter(|p| p.peer.is_none() && p.addr == sender),
        )
        .any(|p| async {
            debug!(peer = %p.addr);
            recv!(p.peer.as_ref().unwrap().get_username().await) == username
        })
        .instrument(tracing::debug_span!("LoginIntro"))
        .await
        {
            info!("AlreadyLogged");
            return LoginStatus::AlreadyLogged;
        }
        if self.game_server.is_some() {
            macro_rules! is_logged_in {
                ($server:expr) => {
                    futures::future::OptionFuture::from(
                        recv!($server.get_peer_id_by_name(username.clone()).await)
                            .map(|p| async move { recv!($server.is_peer_connected(p).await) }),
                    )
                    .await
                    .unwrap_or(false)
                };
            }
            if let Err(status) = async {
                debug!(server=?GameContextKind::from(&self.game_server.as_ref().unwrap().0));
                if match &self.game_server.as_ref().unwrap().0 {
                    GameContext::Home(h) => {
                        recv!(h.get_peer_id_by_name(username.clone()).await).is_some()
                    }
                    GameContext::Roles(r) => {
                        is_logged_in!(r)
                    }
                    GameContext::Game(g) => is_logged_in!(g),
                    _ => unreachable!(),
                } {
                    Err(LoginStatus::AlreadyLogged)
                } else {
                    // Logged. fallthrough
                    Ok(LoginStatus::Logged)
                }
            }
            .instrument(tracing::debug_span!("LoginStateServer"))
            .await
            {
                return status;
            }
        }
        handle.set_username(username).await;
        self.peers.0.push(PeerSlot::new(sender, Some(handle)));
        info!("Logged");
        LoginStatus::Logged
    }
}

#[async_trait::async_trait]
trait ReceiveSharedCmd {
    async fn reduce_shared_cmd(&mut self, msg: SharedCmd) -> anyhow::Result<()>;
}

impl<T> From<ChatLine> for Msg<server::SharedMsg, T> {
    #[inline]
    fn from(value: ChatLine) -> Self {
        Msg::Shared(server::SharedMsg::Chat(value))
    }
}

#[async_trait::async_trait]
impl ReceiveSharedCmd for GameServer {
    #[inline]
    async fn reduce_shared_cmd(&mut self, msg: SharedCmd) -> anyhow::Result<()> {
        self.state.reduce_shared_cmd(msg).await
    }
}

trait GetPeers {
    type Peer;
    fn peers(&self) -> &Room<Self::Peer>;
    fn peers_mut(&mut self) -> &mut Room<Self::Peer>;
}

impl GetPeers for GameState {
    type Peer = (PeerStatus, Handle<Msg<peer::SharedCmd, peer::GameCmd>>);
    #[inline]
    fn peers(&self) -> &Room<(PeerStatus, Handle<Msg<peer::SharedCmd, peer::GameCmd>>)> {
        &self.peers.items
    }
    #[inline]
    fn peers_mut(
        &mut self,
    ) -> &mut Room<(PeerStatus, Handle<Msg<peer::SharedCmd, peer::GameCmd>>)> {
        &mut self.peers.items
    }
}

impl GetPeers for GameServer {
    type Peer = (PeerStatus, Handle<Msg<peer::SharedCmd, peer::GameCmd>>);
    #[inline]
    fn peers(&self) -> &Room<(PeerStatus, Handle<Msg<peer::SharedCmd, peer::GameCmd>>)> {
        &self.state.peers.items
    }
    #[inline]
    fn peers_mut(
        &mut self,
    ) -> &mut Room<(PeerStatus, Handle<Msg<peer::SharedCmd, peer::GameCmd>>)> {
        &mut self.state.peers.items
    }
}
impl GetPeers for RolesServer {
    type Peer = (PeerStatus, Handle<Msg<peer::SharedCmd, peer::RolesCmd>>);
    #[inline]
    fn peers(&self) -> &Room<Self::Peer> {
        &self.peers
    }
    #[inline]
    fn peers_mut(&mut self) -> &mut Room<Self::Peer> {
        &mut self.peers
    }
}
impl GetPeers for HomeServer {
    type Peer = Handle<Msg<peer::SharedCmd, peer::HomeCmd>>;
    #[inline]
    fn peers(&self) -> &Room<Self::Peer> {
        &self.peers
    }
    #[inline]
    fn peers_mut(&mut self) -> &mut Room<Self::Peer> {
        &mut self.peers
    }
}

trait DropPeer {
    fn drop_peer(&mut self, whom: PlayerId) -> Result<(), PeerNotFound>;
}

impl DropPeer for HomeServer {
    fn drop_peer(&mut self, whom: PlayerId) -> Result<(), PeerNotFound> {
        trace!("{} Drop handle in Home server", whom);
        let p = self
            .peers()
            .0
            .iter()
            .position(|p| p.addr == whom)
            .ok_or(PeerNotFound(whom))?;
        self.peers_mut().0.swap_pop(p);
        Ok(())
    }
}

// Roles server and Game server
impl<T, H> DropPeer for StateServer<T>
where
    StateServer<T>: GetPeers<Peer = (PeerStatus, H)>,
{
    fn drop_peer(&mut self, whom: PlayerId) -> Result<(), PeerNotFound> {
        trace!(addr = ?whom, status = ?PeerStatus::Offline, "Drop peer");
        self.peers_mut()
            .0
            .iter_mut()
            .find(|p| p.addr == whom)
            .ok_or(PeerNotFound(whom))?
            .peer
            .0 = PeerStatus::Offline;
        Ok(())
    }
}
// Home, Roles server and Game server
#[async_trait::async_trait]
impl<T> ReceiveSharedCmd for StateServer<T>
where
    T: Send + Sync,
    StateServer<T>: Send + Sync + GetPeers + DropPeer + Broadcast,
    for<'a> GameContextKind: From<&'a StateServer<T>>,
    PeerSlot<<StateServer<T> as GetPeers>::Peer>: GetPeerHandle + Send + Sync,

    <PeerSlot<<StateServer<T> as GetPeers>::Peer> as GetPeerHandle>::State: Send + Sync,

    PeerSlot<<StateServer<T> as GetPeers>::Peer>: TcpSender + Send + Sync,

    <PeerSlot<<StateServer<T> as GetPeers>::Peer> as TcpSender>::Sender:
        SendSocketMessage + Send + Sync,

    <StateServer<T> as Broadcast>::Msg: Clone + std::fmt::Debug + From<ChatLine> + Send + Sync,
{
    async fn reduce_shared_cmd(&mut self, msg: SharedCmd) -> anyhow::Result<()> {
        match msg {
            SharedCmd::Ping(tx) => {
                let _ = tx.send(());
            }
            SharedCmd::GetPeerIdByName(name, tx) => {
                let _ = tx.send({
                    let mut id = None;
                    for p in self.peers().0.iter() {
                        if recv!(p.get_peer_handle().get_username().await) == name {
                            id = Some(p.addr)
                        }
                    }
                    id
                });
            }
            SharedCmd::DropPeer(id) => {
                if let Ok(p) = self.peers().get_peer(id) {
                    trace!("{} Drop a peer handle", id);
                    self.broadcast(
                        id,
                        <StateServer<T> as Broadcast>::Msg::from(ChatLine::Disconnection(recv!(
                            p.get_peer_handle().get_username().await
                        ))),
                    )
                    .await;
                    self.drop_peer(id).expect("Drop if peer is logged");
                }
            }
            SharedCmd::Shutdown(tx) => {
                info!("Shutting down");
                self.peers_mut().shutdown();
                let _ = tx.send(());
            }
            SharedCmd::GetChatLog(tx) => {
                let _ = tx.send(self.chat.clone());
            }
            SharedCmd::AppendChat(line) => {
                self.chat.push(line);
            }
            SharedCmd::GetPeerUsername(id, tx) => {
                let _ = tx.send(recv!(
                    self.peers()
                        .get_peer(id)
                        .unwrap()
                        .get_peer_handle()
                        .get_username()
                        .await
                ));
            }
        };
        Ok(())
    }
}

pub struct ServerConverter<'a, S> {
    server: S,
    intro: &'a mpsc::Sender<ServerHandleByContext>,
}
impl<'a, S> ServerConverter<'a, S> {
    #[inline]
    fn new(server: S, intro: &'a mpsc::Sender<ServerHandleByContext>) -> Self {
        ServerConverter { server, intro }
    }
}

#[async_trait::async_trait]
pub trait AsyncFrom<T> {
    async fn async_from(value: T) -> Self
    where
        T: 'async_trait;
}

pub type SenderPeerId = PlayerId;
#[async_trait::async_trait]
impl<'a> AsyncFrom<(SenderPeerId, &'a mut IntroServer)>
    for StartServer<HomeServer, Rx<Msg<SharedCmd, HomeCmd>>>
where
    (SenderPeerId, &'a mut IntroServer, Tx<ServerHandleByContext>): 'a,
{
    async fn async_from((sender, intro): (SenderPeerId, &'a mut IntroServer)) -> Self {
        let peer_slot = intro
            .peers
            .0
            .iter_mut()
            .find(|p| p.addr == sender)
            .expect("A peer in the Intro state");
        let (tx, rx) = channel::<Msg<SharedCmd, HomeCmd>>(MPSC_CHANNEL_CAPACITY);
        let home_handle = HomeHandle::for_tx(tx);
        let peer_handle = recv!(
            peer_slot
                .peer
                .as_ref()
                .unwrap()
                .enter_game(home_handle.clone())
                .await
        );
        let mut server = HomeServer {
            peers: Default::default(),
            chat: Default::default(),
        };
        server
            .peers
            .0
            .push(PeerSlot::new(peer_slot.addr, peer_handle));
        // peer moved to the home server
        peer_slot.peer = None;
        intro.game_server = Some(ServerHandleByContext(GameContext::Home(home_handle)));
        StartServer::new(server, rx)
    }
}

#[async_trait::async_trait]
impl<'a> AsyncFrom<ServerConverter<'a, HomeServer>>
    for StartServer<RolesServer, Rx<Msg<SharedCmd, RolesCmd>>>
{
    async fn async_from(home: ServerConverter<'a, HomeServer>) -> Self {
        let (tx, rx) = channel::<Msg<SharedCmd, RolesCmd>>(MPSC_CHANNEL_CAPACITY);
        let handle = RolesHandle::for_tx(tx);
        home.intro
            .send(ServerHandleByContext::from(handle.clone()))
            .await
            .expect("Must notify the Intro state");
        let peers: ArrayVec<PeerSlot<(PeerStatus, peer::RolesHandle)>, MAX_PLAYER_COUNT> =
            futures::future::join_all(home.server.peers.0.iter().map(|p| async {
                let handle = handle.clone();
                let peer_handle = recv!(p.peer.start_roles(handle).await);
                PeerSlot::<(PeerStatus, peer::RolesHandle)> {
                    addr: p.addr,
                    peer: (PeerStatus::Online, peer_handle),
                }
            }))
            .await
            .into_iter()
            .collect();
        StartServer::new(
            RolesServer {
                chat: home.server.chat,
                peers: Room::<(PeerStatus, peer::RolesHandle)>(peers),
            },
            rx,
        )
    }
}

#[async_trait::async_trait]
impl<'a> AsyncFrom<ServerConverter<'a, RolesServer>>
    for StartServer<GameServer, Rx<Msg<SharedCmd, GameCmd>>>
{
    async fn async_from(mut roles: ServerConverter<'a, RolesServer>) -> Self {
        let (tx, rx) = channel::<Msg<SharedCmd, GameCmd>>(MPSC_CHANNEL_CAPACITY);
        let handle = GameHandle::for_tx(tx);
        roles
            .intro
            .send(ServerHandleByContext::from(handle.clone()))
            .await
            .expect("Must notify the Intro state");
        let peers: ArrayVec<PeerSlot<(PeerStatus, peer::GameHandle)>, MAX_PLAYER_COUNT> =
            futures::future::join_all(roles.server.peers.0.iter_mut().map(|p| async {
                let peer_handle = recv!(p.peer.1.start_game(handle.clone()).await);
                PeerSlot::<(PeerStatus, peer::GameHandle)> {
                    addr: p.addr,
                    peer: (PeerStatus::Online, peer_handle),
                }
            }))
            .await
            .into_iter()
            .collect();

        use crate::game::Deckable;
        let mut monsters = Deck::default();
        monsters.shuffle();
        let game_server = GameServer {
            state: StateServer {
                chat: roles.server.chat,
                peers: Stateble::with_items(Room::<(PeerStatus, peer::GameHandle)>(peers)),
            },
            monsters: Stateble::<Deck, MONSTERS_PER_LINE_COUNT>::with_items(monsters),
            phase: GamePhaseKind::default(),
        };

        let p = game_server.state.peers.active_items()[0].expect("Always active one");
        use crate::protocol::TurnStatus;
        recv!(
            p.send_tcp(Msg::with(server::GameMsg::Turn(TurnStatus::Ready(
                game_server.phase,
            ))))
            .await
        );
        game_server
            .broadcast(p.addr, Msg::with(server::GameMsg::Turn(TurnStatus::Wait)))
            .await;
        StartServer::new(game_server, rx)
    }
}

macro_rules! from {
    ($($state:ident,)* => $dst:ty) => {
        $(
            paste::item!{
            impl From<[< $state Handle>]> for ServerHandleByContext{
                fn from(value: [< $state Handle>]) -> Self {
                    Self(GameContext::$state(value))
                }
            }
            }
         )*

    }
}
from! {
    Home, Roles, Game, => ServerHandleByContext
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeerStatus {
    Online,
    Offline,
}
#[derive(Debug)]
pub struct PeerSlot<T> {
    addr: PlayerId,
    peer: T,
}

trait GetPeerHandle {
    type State;
    fn get_peer_handle(&self) -> &PeerHandle<Self::State>;
}
impl<T> GetPeerHandle for PeerSlot<PeerHandle<T>> {
    type State = T;
    #[inline]
    fn get_peer_handle(&self) -> &PeerHandle<Self::State> {
        &self.peer
    }
}
impl<T> GetPeerHandle for PeerSlot<Option<PeerHandle<T>>> {
    type State = T;
    #[inline]
    fn get_peer_handle(&self) -> &PeerHandle<Self::State> {
        &self.peer.as_ref().expect("Requested None Peer")
    }
}
impl<T, S> GetPeerHandle for PeerSlot<(S, PeerHandle<T>)> {
    type State = T;
    #[inline]
    fn get_peer_handle(&self) -> &PeerHandle<Self::State> {
        &self.peer.1
    }
}

impl<T> PeerSlot<T> {
    #[inline]
    fn new(addr: PlayerId, peer_handle: T) -> Self {
        PeerSlot {
            addr,
            peer: peer_handle,
        }
    }
}
impl<T> Clone for PeerSlot<T>
where
    T: Clone,
{
    #[inline]
    fn clone(&self) -> Self {
        PeerSlot {
            addr: self.addr,
            peer: self.peer.clone(),
        }
    }
}

#[derive(Debug)]
#[repr(transparent)]
struct Room<T>(pub ArrayVec<PeerSlot<T>, MAX_PLAYER_COUNT>);

impl<T> Default for Room<T> {
    fn default() -> Self {
        Room(Default::default())
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Room is full")]
pub struct PeersCapacityError;

#[derive(thiserror::Error, Debug)]
#[error("Peer with addr {0} not found in the room")]
pub struct PeerNotFound(pub PlayerId);

impl<T> Room<T> {
    #[inline]
    fn get_peer(&self, addr: SocketAddr) -> Result<&PeerSlot<T>, PeerNotFound> {
        self.0
            .iter()
            .find(|p| p.addr == addr)
            .ok_or(PeerNotFound(addr))
    }
}
impl<T> Room<T> {
    fn shutdown(&mut self) {
        trace!("Drop all peers");
        // if it is a last handle, peer actor will shutdown
        self.0.clear();
    }
}

struct ServerState<Server> {
    cancel: Option<oneshot::Sender<SenderPeerId>>,
    __: std::marker::PhantomData<Server>,
}
impl<T> ServerState<T> {
    fn new(cancel: oneshot::Sender<SenderPeerId>) -> Self {
        ServerState {
            cancel: Some(cancel),
            __: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<IntroCmd, &'a mut ServerState<IntroServer>> for IntroServer {
    async fn reduce(
        &mut self,
        msg: IntroCmd,
        state: &'a mut ServerState<IntroServer>,
    ) -> anyhow::Result<()> {
        match msg {
            IntroCmd::Ping(tx) => {
                let _ = tx.send(());
            }
            IntroCmd::LoginPlayer(sender, username, handle, tx) => {
                let _ = tx.send(self.login_player(sender, username, handle).await);
            }
            IntroCmd::EnterGame(sender) => {
                if let Err(e) = state.cancel.take().unwrap().send(sender) {
                    error!(cause = %e, "Failed to cancel");
                }
            }
            IntroCmd::IsPeerConnected(sender, tx) => {
                let _ = tx.send(self.peers.get_peer(sender).is_ok());
            }
            IntroCmd::DropPeer(id) => {
                trace!("{} Drop a peer handle", id);
                // ignore not logged peers
                if let Some(p) = self.peers.0.iter().position(|p| p.addr == id) {
                    self.peers.0.swap_pop(p);
                }
                // Drop state server if all peers disconnected
                if self.peers.0.is_empty() {
                    if let Some(s) = &self.game_server {
                        match &s.0 {
                            GameContext::Home(h) => recv!(h.shutdown().await),
                            GameContext::Roles(r) => recv!(r.shutdown().await),
                            GameContext::Game(h) => recv!(h.shutdown().await),
                            _ => unreachable!(),
                        }
                    }
                    self.game_server = None;
                }
            }
            IntroCmd::Shutdown(tx) => {
                info!("Shutting down");
                self.peers.shutdown();
                self.game_server = None;
                let _ = tx.send(());
            }
            IntroCmd::GetChatLog(tx) => {
                _ = tx.send({
                    if self.game_server.is_some() {
                        Some(match &self.game_server.as_ref().unwrap().0 {
                            GameContext::Home(h) => recv!(h.get_chat_log().await),
                            GameContext::Roles(r) => recv!(r.get_chat_log().await),
                            GameContext::Game(g) => recv!(g.get_chat_log().await),
                            _ => unreachable!(),
                        })
                    } else {
                        None
                    }
                });
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<HomeCmd, &'a mut ServerState<HomeServer>> for HomeServer {
    async fn reduce(
        &mut self,
        msg: HomeCmd,
        state: &'a mut ServerState<HomeServer>,
    ) -> anyhow::Result<()> {
        match msg {
            HomeCmd::AddPeer(id, peer, tx) => {
                let _ = tx.send(
                    self.peers
                        .0
                        .try_push(PeerSlot::new(id, peer))
                        .map_err(|_| PeersCapacityError),
                );
            }
            HomeCmd::Broadcast(sender, msg, tx) => {
                self.broadcast(sender, msg).await;
                let _ = tx.send(());
            }
            HomeCmd::BroadcastToAll(msg, tx) => {
                self.broadcast_to_all(msg).await;
                let _ = tx.send(());
            }
            HomeCmd::StartRoles(sender) => {
                if self.peers.0.is_full() {
                    state.cancel.take().unwrap().send(sender).expect("Done");
                }
            }
        };
        Ok(())
    }
}

macro_rules! get_peer_by_name {
    ($iterable:expr, $name:expr) => {{
        let mut peer = None;
        for p in $iterable {
            if recv!(p.peer.1.get_username().await) == $name {
                peer = Some(PeerSlot {
                    addr: p.addr,
                    peer: p.peer.1.clone(),
                });
                break;
            }
        }
        peer
    }};
}

#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<RolesCmd, &'a mut ServerState<RolesServer>> for RolesServer {
    async fn reduce(
        &mut self,
        msg: RolesCmd,
        state: &'a mut ServerState<RolesServer>,
    ) -> anyhow::Result<()> {
        match msg {
            RolesCmd::Broadcast(sender, msg, tx) => {
                self.broadcast(sender, msg).await;
                let _ = tx.send(());
            }
            RolesCmd::BroadcastToAll(msg, tx) => {
                self.broadcast_to_all(msg).await;
                let _ = tx.send(());
            }
            RolesCmd::SelectRole(sender, role) => {
                let status = self.set_role_for_peer(sender, role).await;
                debug!(?sender, ?role, ?status, "Select");
                let peer = self.peers.get_peer(sender).expect("Must exists");
                peer.peer
                    .1
                    .send_tcp(Msg::with(server::RolesMsg::SelectedStatus(status)))
                    .await;
                if let Ok(r) = &status {
                    self.broadcast(
                        sender,
                        Msg::with(server::SharedMsg::Chat(server::ChatLine::GameEvent(
                            format!("{} select {:?}", recv!(peer.peer.1.get_username().await), r),
                        ))),
                    )
                    .await;
                    self.broadcast_to_all(Msg::with(server::RolesMsg::AvailableRoles(
                        self.collect_roles().await,
                    )))
                    .await;
                };
            }
            RolesCmd::IsPeerConnected(sender, tx) => {
                let _ = tx.send(
                    self.peers
                        .0
                        .iter()
                        .any(|p| p.addr == sender && p.peer.0 == PeerStatus::Online),
                );
            }
            RolesCmd::GetAvailableRoles(tx) => {
                let _ = tx.send(self.collect_roles().await);
            }
            RolesCmd::GetPeerHandleByUsername(name, tx) => {
                let _ = tx.send(get_peer_by_name!(self.peers.0.iter(), name));
            }
            RolesCmd::StartGame(sender) => {
                if self.are_all_have_roles().await {
                    state
                        .cancel
                        .take()
                        .unwrap()
                        .send(sender)
                        .expect("Must not be dropped");
                }
                // TODO else return Status
            }
            RolesCmd::ReconnectPeer(whom, (addr, peer), tx) => {
                let roles = self.collect_roles().await;
                let p = self
                    .peers_mut()
                    .0
                    .iter_mut()
                    .find(|p| p.addr == whom)
                    .ok_or(PeerNotFound(whom))
                    .expect("Reconnect only existing peer");
                *p = PeerSlot {
                    addr,
                    peer: (PeerStatus::Online, peer),
                };
                recv!(
                    p.send_tcp(Msg::with(server::RolesMsg::AvailableRoles(roles)))
                        .await
                );
                debug!(all_peers = ?self.peers().0);
                let _ = tx.send(());
            }
        }
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<GameCmd, &'a mut ServerState<GameServer>> for GameServer {
    async fn reduce(
        &mut self,
        msg: GameCmd,
        _state: &'a mut ServerState<GameServer>,
    ) -> anyhow::Result<()> {
        match msg {
            GameCmd::Broadcast(sender, msg, tx) => {
                self.broadcast(sender, msg).await;
                let _ = tx.send(());
            }
            GameCmd::BroadcastToAll(msg, tx) => {
                self.broadcast_to_all(msg).await;
                let _ = tx.send(());
            }
            GameCmd::GetPeerHandleByUsername(name, tx) => {
                let _ = tx.send(get_peer_by_name!(self.state.peers.items.0.iter(), name));
            }
            GameCmd::IsPeerConnected(sender, tx) => {
                let _ = tx.send(
                    self.state
                        .peers
                        .items
                        .0
                        .iter()
                        .any(|p| p.addr == sender && p.peer.0 == PeerStatus::Online),
                );
            }
            GameCmd::GetMonsters(tx) => {
                let _ = tx.send(self.monsters.active_items().map(|i| i.map(|i| *i)));
            }

            GameCmd::GetActivePlayer(tx) => {
                let _ = tx.send(
                    self.state.peers.active_items()[0]
                        .as_ref()
                        .expect("Always have one active player")
                        .addr,
                );
            }
            GameCmd::GetGamePhase(tx) => {
                let _ = tx.send(self.phase);
            }
            GameCmd::DropMonster(monster, tx) => {
                let _ = tx.send(self.monsters.deactivate_item(&monster));
            }
            GameCmd::SwitchToNextPlayer(tx) => {
                let next_player = self.switch_to_next_player();
                use crate::protocol::TurnStatus;
                self.broadcast(
                    next_player,
                    Msg::with(server::GameMsg::Turn(TurnStatus::Wait)),
                )
                .await;
                recv!(
                    self.state
                        .peers
                        .items
                        .get_peer(next_player)
                        .expect("Must exists")
                        .send_tcp(Msg::with(server::GameMsg::Turn(TurnStatus::Ready(
                            self.phase,
                        ))))
                        .await
                );
                let _ = tx.send(next_player);
            }
            GameCmd::NextMonsters(tx) => {
                let _ = tx.send(self.monsters.next_actives());
            }
            GameCmd::ContinueGameCycle(tx) => {
                // TODO end of game here
                let _ = tx.send(());
            }
            GameCmd::BroadcastGameState(sender) => {
                futures::stream::iter(self.state.peers.items.0.iter().filter(|p| p.addr != sender))
                    .for_each_concurrent(MAX_PLAYER_COUNT, |p| async {
                        let _ = p.peer.1.sync_with_client().await;
                    })
                    .await;
            }
            GameCmd::ReconnectPeer(whom, (addr, peer), tx) => {
                let p = self
                    .peers_mut()
                    .0
                    .iter_mut()
                    .find(|p| p.addr == whom)
                    .ok_or(PeerNotFound(whom))
                    .expect("Remove existing peer");
                *p = PeerSlot {
                    addr,
                    peer: (PeerStatus::Online, peer),
                };
                let _ = tx.send(());
            }
        };

        Ok(())
    }
}

use crate::protocol::{server::SelectRoleError, RoleStatus};

impl RolesServer {
    #[tracing::instrument(skip(self))]
    async fn collect_roles(&self) -> [RoleStatus; Role::count()] {
        let peer_roles =
            futures::future::join_all(self.peers.0.iter().map(|p| p.peer.1.get_role()))
                .await
                .into_iter()
                .map(|p| recv!(p))
                .collect::<Vec<_>>();
        Role::all().map(
            |r| match peer_roles.iter().any(|p| p.is_some_and(|pr| pr == r)) {
                false => RoleStatus::Available(r),
                true => RoleStatus::NotAvailable(r),
            },
        )
    }
    async fn set_role_for_peer(
        &self,
        sender: PlayerId,
        role: Role,
    ) -> Result<Role, SelectRoleError> {
        for p in self.peers.0.iter() {
            if recv!(p.peer.1.get_role().await).is_some_and(|r| r == role) {
                return Err(if p.addr != sender {
                    SelectRoleError::Busy
                } else {
                    SelectRoleError::AlreadySelected
                });
            }
        }
        let p = &self.peers.get_peer(sender).expect("Must exists").peer;
        recv!(p.1.select_role(role).await);
        Ok(role)
    }
    async fn are_all_have_roles(&self) -> bool {
        futures::stream::iter(self.peers.0.iter())
            .all(|p| async move { recv!(p.peer.1.get_role().await).is_some() })
            .await
    }
}

impl GameServer {
    fn switch_to_next_player(&mut self) -> PlayerId {
        match self.phase {
            GamePhaseKind::DropAbility => {
                // TODO errorkind
                tracing::info!("Current active {:?}", self.state.peers.active_items()[0]);
                self.state
                    .peers
                    .deactivate_item_by_index(self.state.peers.actives[0].unwrap_index())
                    .expect("Must drop");
                let _ = self.state.peers.next_actives().map_err(|eof| {
                    self.phase = GamePhaseKind::SelectAbility;
                    self.state.peers.repeat_after_eof(eof)
                });
                tracing::info!("Next active {:?}", self.state.peers.active_items()[0]);
            }
            GamePhaseKind::SelectAbility => {
                self.phase = GamePhaseKind::AttachMonster;
            }
            GamePhaseKind::AttachMonster => {
                self.state
                    .peers
                    .deactivate_item_by_index(self.state.peers.actives[0].unwrap_index())
                    .expect("Must drop");
                self.phase = self.state.peers.next_actives().map_or_else(
                    |eof| {
                        self.state.peers.repeat_after_eof(eof);
                        GamePhaseKind::Defend
                    },
                    |_| GamePhaseKind::SelectAbility,
                );
            }
            GamePhaseKind::Defend => {
                self.state
                    .peers
                    .deactivate_item_by_index(self.state.peers.actives[0].unwrap_index())
                    .expect("Must deactivate");
                let _ = self.state.peers.next_actives().map_err(|eof| {
                    // handle game end here?
                    tracing::info!("Next cycle");
                    let _ = self.monsters.next_actives();
                    tracing::info!("Next monsters {:?}", self.monsters.active_items());
                    self.phase = GamePhaseKind::DropAbility;
                    self.state.peers.repeat_after_eof(eof);
                });
            }
        };
        self.state.peers.active_items()[0]
            .as_ref()
            .expect("Always Some")
            .addr
    }
}

#[tracing::instrument(skip(peers))]
async fn broadcast<'a, T>(
    peers: impl Iterator<Item = &'a PeerSlot<T>> + Send,
    msg: <<PeerSlot<T> as TcpSender>::Sender as SendSocketMessage>::Msg,
) where
    T: 'a,
    PeerSlot<T>: TcpSender,
    <<PeerSlot<T> as TcpSender>::Sender as SendSocketMessage>::Msg: Clone,
{
    futures::stream::iter(peers)
        .for_each_concurrent(MAX_PLAYER_COUNT, |p| async {
            if p.can_send() {
                debug!(addr = ?p.addr);
                let _ = p.send_tcp(msg.clone()).await;
            }
        })
        .await;
}
#[async_trait::async_trait]
trait Broadcast {
    type Msg;
    async fn broadcast(&self, sender: PlayerId, msg: Self::Msg);
    async fn broadcast_to_all(&self, msg: Self::Msg);
}

#[async_trait::async_trait]
impl<T> Broadcast for T
where
    T: Send + Sync + GetPeers,
    PeerSlot<<T as GetPeers>::Peer>: GetPeerHandle + TcpSender + Send + Sync,
    <<PeerSlot<<T as GetPeers>::Peer> as TcpSender>::Sender as SendSocketMessage>::Msg:
        Clone + Send + Sync,
{
    type Msg = <<PeerSlot<<T as GetPeers>::Peer> as TcpSender>::Sender as SendSocketMessage>::Msg;
    #[inline]
    async fn broadcast(&self, sender: PlayerId, msg: Self::Msg) {
        broadcast(self.peers().0.iter().filter(|p| p.addr != sender), msg).await
    }
    #[inline]
    async fn broadcast_to_all(&self, msg: Self::Msg) {
        broadcast(self.peers().0.iter(), msg).await
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Send error. Peer rx closed")]
pub struct SendError;

#[async_trait::async_trait]
pub trait TcpSender {
    type Sender: SendSocketMessage;
    fn can_send(&self) -> bool {
        true
    }
    async fn send_tcp(
        &self,
        msg: <Self::Sender as SendSocketMessage>::Msg,
    ) -> Result<(), SendError>;
}

impl From<Msg<server::SharedMsg, server::RolesMsg>> for Msg<peer::SharedCmd, peer::RolesCmd> {
    fn from(value: Msg<server::SharedMsg, server::RolesMsg>) -> Self {
        Msg::with(peer::RolesCmd::SendTcp(value))
    }
}
impl From<Msg<server::SharedMsg, server::GameMsg>> for Msg<peer::SharedCmd, peer::GameCmd> {
    fn from(value: Msg<server::SharedMsg, server::GameMsg>) -> Self {
        Msg::with(peer::GameCmd::SendTcp(value))
    }
}

#[async_trait::async_trait]
impl<T> TcpSender for PeerSlot<(PeerStatus, PeerHandle<T>)>
where
    PeerHandle<T>: SendSocketMessage + Send + Sync,
    <PeerHandle<T> as SendSocketMessage>::Msg: Send + Sync,
    Msg<peer::SharedCmd, T>: From<<PeerHandle<T> as SendSocketMessage>::Msg> + Send + Sync,
{
    type Sender = PeerHandle<T>;
    #[inline]
    async fn send_tcp(
        &self,
        msg: <PeerHandle<T> as SendSocketMessage>::Msg,
    ) -> Result<(), SendError> {
        let _ = self
            .peer
            .1
            .tx
            .send(Msg::from(msg))
            .await
            .map_err(|_| SendError);
        Ok(())
    }
    #[inline]
    fn can_send(&self) -> bool {
        self.peer.0 == PeerStatus::Online
    }
}
#[async_trait::async_trait]
impl TcpSender for PeerSlot<peer::HomeHandle> {
    type Sender = peer::HomeHandle;
    #[inline]
    async fn send_tcp(
        &self,
        msg: <Self::Sender as SendSocketMessage>::Msg,
    ) -> Result<(), SendError> {
        self.peer.send_tcp(msg).await;
        Ok(())
    }
}
