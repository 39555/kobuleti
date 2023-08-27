use std::{future::Future, net::SocketAddr};

use anyhow::Context as _;
use futures::SinkExt;
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use tracing::{debug, error, info, trace};

use crate::protocol::{
    client, encode_message, server,
    server::{Intro, ServerGameContext},
    AsyncMessageReceiver, MessageDecoder,
};
type Answer<T> = oneshot::Sender<T>;
//type Tx<T> = tokio::sync::mpsc::UnboundedSender<T>;

#[derive(Debug)]
pub struct Handle<T> {
    pub tx: tokio::sync::mpsc::UnboundedSender<T>,
}
impl<T> From<Tx<T>> for Handle<T> {
    fn from(value: Tx<T>) -> Self {
        Handle::for_tx(value)
    }
}
impl<T> Handle<T> {
    pub fn for_tx(tx: Tx<T>) -> Self {
        Handle { tx }
    }
}
impl<T> Clone for Handle<T> {
    fn clone(&self) -> Handle<T> {
        Handle {
            tx: self.tx.clone(),
        }
    }
}

pub mod commands;
pub mod details;
pub mod peer;
pub mod peer2;
pub mod session;
pub mod states;
use commands::{Room, Server, ServerCmd, ServerHandle};
use peer::{Peer, PeerHandle};
use tokio::sync::{mpsc::UnboundedReceiver, oneshot};

use crate::{
    protocol::server::{PlayerId, MAX_PLAYER_COUNT},
    server::commands::PeerStatus,
};

pub async fn listen(
    addr: SocketAddr,
    shutdown: impl Future<Output = std::io::Result<()>>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind a socket to {}", addr))?;
    info!("Listening on: {}", addr);
    let (tx, rx) = mpsc::unbounded_channel();
    let mut join_server = tokio::spawn(async move {
        states::start_intro_server(&mut states::StartServer::new(
            states::IntroServer::default(),
            rx,
        ))
        .await;
    });

    let server_handle = states::IntroHandle::for_tx(tx);

    trace!("Listen for new connections..");
    tokio::select! {
        _ = &mut join_server => {
            Ok(())
        }
        _ = async {
            loop {
                match listener.accept().await {
                    Err(e) => {
                        error!("Failed to accept a new connection {:#}", e);
                        continue;
                    },
                    Ok((mut stream, addr)) => {
                        info!("{} has connected", addr);
                        trace!("Start a task for process connection");
                        tokio::spawn({
                            let server_handle = server_handle.clone();
                            async move {
                            if let Err(e) = peer2::accept_connection(&mut stream,
                                                               server_handle)
                                .await {
                                    error!("Process connection error = {:#}", e);
                            }
                            let _ = stream.shutdown().await;
                            info!("{} has disconnected", addr);
                        }});
                     }
                }
            }
        } => Ok(()),

        sig = shutdown =>{
            match sig {
               Ok(_)    => info!("Shutdown signal has been received..") ,
               Err(err) => error!("Unable to listen for shutdown signal: {:#}", err)
            };
            // send shutdown signal to the server actor and wait
            //server_handle.shutdown().await;
            Ok(())
        }
    }
}
pub type Tx<T> = mpsc::UnboundedSender<T>;

