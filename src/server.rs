use std::io::ErrorKind;
use std::str;
use std::collections::HashMap;
use anyhow::{self, Context};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use std::net::SocketAddr;
use tracing::{debug, info, warn, error};
use std::sync::Arc;
use tokio_stream::StreamExt;
use tokio_util::codec::{Framed, LinesCodec};
use futures::{future, Sink, SinkExt};

use crate::shared::{ClientMessage, ServerMessage, LoginStatus};
/// Shorthand for the transmit half of the message channel.
type Tx = mpsc::UnboundedSender<String>;
/// Shorthand for the receive half of the message channel.
type Rx = mpsc::UnboundedReceiver<String>;

struct Peer {
    username : String
    , addr : SocketAddr
    , tx: Tx
}

pub struct SharedState {
    peers: [Option<Peer>; 2],
}

impl SharedState {
    fn new() -> Self {
        SharedState {
            peers : Default::default()
        }
    }
    /// Send a `LineCodec` encoded message to every peer, except
    /// for the sender.
    async fn broadcast(&mut self, sender: SocketAddr, message: &str) {
        for peer in self.peers.iter_mut().filter(|p| p.is_some()) {
            if peer.as_ref().unwrap().addr != sender {
                let _ = peer.as_ref().unwrap().tx.send(message.into());
            }
        }
    }
    fn check_user_exists(&self, username: &String) -> bool {
        self.peers.iter().any(|p| p.is_some() && p.as_ref().unwrap().username[..] == username[..])
    }
    fn is_full(&self) -> bool {
        self.peers.iter().position(|p| p.is_none()).is_none()
    }
}

pub struct Connection {
    socket: Framed<TcpStream, LinesCodec>
}
impl Connection {
    pub fn new(socket: Framed<TcpStream, LinesCodec>) -> Self {
        Connection { socket }
    }
    pub async fn next_message<A>(&mut self) -> anyhow::Result<A>
    where
        A: for<'b> serde::Deserialize<'b>,
    {
        let addr = self.socket.get_ref().peer_addr()?;
        match self.socket.next().await  {
            Some(Err(e)) => {
                Err(e).context(format!("an error occurred while processing messages from {}", addr))
            }
            Some(Ok(msg1)) => {
                serde_json::from_str::<A>(&msg1).context("")
            }
            // TODO rewrite
            None => {    // The stream has been exhausted.
                Err(std::io::Error::new(ErrorKind::ConnectionAborted, "peer message is None"))
                    .context(format!("{} sends an unknown message. Connection rejected", addr ))
            }
        }
    }
    pub async fn login(&mut self, state: Arc<Mutex<SharedState>>
                       ) -> anyhow::Result<String> {
         let addr = self.socket.get_ref().peer_addr()?;
         match self.next_message().await? {
            ClientMessage::AddPlayer(username) => {
                info!("{} is trying to connect to the game from {}"
                      , &username, addr);
                let  state = state.lock().await;
                // could join
                let msg = {
                    if state.is_full() {
                        warn!("server is full");
                        LoginStatus::PlayerLimit
                    } else if  state.check_user_exists(&username) {
                        warn!("Player {} already logged", username);
                        LoginStatus::AlreadyLogged 
                    } else {
                        LoginStatus::Logged
                    }
                };
                let json = serde_json::to_string(&ServerMessage::LoginStatus(msg)).unwrap();
                self.socket.send(json).await?;
                if msg == LoginStatus::Logged { 
                    Ok(username)
                } else {
                    Err(std::io::Error::new(ErrorKind::AddrNotAvailable, "")).context("")
                }
            },
            _ => Err(std::io::Error::new(ErrorKind::PermissionDenied
                        , "not allowed client message, authentification required")).context("")
        }
    }

    pub async fn process_incoming_messages(&mut self, mut rx: Rx) -> anyhow::Result<()> {
         loop {
            tokio::select! { 
                // a message was received from a peer. send it to the current user.
                Some(msg) = rx.recv() => {
                    self.socket.send(&msg).await?;
                }
                msg = self.next_message() => match msg {
                    Ok(msg) => { 
                        match msg {
                            ClientMessage::RemovePlayer => { break },
                            _ => todo!(),
                        }
                    },
                    Err(e) => { warn!("{}", e); break }
                }
            }
        };
        Ok(())
    }

}

pub struct Server {
      addr: SocketAddr
}

impl Server {
    pub fn new(addr: SocketAddr) -> Self {
        Self {  addr }
    }
    pub async fn listen(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&self.addr)
        .await
        .context(format!("Failed to bind a socket to {}", self.addr))?;
        info!("Listening on: {}", &self.addr);

        // TODO channel size and type
        //let (tx, rx) = mpsc::unbounded_channel();
         
       // tokio::spawn(async move {
        //        let game = Game::new(rx);
        //        game.start().await;
        //});

        let state = Arc::new(Mutex::new(SharedState::new()));

        loop {
            match listener.accept().await {
                Err(e) => { 
                    error!("failed to accept connection {}", e); 
                    continue;
                },
                Ok((stream, peer)) => {
                    info!("{} has connected", peer);
                    let state = Arc::clone(&state);
                    tokio::spawn(async move {
                        if let Err(e) = Server::handle_connection(stream, state).await {
                            error!("an error occurred; error = {:?}", e);
                        }
                        info!("{} has disconnected", peer);
                    });
                }
            }
        }
    }
      

    pub async fn handle_connection(socket: TcpStream, state: Arc<Mutex<SharedState>>) -> anyhow::Result<()> {
        let addr = socket.peer_addr()?;
        let mut cn = Connection::new(Framed::new(socket, LinesCodec::new()));
        // authentification
        let username =  cn.login(state.clone()).await?;     
        // Push a new player into the game
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let msg = ServerMessage::Chat(format!("{} has joined the game", username));
            let mut state = state.lock().await;
            *state.peers.iter_mut().find(|x| x.is_none() )
            .expect("failed to find an empty game slot for a player")
                = Some(Peer{
                             username
                            , addr
                            , tx
                            });
            let json = serde_json::to_string(&msg).unwrap();
            state.broadcast(addr, &json).await;
        }
        cn.process_incoming_messages(rx).await?;
        // If this section is reached it means that the client was disconnected!
        // Let's let everyone still connected know about it.
        {
            let mut state = state.lock().await;
            let peer = state.peers.iter_mut().find(|p| p.is_some() && p.as_ref().unwrap().addr == addr)
                .expect("failed to find a player for disconnect from SharedState");
            let msg = format!("{} has left the chat", peer.as_ref().unwrap().username);
            *peer = None;
            tracing::info!("{}", msg);
            state.broadcast(addr, &msg).await;
        }
        Ok(())
    }

}


