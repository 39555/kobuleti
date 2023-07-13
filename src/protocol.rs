
use anyhow::anyhow;
use serde_json;
use serde::{Serialize, Deserialize};
use std::io::Error;
use tokio_util::codec::{ LinesCodec, Decoder};
use futures::{ Stream, StreamExt};
use std::io::ErrorKind;
use client::ClientGameContext;
use server::ServerGameContext;


/// A lightweight alternative for ServerGameContext, ClientGameContext
#[repr(u8)]
#[derive(Debug, Default, Clone, PartialEq, Copy, Serialize, Deserialize)]
pub enum GameContextId{
    #[default]
    Intro,
    Home,
    Game

}

macro_rules! impl_from {
    ($src: ty, $dst: ty, $($src_variant: pat => $self_variant: ident,)+) => {
        impl From<&$src> for $dst {
            fn from(src: &$src) -> Self {
                #[allow(unreachable_patterns)]
                match src {
                    $($src_variant => Self::$self_variant,)*
                     _ => unimplemented!()
                }
            }
        }
    }
}
impl_from!(  GameContextId,             GameContextId,
             GameContextId::Intro =>    Home ,
             GameContextId::Home  =>    Game  ,
          );
impl_from!( ServerGameContext,             GameContextId,
            ServerGameContext::Intro(_) =>    Intro ,
            ServerGameContext::Home(_)  =>    Home  ,
            ServerGameContext::Game(_)  =>    Game  ,
          );
impl_from!( ClientGameContext,             GameContextId,
            ClientGameContext::Intro(_) =>    Intro ,
            ClientGameContext::Home(_)  =>    Home  ,
            ClientGameContext::Game(_)  =>    Game  ,
          );
impl_from!( server::Message,             GameContextId,
            server::Message::Intro(_) =>    Intro ,
            server::Message::Home(_)  =>    Home  ,
            server::Message::Game(_)  =>    Game  ,
          );
impl_from!( client::Message,             GameContextId,
            client::Message::Intro(_) =>    Intro ,
            client::Message::Home(_)  =>    Home  ,
            client::Message::Game(_)  =>    Game  ,
          );

 macro_rules! stage_msg {
            ($e:expr, $p:path) => {
                match $e {
                    $p(value) => Ok(value),
                    _ => Err(anyhow!("a wrong message type for current stage was received: {:?}", $e )),
                }
            };
        }

type Tx = tokio::sync::mpsc::UnboundedSender<String>;


pub mod server {
    use super::*;
    use enum_dispatch::enum_dispatch;
use std::net::SocketAddr;
use crate::server::State;

use std::sync::{Arc, Mutex};

    pub struct Intro{
     pub state: Arc<Mutex<State>>,
     pub addr : SocketAddr,
     pub tx: Tx
    }
    pub struct Home{
        pub state: Arc<Mutex<State>>,
        pub addr : SocketAddr,
        pub tx: Tx
    }
    pub struct Game{
        pub state: Arc<Mutex<State>>,
        pub addr : SocketAddr,
        pub tx: Tx
    }

    #[enum_dispatch]
    pub enum ServerGameContext {
        Intro,
        Home,
        Game,
    }
    impl ServerGameContext {
        pub fn next(&mut self, next: GameContextId){
            take_mut::take(self, |s| {
            match s {
                    ServerGameContext::Intro(i) => {
                        match next {
                            GameContextId::Intro => ServerGameContext::Intro(i),
                            GameContextId::Home => {
                                ServerGameContext::Home(Home{state: i.state, addr: i.addr, tx: i.tx})

                            },
                            GameContextId::Game => { todo!() }
                        }
                    },
                    ServerGameContext::Home(h) => {
                         match next {
                            GameContextId::Intro => unimplemented!(),
                            GameContextId::Home =>  ServerGameContext::Home(h),
                            GameContextId::Game => { 
                               ServerGameContext::Game(Game{state: h.state, addr: h.addr, tx: h.tx})

                            },
                        }
                    },
                    ServerGameContext::Game(_) => {
                            todo!()
                    },

                }

        })
        }
    }
    impl super::MessageReceiver<client::Message> for ServerGameContext {
    fn message(&mut self, msg: client::Message) -> anyhow::Result<()> {
        match self {
            ServerGameContext::Intro(i) => { 
                i.message(stage_msg!(msg, client::Message::Intro)?)?;
            },
            ServerGameContext::Home(h) =>{
                h.message(stage_msg!(msg, client::Message::Home)?)?;
            },
            ServerGameContext::Game(g) => {
                g.message(stage_msg!(msg, client::Message::Game)?)?;
            },
            }
        Ok(())
}
}
    structstruck::strike! {
    #[strikethrough[derive(Deserialize, Serialize, Clone, Debug)]]
    pub enum Message {
        Intro(
            pub enum IntroEvent {
                LoginStatus( 
                    pub enum LoginStatus {
                        #![derive(PartialEq, Copy)]
                        Logged,
                        InvalidPlayerName,
                        AlreadyLogged,
                        PlayerLimit,
                    }
                )
            }
        ),

        Home(
            pub enum HomeEvent {
                 ChatLog(Vec<ChatLine>),
                 Chat(
                        pub enum ChatLine {
                            Text          (String),
                            GameEvent     (String),
                            Connection    (String),
                            Disconnection (String),
                        }
                 ),
            }
        ),
        Game(
            pub enum GameEvent {
                Chat(ChatLine)
            }
        ),
        Main(
            pub enum MainEvent {
                Logout,
                NextContext(GameContextId)
            }
        )
    } }


    
}