/*
macro_rules! done {
    ($option:expr) => {
        match $option {
            None => return Ok(()),
            Some(x) => x,
        }
    };
}

async fn accept_connection(
    socket: &mut TcpStream,
    server: states::IntroHandle,
) -> anyhow::Result<()> {
    let addr = socket.peer_addr()?;
    let (r, w) = socket.split();
    let mut accept_connection = AcceptConnection {
        writer: FramedWrite::new(w, LinesCodec::new()),
        reader: MessageDecoder::new(FramedRead::new(r, LinesCodec::new())),
    };
    macro_rules! run_peer {
        ($server:expr,$visitor:expr $(,$notify_server:expr)?) => {
            {
                let (to_peer, peer_rx) = mpsc::unbounded_channel();
                let handle = Handle::for_tx(to_peer);
                $(let _ = $notify_server.send(handle.clone());)?
                let (to_socket, socket_rx) = mpsc::unbounded_channel();
                let connection = peer2::Connection::new(addr, $server, to_socket);
                let peer_join = tokio::spawn({
                let connection = connection.clone();
                async move {
                    return peer2::run_state($visitor, connection, peer_rx).await;
                }
                });
                tokio::select!{
                    _result =  peer2::run_state_handle(&mut accept_connection,  handle, connection,  socket_rx)  => {
                        //result;
                        Ok(None)
                    },
                    new_state = peer_join => {
                        new_state?
                    }
                }
            }
        };

    }

    let intro = peer2::Peer::<peer2::Intro> {
        state: peer2::Intro,
    };

    let (home, peer2::NotifyServer(server, tx)) = done!(run_peer!(server, intro)?);

    let (roles, peer2::NotifyServer(server, tx)) = done!(run_peer!(server, home, tx)?);

    let (game, peer2::NotifyServer(server, tx)) = done!(run_peer!(server, roles, tx)?);

    done!(run_peer!(server, game, tx)?);

    Ok(())
}
*/
/*
 * async fn run_as<State, M, Cmd>(
        &mut self,
        (mut visitor, mut rx, mut visitor_handle): (
            State,
            ClientRx<Cmd>,
            <State as HandleType>::Handle,
        ),
        state: &mut Connection,
    ) -> anyhow::Result<Option<StartPeer>>
    where
        for<'a> <State as HandleType>::Handle: AsyncMessageReceiver<M, &'a mut Connection>
            + MessageSender
            + From<tokio::sync::mpsc::UnboundedSender<Msg2<PeerCmd, Cmd>>>
            + Send
            + Sync,
        for<'a> Msg2<client::AppMsg, M>: serde::Deserialize<'a>,
        <<State as HandleType>::Handle as MessageSender>::MsgType: serde::Serialize,
        for<'a> State: AsyncMessageReceiver<Cmd, &'a mut Connection> + HandleType + Send + 'static,
        Cmd: Send + Sync + 'static,
        State: Into<GameContext<Intro, Home, Roles, Game>>,
    {
        let (socket_tx, mut socket_rx) =
            mpsc::unbounded_channel::<<<State as HandleType>::Handle as MessageSender>::MsgType>();

        let mut peer_task = tokio::spawn({
            let mut state = state.clone();
            async move {
                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        Msg2::Shared(peer_cmd) => match peer_cmd {
                            PeerCmd::NextContext(next, tx) => {
                                // TODO
                                let kind = GameContextKind::from(&next);
                                // next.server_tx
                                use crate::protocol::server::ConvertedContext;
                                let ConvertedContext(new_context, client_data) =
                                    ConvertedContext::try_from(ContextConverter(
                                        ServerGameContext(visitor.into()),
                                        next,
                                    ))?;
                                match kind {
                                    GameContextKind::Intro => {
                                        let (tx, mut rx) =
                                            mpsc::unbounded_channel::<Msg2<PeerCmd, IntroCmd>>();
                                        let mut handle = <<Intro as HandleType>::Handle>::from(tx);
                                        // tx.send(handle)
                                        return Ok::<
                                            Option<(ServerGameContext, ClientRxState)>,
                                            anyhow::Error,
                                        >(Some((
                                            new_context,
                                            ClientRxState(GameContext::Intro(rx)),
                                        )));
                                    }
                                    GameContextKind::Home => {}
                                    GameContextKind::Roles => {}
                                    GameContextKind::Game => {}
                                }
                                break;
                                // tx.send()
                            }
                            _ => todo!(),
                        },
                        Msg2::State(state_cmd) => {
                            if let Err(e) = visitor.reduce(state_cmd, &mut state).await {
                                error!("{:#}", e);
                                break;
                            }
                        } //trace!("{} PeerCmd::{:?}", addr, cmd);
                    }
                }
                Ok(None)
            }
        });
        //
        loop {
            tokio::select! {

                next_context = &mut peer_task => {
                    //return next_context?;
                    break
                }

                msg = socket_rx.recv() => match msg {
                    Some(msg) => {
                       //debug!("{} send {:?}", addr, msg);
                       self.writer.send(encode_message(msg)).await
                            .context("Failed to send a message to the socket")?;
                    }
                    None => {
                        //info!("Socket rx closed for {}", addr);
                        // EOF
                        break;
                    }
                },

                msg = self.reader.next::<Msg2<client::AppMsg, M>>() => match msg {
                    Some(msg) => match msg? {
                        Msg2::Shared(shared_msg) => {

                        }
                        Msg2::State(state_msg) => {
                            visitor_handle.reduce(
                                state_msg,
                                state).await?;
                        }
                    },
                    None => {
                        //info!("Connection {} aborted..", addr);
                        //state.server.drop_peer(addr);
                        break
                    }
                }
            }
        }

        Ok(None)
    }
}

 *
 * */

