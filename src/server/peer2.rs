
use super::{Handle, Answer};
use crate::protocol::Msg;
use super::states;
use crate::protocol::{ContextConverter, GameContext, AsyncMessageReceiver, client};


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
pub type HomeHandle  = Handle<Msg<SharedCmd, HomeCmd>>;
pub type RolesHandle = Handle<Msg<SharedCmd, RolesCmd>>;
pub type GameHandle  = Handle<Msg<SharedCmd, GameCmd>>;

pub struct Intro;
pub struct Home;
pub struct Roles;
pub struct Game;

pub struct Peer<T>{
    state: T
}



macro_rules! done {
    ($option:expr) => {
        match $option {
            None => return Ok(()),
            Some(x) => x,
        }
    };
}


trait IncomingSocketMessage 
where
   for<'a> Self::Msg: serde::Deserialize<'a> + core::fmt::Debug,

{  
    type Msg;
}
impl IncomingSocketMessage for IntroHandle { type Msg=client::IntroMsg; }
impl IncomingSocketMessage for HomeHandle  { type Msg=client::HomeMsg; }
impl IncomingSocketMessage for RolesHandle { type Msg=client::RolesMsg; }
impl IncomingSocketMessage for GameHandle  { type Msg=client::GameMsg; }

trait SendSocketMessage 
where
   Self::Msg: serde::Serialize + core::fmt::Debug,
{ 
    type Msg; 
}
use crate::protocol::server;
impl SendSocketMessage for IntroHandle { type Msg=Msg<server::SharedMsg, server::IntroMsg>; }
impl SendSocketMessage for HomeHandle  { type Msg=Msg<server::SharedMsg, server::HomeMsg>; }
impl SendSocketMessage for RolesHandle { type Msg=Msg<server::SharedMsg, server::RolesMsg>; }
impl SendSocketMessage for GameHandle  { type Msg=Msg<server::SharedMsg, server::GameMsg>; }
impl SendSocketMessage for states::IntroHandle { type Msg=Msg<server::SharedMsg, server::IntroMsg>; }
impl SendSocketMessage for states::HomeHandle  { type Msg=Msg<server::SharedMsg, server::HomeMsg>; }
impl SendSocketMessage for states::RolesHandle { type Msg=Msg<server::SharedMsg, server::RolesMsg>; }
impl SendSocketMessage for states::GameHandle  { type Msg=Msg<server::SharedMsg, server::GameMsg>; }
impl SendSocketMessage for Intro       { type Msg=Msg<server::SharedMsg, server::IntroMsg>; }
impl SendSocketMessage for Home        { type Msg=Msg<server::SharedMsg, server::HomeMsg>; }
impl SendSocketMessage for Roles       { type Msg=Msg<server::SharedMsg, server::RolesMsg>; }
impl SendSocketMessage for Game        { type Msg=Msg<server::SharedMsg, server::GameMsg>; }

trait AssociatedHandle
where Self::Handle: SendSocketMessage
{
    type Handle;
}
impl AssociatedHandle for Intro{ type Handle = IntroHandle; }
impl AssociatedHandle for Home{ type Handle = HomeHandle; }
impl AssociatedHandle for Roles{ type Handle = RolesHandle; }
impl AssociatedHandle for Game{ type Handle = GameHandle; }
trait AssociatedServerHandle
where Self::Handle: SendSocketMessage
{
    type Handle;
}

impl AssociatedServerHandle for IntroHandle{ type Handle = states::IntroHandle; }
impl AssociatedServerHandle for HomeHandle{ type Handle = states::HomeHandle; }
impl AssociatedServerHandle for RolesHandle{ type Handle = states::RolesHandle; }
impl AssociatedServerHandle for GameHandle{ type Handle = states::GameHandle; }
trait IncomingCommand 
where
   for<'a> Self::Cmd: core::fmt::Debug + Send,

{  
    type Cmd;
}
impl IncomingCommand for Intro { type Cmd = IntroCmd; }
impl IncomingCommand for Home { type Cmd = HomeCmd; }
impl IncomingCommand for Roles { type Cmd = RolesCmd; }
impl IncomingCommand for Game { type Cmd = GameCmd; }

