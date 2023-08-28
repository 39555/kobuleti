use std::net::SocketAddr;

use arrayvec::ArrayVec;
use futures::stream::StreamExt;
use tokio::sync::{
    mpsc,
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot,
};
use tracing::{debug, error, info, trace};

use super::{
    details::send_oneshot_and_wait,
    peer2::{self as peer, PeerHandle},
    Answer, Handle,
};
use crate::{
    game::{Card, Role},
    protocol::{
        client, server,
        server::{ChatLine, LoginStatus, PlayerId, SharedMsg, MAX_PLAYER_COUNT},
        AsyncMessageReceiver, GameContext, GamePhaseKind, Msg, Username,
    },
};

pub type Rx<T> = UnboundedReceiver<T>;
pub type Tx<T> = UnboundedSender<T>;

impl<M> From<SharedCmd> for Msg<SharedCmd, M> {
    fn from(value: SharedCmd) -> Self {
        Msg::Shared(value)
    }
}
macro_rules! from {
    ($($src:ident,)*) => {
        $(
            impl From<$src> for Msg<SharedCmd, $src>{
                fn from(value: $src) -> Self {
                    Msg::State(value)
                }
            }
        )*
    }
}
from! {IntroCmd, HomeCmd, RolesCmd, GameCmd,}

use super::details::actor_api;
actor_api! { // Shared
    impl<M> Handle<Msg<SharedCmd, M>> {
        pub async fn ping(&self) -> Result<(), RecvError>;
        pub async fn get_chat_log(&self) -> Result<Vec<server::ChatLine>, RecvError>;
        pub fn append_chat(&self, line: server::ChatLine);
        pub async fn get_peer_id_by_name(&self, username: Username) -> Result<Option<PlayerId>, RecvError>;
        pub async fn get_peer_username(&self, whom: PlayerId) -> Result<Username, RecvError> ;
        pub fn drop_peer(&self, whom: PlayerId) ;
        pub async fn shutdown(&self) -> Result<(), RecvError>;
    }
}

actor_api! { // Intro
    impl Handle<Msg<SharedCmd, IntroCmd>> {
        pub async fn login_player(&self, sender: SocketAddr, name: Username, handle: peer::IntroHandle) -> Result<LoginStatus, RecvError>;
        pub async fn is_peer_connected(&self, who: PlayerId) -> Result<bool, RecvError>;
        pub fn enter_game(&self, who: PlayerId);

    }
}

#[derive(thiserror::Error, Debug)]
#[error("Room is full")]
pub struct PeersCapacityError;

use tokio::sync::oneshot::error::RecvError;
actor_api! { // Home
    impl Handle<Msg<SharedCmd, HomeCmd>> {
        pub async fn add_peer(&self, id: PlayerId, handle: peer::HomeHandle) -> Result<Result<(), PeersCapacityError>, RecvError>;
        pub async fn broadcast(&self, sender: SocketAddr, message: Msg<SharedMsg, server::HomeMsg>) -> Result<(), RecvError>;
        pub async fn broadcast_to_all(&self, msg:  Msg<SharedMsg, server::HomeMsg>) -> Result<(), RecvError> ;
        pub fn start_roles(&self);

    }
}
actor_api! { // Roles
    impl Handle<Msg<SharedCmd, RolesCmd>> {
        pub async fn get_available_roles(&self) -> Result<[client::RoleStatus; Role::count()], RecvError>;
        pub fn select_role(&self, sender: PlayerId, role: Role);
        pub async fn broadcast(&self, sender: SocketAddr, message:  Msg<SharedMsg, server::RolesMsg>) -> Result<(), RecvError>;
        pub async fn broadcast_to_all(&self, msg:  Msg<SharedMsg, server::RolesMsg>) -> Result<(), RecvError> ;
        pub async fn is_peer_connected(&self, who: PlayerId) -> Result<bool, RecvError> ;
        pub fn start_game(&self);
        pub async fn get_peer_handle_by_username(&self, whom: Username) -> Result<Option<peer::RolesHandle>, RecvError>;

    }
}

