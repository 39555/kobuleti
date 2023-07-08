use std::io::ErrorKind;
use std::str;
use futures::executor;
use std::collections::HashMap;
use anyhow::anyhow;
use anyhow::{Context};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use std::net::SocketAddr;
use tracing::{debug, info, warn, error};
use std::sync::Arc;
use tokio_stream::StreamExt;
use futures::{future, Sink, SinkExt};
use std::future::Future;
use std::cell::RefCell;
use tokio_util::codec::{LinesCodec, Framed, FramedRead, FramedWrite};
use crate::shared::{ClientMessage, ServerMessage, LoginStatus, MessageDecoder, ChatLine, encode_message};
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
    chat: RefCell<Vec<ChatLine>>
}

impl SharedState {
    fn new() -> Self {
        SharedState {
            peers : Default::default(),
            chat: Default::default()
        }
    }
    /// Send a `LineCodec` encoded message to every peer, except
    /// for the sender.
    fn broadcast(&self, sender: SocketAddr, message: &str) {
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
    fn get_username(&self, addr: SocketAddr) -> anyhow::Result<&str> {
        match self.peers.iter().find(|p| p.is_some() && p.as_ref().unwrap().addr == addr) {
                None => Err(anyhow::anyhow!("failed to find a player with address {}", addr)),
                Some(peer) => Ok(peer.as_ref().unwrap().username.as_str())
        }

    }
    fn is_full(&self) -> bool {
        self.peers.iter().position(|p| p.is_none()).is_none()
    }
    fn add_player(&mut self, username: String, addr: SocketAddr, tx: Tx) -> anyhow::Result<&str> {
        let it = self.peers.iter_mut().find(|x| x.is_none() )
        .context("failed to find an empty game slot for a player")?;
        *it = Some(Peer{ username, addr, tx });
        Ok(it.as_ref().unwrap().username.as_str())



    }
    fn remove_player(&mut self, addr: SocketAddr){
        let peer = self.peers.iter_mut().find(|p| p.is_some() && p.as_ref().unwrap().addr == addr)
                    .expect("failed to find a player for disconnect from SharedState");
        tracing::info!("disconnect player {}", peer.as_ref().unwrap().username);
        *peer = None;
       

    }
}

pub struct Connection {
    socket: TcpStream,
    state: Arc<Mutex<SharedState>>,
    rx: Rx
}
impl Connection {
    pub async fn new(mut socket: TcpStream,state: Arc<Mutex<SharedState>>) -> anyhow::Result<Self> {
        let addr = socket.peer_addr()?; 
        let username = Connection::login(&mut socket, state.clone())
            .await.context("failed to login to the game")?;
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut state = state.lock().await;
            let msg = ServerMessage::Chat(ChatLine::Connection(state.add_player(username, addr, tx)?.to_string()));
            state.broadcast(addr, &encode_message(msg));
        }
        Ok(Connection { socket , state, rx })
    }
    async fn login(socket: &mut TcpStream, state: Arc<Mutex<SharedState>>
                       ) -> anyhow::Result<String> {
         let addr = socket.peer_addr()?;
         let mut socket = Framed::new(socket, LinesCodec::new());
         let mut decoder = MessageDecoder::new(&mut socket);
         match decoder.next().await? {
            ClientMessage::AddPlayer(username) => {
                info!("{} is trying to connect to the game from {}"
                      , &username, addr);
                let  state = state.lock().await;
                // could join
                let msg = {
                    if state.is_full() {
                        warn!("Player limit has been reached");
                        LoginStatus::PlayerLimit
                    } else if  state.check_user_exists(&username) {
                        warn!("Player {} already logged", username );
                        LoginStatus::AlreadyLogged 
                    } else {
                        LoginStatus::Logged
                    }
                };
                socket.send(encode_message(ServerMessage::LoginStatus(msg))).await?;
                if msg == LoginStatus::Logged {
                    Ok(username)
                } else {
                    Err(anyhow!("failed to login a new connection {:?}", msg))
                }
            },
            _ => Err(anyhow!(
                    "accepted not allowed client message from {}, authentification required"
                    , addr))
        }
    }

    pub async fn process_incoming_messages(&mut self
 ) -> anyhow::Result<()> {
        let addr = self.socket.peer_addr()?;
        let (r, w) = self.socket.split();
        let mut writer = FramedWrite::new(w, LinesCodec::new());
        let mut reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));
         loop {
            tokio::select! { 
                // a message was received from a peer. send it to the current user.
                Some(msg) = self.rx.recv() => {
                    writer.send(&msg).await?;
                }
                msg = reader.next() => match msg {
                    Ok(msg) => { 
                        match msg {
                            ClientMessage::Chat(msg) => {
                                let  state = self.state.lock().await;
                                state.chat.borrow_mut().push(ChatLine::Text(format!("{}: {}", state.get_username(addr)?, msg)));
                                state.broadcast(addr, &encode_message(ServerMessage::Chat(state.chat.borrow().last().unwrap().clone())));
                            }
                            ClientMessage::RemovePlayer => {
                                writer.send(encode_message(ServerMessage::Logout)).await?;
                                break
                            },
                            ClientMessage::GetChatLog => {
                                let  state = self.state.lock().await;
                                info!("send the chat history to the client");
                                let chat = ServerMessage::ChatLog(state.chat.borrow().clone());
                                writer.send(encode_message(chat)).await?;
                            }
                            ClientMessage::AddPlayer(_) => unreachable!(),
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

impl Drop for Connection {
    fn drop(&mut self) {
    let addr = self.socket.peer_addr().unwrap(); 
    {
        let state = executor::block_on(self.state.lock());
        state.broadcast(addr, 
                &encode_message(
                    ServerMessage::Chat(ChatLine::Disconnection(
                            state.get_username(addr).unwrap().to_string()))));
    }
    executor::block_on(self.state.lock()).remove_player(addr); 
    }
}

pub struct Server {
      addr: SocketAddr
}

impl Server {
    pub fn new(addr: SocketAddr) -> Self {
        Self {  addr }
    }
    pub async fn listen(&self, shutdown: impl Future) -> anyhow::Result<()> {
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
                            let state = Arc::clone(&state);
                            tokio::spawn(async move {
                                match Connection::new(stream, Arc::clone(&state)).await {
                                    Err(e) => warn!("new connection rejected {}", e),
                                    Ok(mut cn) => {  
                                        if let Err(e) = cn.process_incoming_messages().await {
                                            error!("an error occurred; error = {:?}", e);
                                        }
                                        info!("{} has disconnected", addr);
                                    }
                                };
                            });
                         }
                    }
             } 
            } => Ok(()),
            _ = shutdown => {
                // The shutdown signal has been received.
                info!("server is shutting down");
                Ok(())
            }
        }
    }
      
}