pub mod client {
    use super::*;
    use crate::client::Start;
    use super::{server, GameContextId};
    use enum_dispatch::enum_dispatch;
    use crate::client::Chat;
    use crate::ui::terminal::TerminalHandle;
    use std::sync::{Arc, Mutex};
    type Tx = tokio::sync::mpsc::UnboundedSender<String>;

    

    pub struct Intro{
        pub username: String,
        pub tx: Tx,
        pub _terminal_handle: Option<Arc<Mutex<TerminalHandle>>>,
    }


    pub struct Home{
        pub tx: Tx,
        pub _terminal_handle: Arc<Mutex<TerminalHandle>>,
        pub chat: Chat,
    }


    pub struct Game{
        pub tx: Tx,
        pub _terminal_handle: Arc<Mutex<TerminalHandle>>,
        pub chat: Chat,
    }

    #[enum_dispatch]
    pub enum ClientGameContext {
        Intro,
        Home,
        Game,
    }

    impl ClientGameContext {
        pub fn next(& mut self, next: GameContextId){
             take_mut::take(self, |s| {
                 match s {
                    ClientGameContext::Intro(mut i) => {
                        match next {
                            GameContextId::Intro => ClientGameContext::Intro(i),
                            GameContextId::Home => {
                                ClientGameContext::from(Home{
                                    tx: i.tx, _terminal_handle: i._terminal_handle.take().unwrap(), chat: Chat::default()})
                            },
                            GameContextId::Game => { todo!() }
                        }
                    },
                    ClientGameContext::Home(h) => {
                         match next {
                            GameContextId::Intro => unimplemented!(),
                            GameContextId::Home =>  ClientGameContext::Home(h),
                            GameContextId::Game => { 
                                ClientGameContext::from(Game{
                                    tx: h.tx, _terminal_handle: h._terminal_handle, chat: h.chat})
                            },
                        }
                    },
                    ClientGameContext::Game(_) => {
                            todo!()
                    },

                }
             });
        }
    }

    impl MessageReceiver<server::Message> for ClientGameContext {
    fn message(&mut self, msg: server::Message) -> anyhow::Result<()> {
        match self {
            ClientGameContext::Intro(i) => { 
                i.message(stage_msg!(msg, server::Message::Intro)?)?;
            },
            ClientGameContext::Home(h) =>{
                h.message(stage_msg!(msg, server::Message::Home)?)?;
            },
            ClientGameContext::Game(g) => {
                g.message(stage_msg!(msg, server::Message::Game)?)?;
            },
        }
        Ok(())
}
}
    structstruck::strike! {
    #[strikethrough[derive(Deserialize, Serialize, Clone, Debug)]]
    pub enum Message {
        Intro(
            pub enum IntroEvent {
                AddPlayer(String)
            }
        ),
        Home(
            pub enum HomeEvent {
                GetChatLog,
                Chat(String),
                StartGame
            }
        ),
        Game(
            pub enum GameEvent {
                Chat(String)
            }
        ),
        Main(
            pub enum MainEvent {
                RemovePlayer,
                NextContext
            }
        )
    } }
  
}


pub trait MessageReceiver<M> {
    fn message(&mut self, msg: M)-> anyhow::Result<()>;
}


pub struct MessageDecoder<S> {
    pub stream: S
}
// TODO custom error message type
impl<S> MessageDecoder<S>
where S : Stream<Item=Result<<LinesCodec as Decoder>::Item
                           , <LinesCodec as Decoder>::Error>> 
        + StreamExt 
        + Unpin, {
    pub fn new(stream: S) -> Self {
        MessageDecoder { stream } 
    }
    pub async fn next<M>(&mut self) -> Result<M, Error>
    where
        M: for<'b> serde::Deserialize<'b> {
        match self.stream.next().await  {
            Some(msg) => {
                match msg {
                    Ok(msg) => {
                        serde_json::from_str::<M>(&msg)
                        .map_err(
                            |err| Error::new(ErrorKind::InvalidData, format!(
                                "failed to decode a type {} from the socket stream: {}"
                                    , std::any::type_name::<M>(), err))) 
                    },
                    Err(e) => {
                        Err(Error::new(ErrorKind::InvalidData, format!(
                            "an error occurred while processing messages from the socket: {}", e)))
                    }
                }
            },
            None => { // The stream has been exhausted.
                Err(Error::new(ErrorKind::ConnectionAborted, 
                        "received an unknown message. Connection rejected"))
            }
        }
    }

}


pub fn encode_message<M>(message: M) -> String
where M: for<'b> serde::Serialize {
    serde_json::to_string(&message).expect("failed to serialize a message to json")

}