actor_api! { // Game
    impl Handle<Msg<SharedCmd, GameCmd>> {
        pub async fn get_peer_handle_by_username(&self, whom: Username) -> Result<Option<peer::GameHandle>, RecvError>;
        pub async fn is_peer_connected(&self, who: PlayerId) -> Result<bool, RecvError> ;
        //pub async fn get_peer_handle(&self, whom: PlayerId) -> Option<peer::GameHandle>;
        pub async fn broadcast(&self, sender: SocketAddr, message: Msg<SharedMsg, server::GameMsg>) -> Result<(), RecvError>;
        pub async fn broadcast_to_all(&self, msg:  Msg<SharedMsg, server::GameMsg>) -> Result<(), RecvError> ;
        pub async fn get_monsters(&self)          -> Result<[Option<Card>; 2], RecvError>;
        pub async fn get_active_player(&self)     -> Result<PlayerId, RecvError>;
        pub async fn get_game_phase(&self)        -> Result<GamePhaseKind, RecvError> ;
        pub async fn drop_monster(&self, monster: Card) -> Result<Result<(), super::details::DeactivateItemError>, RecvError> ;
        pub async fn switch_to_next_player(&self) -> Result<PlayerId, RecvError> ;
        pub async fn next_monsters(&self)         -> Result<Result<(), super::details::EndOfItems>, RecvError>;
        pub async fn continue_game_cycle(&self)   -> Result<(), RecvError>;
        pub fn broadcast_game_state(&self, sender: PlayerId);
    }
}

pub struct ServerHandleByContext(GameContext<(), HomeHandle, RolesHandle, GameHandle>);
pub type IntroHandle = Handle<Msg<SharedCmd, IntroCmd>>;
pub type HomeHandle = Handle<Msg<SharedCmd, HomeCmd>>;
pub type RolesHandle = Handle<Msg<SharedCmd, RolesCmd>>;
pub type GameHandle = Handle<Msg<SharedCmd, GameCmd>>;

#[derive(Default)]
pub struct IntroServer {
    peers: Room<Option<peer::IntroHandle>>,
    server: Option<ServerHandleByContext>,
}
pub struct Server<T> {
    chat: Vec<ChatLine>,
    peers: T,
}
pub type HomeServer = Server<Room<peer::HomeHandle>>;
pub type RolesServer = Server<Room<(PeerStatus, peer::RolesHandle)>>;
use super::details::{Stateble, StatebleItem};
use crate::game::Deck;

pub const MONSTER_LINE_LEN: usize = 2;
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

pub struct GameServer {
    state: Server<Stateble<Room<(PeerStatus, peer::GameHandle)>, 1>>,
    monsters: Stateble<Deck, MONSTER_LINE_LEN>,
    phase: GamePhaseKind,
}

macro_rules! recv {
    ($result:expr) => {
        $result.expect("Peer actor is connected")
    };
}

impl IntroServer {
    pub async fn login_player(
        &mut self,
        sender: SocketAddr,
        username: Username,
        handle: peer::IntroHandle,
    ) -> LoginStatus {
        info!("Try login a player {} as {}", sender, username);
        if self.peers.0.is_full() {
            return LoginStatus::PlayerLimit;
        }
        if futures::stream::iter(self.peers.0.iter_mut())
            .any(|p| async {
                p.peer.as_ref().is_some()
                    && recv!(p.peer.as_ref().unwrap().get_username().await) == username
            })
            .await
        {
            return LoginStatus::AlreadyLogged;
        }

        if self.server.is_some() {
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
            if match &self.server.as_ref().unwrap().0 {
                GameContext::Home(h) => {
                    recv!(h.get_peer_id_by_name(username.clone()).await).is_some()
                }
                GameContext::Roles(r) => is_logged_in!(r),
                GameContext::Game(g) => is_logged_in!(g),
                _ => unreachable!(),
            } {
                return LoginStatus::AlreadyLogged;
            } // else
              // fallthrough
        }
        handle.set_username(username);
        self.peers.0.push(PeerSlot::new(sender, Some(handle)));
        LoginStatus::Logged
    }
}

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