/*
pub async fn listen(
    addr: SocketAddr,
    shutdown: impl Future<Output = std::io::Result<()>>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind a socket to {}", addr))?;
    info!("Listening on: {}", addr);
    let (to_server, mut server_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut state = Room::default();
        let mut server = Server::default();
        trace!("Spawn a server actor");
        loop {
            if let Some(command) = server_rx.recv().await {
                if let Err(e) = server.reduce(command, &mut state).await {
                    error!(
                        "failed to process an \
internal command by the server actor = {:#}",
                        e
                    );
                }
            };
        }
    });
    let server_handle = ServerHandle::for_tx(to_server);

    trace!("Listen for new connections..");
    tokio::select! {
        _ = async {
            loop {
                match listener.accept().await {
                    Err(e) => {
                        error!("Failed to accept a new connection {:#}", e);
                        continue;
                    },
                    Ok((mut stream, addr)) => {
                        info!("{} has connected", addr);
                        let server_handle_for_peer = server_handle.clone();
                        trace!("Start a task for process connection");
                        tokio::spawn(async move {
                            if let Err(e) = accept_connection(&mut stream,
                                                               server_handle_for_peer)
                                .await {
                                    error!("Process connection error = {:#}", e);
                            }
                            let _ = stream.shutdown().await;
                            info!("{} has disconnected", addr);
                        });
                     }
                }
            }
        } => Ok(()),

        sig = shutdown =>{
            match sig {
               Ok(_)    => info!("Shutdown signal has been received..") ,
               Err(err) => error!("Unable to listen for shutdown signal: {:#}", err)
            };
            // send shutdown signal to the server actor and wait
            server_handle.shutdown().await;
            Ok(())
        }
    }
}

async fn accept_connection(socket: &mut TcpStream, server: ServerHandle) -> anyhow::Result<()> {
    let addr = socket.peer_addr()?;
    let (r, w) = socket.split();
    let (tx, mut to_socket_rx) = mpsc::unbounded_channel::<server::Msg>();

    let mut connection = Connection::new(addr, tx, server);

    trace!("Spawn a Peer actor for {}", addr);
    let (to_peer, mut peer_rx) = mpsc::unbounded_channel();
    // A peer actor does not drop while tcp io loop alive, or while a server room
    // will not drop a player, because they hold a peer_handle
    tokio::spawn({
        let mut connection = connection.clone();
        async move {
            let mut peer = Peer::new(ServerGameContext::from(Intro::default()));
            while let Some(cmd) = peer_rx.recv().await {
                trace!("{} PeerCmd::{:?}", addr, cmd);
                if let Err(e) = peer.reduce(cmd, &mut connection).await {
                    error!("{:#}", e);
                    break;
                }
            }
            // EOF. The last PeerHandle has been dropped
            info!("Drop Peer actor for {}", addr);
        }
    });

    let mut peer_handle = PeerHandle::for_tx(to_peer);

    // tcp io
    let mut socket_writer = FramedWrite::new(w, LinesCodec::new());
    let mut socket_reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));
    loop {
        tokio::select! {
            msg = to_socket_rx.recv() => match msg {
                Some(msg) => {
                    debug!("{} send {:?}", addr, msg);
                    socket_writer.send(encode_message(msg)).await
                        .context("Failed to send a message to the socket")?;
                }
                None => {
                    info!("Socket rx closed for {}", addr);
                    // EOF
                    break;
                }
            },
            msg = socket_reader.next::<client::Msg>() => match msg {
                Some(msg) => {

                    peer_handle.reduce(
                        msg.context("Failed to receive a message from the client")?,
                        &mut connection).await?;
                },
                None => {
                    info!("Connection {} aborted..", addr);
                    connection.server.drop_peer(addr);
                    break
                }
            }
        }
    }
    Ok(())
}

*/