use tokio::sync::mpsc::UnboundedReceiver;
type Tx<T> = tokio::sync::mpsc::UnboundedSender<T>;
use super::AcceptConnection;
use std::net::SocketAddr;
#[derive(Clone)]
pub struct Connection<ServerHandle>
where ServerHandle: SendSocketMessage
{
    addr: SocketAddr,
    server: ServerHandle,
    socket: Tx<<ServerHandle as SendSocketMessage>::Msg>,
}
pub type IntroConnection = Connection<states::IntroHandle>;
impl<T> Connection<T>
where T: SendSocketMessage
{
    pub fn new(addr: SocketAddr, server: T, socket: Tx<<T as SendSocketMessage>::Msg>) -> Self{
        Connection{addr, server, socket}
    }
}

pub async fn run_peer<PeerState>(
    accept_connection: &mut AcceptConnection<'_>,
    visitor: PeerState, 
    connection: Connection<<<PeerState as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle>,
    socket_rx:  UnboundedReceiver<<<PeerState as AssociatedHandle>::Handle as SendSocketMessage>::Msg>,
    peer_cmd_rx: Rx<Msg<SharedCmd, <PeerState as IncomingCommand>::Cmd>>,
    peer_handle: <PeerState as AssociatedHandle>::Handle,

    ) 
 -> anyhow::Result<Option<(<<<PeerState as NextState>::Next as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle,
                                 oneshot::Sender<<<PeerState as NextState>::Next as AssociatedHandle>::Handle>)>>

    where for<'a>  PeerState:  Send + 'static + NextState +
                IncomingCommand  +
                AssociatedHandle + 
                AsyncMessageReceiver<<PeerState as IncomingCommand>::Cmd,  &'a mut ReduceState::<PeerState>>,
         Connection<<<PeerState as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle>: Clone + Send,
         <<PeerState as AssociatedHandle>::Handle as AssociatedServerHandle> ::Handle: SendSocketMessage, 

        for<'a> <PeerState as AssociatedHandle>::Handle: 
                From<Tx<Msg<SharedCmd, <PeerState as IncomingCommand>::Cmd>>> + 
                AssociatedServerHandle + 
                IncomingSocketMessage +
                AsyncMessageReceiver<<<PeerState as AssociatedHandle>::Handle as IncomingSocketMessage>::Msg,
                    &'a mut Connection<<<PeerState as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle>>,
        <PeerState as IncomingCommand>::Cmd: 'static,
        <<PeerState as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle: 'static,
        <<PeerState as NextState>::Next as AssociatedHandle>::Handle: AssociatedServerHandle + Send + Sync,
        <<<PeerState as NextState>::Next as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle: Send + Sync,
        <<PeerState as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle: Send + Sync,
        <<<PeerState as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle as SendSocketMessage>::Msg: Send + Sync

{
    
    let peer_join = tokio::spawn({
        let connection = connection.clone();
        async move {
            return run_state(visitor, connection, peer_cmd_rx).await;
        }
    });
    tokio::select!{
        result =  run_state_handle(accept_connection,  peer_handle, connection,  socket_rx)  => {
            result;
            Ok(None)
        },
        new_state = peer_join => {
            new_state?
        }

    }
}

use futures::SinkExt;
use anyhow::Context as _;
use crate::protocol::encode_message;
use tracing::{debug, info, trace, error};

