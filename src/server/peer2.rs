use super::{states, Answer, Handle};
use crate::protocol::{client, AsyncMessageReceiver, ContextConverter, GameContext, Msg};

pub type PeerHandle<T> = Handle<Msg<self::SharedCmd, T>>;

#[derive(Debug)]
pub enum SharedCmd {}
#[derive(Debug)]
pub enum IntroCmd {
    StartHome(states::HomeHandle, Answer<PeerHandle<HomeCmd>>),
}
#[derive(Debug)]
pub enum HomeCmd {
    StartRoles(states::RolesHandle, Answer<PeerHandle<RolesCmd>>),
}
#[derive(Debug)]
pub enum RolesCmd {
    StartGame(states::GameHandle, Answer<PeerHandle<GameCmd>>),
}
#[derive(Debug)]
pub enum GameCmd {}

pub type IntroHandle = Handle<Msg<SharedCmd, IntroCmd>>;
pub type HomeHandle = Handle<Msg<SharedCmd, HomeCmd>>;
pub type RolesHandle = Handle<Msg<SharedCmd, RolesCmd>>;
pub type GameHandle = Handle<Msg<SharedCmd, GameCmd>>;

pub struct Peer<State>
where
    State: AssociatedServerHandle + AssociatedHandle,
{
    //server: <State as AssociatedServerHandle>::Handle,
    pub state: State,
}

pub struct Intro;
pub struct Home;
pub struct Roles;
pub struct Game;

/*
pub struct Converter<State>
where State: NextState + AssociatedServerHandle + AssociatedHandle,
      <State as NextState>::Next: AssociatedServerHandle
{
    peer: Peer<State>,
    next_server: <<State as NextState>::Next as AssociatedServerHandle>::Handle
}
*/

impl From<Peer<Intro>> for Peer<Home> {
    fn from(value: Peer<Intro>) -> Self {
        Peer { state: Home }
    }
}
impl From<Peer<Home>> for Peer<Roles> {
    fn from(value: Peer<Home>) -> Self {
        Peer { state: Roles }
    }
}
impl From<Peer<Roles>> for Peer<Game> {
    fn from(value: Peer<Roles>) -> Self {
        Peer { state: Game }
    }
}

pub trait IncomingSocketMessage
where
    for<'a> Self::Msg: serde::Deserialize<'a> + core::fmt::Debug,
{
    type Msg;
}
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

pub trait SendSocketMessage
where
    Self::Msg: serde::Serialize + core::fmt::Debug,
{
    type Msg;
}
use crate::protocol::server;
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

use tokio::sync::mpsc::UnboundedReceiver;
type Tx<T> = tokio::sync::mpsc::UnboundedSender<T>;
use std::net::SocketAddr;

use super::AcceptConnection;
#[derive(Clone)]
pub struct Connection<ServerHandle>
where
    ServerHandle: SendSocketMessage,
{
    addr: SocketAddr,
    server: ServerHandle,
    socket: Tx<<ServerHandle as SendSocketMessage>::Msg>,
}
impl<T> Connection<T>
where
    T: SendSocketMessage,
{
    pub fn new(addr: SocketAddr, server: T, socket: Tx<<T as SendSocketMessage>::Msg>) -> Self {
        Connection {
            addr,
            server,
            socket,
        }
    }
}

use anyhow::Context as _;
use futures::SinkExt;
use tracing::{debug, error, info, trace};

use crate::protocol::encode_message;

type Rx<T> = tokio::sync::mpsc::UnboundedReceiver<T>;
pub async fn run_state_handle<PeerStateHandle>(
    accept_connection: &mut AcceptConnection<'_>,
    mut visitor: PeerStateHandle,
    mut state: Connection<<PeerStateHandle as AssociatedServerHandle>::Handle>,
    mut socket: Rx<<PeerStateHandle as SendSocketMessage>::Msg>,
) -> anyhow::Result<()>
where
    for<'a> PeerStateHandle: AssociatedServerHandle
        + IncomingSocketMessage
        + SendSocketMessage
        + AsyncMessageReceiver<
            <PeerStateHandle as IncomingSocketMessage>::Msg,
            &'a mut Connection<<PeerStateHandle as AssociatedServerHandle>::Handle>,
        >,
    <PeerStateHandle as AssociatedServerHandle>::Handle: SendSocketMessage,
{
    loop {
        tokio::select! {
            msg = socket.recv() => match msg {
                Some(msg) => {
                   debug!("{} send {:?}", state.addr, msg);
                   accept_connection.writer.send(encode_message(msg)).await
                        .context("Failed to send a message to the socket")?;
                }
                None => {
                    info!("Socket rx closed for {}", state.addr);
                    // EOF
                    break;
                }
            },

            msg = accept_connection.reader.next::<Msg<client::SharedMsg,
                <PeerStateHandle as IncomingSocketMessage>::Msg>>() => match msg {
                Some(msg) => match msg? {
                    Msg::Shared(_) => {

                    }
                    Msg::State(msg) => {
                        visitor.reduce(
                            msg,
                           &mut state).await?;
                    }
                },
                None => {
                    info!("Connection {} aborted..", state.addr);
                    //state.server.drop_peer(addr);
                    break
                }
            }
        }
    }

    Ok(())
}
pub struct NotifyServer<State, PeerHandle>(pub State, pub oneshot::Sender<PeerHandle>);
use tokio::{net::TcpStream, sync::mpsc};
macro_rules! done {
    ($option:expr) => {
        match $option {
            None => return Ok(()),
            Some(x) => x,
        }
    };
}