pub async fn start_intro_server(
    intro: &mut StartServer<IntroServer, Rx<Msg<SharedCmd, IntroCmd>>>,
) -> anyhow::Result<()> {
    let (notify_intro, mut rx) = tokio::sync::mpsc::channel::<ServerHandleByContext>(1);
    loop {
        let (cancel, cancel_rx) = oneshot::channel();
        let state = ServerState {
            cancel: Some(cancel),
        };
        let (sender, new_server) = tokio::join! {
            run_state(intro, state, cancel_rx),
            rx.recv()
        };

        intro.server.server = new_server;
        let sender = done!(sender?);

        if intro.server.server.is_none() {
            let home = StartServer::async_from((sender, &mut intro.server)).await;
            tokio::spawn({
                let tx = notify_intro.clone();
                async move { run_server(home, tx).await }
            });
        } else {
            let peer_slot = intro
                .server
                .peers
                .0
                .iter()
                .find(|p| p.addr == sender)
                .ok_or(PeerNotFound(sender))?;
            match &unsafe { intro.server.server.as_ref().unwrap_unchecked() }.0 {
                GameContext::Home(h) => {
                    let handle =
                        recv!(peer_slot.peer.as_ref().unwrap().enter_game(h.clone()).await);
                    h.add_peer(peer_slot.addr, handle)
                        .await
                        .expect("Peer slot must be empty");
                }
                GameContext::Roles(r) => {
                    let handle = recv!(
                        r.get_peer_handle_by_username(recv!(
                            peer_slot.peer.as_ref().unwrap().get_username().await
                        ),)
                            .await
                    )
                    .expect("Must exists until reconnection");
                    peer_slot
                        .peer
                        .as_ref()
                        .unwrap()
                        .reconnect_roles(r.clone(), handle)
                        .await;
                }
                GameContext::Game(g) => {
                    let handle = recv!(
                        g.get_peer_handle_by_username(recv!(
                            peer_slot.peer.as_ref().unwrap().get_username().await
                        ),)
                            .await
                    )
                    .expect("Must exists until reconnection");
                    recv!(
                        peer_slot
                            .peer
                            .as_ref()
                            .unwrap()
                            .reconnect_game(g.clone(), handle)
                            .await
                    );
                }
                _ => unreachable!(),
            }
        };
    }
}

async fn run_server(
    mut start_home: StartServer<HomeServer, Rx<Msg<SharedCmd, HomeCmd>>>,
    intro: mpsc::Sender<ServerHandleByContext>,
) -> anyhow::Result<()> {
    {
        let (cancel, cancel_rx) = oneshot::channel();
        let state = ServerState {
            cancel: Some(cancel),
        };
        let _ = done!(run_state(&mut start_home, state, cancel_rx).await?);
    }
    let mut start_roles =
        StartServer::async_from(ServerConverter::new(start_home.server, &intro)).await;
    {
        let (cancel, cancel_rx) = oneshot::channel();
        let state = ServerState {
            cancel: Some(cancel),
        };
        let _ = done!(run_state(&mut start_roles, state, cancel_rx).await?);
    }
    let mut start_game =
        StartServer::async_from(ServerConverter::new(start_roles.server, &intro)).await;
    {
        let (cancel, cancel_rx) = oneshot::channel();
        let state = ServerState {
            cancel: Some(cancel),
        };
        let _ = done!(run_state(&mut start_game, state, cancel_rx).await?);
    }
    Ok(())
}

pub trait NextContextTag {
    type Tag;
}
#[derive(Debug)]
pub struct RolesTag;
#[derive(Debug)]
pub struct GameTag;
#[derive(Debug)]
pub struct EndTag;
impl NextContextTag for IntroServer {
    type Tag = Sender;
}
impl NextContextTag for HomeServer {
    type Tag = RolesTag;
}
impl NextContextTag for RolesServer {
    type Tag = GameTag;
}
impl NextContextTag for GameServer {
    type Tag = EndTag;
}

