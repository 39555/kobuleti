use std::{future::Future, net::SocketAddr};

use anyhow::Context as _;
use tokio::{
    io::AsyncWriteExt,
    net::TcpListener,
    sync::mpsc::{channel, Receiver, Sender},
    time::{self, Duration},
};
use tracing::{error, info, trace};
pub mod details;
pub mod peer;
pub mod states;

pub const MPSC_CHANNEL_CAPACITY: usize = 32;
pub type Answer<T> = tokio::sync::oneshot::Sender<T>;
pub type Rx<T> = Receiver<T>;
pub type Tx<T> = Sender<T>;

#[derive(derive_more::Debug)]
#[debug("{alias}", alias = {
    // :((( type_name is not a const fn
    std::any::type_name::<Handle<T>>()
        .replace(const_format::concatcp!(crate::consts::APPNAME , "::server::"), "")
        .replace(const_format::concatcp!(crate::consts::APPNAME , "::protocol::"), "")
})]
pub struct Handle<T> {
    pub tx: Tx<T>,
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
/*
*  let mut backoff = 1;

       // Try to accept a few times
       loop {
           // Perform the accept operation. If a socket is successfully
           // accepted, return it. Otherwise, save the error.
           match self.listener.accept().await {
               Ok((socket, _)) => return Ok(socket),
               Err(err) => {
                   if backoff > 64 {
                       // Accept has failed too many times. Return the error.
                       return Err(err.into());
                   }
               }
           }

           // Pause execution until the back off period elapses.
           time::sleep(Duration::from_secs(backoff)).await;

           // Double the back off
           backoff *= 2;
       }
*
* */

pub async fn listen(
    addr: SocketAddr,
    shutdown: impl Future<Output = std::io::Result<()>>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind a socket to {}", addr))?;
    info!("Listening on: {}", addr);
    let (tx, rx) = channel(MPSC_CHANNEL_CAPACITY);
    let mut join_server = tokio::spawn(async move {
        states::run_intro_server(&mut states::StartServer::new(
            states::IntroServer::default(),
            rx,
        ))
        .await
    });

    let server_handle = states::IntroHandle::for_tx(tx);

    let server_end = tokio::select! {
        server_result = &mut join_server => {
            server_result?
        }
        accept_loop_err = async {
            loop {
                // Try to accept a few times. Exponential backoff is used
                let mut backoff = 1;

                'try_connect: loop {
                    match listener.accept().await {
                        Err(err) => {
                            if backoff > 64 {
                                 // Accepting connections from the TCP listener failed multiple times.
                                 // Shutdown the server
                                return Err::<(), anyhow::Error>(anyhow::anyhow!(err))
                            }
                        },
                        Ok((mut stream, addr)) => {
                            tokio::spawn({
                                let server_handle = server_handle.clone();
                                async move {
                                if let Err(err) = peer::accept_connection(&mut stream,
                                                                   server_handle)
                                    .await {
                                        error!(cause = %err, "Failed to accept");
                                }
                                let _ = stream.shutdown().await;
                                info!(?addr, "Disconnected");
                            }});
                            break 'try_connect;
                         }
                    }
                    // Pause execution until the back off period elapses.
                    time::sleep(Duration::from_secs(backoff)).await;
                    backoff *= 2;
                }

            }
        } => accept_loop_err,

        sig = shutdown =>{
            match sig {
               Ok(_)    => info!("Shutdown signal") ,
               Err(err) => error!(cause = ?err, "Unable to listen for shutdown signal")
            };

            Ok(())
        }
    };
    // send shutdown signal to the server actor and wait
    let shutdown = server_handle.shutdown().await.context("Failed to shutdown");
    server_end?;
    shutdown
}

/*
#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use anyhow::anyhow;
    use tokio::{
        net::{ TcpStream, tcp::{ReadHalf, WriteHalf}},
        task::JoinHandle,
        time::{sleep, Duration},
    };
    use tokio_util::sync::CancellationToken;
    use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
    use futures::SinkExt;
    use tracing_test::traced_test;
    use tracing::debug;

    use super::*;
    use crate::protocol::{server, server::LoginStatus, Username, MessageDecoder, encode_message, client};

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
*/
