use anyhow::Context as _;
use std::net::SocketAddr;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn, error, trace, debug};
use async_trait::async_trait;
use crate::{ protocol::{
                client,
                server::{self, 
                    Msg,
                    ServerGameContext,
                    ServerNextContextData,
                    LoginStatus,
                },
                GameContextId,
                AsyncMessageReceiver,
                MessageError,
                encode_message,
                ToContext,
                Tx,



            },
             server::{Answer, ServerHandle},
             game::Role

};


pub struct Peer {
    pub context: ServerGameContext,
}
impl Peer {
    pub fn new(start_context: ServerGameContext) -> Peer {
        Peer{ context: start_context }
    }
}

pub enum ToPeer {
    Send(server::Msg),
    GetAddr(Answer<SocketAddr>),
    GetContextId(Answer<GameContextId>),
    GetUsername(Answer<String>),
    Close(String),
    NextContext(ServerNextContextData), 
    GetRole(Answer<Option<Role>>),
    GetConnectionStatus(Answer<ConnectionStatus>),

}
#[derive(Debug, Clone)]
pub struct PeerHandle{
    pub tx: UnboundedSender<ToPeer>,
}

use crate::server::details::fn_send;
use crate::server::details::fn_send_and_wait_responce;
impl PeerHandle {
    pub fn for_tx(tx: UnboundedSender<ToPeer>) -> Self {
        PeerHandle{tx}
    }
   
    fn_send!(
        ToPeer => tx =>
        //close(reason: String);
        send(msg: server::Msg);
        next_context(for_server: ServerNextContextData);
    );
    fn_send_and_wait_responce!(
        ToPeer => tx =>
        get_context_id() -> GameContextId;
        get_username() -> String;
        get_role() -> Option<Role>;
    );
    
}

pub enum ConnectionStatus {
    NotLogged,
    Connected,
    WaitReconnection
}      
pub struct Connection {
    //pub status: ConnectionStatus,
    pub addr     : SocketAddr,
    pub to_socket: Tx,
    pub server: ServerHandle,
}

impl Connection {
    pub fn new(addr: SocketAddr, socket_tx: Tx, world_handle: ServerHandle) -> Self {
        Connection{//status: ConnectionStatus::NotLogged,
         addr, to_socket: socket_tx, server: world_handle}
    }
}

#[async_trait]
impl<'a> AsyncMessageReceiver<client::Msg, &'a Connection> for Peer {
    async fn message(&mut self, msg: client::Msg, state: &'a Connection)-> Result<(), MessageError>{
         match msg {
            client::Msg::App(e) => {
                match e {
                    client::AppEvent::Logout =>  {
                        let _ = state.to_socket.
                            send(encode_message(server::Msg::App(server::AppEvent::Logout)));
                        info!("Logout");
                        // TODO 
                        //return Err(MessageError::Logout);
                    },
                    client::AppEvent::NextContext => {
                       state.server.request_next_context(state.addr    
                                , GameContextId::from(&self.context));
                    },
                }
            },
            _ => {
                self.context.message(msg, state).await
                    .with_context(|| format!("failed to process a message on the server side: 
                        current context {:?}", GameContextId::from(&self.context) ))
                    .map_err(|e| MessageError::Unknown(format!("{}", e)))?;
            }
        }
        Ok(())
    }
}

// TODO internal commands by contexts?
#[async_trait]
impl<'a> AsyncMessageReceiver<ToPeer, &'a Connection> for Peer {
    async fn message(&mut self, msg: ToPeer, state:  &'a Connection) -> Result<(), MessageError>{
        let mut result = Ok(());
        match msg {
            ToPeer::Close(reason)  => {
                // TODO thiserror errorkind 
                // //self.world_handle.broadcast(self.addr, server::Msg::(ChatLine::Disconnection(
        //                    self.username)));
            }
            ToPeer::Send(msg) => {
                let _ = state.to_socket.send(encode_message(msg));
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
                 let next_ctx_id = GameContextId::from(&next_data);
                 let _ = self.context.to(next_data, state).map_err(
                     |e| result = Err(MessageError::NextContextRequestError{
                        next: next_ctx_id,
                        current: GameContextId::from(&self.context),
                        reason: e.to_string()
                     }));
            },
            _ => (),
            
        }
       result
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.server.drop_player(self.addr);
    }
}


#[async_trait]
impl<'a> AsyncMessageReceiver<client::IntroEvent, &'a Connection> for server::Intro {
    async fn message(&mut self, msg: client::IntroEvent, state:  &'a Connection)-> Result<(), MessageError>{
        use client::IntroEvent;
        match msg {
            IntroEvent::AddPlayer(username) =>  {
                info!("{} is trying to connect to the game as {}",
                      state.addr , &username); 
                let status = state.server.add_player(state.addr, 
                                        username.clone(), 
                                        self.peer_handle.clone()).await;
                self.username = Some(username);
                encode_message(Msg::Intro(
                    server::IntroEvent::LoginStatus(status)));
                if status != LoginStatus::Logged {
                    return Err(MessageError::LoginRejected{
                        reason: format!("{:?}", status)
                    });
                }
            },
            IntroEvent::GetChatLog => {
                // TODO check logging?
                if self.username.is_none() {
                    warn!("Client not logged but the ChatLog requested");
                    return Err(MessageError::NotLogged);
                }
                info!("Send a chat history to the client");
                let _ = state.to_socket.send(encode_message(server::Msg::Intro(
                    server::IntroEvent::ChatLog(state.server.get_chat_log().await))));
            }
        }
        Ok(())
    }
}
#[async_trait]
impl<'a> AsyncMessageReceiver<client::HomeEvent, &'a Connection> for server::Home {
    async fn message(&mut self, msg: client::HomeEvent, state:  &'a Connection)-> Result<(), MessageError>{
        use client::HomeEvent;
        info!("message from client for home");
        match msg {
            HomeEvent::Chat(msg) => {
                let msg = server::ChatLine::Text(
                    format!("{}: {}", self.username , msg));
                state.server.append_chat(msg.clone());
                state.server.broadcast(state.addr, Msg::Home(server::HomeEvent::Chat(msg)));
            },
            _ => (),
        }
        Ok(())
    }
}

#[async_trait]
impl<'a> AsyncMessageReceiver<client::GameEvent, &'a Connection> for server::Game {
    async fn message(&mut self, msg: client::GameEvent, state:  &'a Connection)-> Result<(), MessageError>{
        use client::GameEvent;
        match msg {
            GameEvent::Chat(msg) => {
                let msg = server::ChatLine::Text(
                    format!("{}: {}", self.username, msg));
                state.server.append_chat(msg.clone());
                state.server.broadcast(state.addr, server::Msg::Game(server::GameEvent::Chat(msg)));
            },
        }
        Ok(())
    }
}  
#[async_trait]
impl<'a> AsyncMessageReceiver<client::SelectRoleEvent, &'a Connection> for server::SelectRole {
    async fn message(&mut self, msg: client::SelectRoleEvent, state:  &'a Connection)-> Result<(), MessageError>{
        use client::SelectRoleEvent;
        match msg {
            SelectRoleEvent::Chat(msg) => {
                let msg = server::ChatLine::Text(
                    format!("{}: {}", self.username, msg));
                state.server.append_chat(msg.clone());
                state.server.broadcast(state.addr, server::Msg::SelectRole(server::SelectRoleEvent::Chat(msg)));
            },
            SelectRoleEvent::Select(role) => {
                self.role = Some(role);
                info!("select role {:?}", self.role);
            }
        }
        Ok(())
    }
}  


