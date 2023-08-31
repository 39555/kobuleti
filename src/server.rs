use std::{future::Future, net::SocketAddr};

use anyhow::Context as _;
use tokio::{io::AsyncWriteExt, net::TcpListener, sync::mpsc};
use tracing::{error, info, trace};
pub mod details;
pub mod peer;
pub mod states;

pub type Answer<T> = tokio::sync::oneshot::Sender<T>;

#[derive(derive_more::Debug)]
//#[debug("Handle")]
pub struct Handle<T> {
    pub tx: tokio::sync::mpsc::UnboundedSender<T>,
}

pub type Tx<T> = mpsc::UnboundedSender<T>;
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
        .await
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
                            if let Err(e) = peer::accept_connection(&mut stream,
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
            server_handle.shutdown().await?;
            Ok(())
        }
    }
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