/*

trait MessageSender {
    type MsgType;
}
use crate::protocol::server::{Game, Home, Roles};
impl MessageSender for IntroHandle {
    type MsgType = Msg2<server::SharedMsg, server::IntroMsg>;
}
impl MessageSender for HomeHandle {
    type MsgType = Msg2<server::SharedMsg, server::HomeMsg>;
}
impl MessageSender for GameHandle {
    type MsgType = Msg2<server::SharedMsg, server::GameMsg>;
}
impl MessageSender for RolesHandle {
    type MsgType = Msg2<server::SharedMsg, server::RolesMsg>;
}
use crate::server::peer::{GameCmd, HomeCmd, IntroCmd, PeerCmd, RolesCmd};
impl MessageSender for Intro {
    type MsgType = Msg2<PeerCmd, IntroCmd>;
}
impl MessageSender for Home {
    type MsgType = Msg2<PeerCmd, IntroCmd>;
}
impl MessageSender for Game {
    type MsgType = Msg2<PeerCmd, IntroCmd>;
}
impl MessageSender for Roles {
    type MsgType = Msg2<PeerCmd, IntroCmd>;
}

impl ServerGameContextHandle {
    pub fn as_inner<'a>(
        &'a self,
    ) -> &'a GameContext<IntroHandle, HomeHandle, RolesHandle, GameHandle> {
        &self.0
    }
    pub fn as_inner_mut(
        &mut self,
    ) -> &mut GameContext<IntroHandle, HomeHandle, RolesHandle, GameHandle> {
        &mut self.0
    }
}

trait HandleType {
    type Handle;
}
impl HandleType for Intro {
    type Handle = IntroHandle;
}
impl HandleType for Home {
    type Handle = HomeHandle;
}
impl HandleType for Roles {
    type Handle = RolesHandle;
}
impl HandleType for Game {
    type Handle = GameHandle;
}

use tokio::sync::mpsc::UnboundedSender;

impl From<UnboundedSender<Msg2<PeerCmd, IntroCmd>>> for IntroHandle {
    fn from(value: UnboundedSender<Msg2<PeerCmd, IntroCmd>>) -> Self {
        IntroHandle
    }
}
impl From<UnboundedSender<Msg2<PeerCmd, HomeCmd>>> for HomeHandle {
    fn from(value: UnboundedSender<Msg2<PeerCmd, HomeCmd>>) -> Self {
        HomeHandle
    }
}
impl From<UnboundedSender<Msg2<PeerCmd, RolesCmd>>> for RolesHandle {
    fn from(value: UnboundedSender<Msg2<PeerCmd, RolesCmd>>) -> Self {
        RolesHandle
    }
}
impl From<UnboundedSender<Msg2<PeerCmd, GameCmd>>> for GameHandle {
    fn from(value: UnboundedSender<Msg2<PeerCmd, GameCmd>>) -> Self {
        GameHandle
    }
}
impl From<Intro> for GameContext<Intro, Home, Roles, Game> {
    fn from(value: Intro) -> Self {
        todo!()
    }
}
impl From<Home> for GameContext<Intro, Home, Roles, Game> {
    fn from(value: Home) -> Self {
        todo!()
    }
}
impl From<Roles> for GameContext<Intro, Home, Roles, Game> {
    fn from(value: Roles) -> Self {
        todo!()
    }
}
impl From<Game> for GameContext<Intro, Home, Roles, Game> {
    fn from(value: Game) -> Self {
        todo!()
    }
}

pub type ClientRx<T> = UnboundedReceiver<Msg2<PeerCmd, T>>;
pub struct ClientRxState(
    GameContext<ClientRx<IntroCmd>, ClientRx<HomeCmd>, ClientRx<RolesCmd>, ClientRx<GameCmd>>,
);






pub struct StartPeer(
    GameContext<
        (Intro, ClientRx<IntroCmd>, IntroHandle),
        (Home, ClientRx<HomeCmd>, HomeHandle),
        (Roles, ClientRx<RolesCmd>, RolesHandle),
        (Game, ClientRx<GameCmd>, GameHandle),
    >,
);
impl Connection<'_> {
    #[async_recursion::async_recursion]
    async fn process(
        &mut self,
        start: StartPeer,
        connection: &mut Connection,
    ) -> anyhow::Result<()> {
        macro_rules! unwrap {
            ($option:expr) => {
                match $option {
                    None => return Ok(()),
                    Some(x) => x,
                }
            };
        }
        macro_rules! rx {
            ($ident:ident, $enum:expr) => {
                match $enum {
                    GameContext::$ident(x) => x,
                    _ => return Err(anyhow::anyhow!("Wrong Rx")),
                }
            };
        }
        let new_state = match start.0 {
            GameContext::Intro(intro) => {
                unwrap!(self.run_as(intro, connection).await?)
            }
            GameContext::Home(home) => {
                unwrap!(self.run_as(home, connection).await?)
            }
            GameContext::Roles(roles) => {
                unwrap!(self.run_as(roles, connection).await?)
            }
            GameContext::Game(game) => {
                unwrap!(self.run_as(game, connection).await?)
            }
        };
        self.process(new_state, connection).await
    }


//pub struct ServerHandle2();

async fn accept_connection2(
    socket: &mut TcpStream,
    server: states::IntroHandle,
) -> anyhow::Result<()> {
    use crate::protocol::Msg;
    let addr = socket.peer_addr()?;
    let (r, w) = socket.split();
    let mut accept_connection = Connection {
        writer: FramedWrite::new(w, LinesCodec::new()),
        reader: MessageDecoder::new(FramedRead::new(r, LinesCodec::new())),
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg<PeerCmd, IntroCmd>>();

    let (tx2, mut to_socket_rx2) = mpsc::unbounded_channel();
    let (tx3, mut to_socket_rx3) = mpsc::unbounded_channel();
    let mut connection = Connection::new(addr, tx3, ServerHandle::for_tx(tx2));
    accept_connection
        .process(
            StartPeer(GameContext::Intro((Intro::default(), rx, IntroHandle))),
            &mut connection,
        )
        .await
}

*/

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use anyhow::anyhow;
    use tokio::{
        net::tcp::{ReadHalf, WriteHalf},
        task::JoinHandle,
        time::{sleep, Duration},
    };
    use tokio_util::sync::CancellationToken;
    use tracing_test::traced_test;

    use super::*;
    use crate::protocol::{server::LoginStatus, Username};

    fn host() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)
    }
    fn spawn_server(cancel: CancellationToken) -> JoinHandle<anyhow::Result<()>> {
        tokio::spawn(async move {
            listen(host(), async move {
                cancel.cancelled().await;
                Ok(())
            })
            .await
        })
    }
    fn spawn_simple_client(
        username: String,
        cancel: CancellationToken,
    ) -> JoinHandle<anyhow::Result<()>> {
        tokio::spawn(async move {
            let mut socket = TcpStream::connect(host()).await.unwrap();
            let (mut r, mut w) = split_to_read_write(&mut socket);
            login(username, &mut w, &mut r).await?;
            cancel.cancelled().await;
            Ok::<(), anyhow::Error>(())
        })
    }
    fn split_to_read_write(
        socket: &mut TcpStream,
    ) -> (
        MessageDecoder<FramedRead<ReadHalf<'_>, LinesCodec>>,
        FramedWrite<WriteHalf<'_>, LinesCodec>,
    ) {
        let (r, w) = socket.split();
        (
            MessageDecoder::new(FramedRead::new(r, LinesCodec::new())),
            FramedWrite::new(w, LinesCodec::new()),
        )
    }
    async fn login(
        username: String,
        w: &mut FramedWrite<WriteHalf<'_>, LinesCodec>,
        r: &mut MessageDecoder<FramedRead<ReadHalf<'_>, LinesCodec>>,
    ) -> anyhow::Result<()> {
        w.send(encode_message(client::Msg::from(client::IntroMsg::Login(
            Username(username),
        ))))
        .await
        .unwrap();
        if let server::Msg::Intro(server::IntroMsg::LoginStatus(status)) = r
            .next::<server::Msg>()
            .await
            .context("A Socket must be connected")?
            .context("Must be a message")?
        {
            debug!("Test client login status {:?}", status);
            if status == LoginStatus::Logged {
                Ok(())
            } else {
                Err(anyhow!("Failed to login {:?}", status))
            }
        } else {
            Err(anyhow!("Login status not received"))
        }
    }

    #[traced_test]
    #[tokio::test]
    async fn accept_connection_and_disconnection() {
        let cancel_token = CancellationToken::new();
        let server = spawn_server(cancel_token.clone());
        let mut clients = Vec::new();
        for i in 0..2 {
            let cancel = cancel_token.clone();
            clients.push(tokio::spawn(async move {
                let mut socket = TcpStream::connect(host()).await.unwrap();
                let (mut r, mut w) = split_to_read_write(&mut socket);
                w.send(encode_message(client::Msg::from(client::SharedMsg::Ping)))
                    .await
                    .unwrap();
                let res = r.next::<server::Msg>().await;
                let _ = socket.shutdown().await;
                // wait a disconnection of the last client and shutdown  the server
                if i == 1 {
                    sleep(Duration::from_millis(100)).await;
                    cancel.cancel();
                }
                match res {
                    Some(Ok(server::Msg::App(server::SharedMsg::Pong))) => Ok(()),
                    Some(Err(e)) => Err(anyhow!("Pong was not reseived correctly {}", e)),
                    None => Err(anyhow!("Pong was not received")),
                    _ => Err(anyhow!("Unknown message from server, not Pong")),
                }
            }));
        }
        let (server_ping_result, client1_pong_result, client2_pong_result) =
            tokio::join!(server, clients.pop().unwrap(), clients.pop().unwrap());
        match server_ping_result {
            Ok(Err(e)) => panic!("Server ping failed: {}", e),
            Err(e) => panic!("{}", e),
            _ => (),
        }
        for (i, c) in [client1_pong_result, client2_pong_result]
            .iter()
            .enumerate()
        {
            match c {
                Ok(Err(e)) => panic!("Pong failed for client {} : {}", i, e),
                Err(e) => panic!("{}", e),
                _ => (),
            }
        }
    }
    #[traced_test]
    #[tokio::test]
    async fn reject_login_with_existing_username() {
        let cancel_token = CancellationToken::new();
        let server = spawn_server(cancel_token.clone());
        let mut clients = Vec::new();
        for i in 0..2 {
            let cancel = cancel_token.clone();
            clients.push(tokio::spawn(async move {
                let mut socket = TcpStream::connect(host()).await.unwrap();
                let (mut r, mut w) = split_to_read_write(&mut socket);
                w.send(encode_message(client::Msg::from(client::IntroMsg::Login(
                    Username("Ig".into()),
                ))))
                .await
                .unwrap();
                let result_message = r
                    .next::<server::Msg>()
                    .await
                    .context("the server must send a LoginStatus message to the client");
                let _ = socket.shutdown().await;
                // wait a disconnection of the last client and shutdown the server
                if i == 1 {
                    sleep(Duration::from_millis(200)).await;
                    cancel.cancel();
                }
                match result_message? {
                    Ok(server::Msg::Intro(server::IntroMsg::LoginStatus(status))) => {
                        use crate::protocol::server::LoginStatus;
                        match i {
                            0 => match status {
                                LoginStatus::Logged => Ok(()),
                                _ => Err(anyhow!("Client 1 was not logged")),
                            },
                            1 => match status {
                                LoginStatus::AlreadyLogged => Ok(()),
                                _ => {
                                    Err(anyhow!("Client 2 with existing username was not rejected"))
                                }
                            },
                            _ => unreachable!(),
                        }
                    }
                    Err(e) => Err(anyhow!("Error = {}", e)),
                    _ => Err(anyhow!("Unexpected message from the server")),
                }
            }));
        }
        let (server, client1, client2) =
            tokio::join!(server, clients.pop().unwrap(), clients.pop().unwrap());
        match server {
            Ok(Err(e)) => panic!("Server error = {}", e),
            Err(e) => panic!("{}", e),
            _ => (),
        }
        for (i, c) in [client1, client2].iter().enumerate() {
            match c {
                Ok(Err(e)) => panic!("Client {} error = {}", i, e),
                Err(e) => panic!("{}", e),
                _ => (),
            }
        }
    }
    async fn shutdown(socket: &mut TcpStream, cancel: CancellationToken) {
        let _ = socket.shutdown().await;
        sleep(Duration::from_millis(100)).await;
        cancel.cancel();
    }

    #[traced_test]
    #[tokio::test]
    async fn drop_peer_actor_after_logout() {
        let cancel_token = CancellationToken::new();
        let server = spawn_server(cancel_token.clone());
        let client = tokio::spawn(async move {
            let client2_cancel = CancellationToken::new();
            spawn_simple_client("Ig".into(), cancel_token.clone());
            let client2 = spawn_simple_client("We".into(), client2_cancel.clone());

            sleep(Duration::from_millis(100)).await;
            let mut socket = TcpStream::connect(host()).await.unwrap();
            let (mut r, mut w) = split_to_read_write(&mut socket);
            if login("Ks".into(), &mut w, &mut r).await.is_ok() {
                debug!("test client was logged but it is unexpected");
                shutdown(&mut socket, cancel_token).await;
                return Err(anyhow!(
                    "3 client must not logged, the server should be full"
                ));
            }
            // disconnect a second client
            client2_cancel.cancel();
            let _ = client2.await;

            let mut socket = TcpStream::connect(host()).await.unwrap();
            let (mut r, mut w) = split_to_read_write(&mut socket);
            if let Err(e) = login("Ks".into(), &mut w, &mut r).await {
                debug!("Test client login Error {}", e);
                shutdown(&mut socket, cancel_token).await;
                return Err(anyhow!(
                    "Must login after a second player will disconnected"
                ));
            }

            shutdown(&mut socket, cancel_token).await;
            Ok::<(), anyhow::Error>(())
        });
        let (_, client) = tokio::join!(server, client);
        match client {
            Ok(Ok(_)) => (),
            Ok(Err(e)) => panic!("client error {}", e),
            Err(e) => panic!("unexpected eror {}", e),
        }
    }
    #[traced_test]
    #[tokio::test]
    async fn should_reconnect_if_select_role_context() {
        let cancel_token = CancellationToken::new();
        let server = spawn_server(cancel_token.clone());
        let cancel_token_cloned = cancel_token.clone();
        let client1 = tokio::spawn(async move {
            let mut socket = TcpStream::connect(host()).await.unwrap();
            let res = async {
                let (mut r, mut w) = split_to_read_write(&mut socket);
                login("Ig".into(), &mut w, &mut r).await?;
                w.send(encode_message(client::Msg::from(
                    client::SharedMsg::NextContext,
                )))
                .await?;
                sleep(Duration::from_millis(100)).await;
                w.send(encode_message(client::Msg::from(
                    client::SharedMsg::NextContext,
                )))
                .await?;
                sleep(Duration::from_millis(100)).await;
                let _ = socket.shutdown().await;
                sleep(Duration::from_millis(100)).await;
                let mut socket = TcpStream::connect(host()).await.unwrap();
                let (mut r, mut w) = split_to_read_write(&mut socket);
                w.send(encode_message(client::Msg::from(client::IntroMsg::Login(
                    Username("Ig".into()),
                ))))
                .await
                .unwrap();
                if let server::Msg::Intro(server::IntroMsg::LoginStatus(status)) = r
                    .next::<server::Msg>()
                    .await
                    .context("A Socket must be connected")?
                    .context("Must be a message")?
                {
                    debug!("Test client reconnection status {:?}", status);
                    if status == LoginStatus::Reconnected {
                        return Ok(());
                    } else {
                        return Err(anyhow!("Failed to login {:?}", status));
                    }
                }
                Ok(())
            }
            .await;
            shutdown(&mut socket, cancel_token_cloned).await;
            res
        });
        let client2 = tokio::spawn(async move {
            let mut socket = TcpStream::connect(host()).await.unwrap();
            let (mut r, mut w) = split_to_read_write(&mut socket);
            login("Ks".into(), &mut w, &mut r).await?;
            w.send(encode_message(client::Msg::from(
                client::SharedMsg::NextContext,
            )))
            .await?;
            cancel_token.cancelled().await;
            Ok::<(), anyhow::Error>(())
        });
        let (_, _, res) = tokio::try_join!(server, client2, client1).unwrap();
        match res {
            Ok(_) => (),
            Err(e) => panic!("client error {}", e),
        }
    }
}
