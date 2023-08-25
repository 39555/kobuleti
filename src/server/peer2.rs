use super::{states, Answer, Handle};
use crate::protocol::{client, AsyncMessageReceiver, Msg};
use tokio::sync::oneshot;
use anyhow::Context as _;
use futures::SinkExt;
use tracing::{debug, error, info, trace, warn};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use crate::protocol::{MessageDecoder, Username};
use crate::protocol::encode_message;

use tokio::{net::TcpStream, sync::mpsc};

pub type PeerHandle<T> = Handle<Msg<self::SharedCmd, T>>;

#[derive(Debug)]
pub enum SharedCmd {
    Close,
}
#[derive(Debug)]
pub enum IntroCmd {
    SetUsername(Username),
    EnterGame(GameContext<(), 
              (states::HomeHandle , Answer<HomeHandle> ), 
              (states::RolesHandle, Answer<RolesHandle>), 
              (states::GameHandle , Answer<GameHandle> )>
              ),
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
    pub state: State,
}

pub struct Intro;
pub struct Home;
pub struct Roles;
pub struct Game;


impl From<Peer<Intro>> for Peer<Home> {
    fn from(value: Peer<Intro>) -> Self {
        Peer { state: Home }
    }
}
impl From<Peer<Intro>> for Peer<Roles> {
    fn from(value: Peer<Intro>) -> Self {
        Peer { state: Roles }
    }
}
impl From<Peer<Intro>> for Peer<Game> {
    fn from(value: Peer<Intro>) -> Self {
        Peer { state: Game }
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
    socket: Option<Tx<<ServerHandle as SendSocketMessage>::Msg>>,
}
impl<T> Connection<T>
where
    T: SendSocketMessage,
{
    pub fn new(addr: SocketAddr, server: T, socket: Tx<<T as SendSocketMessage>::Msg>) -> Self {
        Connection {
            addr,
            server,
            socket: Some(socket),
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



struct NotifyServer<State, PeerHandle>(pub State, pub oneshot::Sender<PeerHandle>);
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
        ($visitor:expr, $connection:expr, $peer_rx:expr ) => { async {
            let (done, mut done_rx) = oneshot::channel();
            let mut state = ReduceState {
                connection: $connection,
                done: Some(done),
            };
            loop {
                tokio::select! {
                    new_state = &mut done_rx => {
                        let new_state = new_state?;
                        return Ok::<Option<_>, anyhow::Error>(Some(($visitor, new_state)))
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
            async {
                let mut visitor = $visitor;
                let (to_peer, mut peer_rx) = mpsc::unbounded_channel();
                let mut handle = Handle::for_tx(to_peer);
                $(let _ = $notify_server.send(handle.clone());)?
                let (to_socket, mut socket_rx) = mpsc::unbounded_channel();
                let mut connection = Connection::new(addr, $server, to_socket);
                let peer_join = tokio::spawn({
                    let connection = connection.clone();
                    async move {
                        return run_state!(visitor, connection, peer_rx).await;
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


    let (intro, start_state)  = done!(run_peer!(Peer::<Intro> { state: Intro }, server).await?);

    match start_state {
        GameContext::Home((server, tx)) => {
            let (home,  NotifyServer(server, tx))  = done!(run_peer!(Peer::<Home>::from(intro), server, tx).await?);
            let (roles, NotifyServer(server, tx))  = done!(run_peer!(Peer::<Roles>::from(home), server, tx).await?);
            done!(run_peer!(Peer::<Game>::from(roles), server, tx).await?);

        },
        GameContext::Roles((server, tx)) => {
            let (roles, NotifyServer(server, tx))  = done!(run_peer!(Peer::<Roles>::from(intro), server, tx).await?);
            done!(run_peer!(Peer::<Game>::from(roles), server, tx).await?);

        },
        GameContext::Game((server, tx)) => {
            done!(run_peer!(Peer::<Game>::from(intro), server, tx).await?);

        }
        _ => unreachable!(),
    };
    Ok(())
}


async fn close_peer<S, M>(state: &mut Connection<S>, peer: &Handle<Msg<SharedCmd, M>>)
where S: SendSocketMessage
{
    // should close but wait the socket writer EOF,
    // so it just drops socket tx
    let _ = peer.tx.send(Msg::Shared(SharedCmd::Close));
    trace!("Close the socket tx on the PeerHandle side");
    state.socket = None;
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
        use client::IntroMsg;
        use crate::protocol::server::LoginStatus;
        match msg {
            IntroMsg::AddPlayer(username) => {
                info!("{} is trying to login as {}", state.addr, &username);
                let status = state
                    .server
                    .login_player(state.addr, username, self.clone())
                    .await;
                trace!("Connection status: {:?}", status);
                let _ = state
                    .socket
                    .as_ref()
                    .unwrap()
                    .send(Msg::State(server::IntroMsg::LoginStatus(status)));
                match status {
                    LoginStatus::Logged => (),
                    LoginStatus::Reconnected => {
                        // TODO
                        // this get handle to previous peer actor and drop the current handle,
                        // so new actor will shutdown
                        //*self.0 = state.server.get_peer_handle(state.addr).await;
                        //send_oneshot_and_wait(&self.0.tx, |oneshot| {
                        //    PeerCmd::SyncReconnection(state.clone(), oneshot)
                        //})
                        //.await;
                        let _ = state.socket.as_ref().unwrap().send(Msg::Shared(
                            server::SharedMsg::ChatLog(state.server.get_chat_log().await),
                        ));
                    }
                    _ => {
                        // connection fail
                        warn!("Login attempt rejected = {:?}", status);
                        close_peer(state, self).await;
                    }
                }
            }
            _ => (),
        }


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
    Self::Next: Send ,
{
    type Next;
}

use crate::protocol::GameContext;
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


trait DoneType{
    type Type;
}
impl DoneType for Intro{
    type Type = GameContext<(), 
              (states::HomeHandle , Answer<HomeHandle> ), 
              (states::RolesHandle, Answer<RolesHandle>), 
              (states::GameHandle , Answer<GameHandle> )>;
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
done_type!{Home, Roles, Game,}


struct ReduceState<ContextState>
where
     ContextState: AssociatedServerHandle + DoneType,
{
    connection:
        Connection<<ContextState as AssociatedServerHandle>::Handle>,
    done: Option<
        oneshot::Sender<<ContextState as DoneType>::Type>,
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
            IntroCmd::EnterGame(home_server) => {
                let _ = state
                    .done
                    .take()
                    .unwrap()
                    .send(home_server);
            }
            _ => (),
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