use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

use crate::protocol::MessageDecoder;

pub async fn accept_connection(
    socket: &mut TcpStream,
    server: states::IntroHandle,
) -> anyhow::Result<()> {
    let addr = socket.peer_addr()?;
    let (r, w) = socket.split();
    let mut accept_connection = AcceptConnection {
        writer: FramedWrite::new(w, LinesCodec::new()),
        reader: MessageDecoder::new(FramedRead::new(r, LinesCodec::new())),
    };
    macro_rules! run_state {
        ($visitor:expr, $connection:expr, $peer_rx:expr ) => {{
            let (done, mut done_rx) = oneshot::channel();
            let mut state = ReduceState {
                connection: $connection,
                done: Some(done),
            };
            loop {
                tokio::select! {
                    new_state = &mut done_rx => {
                        let new_state = new_state?;
                        return Ok::<Option<_>, anyhow::Error>(Some((Peer::<_>::from($visitor), new_state)))
                    }
                    cmd = $peer_rx.recv() => match cmd {
                        Some(cmd) => match cmd {
                            Msg::Shared(_) =>{

                            },
                            Msg::State(cmd) => {
                                trace!("{} Cmd::{:?}", state.connection.addr, cmd);
                                if let Err(e) = $visitor.reduce(cmd, &mut state).await {
                                    error!("{:#}", e);
                                    break;
                                }
                            }
                        }
                        None => {
                            // EOF. The last PeerHandle has been dropped
                            info!("Drop Peer actor for {}", state.connection.addr);
                            break
                        }
                    }
                }
            }
            Ok(None)
        }};
    }
    macro_rules! run_state_handle {
        ($handle:expr, $connection:expr, $socket:expr ) => {
            async {
            loop {
                tokio::select! {
                    msg = $socket.recv() => match msg {
                        Some(msg) => {
                           debug!("{} send {:?}", $connection.addr, msg);
                           accept_connection.writer.send(encode_message(msg)).await
                                .context("Failed to send a message to the socket")?;
                        }
                        None => {
                            info!("Socket rx closed for {}", $connection.addr);
                            // EOF
                            break;
                        }
                    },

                    msg = accept_connection.reader.next::<Msg<client::SharedMsg, _>>() => match msg {
                        Some(msg) => match msg? {
                            Msg::Shared(_) => {

                            }
                            Msg::State(msg) => {
                                $handle.reduce(
                                    msg,
                                   &mut $connection).await?;
                            }
                        },
                        None => {
                            info!("Connection {} aborted..", $connection.addr);
                            $connection.server.drop_peer(addr);
                            break
                        }
                    }
                }
            }
            Ok::<(), anyhow::Error>(())
        }}
    }

    macro_rules! run_peer {
        ($visitor:expr, $server:expr $(,$notify_server:expr)?) => {
            {
                let (to_peer, mut peer_rx) = mpsc::unbounded_channel();
                let mut handle = Handle::for_tx(to_peer);
                $(let _ = $notify_server.send(handle.clone());)?
                let (to_socket, mut socket_rx) = mpsc::unbounded_channel();
                let mut connection = Connection::new(addr, $server, to_socket);
                let peer_join = tokio::spawn({
                    let connection = connection.clone();
                    async move {
                        return run_state!($visitor, connection, peer_rx);
                    }
                });
                tokio::select!{
                    result =  run_state_handle!(handle, connection,  socket_rx)  => {
                        result?;
                        Ok(None)
                    },
                    new_state = peer_join => {
                        new_state?
                    }
                }
            }
        };

    }

    let mut intro = Peer::<Intro> { state: Intro };

    let (mut home, NotifyServer(server, tx))  = done!(run_peer!(intro, server)?);

    let (mut roles, NotifyServer(server, tx)) = done!(run_peer!(home, server, tx)?);

    let (mut game, NotifyServer(server, tx))  = done!(run_peer!(roles, server, tx)?);

    done!(run_peer!(game, server, tx)?);

    Ok(())
}

