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
use crate::protocol::{AsyncMessageReceiver, GameContextId, MessageReceiver, MessageDecoder, encode_message};
use crate::protocol::{server, client, ToContext, Next, Role};
use crate::protocol::server::{ServerGameContext, Connection, Intro, Home, SelectRole, Game};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
/// Shorthand for the transmit half of the message channel.
type Tx = mpsc::UnboundedSender<String>;
/// Shorthand for the receive half of the message channel.
type Rx = mpsc::UnboundedReceiver<String>;
use async_trait::async_trait;
use crate::protocol::{NextContextData, ClientStartGameData};
use crate::game::{AbilityDeck, HealthDeck, Deckable, Deck, MonsterDeck, Card, Rank, Suit};

struct Peer {
    pub context: ServerGameContext,
}


pub enum ToPeer {
    Send(server::Msg),
    GetAddr(Answer<SocketAddr>),
    GetContextId(Answer<GameContextId>),
    GetUsername(Answer<String>),
    Close(String),
    NextContext(crate::protocol::ServerNextContextData), 
    // TODO contexts?????!!!
    GetRole(Answer<Option<Role>>),

}

#[async_trait]
impl<'a> AsyncMessageReceiver<client::Msg, &'a Connection> for Peer {
    async fn message(&mut self, msg: client::Msg, state: &'a Connection)-> anyhow::Result<()>{
         match msg {
            client::Msg::App(e) => {
                match e {
                    client::AppEvent::Logout =>  {
                        state.to_socket.
                            send(encode_message(server::Msg::App(server::AppEvent::Logout)))?;
                        info!("Logout");
                        // TODO error kind
                        return Err(anyhow!("quit"));
                    },
                    client::AppEvent::NextContext => {
                       state.world.request_next_context(state.addr    
                                , GameContextId::from(&self.context));
                    },
                }
            },
            _ => {
                self.context.message(msg, state).await
                    .with_context(|| format!("failed to process a message on the server side: 
                        current context {:?}", GameContextId::from(&self.context) ))?;
            }
        }
        Ok(())
    }
}

// TODO internal commands by contexts?
#[async_trait]
impl<'a> AsyncMessageReceiver<ToPeer, &'a Connection> for Peer {
    async fn message(&mut self, msg: ToPeer, state:  &'a Connection)-> anyhow::Result<()>{
        match msg {
            ToPeer::Close(reason)  => {
                // TODO thiserror errorkind 
                // //self.world_handle.broadcast(self.addr, server::Msg::(ChatLine::Disconnection(
        //                    self.username)));
            }
            ToPeer::Send(msg) => {
                state.to_socket.send(encode_message(msg))?;
            },
            ToPeer::GetAddr(to) => {
                let _ = to.send(state.addr);
            },
            ToPeer::GetContextId(to) => {
                let _ = to.send(GameContextId::from(&self.context));

            },
            ToPeer::GetUsername(to) => { 
                let n = match &self.context {
                    ServerGameContext::Intro(i) => i.username.as_ref()
                        .expect("if world has a peer, this peer must has a username"),
                    ServerGameContext::Home(h) => &h.username,
                    ServerGameContext::SelectRole(r) => &r.username, 
                    ServerGameContext::Game(g) => &g.username,
                };
                let _ = to.send(n.clone());

            },
            
            ToPeer::GetRole(to) => {
                // TODO contexts??!!!!
                info!("ctx: {:?}", GameContextId::from(&self.context));
                if let ServerGameContext::SelectRole(r) = &self.context {
                        let _ = to.send(r.role);
                } else { let _ = to.send(None);}
            },
            ToPeer::NextContext(next_data) => {
                 self.context.to(next_data, state);
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct PeerHandle{
    pub to_peer: mpsc::UnboundedSender<ToPeer>
}


impl Drop for Connection {
    fn drop(&mut self) {
        self.world.drop_player(self.addr);
    }
}
use tokio::sync::oneshot;

type Answer<T> = oneshot::Sender<T>;

pub enum ToServer {
    AddPlayer(SocketAddr, PeerHandle),
    Broadcast(SocketAddr, server::Msg ),
    IsServerFull (Answer<bool>),
    IsUserExists(String, Answer<bool>),
    DropPlayer(SocketAddr),
    AppendChat(server::ChatLine),
    GetChatLog(Answer<Vec<server::ChatLine>>),
    RequestNextContext(SocketAddr, GameContextId),

}

struct PeerSlot {
    pub addr:   SocketAddr,
    pub handle: PeerHandle,
}

pub struct ServerState {
    peers: [Option<PeerSlot>; 2] ,
    chat: Vec<server::ChatLine>  ,
}

pub struct Server {}



pub struct GameSession {
    _monsters : Deck ,
    monster_line : [Option<Card>; 4],
    //pub to_server: UnboundedSender<ToServer> ,
}

impl GameSession {
    fn monsters(&self) -> &[Option<Card>; 4] {
        &self.monster_line
    }
    fn update_monsters(&mut self){
       self.monster_line.iter_mut().filter(|m| m.is_none() ).for_each( |m| {
           *m = self._monsters.cards.pop();
       }); 
    }
}

impl GameSession { 
    fn new() -> Self{ //tx: UnboundedSender<ToServer> ) -> Self {
        let mut s = GameSession{ _monsters: Deck::new_monster_deck(), monster_line: Default::default() };
        s.update_monsters();
        s
    }
}

pub enum ToSession {
    GetMonsters(Answer<[Option<Card>; 4]>)

}


#[derive(Clone, Debug)]
pub struct GameSessionHandle{
    pub to_session: UnboundedSender<ToSession>
}






impl ServerState {
    fn peer_iter(&self) -> impl Iterator<Item=&PeerSlot>{
        self.peers.iter().filter(|p| p.is_some()).map(move |p| p.as_ref().unwrap())
    }

    async fn get_peer(&self, addr: SocketAddr) -> anyhow::Result<&PeerHandle> {
        for p in self.peer_iter(){
            if p.addr == addr {
               return  Ok(&p.handle);
            }
        }
        Err(anyhow!("peer not found with addr {}", addr))
    }
    async fn broadcast(&self, sender: SocketAddr, message: server::Msg) -> anyhow::Result<()>{
        for peer in self.peers.iter().filter(|p| p.is_some()) {
            let p = peer.as_ref().unwrap();
            if p.addr != sender {
                // ignore message from other contexts
                if matches!(&message, server::Msg::App(_)) 
                    || ( p.handle.get_context_id().await == GameContextId::from(&message) ) {
                    info!("send");
                    let _ = p.handle.send(message.clone());
                }
            }
        }
        Ok(())
    }
    
    async fn is_user_exists(&self, username: &String) -> bool {
        for p in self.peer_iter() {
            if p.handle.get_username().await[..] == username[..] { return true; }
        }
        false
    }
  
    fn is_full(&self) -> bool {
        self.peers.iter().position(|p| p.is_none()).is_none()
    }
    fn add_player(&mut self, addr: SocketAddr, player: PeerHandle){
        let it = self.peers.iter_mut().position(|x| x.is_none() )
        .expect("failed to find an empty game slot for a player");
        self.peers[it] = Some(PeerSlot{addr, handle: player});
    }
    fn drop_player(&mut self, who: SocketAddr) {
        for p in self.peers.iter_mut().filter(|p| p.is_some()) {
            info!("try {}", p.as_ref().unwrap().addr);
           if p.as_ref().unwrap().addr == who {  
              *p = None;
              return;
           }
        }
        panic!("failed to find a player for disconnect from SharedState");
        //tracing::info!("disconnect player {}", peer.as_ref().unwrap().connection().username);
    }
    async fn are_all_have_roles(&self) -> bool {
        for p in self.peer_iter() {
                 if p.handle.get_role().await.is_none(){ 
                    return false;
                }
         }
        true
    }
   
    // TODO &str
    async fn get_username(&self, addr: SocketAddr) -> String {
        for p in self.peers.iter().filter(|p| p.is_some()) {
            if p.as_ref().unwrap().addr == addr {
                return p.as_ref().unwrap().handle.get_username().await;
            }
        }
        panic!("addr not found {}", addr)
    }

}
#[async_trait]
impl<'a> AsyncMessageReceiver<ToServer, &'a mut ServerState> for Server {
    async fn message(&mut self, msg: ToServer, state:  &'a mut ServerState)-> anyhow::Result<()>{
        match msg {
            ToServer::Broadcast(sender,  message) => state.broadcast(sender, message).await? ,
            ToServer::IsServerFull(tx) => { 
                let _ = tx.send(state.is_full());} ,
            ToServer::AddPlayer(addr, p) => { 
                state.add_player(addr, p); },
            ToServer::DropPlayer(addr) => { 
                state.drop_player(addr); 
            },
            ToServer::IsUserExists(username, tx) => {
                let _ = tx.send(state.is_user_exists(&username).await).unwrap();}
            ToServer::AppendChat(line) => {
                state.chat.push(line); 
            },
            ToServer::GetChatLog(tx) => { 
                let _ = tx.send(state.chat.clone());}
            ToServer::RequestNextContext(addr, current) => {
                    let p = state.get_peer(addr)
                        .await.expect("failed to find the peer in the world storage");
                    use GameContextId as Id;
                    match current {
                        Id::Intro(_) => {
                            p.next_context(crate::protocol::ServerNextContextData::Home); 
                            state.broadcast(addr, server::Msg::Home(
                                server::HomeEvent::Chat(server::ChatLine::Connection(
                                p.get_username().await)))).await?;
                        },
                        Id::Home(_) => {
                            if state.is_full() {
                                for p in state.peer_iter(){
                                    p.handle.next_context(crate::protocol::ServerNextContextData::SelectRole); 
                                };
                            }
                        },
                        Id::SelectRole(_) => {
                            if state.are_all_have_roles().await {
                                // TODO start game on server
                                let session = GameSession::new();
                                let (to_session, mut session_rx) = mpsc::unbounded_channel::<ToSession>();
                                let handle = GameSessionHandle::for_tx(to_session);
                                tokio::spawn(async move {
                                    loop {
                                        if let Some(cmd) = session_rx.recv().await {
                                            //if let Err(e) = server.message(command, &mut state).await {
                                            //    error!("failed to process messages by world: {}", e);
                                            //    break;
                                        }
                                    };
                                 });


                                for p in state.peer_iter(){
                                    p.handle.next_context(crate::protocol::ServerNextContextData::Game(
                                            crate::protocol::ServerStartGameData{
                                            session: handle.clone(), monsters: *session.monsters()
                                            }
                                            ));
                                };
                            }
                        },
                        Id::Game(_) => (),
                    };
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ServerHandle {
    pub to_world: UnboundedSender<ToServer>,
}

macro_rules! fn_send {
    ($cmd: expr => $sink: expr => $( $fname: ident($($vname:ident : $type: ty $(,)?)*); )+) => {
        paste::item! {
            $( fn $fname(&self, $($vname: $type,)*){
                let _ = self.$sink.send($cmd::[<$fname:camel>]($($vname, )*));
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
                let _ = self.$sink.send($cmd::[<$fname:camel>]($($vname, )* tx));
                rx.await.expect(concat!("failed to process ", stringify!($fname)))
            }
            )*
        }
    }
}

impl GameSessionHandle {
    fn for_tx(tx: UnboundedSender<ToSession>) -> Self{
        GameSessionHandle{to_session: tx}
    } 
    fn_send_and_wait_responce!(
        ToSession => to_session =>
        get_monsters() -> [Option<Card>; 4];
    );
}

impl ServerHandle {
    fn for_tx(tx: UnboundedSender<ToServer>) -> Self{
        ServerHandle{to_world: tx}
    }
    //fn broadcast_to_all(&self, message: server::Msg){
    //    use std::net::{IpAddr, Ipv4Addr};
    //    self.broadcast(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(000, 0, 0, 0)), 0), message);
   // }
    fn_send!(
        ToServer => to_world  =>
            broadcast(sender: SocketAddr, message: server::Msg);
            add_player(addr: SocketAddr, peer: PeerHandle);
            drop_player(who: SocketAddr);
            append_chat(line: server::ChatLine);
            request_next_context(sender: SocketAddr, current: GameContextId);
    );
    fn_send_and_wait_responce!(
         ToServer => to_world =>
        is_server_full() -> bool ;
        is_user_exists(username: String ) -> bool ;
        get_chat_log() -> Vec<server::ChatLine>;
        );
}

impl PeerHandle {
    pub fn for_tx(tx: UnboundedSender<ToPeer>) -> Self {
        PeerHandle{to_peer: tx}
    }
   
    fn_send!(
        ToPeer => to_peer =>
        //close(reason: String);
        send(msg: server::Msg);
        next_context(for_server: crate::protocol::ServerNextContextData);
    );
    fn_send_and_wait_responce!(
        ToPeer => to_peer =>
        get_context_id() -> GameContextId;
        get_username() -> String;
        get_role() -> Option<Role>;
    );
    
}



async fn process_connection(mut socket: TcpStream, world: ServerHandle ) -> anyhow::Result<()> {
        let addr = socket.peer_addr()?;
        let (r, w) = socket.split();
        let (tx, mut rx) = mpsc::unbounded_channel();

        // spawn peer actor
        let (to_peer, mut peer_rx) = mpsc::unbounded_channel();
        let connection = Connection::new(addr, tx, world);
        let mut peer = Peer{context: ServerGameContext::from(Intro::new(PeerHandle::for_tx(to_peer)) )};
        
        let mut socket_writer = FramedWrite::new(w, LinesCodec::new());
        let mut socket_reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));
       
        loop {
            tokio::select! { 
                // a message was received from a peer. send it to the current user.
                Some(msg) = rx.recv() => {
                    socket_writer.send(&msg).await?;
                }

                Some(command) = peer_rx.recv() => {
                    if let Err(e) = peer.message(command, &connection).await{
                        error!("failed to process internal commands by Peer: {}", e);
                        break;
                    }
                }

                msg = socket_reader.next::<client::Msg>() => match msg {
                    Ok(msg) => { 
                        if let Err(e) = peer.message(msg, &connection).await {
                            error!("failed to process client messages by peer {}", e);
                            break;
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
        info!("disconnect");
        Ok(())
    }


pub async fn listen(addr: SocketAddr, shutdown: impl Future) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&addr)
    .await
    .context(format!("Failed to bind a socket to {}", addr))?;
    info!("Listening on: {}", addr);

    // spawn world actor
    let (to_world, mut world_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut state = ServerState{chat: Vec::default(), peers: Default::default()};
            let mut server = Server{};
            loop {
                if let Some(command) = world_rx.recv().await {
                    if let Err(e) = server.message(command, &mut state).await {
                        error!("failed to process messages by world: {}", e);
                        break;
                    }
                };
            }
        });
    let world_handle =  ServerHandle::for_tx(to_world);
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
impl<'a> AsyncMessageReceiver<client::IntroEvent, &'a Connection> for server::Intro {
    async fn message(&mut self, msg: client::IntroEvent, state:  &'a Connection)-> anyhow::Result<()>{
        use server::LoginStatus::*;
        use client::IntroEvent;
        match msg {
            IntroEvent::AddPlayer(username) =>  {
                info!("{} is trying to connect to the game from"
                      , &username);
                // could join
                let msg = {
                    if state.world.is_server_full().await {
                        warn!("Player limit has been reached");
                        PlayerLimit
                            // TODO clone
                    } else if  state.world.is_user_exists(username.clone()).await {
                        warn!("Player {} already logged", username );
                        AlreadyLogged
                    } else {
                        info!("logged");
                        Logged
                    }
                };
                state.to_socket.send(encode_message(server::Msg::Intro(server::IntroEvent::LoginStatus(msg))))?;
                if msg == Logged {
                    self.username = Some(username);
                    state.world.add_player(state.addr, self.peer_handle.clone());
                } else {
                    //self.connection.peer.to_peer.send(PeerCommand::Close);
                    return Err(anyhow!("failed to accept a new connection {:?}", msg));
                }
            },
            IntroEvent::GetChatLog => {
                info!("send the chat history to the client");
                state.to_socket.send(encode_message(server::Msg::Intro(
                    server::IntroEvent::ChatLog(state.world.get_chat_log().await))))?;
            }
           // _ => todo!() ,// Err(anyhow!(
                  //  "accepted not allowed client message from {}, authentification required"
                   // , addr))
        }
        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<client::HomeEvent, &'a Connection> for server::Home {
    async fn message(&mut self, msg: client::HomeEvent, state:  &'a Connection)-> anyhow::Result<()>{
        use client::HomeEvent::*;
        info!("message from client for home");
        match msg {
            Chat(msg) => {
                let msg = server::ChatLine::Text(
                    format!("{}: {}", self.username , msg));
                state.world.append_chat(msg.clone());
                state.world.broadcast(state.addr, server::Msg::Home(server::HomeEvent::Chat(msg)));
            },
            _ => (),
        }
        Ok(())
    }
}

#[async_trait]
impl<'a> AsyncMessageReceiver<client::GameEvent, &'a Connection> for server::Game {
    async fn message(&mut self, msg: client::GameEvent, state:  &'a Connection)-> anyhow::Result<()>{
        use client::GameEvent::*;
        match msg {
            Chat(msg) => {
                let msg = server::ChatLine::Text(
                    format!("{}: {}", self.username, msg));
                state.world.append_chat(msg.clone());
                state.world.broadcast(state.addr, server::Msg::Game(server::GameEvent::Chat(msg)));
            },
        }
        Ok(())
    }
}  
#[async_trait]
impl<'a> AsyncMessageReceiver<client::SelectRoleEvent, &'a Connection> for server::SelectRole {
    async fn message(&mut self, msg: client::SelectRoleEvent, state:  &'a Connection)-> anyhow::Result<()>{
        use client::SelectRoleEvent::*;
        match msg {
            Chat(msg) => {
                let msg = server::ChatLine::Text(
                    format!("{}: {}", self.username, msg));
                state.world.append_chat(msg.clone());
                state.world.broadcast(state.addr, server::Msg::SelectRole(server::SelectRoleEvent::Chat(msg)));
            },
            Select(role) => {
                self.role = Some(role);
                info!("select role {:?}", self.role);
            }
        }
        Ok(())
    }
}  

