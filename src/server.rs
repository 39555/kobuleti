use std::io::ErrorKind;
use std::str;
use futures::executor;
use std::collections::HashMap;
use anyhow::anyhow;
use anyhow::{Context};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc};
use std::net::SocketAddr;
use tracing::{debug, info, warn, error};
use std::sync::{Arc, Mutex};
use tokio_stream::StreamExt;
use futures::{future, Sink, SinkExt};
use std::future::Future;
use tokio_util::codec::{LinesCodec, Framed, FramedRead, FramedWrite};
use crate::protocol::{ GameContextId, MessageReceiver, MessageDecoder, encode_message};
use crate::protocol::{server, client};

    use enum_dispatch::enum_dispatch;
/// Shorthand for the transmit half of the message channel.
type Tx = mpsc::UnboundedSender<String>;
/// Shorthand for the receive half of the message channel.
type Rx = mpsc::UnboundedReceiver<String>;

struct PeerId {
    username : String
    , addr : SocketAddr
    , tx: Tx
    , context: GameContextId,
}

#[derive(Default)]
pub struct State {
    peers: [Option<PeerId>; 2],
    chat: Vec<server::ChatLine>
    //game: Arc<Mutex<Game>>,
}



impl State {
    /// Send a `LineCodec` encoded message to every peer, except
    /// for the sender.
    fn broadcast(&self, sender: SocketAddr, message: server::Message) {
        let msg = &encode_message(&message);
        for peer in self.peers.iter().filter(|p| p.is_some()) {
            let p = peer.as_ref().unwrap();
            if p.addr != sender {
                // ignore message from other contexts
                if matches!(message, server::Message::Common(_)) || p.context == GameContextId::from(&message){
                    let _ = p.tx.send(msg.into());
                }
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
    fn change_context_for_player(&mut self, addr: SocketAddr, ctx: GameContextId) -> anyhow::Result<()> {
         let it = self.peers.iter_mut().find(|x| x.is_some() && x.as_ref().unwrap().addr == addr )
            .with_context(|| format!("failed to find a player with address {}", addr))?;
         it.as_mut().unwrap().context = ctx;
         Ok(())

    }
    fn is_full(&self) -> bool {
        self.peers.iter().position(|p| p.is_none()).is_none()
    }
    fn add_player(&mut self, username: String, addr: SocketAddr, tx: Tx, context: GameContextId) -> anyhow::Result<&str> {
        let it = self.peers.iter_mut().find(|x| x.is_none() )
        .context("failed to find an empty game slot for a player")?;
        *it = Some(PeerId{ username, addr, tx , context});
        Ok(it.as_ref().unwrap().username.as_str())
    }
    fn remove_player(&mut self, addr: SocketAddr){
        let peer = self.peers.iter_mut().find(|p| p.is_some() && p.as_ref().unwrap().addr == addr)
                    .expect("failed to find a player for disconnect from SharedState");
        tracing::info!("disconnect player {}", peer.as_ref().unwrap().username);
        *peer = None;

    }
}

type SharedState = Arc<Mutex<State>>;

pub struct Connection {
    socket: TcpStream,
    // TODO weak
    //state: Arc<Mutex<State>>,
    context: server::ServerGameContext,
    rx: Rx
}
impl Connection {
    pub async fn new(socket: TcpStream,state: Arc<Mutex<State>>) -> anyhow::Result<Self> {
        let addr = socket.peer_addr()?; 
        let (tx, rx) = mpsc::unbounded_channel();
        Ok(Connection { socket ,  context: server::ServerGameContext::Intro(server::Intro{tx, addr, state}),   rx })
    }

    pub async fn process_incoming_messages(&mut self
 ) -> anyhow::Result<()> {
        //let addr = self.socket.peer_addr()?;
        let (r, w) = self.socket.split();
        let mut writer = FramedWrite::new(w, LinesCodec::new());
        let mut reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));
        /*
        {
            let state = self.state.lock().await;
            if state.is_full() {
                // TODO why error?
                //state.peers[0].as_ref().unwrap().tx.send(encode_message(ServerMessage::CanPlay))?;
            }
        }
        */
         loop {
            tokio::select! { 
                // a message was received from a peer. send it to the current user.
                Some(msg) = self.rx.recv() => {
                    writer.send(&msg).await?;
                }

                msg = reader.next() => match msg {
                    Ok(msg) => { 
                         match msg {
                            client::Message::Common(e) => {
                                match e {
                                    client::CommonEvent::RemovePlayer =>  {
                                        writer.send(encode_message(server::Message::Common(server::CommonEvent::Logout))).await?;
                                        info!("Logout");
                                        break  
                                    },
                                    client::CommonEvent::NextContext => {
                                        self.context.next();
                                        let new_id = GameContextId::from(&self.context);
                                        match &self.context {
                                            server::ServerGameContext::Intro(i) => {
                                                ()
                                            }
                                            server::ServerGameContext::Home(h) => {
                                                h.state.lock().unwrap().change_context_for_player(h.addr, new_id)?;
                                                 writer.send(encode_message(server::Message::Common(server::CommonEvent::NextContext(
                                                        new_id)))).await?;
                                                {
                                                    let state = h.state.lock().unwrap();
                                                    state.broadcast( h.addr, server::Message::HomeStage(
                                                            server::HomeStageEvent::Chat(server::ChatLine::Connection(
                                                                            state.get_username(h.addr).unwrap().to_string()))));
                                                }
                                            },
                                            server::ServerGameContext::Game(g) => (),

                                        }
                                    }
                                }
                            },
                            _ => {
                                self.context.message(msg)
                                    .with_context(|| format!("current context {:?}", GameContextId::from(&self.context) ))?;
                            }
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
    //let addr = self.socket.peer_addr().unwrap(); 
    {
        //let state = executor::block_on(self.state.lock());
        //state.broadcast(addr, 
        //        &encode_message(
        //            server::Message::Chat(ChatLine::Disconnection(
         //                   state.get_username(addr).unwrap().to_string()))));
          match &self.context {
            server::ServerGameContext::Intro(i) => {
                i.state.lock().unwrap().remove_player(i.addr);
            }
            ,
            server::ServerGameContext::Home(h) => {
                h.state.lock().unwrap().remove_player(h.addr);
            },
            server::ServerGameContext::Game(g) => {
                g.state.lock().unwrap().remove_player(g.addr);
            },
        }
    }
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
        let state = SharedState::default();
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
impl MessageReceiver<client::Message> for server::ServerGameContext {
    fn message(&mut self, msg: client::Message) -> anyhow::Result<()> {
        macro_rules! stage_msg {
            ($e:expr, $p:path) => {
                match $e {
                    $p(value) => Ok(value),
                    _ => Err(anyhow!("a wrong message type for current stage was received: {:?}", $e )),
                }
            };
        }
        match self {
            server::ServerGameContext::Intro(i) => { 
                i.message(stage_msg!(msg, client::Message::IntroStage)?)?;
            },
            server::ServerGameContext::Home(h) =>{
                h.message(stage_msg!(msg, client::Message::HomeStage)?)?;
            },
            server::ServerGameContext::Game(g) => {
                g.message(stage_msg!(msg, client::Message::GameStage)?)?;
            },
            }
        Ok(())
}
}

impl MessageReceiver<client::IntroStageEvent> for server::Intro {
    fn message(&mut self, msg: client::IntroStageEvent)-> anyhow::Result<()>{
        match msg {
            client::IntroStageEvent::AddPlayer(username) =>  {
                info!("{} is trying to connect to the game from {}"
                      , &username, self.addr);
                // could join
                let msg = {
                    
                    if self.state.lock().unwrap().is_full() {
                        warn!("Player limit has been reached");
                        server::LoginStatus::PlayerLimit
                    } else if  self.state.lock().unwrap().check_user_exists(&username) {
                        warn!("Player {} already logged", username );
                        server::LoginStatus::AlreadyLogged
                    } else {
                        server::LoginStatus::Logged
                    }
                    
                };
                self.tx.send(encode_message(server::Message::IntroStage(server::IntroStageEvent::LoginStatus(msg))))?;
                if msg == server::LoginStatus::Logged {
                    self.state.lock().unwrap().add_player(username, self.addr, self.tx.clone(), GameContextId::Intro)?;
                } else {
                    return Err(anyhow!("failed to accept a new connection {:?}", msg));
                }
            },
            _ => todo!() ,// Err(anyhow!(
                  //  "accepted not allowed client message from {}, authentification required"
                   // , addr))
        }
        Ok(())
    }
}
impl MessageReceiver<client::HomeStageEvent> for server::Home {
    fn message(&mut self, msg: client::HomeStageEvent)-> anyhow::Result<()>{
        match msg {
            client::HomeStageEvent::Chat(msg) => {
                let msg = server::ChatLine::Text(format!("{}: {}", self.state.lock().unwrap().get_username(self.addr)?, msg));
                let mut state = self.state.lock().unwrap();
                state.chat.push(msg);
                state.broadcast(self.addr,
                        server::Message::HomeStage(server::HomeStageEvent::Chat(state.chat.last().unwrap().clone())));
            },
            client::HomeStageEvent::GetChatLog => {
                let  state = self.state.lock().unwrap();
                info!("send the chat history to the client");
                let chat = server::Message::HomeStage(server::HomeStageEvent::ChatLog(state.chat.clone()));
                self.tx.send(encode_message(chat))?;
            }
            _ => (),
        }
        Ok(())
    }
}
impl MessageReceiver<client::GameStageEvent> for server::Game {
    fn message(&mut self, msg: client::GameStageEvent)-> anyhow::Result<()>{
        Ok(())
    }
}  /*
                        match msg {

                            ClientMessage::AddPlayer(_) => unreachable!(),
                            ClientMessage::StartGame => {
                                info!("start a game");
                                self.state.lock().await.broadcast(addr, &encode_message(ServerMessage::StartGame));
                            }
                        }
                        */
