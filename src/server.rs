use anyhow::anyhow;
use anyhow::Context as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc};
use tokio_util::sync::CancellationToken;
use std::net::SocketAddr;
use tracing::{trace, debug, info, warn, error};
use futures::{future, Sink, SinkExt};
use std::future::Future;
use tokio_util::codec::{LinesCodec, Framed, FramedRead, FramedWrite};
use crate::protocol::{AsyncMessageReceiver, GameContextKind, MessageReceiver, MessageDecoder, encode_message};
use crate::protocol::{server, client, ToContext, TryNextContext};
use crate::protocol::server::{ServerGameContext, Intro, Home, SelectRole, Game};
/// Shorthand for the transmit half of the message channel.
type Tx = mpsc::UnboundedSender<String>;
/// Shorthand for the receive half of the message channel.
type Rx = mpsc::UnboundedReceiver<String>;
type Answer<T> = oneshot::Sender<T>;

pub mod peer;
pub mod details;
pub mod session;
pub mod commands;
use commands::{Room, ServerCmd, ServerHandle, Server};
use peer::{Peer, PeerHandle, Connection, ServerGameContextHandle, ContextCmd};
use tokio::sync::oneshot;
use scopeguard::guard;


pub async fn listen(addr: SocketAddr, shutdown: impl Future<Output=std::io::Result<()>>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind a socket to {}", addr))?;
    info!("Listening on: {}", addr);
    let (to_server, mut server_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut state  = Room::default();
        let mut server = Server::default();
        trace!("Spawn a server actor");
        loop {
            if let Some(command) = server_rx.recv().await {
                if let Err(e) = server.message(command, &mut state).await {
                    error!("failed to process an \
internal command by the server actor = {:#}", e);
                }
            };
        }
    });
    let server_handle =  ServerHandle::for_tx(to_server);

    trace!("Listen for new connections..");
    tokio::select!{
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
                            if let Err(e) = process_connection(&mut stream, 
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


async fn process_connection(socket: &mut TcpStream, 
                            server: ServerHandle ) -> anyhow::Result<()> {
    let addr    = socket.peer_addr()?;
    let (r, w)  = socket.split();
    let (tx, mut to_socket_rx) = mpsc::unbounded_channel::<String>();

    let mut connection = Connection::new(addr, tx, server);

    trace!("Spawn a Peer actor for {}", addr);
    let (to_peer, mut peer_rx) = mpsc::unbounded_channel();
    let mut connection_for_peer = connection.clone();
    // A peer actor does not drop while tcp io loop alive, or while a server room
    // will not drop a player, because they hold a peer_handle
    tokio::spawn(async move {
        let mut peer = Peer::new(ServerGameContext::from(Intro::default()));
         loop {
            match peer_rx.recv().await {
                Some(cmd) => {
                    trace!("Peer {} = {}", addr, cmd);
                    if let Err(e) =
                        peer.message(cmd, &mut connection_for_peer).await {
                            error!("{:#}", e);
                            break
                    }
                },
                None => {
                    // EOF. The last PeerHandle has been dropped
                    break;
                }
            }
         }
         info!("Drop Peer actor for {}", addr);
    }); 

    let mut peer_handle = PeerHandle::for_tx(to_peer);

    // tcp io
    let mut socket_writer = FramedWrite::new(w, LinesCodec::new());
    let mut socket_reader = MessageDecoder::new(
                                FramedRead::new(r, LinesCodec::new()));
    loop{
        tokio::select! { 
            msg = to_socket_rx.recv() => match msg {
                Some(msg) => {
                    debug!("Peer {} msg {} -> client", addr,  msg);      
                    socket_writer.send(&msg).await
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

                    peer_handle.message(
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
    };
    Ok(())
}




#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{SocketAddr, Ipv4Addr, IpAddr};
    use tracing_test::traced_test;
    use tokio::time::{sleep, Duration};
    use tokio::task::JoinHandle;
    use tokio::net::tcp::{ReadHalf, WriteHalf};
    use crate::protocol::server::LoginStatus;

    fn host() -> SocketAddr {
         SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)
    }
    fn spawn_server(cancel: CancellationToken) -> JoinHandle<anyhow::Result<()>> {
        tokio::spawn(async move  {
            listen(host()
                   , async move {
                       cancel.cancelled().await;
                       Ok(())
                   }).await 
        })          
    } 
    fn spawn_simple_client(username: String, cancel: CancellationToken) -> JoinHandle<anyhow::Result<()>>{
        tokio::spawn(async move {
            let mut socket = TcpStream::connect(host()).await.unwrap();   
            let (mut r, mut w) = split_to_read_write(&mut socket);
            login(username, &mut w, &mut r).await?;
            cancel.cancelled().await;
            Ok::<(), anyhow::Error>(())
        })
    }
    fn split_to_read_write<'a>(socket: &'a mut TcpStream) -> (
                                    MessageDecoder<FramedRead<ReadHalf<'a>, LinesCodec>>, 
                                    FramedWrite<WriteHalf<'a>, LinesCodec>,
                                 ){
    
    let (r, w) = socket.split(); 
    (
        MessageDecoder::new(FramedRead::new(r, LinesCodec::new())),
        FramedWrite::new(w, LinesCodec::new()),
    )
    }
    async fn login(username: String, 
                       w: & mut FramedWrite<WriteHalf<'_>, LinesCodec>,
                       r: & mut MessageDecoder<FramedRead<ReadHalf<'_>, LinesCodec>>, 
                       ) -> anyhow::Result<()> {
        w.send(encode_message(client::Msg::from(
                                client::IntroMsg::AddPlayer(username))))
                        .await.unwrap();
        if let server::Msg::Intro(
            server::IntroMsg::LoginStatus(status)) = r.next::<server::Msg>().await
            .context("A Socket must be connected")?
            .context("Must be a message")?
        {
            debug!("Test client login status {:?}", status);
            if status == LoginStatus::Logged {
                Ok(())
            } else {
                Err(anyhow!("Failed to login {:?}", status))
            }
        }
        else {
            Err(anyhow!("Login status not received"))
        }

    }

    #[traced_test]
    #[tokio::test]
    async fn accept_connection_and_disconnection(){
        let cancel_token = CancellationToken::new();
        let server = spawn_server(cancel_token.clone());
        let mut clients = Vec::new();
        for i in 0..2{
            let cancel = cancel_token.clone();
            clients.push(
                tokio::spawn(async move {
                    let mut socket = TcpStream::connect(host()).await.unwrap();   
                    let (mut r, mut w) = split_to_read_write(&mut socket);
                    w.send(encode_message(client::Msg::from(client::AppMsg::Ping))).await.unwrap();
                    let res = r.next::<server::Msg>().await;
                    let _ = socket.shutdown().await;
                    // wait a disconnection of the last client and shutdown  the server
                    if i == 1 {
                        sleep(Duration::from_millis(100)).await;
                        cancel.cancel();
                    }
                    match res {
                        Some(Ok(server::Msg::App(server::AppMsg::Pong)))  => Ok(()),
                        Some(Err(e)) => Err(anyhow!("Pong was not reseived correctly {}", e)),
                        None => Err(anyhow!("Pong was not received")),
                        _ => Err(anyhow!("Unknown message from server, not Pong")),
                    }
                })
            );
        }
        let (server_ping_result, client1_pong_result , client2_pong_result) 
            = tokio::join!(server, clients.pop().unwrap(), clients.pop().unwrap());
        match server_ping_result {
            Ok(Err(e)) => assert!(false, "Server ping failed: {}", e),
            Err(e) => assert!(false, "{}", e),
            _ => (),
        }
        for (i, c) in [client1_pong_result, client2_pong_result].iter().enumerate(){
            match c {
                Ok(Err(e)) => assert!(false, "Pong failed for client {} : {}", i, e),
                Err(e) => assert!(false, "{}", e),
                _ => (),
            }

        }
    }
    #[traced_test]
    #[tokio::test]
    async fn reject_login_with_existing_username(){
        let cancel_token = CancellationToken::new();
        let server = spawn_server(cancel_token.clone());
        let mut clients = Vec::new();
        for i in 0..2{
            let cancel = cancel_token.clone();
            clients.push(
                tokio::spawn(async move {
                    let mut socket = TcpStream::connect(host()).await.unwrap();   
                    let (mut r, mut w) = split_to_read_write(&mut socket);
                    w.send(encode_message(client::Msg::from(
                                client::IntroMsg::AddPlayer("Ig".into()))))
                        .await.unwrap();
                    let result_message = r.next::<server::Msg>().await
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
                                0 => {
                                    match status {
                                        LoginStatus::Logged => Ok(()),
                                        _ => Err(anyhow!("Client 1 was not logged"))
                                    }
                                }, 
                                1 => {
                                    match status {
                                        LoginStatus::AlreadyLogged => Ok(()), 
                                        _ => Err(anyhow!("Client 2 with existing username was not rejected"))
                                    }
                                },
                                _ => unreachable!()
                            }
                        }
                        Err(e) => Err(anyhow!("Error = {}", e)),
                        _      => Err(anyhow!("Unexpected message from the server"))
                    }
                })
            );
        }
        let (server, client1 , client2) 
            = tokio::join!(server, clients.pop().unwrap(), clients.pop().unwrap());
        match server {
            Ok(Err(e)) => assert!(false, "Server error = {}", e),
            Err(e) => assert!(false, "{}", e),
            _ => (),
        }
        for (i, c) in [client1, client2].iter().enumerate(){
            match c {
                Ok(Err(e)) => assert!(false, "Client {} error = {}", i, e),
                Err(e) => assert!(false, "{}", e),
                _ => (),
            }

        }
    }
    async fn shutdown(socket: &mut TcpStream, cancel: CancellationToken){
        let _ = socket.shutdown().await;
        sleep(Duration::from_millis(100)).await;
        cancel.cancel();
    }

    #[traced_test]
    #[tokio::test]
    async fn drop_peer_actor_after_logout(){
        let cancel_token = CancellationToken::new();
        let server = spawn_server(cancel_token.clone());
        let client = tokio::spawn(async move {
            let client2_cancel = CancellationToken::new();
            spawn_simple_client("Ig".into(),  cancel_token.clone());
            let client2 = spawn_simple_client("We".into(),  client2_cancel.clone());

            sleep(Duration::from_millis(100)).await;
            let mut socket = TcpStream::connect(host()).await.unwrap();   
            let (mut r, mut w) = split_to_read_write(&mut socket);
            if login("Ks".into(), &mut w, &mut r).await.is_ok(){
                debug!("test client was logged but it is unexpected");
                shutdown(&mut socket, cancel_token).await;
                return Err(anyhow!("3 client must not logged, the server should be full"));
            }
            // disconnect a second client
            client2_cancel.cancel();
            let _ = client2.await;

            let mut socket = TcpStream::connect(host()).await.unwrap();   
            let (mut r, mut w) = split_to_read_write(&mut socket);
            if let Err(e) = login("Ks".into(),  &mut w, &mut r).await{
                debug!("Test client login Error {}", e);
                shutdown(&mut socket, cancel_token).await;
                return Err(
                anyhow!("Must login after a second player will disconnected"));
            }
            
            shutdown(&mut socket, cancel_token).await;
            Ok::<(), anyhow::Error>(())
        }); 
        let (_, client) 
            = tokio::join!(server, client);
        match client{
            Ok(Ok(_))  => assert!(true),
            Ok(Err(e)) => assert!(false, "client error {}", e),
            Err(e) => assert!(false, "unexpected error {}", e),


        }

    } 
    #[traced_test]
    #[tokio::test]
    async fn should_reconnect_if_select_role_context(){
        let cancel_token = CancellationToken::new();
        let server = spawn_server(cancel_token.clone());
        let cancel_token_cloned = cancel_token.clone();
        let client1 = tokio::spawn(async move {
        let mut socket = TcpStream::connect(host()).await.unwrap();   
            let res = async {
                let (mut r, mut w) = split_to_read_write(&mut socket);
                login("Ig".into(), &mut w, &mut r).await?;
                w.send(encode_message(client::Msg::from(client::AppMsg::NextContext))).await?;
                sleep(Duration::from_millis(100)).await; 
                w.send(encode_message(client::Msg::from(client::AppMsg::NextContext))).await?;
                sleep(Duration::from_millis(100)).await; 
                let _ = socket.shutdown().await;
                sleep(Duration::from_millis(100)).await; 
                let mut socket = TcpStream::connect(host()).await.unwrap();   
                let (mut r, mut w) = split_to_read_write(&mut socket);
                w.send(encode_message(client::Msg::from(
                                client::IntroMsg::AddPlayer("Ig".into()))))
                        .await.unwrap();
                if let server::Msg::Intro(
                    server::IntroMsg::LoginStatus(status)) = r.next::<server::Msg>().await
                    .context("A Socket must be connected")?
                    .context("Must be a message")? {
                    debug!("Test client reconnection status {:?}", status);
                    if status == LoginStatus::Reconnected {
                        return Ok(())
                    } else {
                        return Err(anyhow!("Failed to login {:?}", status))
                    }
                }
                Ok(())
            }.await;
            shutdown(&mut socket, cancel_token_cloned).await;
            res

        });
        let client2 = tokio::spawn(async move {
            let mut socket = TcpStream::connect(host()).await.unwrap();   
            let (mut r, mut w) = split_to_read_write(&mut socket);
            login("Ks".into(), &mut w, &mut r).await?;
            w.send(encode_message(client::Msg::from(client::AppMsg::NextContext))).await?;
            cancel_token.cancelled().await;
            Ok::<(), anyhow::Error>(())

        });
        let (_, _, res) = tokio::try_join!(server, client2, client1).unwrap();
         match res{
            Ok(_)  => assert!(true),
            Err(e) => assert!(false, "client error {}", e),


        }

    }

   
}