async fn run_state<Server, M, State>(
    StartServer {
        server: visitor,
        server_rx: rx,
    }: &mut StartServer<Server, Rx<Msg<SharedCmd, M>>>,
    mut state: State,
    mut cancel: oneshot::Receiver<<Server as NextContextTag>::Tag>,
) -> anyhow::Result<Option<<Server as NextContextTag>::Tag>>
where
    for<'a> Server: AsyncMessageReceiver<M, &'a mut State> + NextContextTag + Send,
    M: Send + Sync + 'static,
{
    loop {
        tokio::select! {
            new =  &mut cancel => {
                return Ok(Some(new?))

            }
            cmd = rx.recv() => match cmd {
                Some(cmd) => {
                    match cmd {
                        Msg::Shared(msg) => match msg {
                            _ => (),
                        },
                        Msg::State(msg) => {
                            if let Err(e) = visitor.reduce(msg, &mut state).await {
                                tracing::error!("{:#}", e);
                            }

                        }
                    }
                }
                None => {
                    break
                }
            }
        }
    }

    Ok(None)
}

pub struct ServerConverter<'a, S> {
    server: S,
    intro: &'a mpsc::Sender<ServerHandleByContext>,
}
impl<'a, S> ServerConverter<'a, S> {
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

pub type Sender = PlayerId;
#[async_trait::async_trait]
impl<'a> AsyncFrom<(Sender, &'a mut IntroServer)>
    for StartServer<HomeServer, Rx<Msg<SharedCmd, HomeCmd>>>
