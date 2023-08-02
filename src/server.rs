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
use crate::protocol::{AsyncMessageReceiver, GameContextId, MessageReceiver, MessageDecoder, encode_message};
use crate::protocol::{server, client, ToContext, TryNextContext};
use crate::protocol::server::{ServerGameContext, Intro, Home, SelectRole, Game};
/// Shorthand for the transmit half of the message channel.
type Tx = mpsc::UnboundedSender<String>;
/// Shorthand for the receive half of the message channel.
type Rx = mpsc::UnboundedReceiver<String>;
type Answer<T> = oneshot::Sender<T>;
use crate::protocol::MessageError;

pub mod peer;
pub mod details;
pub mod session;
pub mod room;
use room::{Room, ToServer, ServerHandle, Server};
use peer::{Peer, PeerHandle, Connection, ServerGameContextHandle, ContextCmd};
use tokio::sync::oneshot;
use scopeguard::guard;


pub async fn listen(addr: SocketAddr, shutdown: impl Future<Output=std::io::Result<()>>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr)
        .await
        .context(format!("Failed to bind a socket to {}", addr))?;
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
internal command by the server actor: {}", e);
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
                        error!("Failed to accept a new connection {}", e); 
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
                                    error!("An error occurred; error = {:?}", e);
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
               Err(err) => error!("Unable to listen for shutdown signal: {}", err)     
            };
            // send shutdown signal to application and wait    
            //info!("Shutting down server...");
            //server.shutdown().await;
            Ok(())
        }
    }
}


async fn process_connection(socket: &mut TcpStream, 
                            server: ServerHandle ) -> anyhow::Result<()> {
    let addr    = socket.peer_addr()?;
    let (r, w)  = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    trace!("Spawn a Peer actor for {}", addr);
    let (to_peer, mut peer_rx) = mpsc::unbounded_channel();
    let connection = Connection::new(addr, tx, server); 
    let mut peer_handle = PeerHandle::new(to_peer, GameContextId::Intro(()));
    let mut peer = Peer::new(
        ServerGameContext::from(
            Intro::new(
                peer_handle.clone())));
    let mut socket_writer = FramedWrite::new(w, LinesCodec::new());
    let mut socket_reader = MessageDecoder::new(
                                FramedRead::new(r, LinesCodec::new()));
    let connection_ref = &connection;
    let cancel_token = CancellationToken::new();
    let peercmd_cancel_token = cancel_token.clone();  
    // process commands from server
    let peer_commands = async move {
         loop {
            tokio::select! {
                _ = peercmd_cancel_token.cancelled() => {
                    trace!("Abort peer commands for {}", addr);
                    break;
                }   
                cmd = peer_rx.recv() => match cmd {
                    Some(cmd) => {
                        trace!("Peer {} received a server internal command ", addr);
                        peer.message(cmd, connection_ref).await
                            .context("Failed to process internal commands by Peer")?;
                    },
                    None => {
                        break;
                    }
                }

            }
                  
         }
         Ok::<(), anyhow::Error>(())
    } ; 

    // socket messages
    let socket_io = async move {
        loop {

            let _cancel_peer_commands_after_return = scopeguard::guard((), |_| {
                cancel_token.cancel();
            });

            tokio::select! { 
                msg = rx.recv() => match msg {
                    Some(msg) => {
                        trace!("Message from a peer. \
Send to associated client {}", addr);      
                        socket_writer.send(&msg).await
                            .context("Failed to send a message to the socket")?;    
                    }
                    None => {
                        break;
                    }
                },
                msg = socket_reader.next::<client::Msg>() => match msg {
                    Some(Ok(msg)) => { 
                        trace!("Process a new message from {}: {:?}", addr, msg);
                        if let Err(e) = peer_handle.message(msg, connection_ref).await {
                            match e {
                                MessageError::LoginRejected { reason } => {
                                    warn!("Login attempt rejected: {}", reason);
                                }
                                _ => {
                                    return Err(anyhow!(e))
                                }
                            }
                            break;
                        }
                    },
                    Some(Err(e)) => { 
                        return Err(anyhow!(e));
                    }
                    None => {
                        info!("Connection {} aborted..", addr);
                        break
                    }
                }
            }
        };
        Ok::<(), anyhow::Error>(())
    };  
    let _ = tokio::try_join!(socket_io, peer_commands)?;

    Ok(())
}




#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{SocketAddr, Ipv4Addr, IpAddr};
    use tracing_test::traced_test;
    use tokio::time::{sleep, Duration};
    use tokio::task::JoinHandle;

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
                    let (r, w) = socket.split();
                    let mut socket_writer = FramedWrite::new(w, LinesCodec::new());
                    let mut socket_reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));
                    socket_writer.send(encode_message(client::Msg::from(client::AppMsg::Ping))).await.unwrap();
                    let res = socket_reader.next::<server::Msg>().await;
                    assert!(matches!(res, Some(Ok(server::Msg::App(server::AppMsg::Pong))))) ;
                    let _ = socket.shutdown().await;
                    // wait client disconnection and shutdown server
                    if i == 1 {
                        sleep(Duration::from_millis(100)).await;
                        cancel.cancel();
                    }

                })
            );
        }
        let (server_result, _ , _) = tokio::join!(server, clients.pop().unwrap(), clients.pop().unwrap());
        assert!(server_result.is_ok());
    }

   
}