type Rx<T> = tokio::sync::mpsc::UnboundedReceiver<T>;
async fn run_state_handle<PeerStateHandle>(accept_connection: &mut AcceptConnection<'_>, 
                                          mut visitor: PeerStateHandle, 
                                          mut state:  Connection<<PeerStateHandle as AssociatedServerHandle>::Handle>,
                                          mut socket: Rx<<PeerStateHandle as SendSocketMessage>::Msg>) -> anyhow::Result<()> 
    where for<'a>  PeerStateHandle:  
                AssociatedServerHandle  + 
                IncomingSocketMessage +
                SendSocketMessage +
                AsyncMessageReceiver<<PeerStateHandle as IncomingSocketMessage>::Msg
                    , &'a mut Connection<<PeerStateHandle as AssociatedServerHandle>::Handle>>,
        <PeerStateHandle as AssociatedServerHandle>::Handle: SendSocketMessage
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
use tokio::sync::oneshot;
async fn run_state<PeerState>(mut visitor: PeerState, 
                              connection: Connection<<<PeerState as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle>,
                              mut peer_rx: Rx<Msg<SharedCmd, <PeerState as IncomingCommand>::Cmd>>,
                              //mut cancel: oneshot::Receiver<()>
 
                             ) -> anyhow::Result<Option<(
                                 <<<PeerState as NextState>::Next as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle,
                                 oneshot::Sender<<<PeerState as NextState>::Next as AssociatedHandle>::Handle>)>>

    where  for<'a> PeerState: 
            NextState +
            Send +
            IncomingCommand +
            AssociatedHandle +
            AsyncMessageReceiver<<PeerState as IncomingCommand>::Cmd,  &'a mut ReduceState::<PeerState>>,
         <PeerState as AssociatedHandle>::Handle: AssociatedServerHandle,
        <<PeerState as NextState>::Next as AssociatedHandle>::Handle: AssociatedServerHandle + Send,
        <<PeerState as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle : SendSocketMessage
{

    let (done , mut done_rx) = oneshot::channel();
    let mut state = ReduceState::<PeerState>{connection, done: Some(done) };

    loop {
        tokio::select!{
            new_state = &mut done_rx => {
                return Ok(Some(new_state?))
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
impl<'a> AsyncMessageReceiver<client::IntroMsg, &'a mut Connection<states::IntroHandle>> for IntroHandle {
    async fn reduce(
        &mut self,
        msg: client::IntroMsg,
        state: &'a mut IntroConnection,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<client::HomeMsg, &'a mut Connection<states::HomeHandle>> for HomeHandle {
    async fn reduce(
        &mut self,
        msg: client::HomeMsg,
        state: &'a mut Connection<states::HomeHandle>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<client::RolesMsg, &'a mut Connection<states::RolesHandle>> for RolesHandle {
    async fn reduce(
        &mut self,
        msg: client::RolesMsg,
        state: &'a mut Connection<states::RolesHandle>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<client::GameMsg, &'a mut Connection<states::GameHandle>> for GameHandle {
    async fn reduce(
        &mut self,
        msg: client::GameMsg,
        state: &'a mut Connection<states::GameHandle>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

trait NextState
where Self::Next: AssociatedHandle + Send,
  <Self::Next as AssociatedHandle>::Handle: AssociatedServerHandle
{ type Next; }
impl NextState for Intro { type Next = Home; }
impl NextState for Home {  type Next = Roles; }
impl NextState for Roles { type Next = Game; }
impl NextState for Game {  type Next = Game; }

struct ReduceState<ContextState>
where  ContextState:  NextState + AssociatedHandle,
      <ContextState as AssociatedHandle>::Handle: AssociatedServerHandle,
      <ContextState as NextState>::Next: AssociatedHandle,
      <<ContextState as NextState>::Next as AssociatedHandle>::Handle: AssociatedServerHandle
{
    connection: Connection<<<ContextState as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle>,
    done: Option<oneshot::Sender<(<<<ContextState as NextState>::Next as AssociatedHandle>::Handle as AssociatedServerHandle>::Handle, 
                           oneshot::Sender<<<ContextState as NextState>::Next as AssociatedHandle>::Handle>)>>
}



#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<IntroCmd,  &'a mut  ReduceState<Intro> > for Intro {
    async fn reduce(
        &mut self,
        msg: IntroCmd,
        state:  &'a mut  ReduceState<Intro> ,
    ) -> anyhow::Result<()> {
        match msg {
            IntroCmd::StartHome(home_server, tx) => {
                let _ = state.done.take().unwrap().send((home_server, tx));

            }
        }
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<HomeCmd,   &'a mut  ReduceState<Home> > for Home {
    async fn reduce(
        &mut self,
        msg: HomeCmd,
        state:   &'a mut  ReduceState<Home> ,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<RolesCmd,   &'a mut ReduceState<Roles>   > for Roles {
    async fn reduce(
        &mut self,
        msg: RolesCmd,
        state: &'a mut ReduceState<Roles>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
#[async_trait::async_trait]
impl<'a> AsyncMessageReceiver<GameCmd,  &'a mut ReduceState<Game>  > for Game {
    async fn reduce(
        &mut self,
        msg: GameCmd,
        state:  &'a mut ReduceState<Game>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

