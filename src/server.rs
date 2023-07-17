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
use tracing::{trace, debug, info, warn, error};
use std::sync::{Arc, Mutex};
use tokio_stream::StreamExt;
use futures::{future, Sink, SinkExt};
use std::future::Future;
use tokio_util::codec::{LinesCodec, Framed, FramedRead, FramedWrite};
use crate::protocol::{ GameContextId, MessageReceiver, MessageDecoder, encode_message};
use crate::protocol::{server, client, To, Next, Role};
use crate::protocol::server::{ServerGameContext, Connection};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
    use enum_dispatch::enum_dispatch;
/// Shorthand for the transmit half of the message channel.
type Tx = mpsc::UnboundedSender<String>;
/// Shorthand for the receive half of the message channel.
type Rx = mpsc::UnboundedReceiver<String>;
use async_trait::async_trait;

struct Peer {
    pub addr     : SocketAddr,
    pub username : Option<String>,
    pub context  : ServerGameContext,
    pub world_handle: WorldHandle
}

impl Peer {
    fn new(addr: SocketAddr, tx: Tx, world_handle: WorldHandle) -> PeerHandle {
        let (to_peer, mut rx) = mpsc::unbounded_channel();
        let handle = PeerHandle{to_peer};
        let peer = Peer{addr, username: Default::default(), context: ServerGameContext::from(
                    ServerGameContext::Intro(server::Intro{ connection: Connection{tx, peer: handle.clone()},world_handle: world_handle.clone()}))
                , world_handle
            };
        tokio::spawn(async move {
            loop {
                if let Some(command) = rx.recv().await {
                    // TODO return errokind
                    if let Err(e) = peer.message(command).await{
                        error!("failed to process messages by Peer: {}", e);
                        break;
                    }
                };
            }
        });
        handle
        
    }
}
#[derive(Debug)]
enum PeerCommand {
    ClientMessage(client::Msg),
    Send(server::Msg),
    GetAddr(Response<SocketAddr>),
    GetContextId(Response<GameContextId>),
    GetUsername(Response<String>),
    Close(String),

}
#[async_trait]
impl AsyncMessageReceiver<PeerCommand> for Peer {
    async fn message(&mut self, msg: PeerCommand)-> anyhow::Result<()>{
        match msg {
            PeerCommand::Close(reason)  => {
                // TODO thiserror errorkind
            }
            PeerCommand::ClientMessage(msg) => {
                match msg {
                    client::Msg::App(e) => {
                        match e {
                            client::AppEvent::Logout =>  {
                                self.context.connection().tx.
                                    send(encode_message(server::Msg::App(server::AppEvent::Logout)))?;
                                info!("Logout");
                                // TODO error kind
                                return Err(anyhow!("quit"));
                            },
                            client::AppEvent::NextContext => {
                                let curr = GameContextId::from(&self.context);
                                let next = GameContextId::next(curr);
                                match &self.context {
                                    ServerGameContext::Intro(i) => {
                                         self.context.connection().
                                             tx.send(encode_message(server::Msg::App(server::AppEvent::NextContext(
                                                next))))?;
                                        {
                                            self.world_handle.broadcast( self.addr, server::Msg::Home(
                                                    server::HomeEvent::Chat(server::ChatLine::Connection(
                                                        self.username.unwrap()))));
                                        }
                                        self.context.to(next);
                                    }
                                    ServerGameContext::Home(h) => {
                                         if self.world_handle.is_server_full().await {
                                             // TODO change context in other peers
                                            self.world_handle.broadcast_to_all(server::Msg::App(server::AppEvent::NextContext(
                                                next)));
                                            self.context.to(next);
                                        }
                                    },
                                    ServerGameContext::SelectRole(r) => {
                                            info!("change context to game");
                                            //if r.connection.state.lock().unwrap().peers.iter().all(
                                               // |p| p.is_some() && p.as_ref().unwrap().role.is_some() ) {
                                                //r.connection.state.lock().unwrap().change_context_for_all(next);
                                                //r.connection.state.lock().unwrap().broadcast_to_all(server::Msg::App(server::AppEvent::NextContext(
                                                //    next)));
                                                self.context.to(next);
                                            //}
                                    },
                                    ServerGameContext::Game(g) => {
                                       () 
                                    },

                                }
                            
                                }
                        }
                    },
                    _ => {
                        self.context.message(msg)
                            .with_context(|| format!("failed to process a message on the server side: 
                                                     current context {:?}", GameContextId::from(&self.context) ))?;
                    }
                }
            }

            _ => (),
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct PeerHandle{
    pub to_peer: mpsc::UnboundedSender<PeerCommand>
}


impl Drop for Peer {
    fn drop(&mut self) {
        //self.world_handle.broadcast(self.addr, server::Msg::(ChatLine::Disconnection(
        //                    self.username)));
        self.world_handle.drop_player(self.addr);
    }
}
use tokio::sync::oneshot;
use tokio::sync::oneshot::Sender;


#[derive(Debug)]
struct Response<T>(oneshot::Sender<T>);
impl<T> From<Sender<T>> for Response<T>{
    fn from(src: Sender<T>) -> Self{
        Response::<T>(src)
    }
}
pub enum WorldCommand {
    AddPlayer(PeerHandle),
    Broadcast(SocketAddr, server::Msg ),
    IsServerFull (Response<bool>),
    // TODO &str
    IsUserExists(String, Response<bool>),
    DropPlayer(SocketAddr),
    AppendChat(server::ChatLine),
    GetChatLog(Response<Vec<server::ChatLine>>),
    GetUsername(SocketAddr,Response<String>)
}

pub struct World {
    peers: [Option<PeerHandle>; 2],
    chat: Vec<server::ChatLine>,
}
impl World {
    fn new() -> WorldHandle {
        let (to_world, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let world = World{chat: Vec::default(), peers: Default::default()};
            loop {
                if let Some(command) = rx.recv().await {
                    if let Err(e) = world.message(command).await {
                        error!("failed to process messages by world: {}", e);
                        break;
                    }
                };
            }
        });
        WorldHandle{to_world}
    }
    async fn broadcast(&self, sender: SocketAddr, message: server::Msg) -> anyhow::Result<()>{
        for peer in self.peers.iter().filter(|p| p.is_some()) {
            let p = peer.as_ref().unwrap();
            if p.get_addr().await != sender {
                // ignore message from other contexts
                if matches!(&message, server::Msg::App(_)) 
                    || ( p.get_context_id().await == GameContextId::from(&message) ) {
                    let _ = p.to_peer.send(PeerCommand::Send(message.clone()));
                }
            }
        }
        Ok(())
    }
    async fn is_user_exists(&self, username: &String) -> bool {
        for p in self.peers.iter().filter(|p| p.is_some()) {
            if p.as_ref().unwrap().get_username().await[..] == username[..] { return true; }
        }
        false
    }
  
    fn is_full(&self) -> bool {
        self.peers.iter().position(|p| p.is_none()).is_none()
    }
    fn add_player(&mut self, player: PeerHandle) -> anyhow::Result<()> {
        let it = self.peers.iter_mut().position(|x| x.is_none() )
        .context("failed to find an empty game slot for a player")?;
        self.peers[it] = Some(player);
        Ok(())
    }
    async fn remove_player(&mut self, who: SocketAddr) -> anyhow::Result<()> {
        for p in self.peers.iter_mut().filter(|p| p.is_some()) {
           if p.as_ref().unwrap().get_addr().await == who {   
              *p = None;
              return Ok(());
           }
        }
        Err(anyhow!("failed to find a player for disconnect from SharedState"))
        //tracing::info!("disconnect player {}", peer.as_ref().unwrap().connection().username);
    }
    fn append_chat(&mut self, l: server::ChatLine) {
        self.chat.push(l);
    }
    // TODO &str
    async fn get_username(&self, addr: SocketAddr) -> String {
        for p in self.peers.iter().filter(|p| p.is_some()) {
            if p.as_ref().unwrap().get_addr().await == addr {
                return p.as_ref().unwrap().get_username().await;
            }
        }
        panic!("addr not found {}", addr)
    }

}
#[async_trait]
pub trait AsyncMessageReceiver<M> {
    async fn message(&mut self, msg: M)-> anyhow::Result<()>;
}
#[async_trait]
impl AsyncMessageReceiver<WorldCommand> for World {
    async fn message(&mut self, msg: WorldCommand)-> anyhow::Result<()>{
        match msg {
            WorldCommand::Broadcast(sender,  message) => self.broadcast(sender, message).await? ,
            WorldCommand::IsServerFull(respond_to) => { respond_to.0.send(self.is_full());} ,
            WorldCommand::AddPlayer(p) => { self.add_player(p); },
            WorldCommand::DropPlayer(addr) => { self.remove_player(addr); },
            WorldCommand::IsUserExists(username, respond_to) => {respond_to.0.send(self.is_user_exists(&username).await);}
            WorldCommand::AppendChat(line) => { self.chat.push(line); },
            WorldCommand::GetChatLog(respond_to) => { respond_to.0.send(self.chat.clone());}
            WorldCommand::GetUsername(addr, respond_to) => {respond_to.0.send(self.get_username(addr).await);}
            _ => (),
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct WorldHandle {
    pub to_world: UnboundedSender<WorldCommand>,
}

macro_rules! fn_send {
    ($cmd: expr => $sink: expr => $( $fname: ident($($vname:ident : $type: ty $(,)?)*); )+) => {
        paste::item! {
            $( fn $fname(&self, $($vname: $type,)*){
                self.$sink.send($cmd::[<$fname:camel>]($($vname, )*));
            }
            )*
        }
    }
}
macro_rules! fn_send_and_wait_responce {
    ($cmd: expr => $sink: expr => $( $fname: ident($($vname:ident : $type: ty $(,)?)*) -> $ret: ty; )+) => {
        paste::item! {
            $( async fn $fname(&self, $($vname: $type,)*) -> $ret {
                let (tx, rx) = oneshot::channel();
                self.$sink.send($cmd::[<$fname:camel>]($($vname, )* tx.into()));
                rx.await.expect(concat!("failed to process ", stringify!($fname)))
            }
            )*
        }
    }
}
impl WorldHandle {
    fn broadcast_to_all(&self, message: server::Msg){
        use std::net::IpAddr;
        use std::str::FromStr;
        self.broadcast(SocketAddr::new(IpAddr::from_str("000.0.0.0").unwrap(), 0), message);
    }
    fn_send!(
        WorldCommand => to_world  =>
            broadcast(sender: SocketAddr, message: server::Msg);
            add_player(peer: PeerHandle);
            drop_player(who: SocketAddr);
            append_chat(line: server::ChatLine);
    );
    fn_send_and_wait_responce!(
         WorldCommand => to_world =>
        is_server_full() -> bool ;
        is_user_exists(username: String ) -> bool ;
        get_username(addr: SocketAddr) -> String ;
        get_chat_log() -> Vec<server::ChatLine>;
        );
}

impl PeerHandle {
    fn_send!(
        PeerCommand => to_peer =>
        close(reason: String);
    );
    fn_send_and_wait_responce!(
        PeerCommand => to_peer =>
        get_addr() -> SocketAddr;
        get_context_id() -> GameContextId;
        get_username() -> String;
    );
}



async fn process_connection(mut socket: TcpStream, world: WorldHandle ) -> anyhow::Result<()> {
        let addr = socket.peer_addr()?;
        let (r, w) = socket.split();
        let (tx, mut rx) = mpsc::unbounded_channel();

        let player_handle = Peer::new(addr, tx, world.clone());
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
                        player_handle.to_peer.send(PeerCommand::ClientMessage(msg))?;
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


pub async fn listen(addr: SocketAddr, shutdown: impl Future) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr)
    .await
    .context(format!("Failed to bind a socket to {}", addr))?;
    info!("Listening on: {}", addr);

    let world_handle = World::new();
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
                        let world = world_handle.clone();
                        tokio::spawn(async move {
                            if let Err(e) = process_connection(stream, world).await {
                                error!("an error occurred; error = {:?}", e);
                            }
                            info!("{} has disconnected", addr);
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


#[async_trait]
impl AsyncMessageReceiver<client::IntroEvent> for server::Intro {
    async fn message(&mut self, msg: client::IntroEvent)-> anyhow::Result<()>{
        use server::LoginStatus::*;
        use client::IntroEvent;
        match msg {
            IntroEvent::AddPlayer(username) =>  {
                info!("{} is trying to connect to the game from"
                      , &username);
                // could join
                let msg = {
                    if self.world_handle.is_server_full().await {
                        warn!("Player limit has been reached");
                        PlayerLimit
                    } else if  self.world_handle.is_user_exists(username).await {
                        warn!("Player {} already logged", username );
                        AlreadyLogged
                    } else {
                        info!("logged");
                        Logged
                    }
                };
                self.connection.tx.send(encode_message(server::Msg::Intro(server::IntroEvent::LoginStatus(msg))))?;
                if msg == Logged {
                    self.world_handle.add_player(self.connection.peer.clone());
                } else {
                    //self.connection.peer.to_peer.send(PeerCommand::Close);
                    return Err(anyhow!("failed to accept a new connection {:?}", msg));
                }
            },
            IntroEvent::GetChatLog => {
                info!("send the chat history to the client");
                self.connection.tx.send(encode_message(server::Msg::Intro(
                    server::IntroEvent::ChatLog(self.world_handle.get_chat_log().await))))?;
            }
            _ => todo!() ,// Err(anyhow!(
                  //  "accepted not allowed client message from {}, authentification required"
                   // , addr))
        }
        Ok(())
    }
}
#[async_trait]
impl AsyncMessageReceiver<client::HomeEvent> for server::Home {
    async fn message(&mut self, msg: client::HomeEvent)-> anyhow::Result<()>{
        use client::HomeEvent::*;
        match msg {
            Chat(msg) => {
                let addr =  self.connection.peer.get_addr().await;
                let msg = server::ChatLine::Text(
                    format!("{}: {}", self.world_handle.get_username(addr
                            ).await, msg));
                self.world_handle.append_chat(msg.clone());
                self.world_handle.broadcast(addr, server::Msg::Home(server::HomeEvent::Chat(msg)));
            },
            _ => (),
        }
        Ok(())
    }
}
impl MessageReceiver<client::GameEvent> for server::Game {
    fn message(&mut self, msg: client::GameEvent)-> anyhow::Result<()>{
        Ok(())
    }
}  
#[async_trait]
impl AsyncMessageReceiver<client::SelectRoleEvent> for server::SelectRole {
    async fn message(&mut self, msg: client::SelectRoleEvent)-> anyhow::Result<()>{
        use client::SelectRoleEvent::*;
        match msg {
            Chat(msg) => {
                let addr =  self.connection.peer.get_addr().await;
                let msg = server::ChatLine::Text(
                    format!("{}: {}", self.world_handle.get_username(addr
                            ).await, msg));
                self.world_handle.append_chat(msg.clone());
                self.world_handle.broadcast(addr, server::Msg::SelectRole(server::SelectRoleEvent::Chat(msg)));
            },
            Select(role) => {
                self.role = Some(role);
            }
           
            _ => (),
        }
        Ok(())
    }
}  