pub struct StartPeer<State>(
    pub Peer<State>,
    pub Rx<Msg<SharedCmd, <State as IncomingCommand>::Cmd>>,
)
where
    State: IncomingCommand + AssociatedServerHandle + AssociatedHandle;
pub type StartNextState<State> = StartPeer<State>;

use tokio::sync::oneshot;
pub async fn run_state<PeerState>(
    mut visitor: Peer<PeerState>,
    connection: Connection<
        <<PeerState as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle,
    >,
    mut peer_rx: Rx<Msg<SharedCmd, <PeerState as IncomingCommand>::Cmd>>,
) -> anyhow::Result<
    Option<(
        Peer<<PeerState as NextState>::Next>,
        NotifyServer<
            <<PeerState as NextState>::Next as AssociatedServerHandle>::Handle,
            <<PeerState as NextState>::Next as AssociatedHandle>::Handle,
        >,
    )>,
>
where
    PeerState: NextState + IncomingCommand + AssociatedHandle + AssociatedServerHandle + Send,
    for<'a> Peer<PeerState>:
        AsyncMessageReceiver<<PeerState as IncomingCommand>::Cmd, &'a mut ReduceState<PeerState>>,
    <PeerState as AssociatedHandle>::Handle: AssociatedServerHandle,
    <<PeerState as NextState>::Next as AssociatedHandle>::Handle: AssociatedServerHandle + Send,
    <<PeerState as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle: SendSocketMessage,
    Peer<<PeerState as NextState>::Next>: From<Peer<PeerState>>,
    <PeerState as NextState>::Next: IncomingCommand + AssociatedServerHandle + AssociatedHandle,
{
    let (done, mut done_rx) = oneshot::channel();
    let mut state = ReduceState::<PeerState> {
        connection,
        done: Some(done),
    };

    loop {
        tokio::select! {
            new_state = &mut done_rx => {
                let new_state = new_state?;
                //return Ok(None)
                return Ok(Some((Peer::<<PeerState as NextState>::Next>::from(visitor), new_state )))
            }
            cmd = peer_rx.recv() => match cmd {
                Some(cmd) => match cmd {
                    Msg::Shared(_) =>{

                    },
                    Msg::State(cmd) => {
                        trace!("{} Cmd::{:?}", state.connection.addr, cmd);
                        if let Err(e) = visitor.reduce(cmd, &mut state).await {
                            error!("{:#}", e);
                            break;
                        }
                    }
                }
                None => {
                    // EOF. The last PeerHandle has been dropped
                    info!("Drop Peer actor for {}", state.connection.addr);
                    break
                }
            }
        }
    }
    Ok(None)
}

#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<client::IntroMsg, &'a mut Connection<states::IntroHandle>>
    for IntroHandle
{
    async fn reduce(
        &mut self,
        msg: client::IntroMsg,
        state: &'a mut Connection<states::IntroHandle>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<client::HomeMsg, &'a mut Connection<states::HomeHandle>>
    for HomeHandle
{
    async fn reduce(
        &mut self,
        msg: client::HomeMsg,
        state: &'a mut Connection<states::HomeHandle>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<client::RolesMsg, &'a mut Connection<states::RolesHandle>>
    for RolesHandle
{
    async fn reduce(
        &mut self,
        msg: client::RolesMsg,
        state: &'a mut Connection<states::RolesHandle>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<client::GameMsg, &'a mut Connection<states::GameHandle>>
    for GameHandle
{
    async fn reduce(
        &mut self,
        msg: client::GameMsg,
        state: &'a mut Connection<states::GameHandle>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

pub trait NextState
where
    Self::Next: AssociatedHandle + Send + AssociatedServerHandle,
{
    type Next;
}
impl NextState for Intro {
    type Next = Home;
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

pub struct ReduceState<ContextState>
where
    ContextState: NextState + AssociatedHandle,
    <ContextState as AssociatedHandle>::Handle: AssociatedServerHandle,
    <ContextState as NextState>::Next: AssociatedHandle,
    <<ContextState as NextState>::Next as AssociatedHandle>::Handle: AssociatedServerHandle,
{
    connection:
        Connection<<<ContextState as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle>,
    done: Option<
        oneshot::Sender<
            NotifyServer<
                <<ContextState as NextState>::Next as AssociatedServerHandle>::Handle,
                <<ContextState as NextState>::Next as AssociatedHandle>::Handle,
            >,
        >,
    >,
}

#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<IntroCmd, &'a mut ReduceState<Intro>> for Peer<Intro> {
    async fn reduce(
        &mut self,
        msg: IntroCmd,
        state: &'a mut ReduceState<Intro>,
    ) -> anyhow::Result<()> {
        match msg {
            IntroCmd::StartHome(home_server, tx) => {
                let _ = state
                    .done
                    .take()
                    .unwrap()
                    .send(NotifyServer(home_server, tx));
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
        Ok(())
    }
}
