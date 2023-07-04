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
use futures::{future, Sink, SinkExt};
use std::future::Future;
use tokio_util::codec::{LinesCodec, Framed, FramedRead, FramedWrite};
use crate::shared::{ClientMessage, ServerMessage, LoginStatus, MessageDecoder};
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
        for peer in self.peers.iter().filter(|p| p.is_some()) {
            let p = peer.as_ref().unwrap();
            if p.addr != sender {
                let _ = p.tx.send(message.into());
            }
        }
    }
    fn check_user_exists(&self, username: &String) -> bool {
        self.peers.iter().any(|p| p.is_some() && p.as_ref().unwrap().username[..] == username[..])
    }
    fn is_full(&self) -> bool {
        self.peers.iter().position(|p| p.is_none()).is_none()
    }
    fn add_player(&mut self, username: String, addr: SocketAddr, tx: Tx) {
        *self.peers.iter_mut().find(|x| x.is_none() )
        .expect("failed to find an empty game slot for a player")
                = Some(Peer{ username, addr, tx });


    }
    async fn remove_player(&mut self, addr: SocketAddr){
        let peer = self.peers.iter_mut().find(|p| p.is_some() && p.as_ref().unwrap().addr == addr)
                    .expect("failed to find a player for disconnect from SharedState");
        let msg = format!("{} has left the chat", peer.as_ref().unwrap().username);
        *peer = None;
        tracing::info!("{}", msg);
        self.broadcast(addr, &serde_json::to_string(&ServerMessage::Chat(msg)).unwrap()).await;

    }
}

pub struct Connection {
    socket: TcpStream
}
impl Connection {
    pub fn new(socket: TcpStream) -> Self {
        Connection { socket }
    }
    pub async fn login(&mut self, state: Arc<Mutex<SharedState>>
                       ) -> anyhow::Result<String> {
         let addr = self.socket.peer_addr()?;
         let mut lines = Framed::new(&mut self.socket, LinesCodec::new());
         let mut codec = MessageDecoder::new(&mut lines);
         match codec.next().await? {
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
                lines.send(json).await?;
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

    pub async fn process_incoming_messages(&mut self, mut rx: Rx,  state: Arc<Mutex<SharedState>>
 ) -> anyhow::Result<()> {
        let addr = self.socket.peer_addr()?;
        let (r, w) = self.socket.split();
        let mut writer = FramedWrite::new(w, LinesCodec::new());
        let mut reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));
         loop {
            tokio::select! { 
                // a message was received from a peer. send it to the current user.
                Some(msg) = rx.recv() => {
                    writer.send(&msg).await?;
                }
                msg = reader.next() => match msg {
                    Ok(msg) => { 
                        match msg {
                            ClientMessage::RemovePlayer => { break },
                            ClientMessage::Chat(msg) => {
                            // TODO optimize?
                               state.lock()
                                .await
                                .broadcast(addr, &serde_json::to_string(&ServerMessage::Chat(msg)).unwrap()).await;
                            }
                            _ => todo!(),
                        }
                    },
                    Err(e) => { 
                        warn!("{}", e);
                        if e.kind() == ErrorKind::ConnectionAborted {
                            break
                        }

                    }
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
    pub async fn listen(&self, shutdown_signal: impl Future) -> anyhow::Result<()> {
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
        
        tokio::select!{
            _ = async {  
                loop {
                    match listener.accept().await {
                        Err(e) => { 
                            error!("failed to accept connection {}", e); 
                            continue;
                        },
                        Ok((stream, addr)) => {
                            info!("{} has connected", addr);
                            let state_in_connection = Arc::clone(&state);
                            let state = Arc::clone(&state);
                            tokio::spawn(async move {
                                if let Err(e) = Server::handle_connection(stream, state_in_connection).await {
                                    error!("an error occurred; error = {:?}", e);
                                }
                                // If this section is reached it means that the client was disconnected!
                                // Let's let everyone still connected know about it.
                                state.lock().await.remove_player(addr).await;
                                info!("{} has disconnected", addr);
                            });
                         }
                    }
             } 
            } => Ok(()),
            _ = shutdown_signal => {
                // The shutdown signal has been received.
                info!("server is shutting down");
                Ok(())
            }
        }
    }
      

    pub async fn handle_connection(socket: TcpStream, state: Arc<Mutex<SharedState>>) -> anyhow::Result<()> {
        let addr = socket.peer_addr()?; 
        let mut cn = Connection::new(socket);
        // authentification
        let username =  cn.login(state.clone()).await.context("failed to login to the game")?;     
        // Push a new player into the game
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let msg = ServerMessage::Chat(format!("{} has joined the game", username));
            let mut state = state.lock().await;
            state.add_player(username, addr, tx);
            let json = serde_json::to_string(&msg).unwrap();
            state.broadcast(addr, &json).await;
        }
        cn.process_incoming_messages(rx, state).await
    }

}


