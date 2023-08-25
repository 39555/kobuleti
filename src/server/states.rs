use std::net::SocketAddr;

use futures::stream::StreamExt;
use tokio::sync::{
    mpsc,
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot,
};
use tokio_util::sync::CancellationToken;

use super::{
    details::send_oneshot_and_wait,
    peer2::{self as peer, PeerHandle},
    Answer, Handle,
};
use crate::protocol::{
    server::{ChatLine, PlayerId, SharedMsg, MAX_PLAYER_COUNT},
    AsyncMessageReceiver, GameContext, GameContextKind, Msg,
};
pub type Rx<T> = UnboundedReceiver<T>;
pub type Tx<T> = UnboundedSender<T>;

#[derive(Debug)]
pub enum SharedCmd {
    DropPeer(PlayerId),
}
#[derive(Debug)]
pub enum IntroCmd {
    EnterGame(PlayerId),
}
#[derive(Debug)]
pub enum HomeCmd {
    AddPeer(PeerHandle<peer::HomeCmd>),
    StartRoles,
}
#[derive(Debug)]
pub enum RolesCmd {
    StartGame,
}
#[derive(Debug)]
pub enum GameCmd {}

pub struct Server<T> {
    chat: Vec<ChatLine>,
    peers: Peers<T>,
}

struct ServerHandle(GameContext<(), HomeHandle, RolesHandle, GameHandle>);
pub type IntroHandle = Handle<Msg<SharedCmd, IntroCmd>>;
pub type HomeHandle  = Handle<Msg<SharedCmd, HomeCmd>>;
pub type RolesHandle = Handle<Msg<SharedCmd, RolesCmd>>;
pub type GameHandle  = Handle<Msg<SharedCmd, GameCmd>>;

impl<M> Handle<Msg<SharedCmd, M>>{
    pub fn drop_peer(&self, whom: PlayerId){
        let _ = self.tx.send(Msg::Shared(SharedCmd::DropPeer(whom)));
    }

}

#[derive(Default)]
pub struct IntroServer {
    peers: Peers<peer::IntroCmd>,
    server: Option<ServerHandle>,
}
pub type HomeServer = Server<peer::HomeCmd>;
pub type RolesServer = Server<peer::RolesCmd>;
pub type GameServer = Server<peer::GameCmd>;

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
                    let peer_slot = intro.server.peers.get_peer(sender)?;
                    let handle = send_oneshot_and_wait(&peer_slot.peer.tx, |tx| {
                        Msg::State(peer::IntroCmd::StartHome(h.clone(), tx))
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
        let peer_slot = &intro.peers.get_peer(sender).unwrap();
        let (tx_to_server, rx) = unbounded_channel::<Msg<SharedCmd, HomeCmd>>();
        let peer_handle = send_oneshot_and_wait(&peer_slot.peer.tx, |tx| {
            Msg::State(peer::IntroCmd::StartHome(
                HomeHandle::for_tx(tx_to_server.clone()),
                tx,
            ))
        })
        .await;

        let mut server = HomeServer {
            peers: Default::default(),
            chat: Default::default(),
        };
        server.peers.0[0] = Some(PeerSlot::new(peer_slot.addr, peer_handle));
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
        let peers: [Option<PeerSlot<peer::RolesCmd>>; MAX_PLAYER_COUNT] =
            futures::future::join_all(home.server.peers.slot_iter_mut().map(|p| async {
                let handle = handle.clone();
                futures::future::OptionFuture::from(p.take().map(|p| async move {
                    let peer_handle = send_oneshot_and_wait(&p.peer.tx, |tx| {
                        Msg::State(peer::HomeCmd::StartRoles(handle, tx))
                    })
                    .await;
                    PeerSlot::<peer::RolesCmd> {
                        addr: p.addr,
                        status: p.status,
                        peer: peer_handle,
                    }
                }))
                .await
            }))
            .await
            .try_into()
            .expect("Peers from Home == for Roles");
        StartServer::new(
            RolesServer {
                chat: home.server.chat,
                peers: Peers::<peer::RolesCmd>(peers),
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
        let peers: [Option<PeerSlot<peer::GameCmd>>; MAX_PLAYER_COUNT] =
            futures::future::join_all(roles.server.peers.slot_iter_mut().map(|p| async {
                let handle = handle.clone();
                futures::future::OptionFuture::from(p.take().map(|p| async move {
                    let peer_handle = send_oneshot_and_wait(&p.peer.tx, |tx| {
                        Msg::State(peer::RolesCmd::StartGame(handle, tx))
                    })
                    .await;
                    PeerSlot::<peer::GameCmd> {
                        addr: p.addr,
                        status: p.status,
                        peer: peer_handle,
                    }
                }))
                .await
            }))
            .await
            .try_into()
            .expect("Peers from Roles == for Game");
        StartServer::new(
            GameServer {
                chat: roles.server.chat,
                peers: Peers::<peer::GameCmd>(peers),
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
    status: PeerStatus,
    peer: PeerHandle<T>,
}
impl<T> PeerSlot<T> {
    fn new(addr: PlayerId, handle: PeerHandle<T>) -> Self {
        PeerSlot {
            addr,
            status: PeerStatus::Connected,
            peer: handle,
        }
    }
}
impl<T> Clone for PeerSlot<T> {
    fn clone(&self) -> Self {
        PeerSlot {
            addr: self.addr,
            status: self.status,
            peer: self.peer.clone(),
        }
    }
}

pub struct Peers<T>(pub [Option<PeerSlot<T>>; MAX_PLAYER_COUNT]);
impl<T> Default for Peers<T> {
    fn default() -> Self {
        Peers(Default::default())
    }
}

#[derive(thiserror::Error, Debug)]
#[error("Peer with addr {0} not found in the room")]
pub struct PeerNotFound(pub PlayerId);

impl<T> Peers<T> {
    pub fn iter(&self) -> impl Iterator<Item = &PeerSlot<T>> {
        self.0.iter().filter_map(|p| p.as_ref())
    }
    pub fn slot_iter(&self) -> impl Iterator<Item = &Option<PeerSlot<T>>> {
        self.0.iter()
    }
    pub fn slot_iter_mut(&mut self) -> impl Iterator<Item = &mut Option<PeerSlot<T>>> {
        self.0.iter_mut()
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut PeerSlot<T>> {
        self.0.iter_mut().filter_map(|p| p.as_mut())
    }
    fn get_peer(&self, addr: SocketAddr) -> Result<&PeerSlot<T>, PeerNotFound> {
        self.iter()
            .find(|p| p.addr == addr)
            .ok_or(PeerNotFound(addr))
    }

    async fn broadcast(&self, sender: PlayerId, msg: Msg<SharedMsg, T>) {
        self.impl_broadcast(self.iter().filter(|p| p.addr != sender), msg)
            .await
    }

    async fn broadcast_to_all(&self, msg: Msg<SharedMsg, T>) {
        self.impl_broadcast(self.iter(), msg).await
    }
    async fn impl_broadcast<'a>(
        &'a self,
        peers: impl Iterator<Item = &'a PeerSlot<T>>,
        msg: Msg<SharedMsg, T>,
    ) {
        //trace!("Broadcast {:?}", msg);
        use futures::stream::StreamExt;
        futures::stream::iter(peers)
            .for_each_concurrent(MAX_PLAYER_COUNT, |p| async {
                //p.peer.send_tcp(msg.clone())
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