where
    (
        Sender,
        &'a mut IntroServer,
        UnboundedSender<ServerHandleByContext>,
    ): 'a,
{
    async fn async_from((sender, intro): (Sender, &'a mut IntroServer)) -> Self {
        let peer_slot = intro
            .peers
            .0
            .iter_mut()
            .find(|p| p.addr == sender)
            .expect("Sender in Intro");
        let (tx, rx) = unbounded_channel::<Msg<SharedCmd, HomeCmd>>();
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
        intro.server = Some(ServerHandleByContext(GameContext::Home(home_handle)));
        StartServer::new(server, rx)
    }
}

#[async_trait::async_trait]
impl<'a> AsyncFrom<ServerConverter<'a, HomeServer>>
    for StartServer<RolesServer, Rx<Msg<SharedCmd, RolesCmd>>>
{
    async fn async_from(home: ServerConverter<'a, HomeServer>) -> Self {
        let (tx, rx) = unbounded_channel::<Msg<SharedCmd, RolesCmd>>();
        let handle = RolesHandle::for_tx(tx);
        let _ = home.intro.send(ServerHandleByContext::from(handle.clone()));
        let peers: ArrayVec<PeerSlot<(PeerStatus, peer::RolesHandle)>, MAX_PLAYER_COUNT> =
            futures::future::join_all(home.server.peers.0.iter().map(|p| async {
                let handle = handle.clone();
                let peer_handle = recv!(p.peer.start_roles(handle).await);
                PeerSlot::<(PeerStatus, peer::RolesHandle)> {
                    addr: p.addr,
                    peer: (PeerStatus::Connected, peer_handle),
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
        let (tx, rx) = unbounded_channel::<Msg<SharedCmd, GameCmd>>();
        let handle = GameHandle::for_tx(tx);
        let _ = roles
            .intro
            .send(ServerHandleByContext::from(handle.clone()));
        let peers: ArrayVec<PeerSlot<(PeerStatus, peer::GameHandle)>, MAX_PLAYER_COUNT> =
            futures::future::join_all(roles.server.peers.0.iter_mut().map(|p| async {
                let handle = handle.clone();
                let peer_handle = recv!(p.peer.1.start_game(handle).await);
                PeerSlot::<(PeerStatus, peer::GameHandle)> {
                    addr: p.addr,
                    peer: (PeerStatus::Connected, peer_handle),
                }
            }))
            .await
            .into_iter()
            .collect();
        use crate::game::Deckable;
        let mut monsters = Deck::default();
        monsters.shuffle();
        StartServer::new(
            GameServer {
                state: Server {
                    chat: roles.server.chat,
                    peers: Stateble::with_items(Room::<(PeerStatus, peer::GameHandle)>(peers)),
                },
                monsters: Stateble::<Deck, MONSTER_LINE_LEN>::with_items(monsters),
                phase: GamePhaseKind::default(),
            },
            rx,
        )
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
pub enum PeerStatus {
    Connected,
    WaitReconnection,
}
#[derive(Debug)]
pub struct PeerSlot<T> {
    addr: PlayerId,
    peer: T,
}

pub trait GetPeerHandle {
    type State;
    fn get_peer_handle(&self) -> &PeerHandle<Self::State>;
}
impl<T> GetPeerHandle for PeerSlot<PeerHandle<T>> {
    type State = T;
    fn get_peer_handle(&self) -> &PeerHandle<Self::State> {
        &self.peer
    }
}
impl<T> GetPeerHandle for PeerSlot<Option<PeerHandle<T>>> {
    type State = T;
    fn get_peer_handle(&self) -> &PeerHandle<Self::State> {
        &self.peer.as_ref().expect("Requested None Peer")
    }
}
impl<T, S> GetPeerHandle for PeerSlot<(S, PeerHandle<T>)> {
    type State = T;
    fn get_peer_handle(&self) -> &PeerHandle<Self::State> {
        &self.peer.1
    }
}

impl<T> PeerSlot<T> {
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
    fn clone(&self) -> Self {
        PeerSlot {
            addr: self.addr,
            peer: self.peer.clone(),
        }
    }
}
pub struct Room<T>(pub ArrayVec<PeerSlot<T>, MAX_PLAYER_COUNT>);

impl<T> Default for Room<T> {
    fn default() -> Self {
        Room(Default::default())
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Peer with addr {0} not found in the room")]
pub struct PeerNotFound(pub PlayerId);

impl<T> Room<T> {
    fn get_peer(&self, addr: SocketAddr) -> Result<&PeerSlot<T>, PeerNotFound> {
        self.0
            .iter()
            .find(|p| p.addr == addr)
            .ok_or(PeerNotFound(addr))
    }
}

impl<T> Room<(PeerStatus, PeerSlot<T>)> {
    async fn drop_peer_handle(&mut self, whom: PlayerId) -> Result<(), PeerNotFound> {
        let p = self
            .0
            .iter_mut()
            .find(|p| p.addr == whom)
            .ok_or(PeerNotFound(whom))?;
        p.peer.0 = PeerStatus::WaitReconnection;
        Ok(())
    }
}

use peer::{SendSocketMessage, TcpSender};

impl<T> TcpSender for PeerSlot<(PeerStatus, T)>
where
    T: TcpSender,
    <T as TcpSender>::Sender: SendSocketMessage,
{
    type Sender = <T as TcpSender>::Sender;
    fn send_tcp(&self, msg: <<T as TcpSender>::Sender as SendSocketMessage>::Msg) {
        self.peer.1.send_tcp(msg);
    }
    fn can_send(&self) -> bool {
        self.peer.0 == PeerStatus::Connected
    }
}

impl<T> Room<T>
where
    PeerSlot<T>: GetPeerHandle,

    PeerHandle<<PeerSlot<T> as GetPeerHandle>::State>: peer::TcpSender,
    <PeerHandle<<PeerSlot<T> as GetPeerHandle>::State> as TcpSender>::Sender: SendSocketMessage,
    <<PeerHandle<<PeerSlot<T> as GetPeerHandle>::State> as TcpSender>::Sender as SendSocketMessage>::Msg: Clone + std::fmt::Debug

{
    async fn broadcast(&self, sender: PlayerId,
                       msg: <<PeerHandle<<PeerSlot<T> as GetPeerHandle>::State> as TcpSender>::Sender as SendSocketMessage>::Msg)
    {
        self.impl_broadcast(self.0.iter().filter(|p| p.addr != sender), msg)
            .await
    }

    async fn broadcast_to_all(&self,
                              msg: <<PeerHandle<<PeerSlot<T> as GetPeerHandle>::State> as TcpSender>::Sender as SendSocketMessage>::Msg)
    {
        self.impl_broadcast(self.0.iter(), msg).await
    }
    async fn impl_broadcast<'a>(
        &'a self,
        peers: impl Iterator<Item = &'a PeerSlot<T>>,
        msg: <<PeerHandle<<PeerSlot<T> as GetPeerHandle>::State> as TcpSender>::Sender as SendSocketMessage>::Msg,
    ){
        tracing::trace!("Broadcast {:?}", msg);
        futures::stream::iter(peers)
            .for_each_concurrent(MAX_PLAYER_COUNT, |p| async {
                let p = p.get_peer_handle();
                p.can_send().then(|| p.send_tcp(msg.clone()));
            })
            .await;
    }
    fn shutdown(&mut self) {
        trace!("Drop all peers");
        // if it is a last handle, peer actor will shutdown
        self.0.clear();
    }
}

pub struct ServerState<Server>
where
    Server: NextContextTag,
{
    cancel: Option<oneshot::Sender<<Server as NextContextTag>::Tag>>,
}

#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<IntroCmd, &'a mut ServerState<IntroServer>> for IntroServer {
    async fn reduce(
        &mut self,
        msg: IntroCmd,
        state: &'a mut ServerState<IntroServer>,
    ) -> anyhow::Result<()> {
        match msg {
            IntroCmd::LoginPlayer(sender, username, handle, tx) => {
                let _ = tx.send(self.login_player(sender, username, handle).await);
            }
            IntroCmd::EnterGame(sender) => {
                let _ = state.cancel.take().unwrap().send(sender);
            }
            IntroCmd::IsPeerConnected(sender, tx) => {
                let _ = tx.send(self.peers.get_peer(sender).is_ok());
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
                self.peers.broadcast(sender, msg).await;
                let _ = tx.send(());
            }
            HomeCmd::BroadcastToAll(msg, tx) => {
                self.peers.broadcast_to_all(msg).await;
                let _ = tx.send(());
            }
            HomeCmd::StartRoles() => {
                state.cancel.take().unwrap().send(RolesTag).unwrap();
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
                peer = Some(p.peer.1.clone());
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
                self.peers.broadcast(sender, msg).await;
                let _ = tx.send(());
            }
            RolesCmd::BroadcastToAll(msg, tx) => {
                self.peers.broadcast_to_all(msg).await;
                let _ = tx.send(());
            }
            RolesCmd::SelectRole(sender, role) => {
                let status = self.set_role_for_peer(sender, role).await;
                let peer = self.peers.get_peer(sender).expect("Must exists");
                if let Ok(r) = &status {
                    self.peers
                        .broadcast(
                            sender,
                            Msg::Shared(server::SharedMsg::Chat(server::ChatLine::GameEvent(
                                format!(
                                    "{} select {:?}",
                                    recv!(peer.peer.1.get_username().await),
                                    r
                                ),
                            ))),
                        )
                        .await;
                };
                peer.peer
                    .1
                    .send_tcp(Msg::State(server::RolesMsg::SelectedStatus(status)));
                let roles = self.collect_roles().await;
                debug!("Available roles {:?}", roles);
                self.peers
                    .broadcast_to_all(Msg::State(server::RolesMsg::AvailableRoles(roles)))
                    .await;
            }
            RolesCmd::IsPeerConnected(sender, tx) => {
                let _ = tx.send(
                    self.peers
                        .0
                        .iter()
                        .any(|p| p.addr == sender && p.peer.0 == PeerStatus::Connected),
                );
            }
            RolesCmd::GetAvailableRoles(tx) => {
                let _ = tx.send(self.collect_roles().await);
            }
            RolesCmd::GetPeerHandleByUsername(name, tx) => {
                let _ = tx.send(get_peer_by_name!(self.peers.0.iter(), name));
            }
            RolesCmd::StartGame() => {
                state
                    .cancel
                    .take()
                    .unwrap()
                    .send(GameTag)
                    .expect("the Done Rx must not be dropped");
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
        state: &'a mut ServerState<GameServer>,
    ) -> anyhow::Result<()> {
        match msg {
            GameCmd::Broadcast(sender, msg, tx) => {
                self.state.peers.items.broadcast(sender, msg).await;
                let _ = tx.send(());
            }
            GameCmd::BroadcastToAll(msg, tx) => {
                self.state.peers.items.broadcast_to_all(msg).await;
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
                        .any(|p| p.addr == sender && p.peer.0 == PeerStatus::Connected),
                );
            }
            GameCmd::GetMonsters(tx) => {
                let _ = tx.send(self.monsters.active_items());
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
                self.state.peers.items.broadcast(
                    next_player,
                    Msg::State(server::GameMsg::Turn(TurnStatus::Wait)),
                );
                self.state
                    .peers
                    .items
                    .get_peer(next_player)
                    .expect("Must exists")
                    .send_tcp(Msg::State(server::GameMsg::Turn(TurnStatus::Ready(
                        self.phase,
                    ))));
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
                self.state
                    .peers
                    .items
                    .0
                    .iter()
                    .filter(|p| p.addr != sender)
                    .for_each(|p| {
                        let _ = p.peer.1.sync_with_client();
                    });
            }
        };

        Ok(())
    }
}

use crate::protocol::{client::RoleStatus, server::SelectRoleError};

impl RolesServer {
    async fn collect_roles(&self) -> [RoleStatus; Role::count()] {
        trace!("Collect roles from peers");
        futures::future::join_all(Role::iter().map(|r| async move {
            match futures::stream::iter(self.peers.0.iter())
                .any(|p| async { recv!(p.peer.1.get_role().await).is_some_and(|pr| pr == r) })
                .await
            {
                false => RoleStatus::Available(r),
                true => RoleStatus::NotAvailable(r),
            }
        }))
        .await
        .try_into()
        .expect("Role.iter().map().len() == Role.count()")
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
        p.1.select_role(role).await;
        Ok(role)
    }
    async fn are_all_have_roles(&self) -> bool {
        futures::stream::iter(self.peers.0.iter())
            .all(|p| async move { recv!(p.peer.1.get_role().await).is_some() })
            .await
    }
}

impl GameServer {
    fn next_player_for_turn(&self, current: PlayerId) -> &PeerSlot<(PeerStatus, peer::GameHandle)> {
        assert!(
            self.state.peers.items.0.iter().count() >= self.state.peers.items.0.len(),
            "Require at least two players"
        );
        let mut i = self
            .state
            .peers
            .items
            .0
            .iter()
            .position(|i| i.addr == current)
            .expect("Peer must exists");
        loop {
            i += 1;
            if i >= self.state.peers.items.0.len() {
                i = 0;
            }
            if self.state.peers.items.0[i].addr != current {
                break;
            }
        }
        &self.state.peers.items.0[i]
    }
    fn switch_to_next_player(&mut self) -> PlayerId {
        match self.phase {
            GamePhaseKind::DropAbility => {
                // TODO errorkind
                tracing::info!("Current active {:?}", self.state.peers.active_items()[0]);
                self.state
                    .peers
                    .deactivate_item(self.state.peers.active_items()[0].as_ref().unwrap())
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
                    .deactivate_item(self.state.peers.active_items()[0].as_ref().unwrap())
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
                    .deactivate_item(self.state.peers.active_items()[0].as_ref().unwrap())
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
