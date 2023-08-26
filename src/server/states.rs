use std::net::SocketAddr;

use futures::stream::StreamExt;
use tokio::sync::{
    mpsc,
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot,
};
use arrayvec::ArrayVec;
use tracing::{warn, info, trace, debug, error};
use tokio_util::sync::CancellationToken;
use crate::protocol::{Username, server::LoginStatus, };
use super::{
    details::send_oneshot_and_wait,
    peer2::{self as peer, PeerHandle},
    Answer, Handle,
};
use crate::protocol::{
    server::{ChatLine, PlayerId, SharedMsg, MAX_PLAYER_COUNT},
    AsyncMessageReceiver, GameContext, GameContextKind, Msg,
};
use crate::game::Role;
use crate::protocol::{server, client};
pub type Rx<T> = UnboundedReceiver<T>;
pub type Tx<T> = UnboundedSender<T>;


impl<M> From<SharedCmd> for Msg<SharedCmd, M>{
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
from!{IntroCmd, HomeCmd, RolesCmd, GameCmd,}


use super::details::actor_api;
actor_api!{ // Shared
    impl<M> Handle<Msg<SharedCmd, M>> {
        pub async fn ping(&self) -> ();
        pub async fn get_chat_log(&self) -> Vec<server::ChatLine>;
        pub fn append_chat(&self, line: server::ChatLine);
        pub async fn get_peer_id_by_name(&self, username: Username) -> Option<PlayerId>;
        pub async fn get_peer_username(&self, whom: PlayerId) -> Username ;
        pub fn drop_peer(&self, whom: PlayerId) ;
        pub async fn shutdown(&self) -> ();
    }
}

actor_api! { // Intro
    impl Handle<Msg<SharedCmd, IntroCmd>> {
        pub async fn login_player(&self, sender: SocketAddr, name: Username, handle: peer::IntroHandle) -> LoginStatus;
        pub async fn broadcast(&self, sender: SocketAddr, message: Msg<SharedMsg, server::IntroMsg>) -> ();
        pub async fn broadcast_to_all(&self, msg: Msg<SharedMsg, server::IntroMsg>) -> () ;
        pub fn enter_game(&self, who: PlayerId);

    }
}
actor_api! { // Home
    impl Handle<Msg<SharedCmd, HomeCmd>> {
        pub fn add_peer(&self, handle: peer::HomeHandle);
        pub async fn broadcast(&self, sender: SocketAddr, message: Msg<SharedMsg, server::HomeMsg>) -> ();
        pub async fn broadcast_to_all(&self, msg:  Msg<SharedMsg, server::HomeMsg>) -> () ;
        pub fn start_roles(&self);

    }
}
actor_api! { // Roles
    impl Handle<Msg<SharedCmd, RolesCmd>> {
        pub async fn get_available_roles(&self) -> [client::RoleStatus; Role::count()];
        pub fn select_role(&self, sender: PlayerId, role: Role);
        pub async fn broadcast(&self, sender: SocketAddr, message:  Msg<SharedMsg, server::RolesMsg>) -> ();
        pub async fn broadcast_to_all(&self, msg:  Msg<SharedMsg, server::RolesMsg>) -> () ;
        pub async fn is_peer_connected(&self, who: PlayerId) -> bool ;
        pub fn start_game(&self);

    }
}
actor_api! { // Game
    impl Handle<Msg<SharedCmd, GameCmd>> {
        pub async fn is_peer_connected(&self, who: PlayerId) -> bool ;
        pub async fn broadcast(&self, sender: SocketAddr, message: Msg<SharedMsg, server::GameMsg>) -> ();
        pub async fn broadcast_to_all(&self, msg:  Msg<SharedMsg, server::GameMsg>) -> () ;
    }
}


pub struct ServerHandle(GameContext<(), HomeHandle, RolesHandle, GameHandle>);
pub type IntroHandle = Handle<Msg<SharedCmd, IntroCmd>>;
pub type HomeHandle  = Handle<Msg<SharedCmd, HomeCmd>>;
pub type RolesHandle = Handle<Msg<SharedCmd, RolesCmd>>;
pub type GameHandle  = Handle<Msg<SharedCmd, GameCmd>>;




#[derive(Default)]
pub struct IntroServer {
    peers:  Room<Option<peer::IntroHandle>>, //[Option<(PlayerId, Option<PeerHandle<peer::IntroCmd>>)>; MAX_PLAYER_COUNT],
    server: Option<ServerHandle>,
}
pub struct Server<T> 
{
    chat: Vec<ChatLine>,
    peers: Room<T>,
}
pub type HomeServer  = Server<peer::HomeHandle>;
pub type RolesServer = Server<(PeerStatus, peer::RolesHandle)>;
pub type GameServer  = Server<(PeerStatus, peer::GameHandle)>;



impl IntroServer {
    pub async fn login_player(&mut self, sender: SocketAddr, username: Username, handle: peer::IntroHandle) -> LoginStatus {
        info!("Try login a player {} as {}", sender, username);
        if self.peers.0.is_full(){
            return LoginStatus::PlayerLimit;
        }
        if futures::stream::iter(self.peers.0.iter_mut())
                .any(|p| async { p.peer.as_ref().is_some() && p.peer.as_ref().unwrap().get_username().await == username } ).await{
                return LoginStatus::AlreadyLogged;
        }
        
        if self.server.is_some() {
            if match &self.server.as_ref().unwrap().0 {
                GameContext::Home(h) => h.get_peer_id_by_name(username.clone()).await.is_some(),
                GameContext::Roles(r) => {
                    futures::future::OptionFuture::from(r.get_peer_id_by_name(username.clone()).await
                                                                    .map( |p| async move {
                                    r.is_peer_connected(p).await
                    })).await.unwrap_or(false)
                },
                GameContext::Game(g) => {
                     futures::future::OptionFuture::from(g.get_peer_id_by_name(username.clone()).await
                                                                    .map( |p| async move {
                                    g.is_peer_connected(p).await
                    })).await.unwrap_or(false)

                }
                _ => unreachable!()

            }{
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
    let (notify_intro, mut rx) = tokio::sync::mpsc::channel::<ServerHandle>(1);
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
            match &unsafe { intro.server.server.as_ref().unwrap_unchecked() }.0 {
                GameContext::Home(h) => {
                    let peer_slot = intro.server.peers.0.iter().find(|p| p.addr == sender)
                        .ok_or(PeerNotFound(sender))?;
                    let handle = send_oneshot_and_wait(&peer_slot.peer.as_ref().unwrap().tx, |tx| {
                        Msg::State(peer::IntroCmd::EnterGame(GameContext::Home((h.clone(), tx))))
                    })
                    .await;
                    let _ = h.tx.send(Msg::State(HomeCmd::AddPeer(handle)));
                }
                _ => todo!("Reconnection here"),
            }
        };
    }
    //Ok(())
}

async fn run_server(
    mut start_home: StartServer<HomeServer, Rx<Msg<SharedCmd, HomeCmd>>>,
    intro: mpsc::Sender<ServerHandle>,
) -> anyhow::Result<()> {
    let (cancel, cancel_rx) = oneshot::channel();
    let state = ServerState {
        cancel: Some(cancel),
    };
    let _ = done!(run_state(&mut start_home, state, cancel_rx).await?);

    let (cancel, cancel_rx) = oneshot::channel();
    let state = ServerState {
        cancel: Some(cancel),
    };
    let mut start_roles =
        StartServer::async_from(ServerConverter::new(start_home.server, &intro)).await;
    let _ = done!(run_state(&mut start_roles, state, cancel_rx).await?);

    let (cancel, cancel_rx) = oneshot::channel();
    let state = ServerState {
        cancel: Some(cancel),
    };
    let mut start_game =
        StartServer::async_from(ServerConverter::new(start_roles.server, &intro)).await;
    let _ = done!(run_state(&mut start_game, state, cancel_rx).await?);
    Ok(())
}

pub trait NextContextTag {
    type Tag;
}
pub struct RolesTag;
pub struct GameTag;
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
    intro: &'a mpsc::Sender<ServerHandle>,
}
impl<'a, S> ServerConverter<'a, S> {
    fn new(server: S, intro: &'a mpsc::Sender<ServerHandle>) -> Self {
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
    (Sender, &'a mut IntroServer, UnboundedSender<ServerHandle>): 'a,
{
    async fn async_from((sender, intro): (Sender, &'a mut IntroServer)) -> Self {
        let peer_slot = intro.peers.0.iter().find(|p| p.addr == sender).unwrap();
        let (tx_to_server, rx) = unbounded_channel::<Msg<SharedCmd, HomeCmd>>();
        let peer_handle = send_oneshot_and_wait(&peer_slot.peer.as_ref().unwrap().tx, |tx| {
            Msg::State(peer::IntroCmd::EnterGame(
                GameContext::Home((HomeHandle::for_tx(tx_to_server.clone()),
                tx,
            ))))
        })
        .await;

        let mut server = HomeServer {
            peers: Default::default(),
            chat: Default::default(),
        };
        server.peers.0.push(PeerSlot::new(peer_slot.addr, peer_handle));
        intro.server = Some(ServerHandle(GameContext::Home(HomeHandle::for_tx(
            tx_to_server,
        ))));
        StartServer::new(server, rx)
    }
}

#[async_trait::async_trait]
impl<'a> AsyncFrom<ServerConverter<'a, HomeServer>>
    for StartServer<RolesServer, Rx<Msg<SharedCmd, RolesCmd>>>
{
    async fn async_from(mut home: ServerConverter<'a, HomeServer>) -> Self {
        let (tx, rx) = unbounded_channel::<Msg<SharedCmd, RolesCmd>>();
        let handle = RolesHandle::for_tx(tx);
        let _ = home.intro.send(ServerHandle::from(handle.clone()));
        let peers: ArrayVec<PeerSlot<(PeerStatus, peer::RolesHandle)>, MAX_PLAYER_COUNT> =
            futures::future::join_all(home.server.peers.0.iter().map(|p| async {
                let handle = handle.clone();
                let peer_handle = send_oneshot_and_wait(&p.peer.tx, |tx| {
                    Msg::State(peer::HomeCmd::StartRoles(handle, tx))
                })
                .await;
                PeerSlot::<(PeerStatus, peer::RolesHandle)> {
                    addr: p.addr,
                    //status: p.status,
                    peer: (PeerStatus::Connected, peer_handle),
                }
            }))
            .await.into_iter().collect();
            //.try_into()
            //.expect("Peers from Home == for Roles");
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
        let _ = roles.intro.send(ServerHandle::from(handle.clone()));
        let peers: ArrayVec<PeerSlot<(PeerStatus, peer::GameHandle)>, MAX_PLAYER_COUNT> =
            futures::future::join_all(roles.server.peers.0.iter_mut().map(|p| async {
                let handle = handle.clone();
                let peer_handle = send_oneshot_and_wait(&p.peer.1.tx, |tx| {
                    Msg::State(peer::RolesCmd::StartGame(handle, tx))
                })
                .await;
                PeerSlot::<(PeerStatus, peer::GameHandle)> {
                    addr: p.addr,
                    //status: p.status,
                    peer: (PeerStatus::Connected, peer_handle),
                }
            
            }))
            .await.into_iter().collect();
        StartServer::new(
            GameServer {
                chat: roles.server.chat,
                peers: Room::<(PeerStatus, peer::GameHandle)>(peers),
            },
            rx,
        )
    }
}

macro_rules! from {
    ($($state:ident,)* => $dst:ty) => {
        $(
            paste::item!{
            impl From<[< $state Handle>]> for ServerHandle{
                fn from(value: [< $state Handle>]) -> Self {
                    Self(GameContext::$state(value))
                }
            }
            }
         )*

    }
}
from! {
    Home, Roles, Game, => ServerHandle
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

pub trait GetPeerHandle{
    type State;
    fn get_peer_handle(&self) -> &PeerHandle<Self::State>;
}
impl<T> GetPeerHandle for PeerSlot<PeerHandle<T>>{
    type State = T;
    fn get_peer_handle(&self) -> &PeerHandle<Self::State> {
        &self.peer
    }
}
impl<T> GetPeerHandle for PeerSlot<Option<PeerHandle<T>>>{
    type State = T;
    fn get_peer_handle(&self) -> &PeerHandle<Self::State> {
        &self.peer.as_ref().expect("Requested None Peer")
    }
}
impl<T, S> GetPeerHandle for PeerSlot<(S, PeerHandle<T>)>{
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
where T : Clone
{
    fn clone(&self) -> Self {
        PeerSlot {
            addr: self.addr,
            peer: self.peer.clone(),
        }
    }
}
pub struct Room<T>(pub ArrayVec<PeerSlot<T>, MAX_PLAYER_COUNT>);

impl<T> Default for Room<T> 
{
    fn default() -> Self {
        Room(Default::default())
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Peer with addr {0} not found in the room")]
pub struct PeerNotFound(pub PlayerId);

use peer::{TcpSender, SendSocketMessage};

impl<T> Room<T>{
    fn get_peer(&self, addr: SocketAddr) -> Result<&PeerSlot<T>, PeerNotFound> {
        self.0.iter()
            .find(|p| p.addr == addr)
            .ok_or(PeerNotFound(addr))
    }
}

impl<T> Room<T> 
where PeerSlot<T>: GetPeerHandle, 
      
      PeerHandle<<PeerSlot<T> as GetPeerHandle>::State> : peer::TcpSender,
      T : peer::SendSocketMessage
{
    async fn broadcast(&self, sender: PlayerId, 
                       msg: <Handle<Msg<peer::SharedCmd, <PeerSlot<T> as GetPeerHandle>::State>> as SendSocketMessage>::Msg) 
    where <Handle<Msg<peer::SharedCmd, <PeerSlot<T> as GetPeerHandle>::State>> as SendSocketMessage>::Msg: Clone + std::fmt::Debug 
    {
        self.impl_broadcast(self.0.iter().filter(|p| p.addr != sender), msg)
            .await
    }

    async fn broadcast_to_all(&self, 
                              msg: <Handle<Msg<peer::SharedCmd, <PeerSlot<T> as GetPeerHandle>::State>> as SendSocketMessage>::Msg) 
    where <Handle<Msg<peer::SharedCmd, <PeerSlot<T> as GetPeerHandle>::State>> as SendSocketMessage>::Msg: Clone + std::fmt::Debug 
    {
        self.impl_broadcast(self.0.iter(), msg).await
    }
    async fn impl_broadcast<'a>(
        &'a self,
        peers: impl Iterator<Item = &'a PeerSlot<T>>,
        msg: <Handle<Msg<peer::SharedCmd, <PeerSlot<T> as GetPeerHandle>::State>> as SendSocketMessage>::Msg,
    ) where <Handle<Msg<peer::SharedCmd, <PeerSlot<T> as GetPeerHandle>::State>> as SendSocketMessage>::Msg : Clone + std::fmt::Debug {
        tracing::trace!("Broadcast {:?}", msg);
        futures::stream::iter(peers)
            .for_each_concurrent(MAX_PLAYER_COUNT, |p| async {
                p.get_peer_handle().send_tcp(msg.clone());
            })
            .await;
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
            IntroCmd::EnterGame(sender) => {
                let _ = state.cancel.take().unwrap().send(sender);
            }
            IntroCmd::LoginPlayer(sender, username, handle, tx) => {
                let _ = tx.send(self.login_player(sender, username, handle).await);


            }
            _ => (),
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
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<RolesCmd, &'a mut ServerState<RolesServer>> for RolesServer {
    async fn reduce(
        &mut self,
        msg: RolesCmd,
        state: &'a mut ServerState<RolesServer>,
    ) -> anyhow::Result<()> {
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
        Ok(())
    }
}
