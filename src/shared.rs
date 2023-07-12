use serde_json;
use serde::{Serialize, Deserialize};
use std::io::Error;
use tokio_util::codec::{ LinesCodec, Decoder};
use futures::{ Stream, StreamExt};
use std::io::ErrorKind;

/// A lightweight alternative to server::ServerGameContext, client::GameContext
#[repr(u8)]
#[derive(Debug, Default, Clone, PartialEq, Copy, Serialize, Deserialize)]
pub enum GameContextId{
    #[default]
    Intro,
    Home,
    Game

}

impl From<&server::ServerGameContext> for GameContextId {
    fn from(context: &server::ServerGameContext) -> Self {
        match context {
            server::ServerGameContext::Intro(_) => GameContextId::Intro,
            server::ServerGameContext::Home(_) => GameContextId::Home,
            server::ServerGameContext::Game(_) => GameContextId::Game
        }
    }
}
impl From<&game_stages::GameContext> for GameContextId {
    fn from(context: &game_stages::GameContext) -> Self {
        match context {
            game_stages::GameContext::Intro(_) => GameContextId::Intro,
            game_stages::GameContext::Home(_) => GameContextId::Home,
            game_stages::GameContext::Game(_) => GameContextId::Game
        }
    }
}
impl From<&server::Message> for GameContextId {
    fn from(context: &server::Message) -> Self {
        match context {
            server::Message::IntroStage(_) => GameContextId::Intro,
            server::Message::HomeStage(_) => GameContextId::Home,
            server::Message::GameStage(_) => GameContextId::Game,
            server::Message::Common(_) => unimplemented!()
        }
    }
}
pub mod game_stages {
    use super::{server, client, GameContextId};
    use super::server::ChatLine;
    use enum_dispatch::enum_dispatch;
    use crate::client::Start;
    use crate::input::InputMode;
    use crate::ui::terminal::TerminalHandle;
    use tui_input::Input;


use std::sync::{Arc, Mutex};
type Tx = tokio::sync::mpsc::UnboundedSender<String>;


   // #[derive(Debug)]
    pub struct Intro{
        pub username: String,
        pub tx: Tx,
        pub _terminal_handle: Option<Arc<Mutex<TerminalHandle>>>,
    }


    //#[derive(Debug)]
    pub struct Home{
        pub tx: Tx,
        pub _terminal_handle: Arc<Mutex<TerminalHandle>>,
        pub chat: Chat,
    }


    //#[derive(Debug)]
    pub struct Game{
        pub tx: Tx,
        pub _terminal_handle: Arc<Mutex<TerminalHandle>>,
        pub chat: Chat,
    }

    #[enum_dispatch]
    //#[derive(Debug)]
    pub enum GameContext {
        Intro,
        Home,
        Game,
    }

    impl GameContext {
        pub fn next(& mut self, next: GameContextId){
             take_mut::take(self, |s| {
                 match s {
                    GameContext::Intro(mut i) => {
                        match next {
                            GameContextId::Intro => GameContext::Intro(i),
                            GameContextId::Home => {
                                GameContext::Home(Home{tx: i.tx, _terminal_handle: i._terminal_handle.take().unwrap(), chat: Chat::default()})
                            },
                            GameContextId::Game => { todo!() }
                        }
                    },
                    GameContext::Home(h) => {
                        todo!()
                    },
                    GameContext::Game(g) => {
                            todo!()
                    },

                }
             });
        }
    }
    
      #[derive(Default, Debug)]
    pub struct Chat {
        pub input_mode: InputMode,
         /// Current value of the input box
        pub input: Input,
        /// History of recorded messages
        pub messages: Vec<server::ChatLine>,
    }

    pub enum StageEvent {
        Next
    }
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
    pub struct None;

    #[enum_dispatch]
    //#[derive(Debug)]
    pub enum ServerGameContext {
        Intro,
        Home,
        Game,
    }
    impl ServerGameContext {
        pub fn next(&mut self){
            take_mut::take(self, |s| {
             match s {
                ServerGameContext::Intro(i) => {
                    ServerGameContext::Home(Home{state: i.state, addr: i.addr, tx: i.tx})
                },
                ServerGameContext::Home(h) => {
                    todo!()
                },
                ServerGameContext::Game(g) => {
                        todo!()
                },

            }
        })
        }
    }

    structstruck::strike! {
    #[strikethrough[derive(Deserialize, Serialize, Clone, Debug)]]
    pub enum Message {
        IntroStage(
            pub enum IntroStageEvent {
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

        HomeStage(
            pub enum HomeStageEvent {
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
        GameStage(
            pub enum GameStageEvent {
                Chat(ChatLine)
            }
        ),
        Common(
            pub enum CommonEvent {
                Logout,
                NextContext(GameContextId)
            }
        )
    } }


    
}



pub mod client {
    use super::*;
    structstruck::strike! {
    #[strikethrough[derive(Deserialize, Serialize, Clone, Debug)]]
    pub enum Message {
        IntroStage(
            pub enum IntroStageEvent {
                AddPlayer(String)
            }
        ),
        HomeStage(
            pub enum HomeStageEvent {
                GetChatLog,
                Chat(String),
                StartGame
            }
        ),
        GameStage(
            pub enum GameStageEvent {
                Chat(String)
            }
        ),
        Common(
            pub enum CommonEvent {
                RemovePlayer,
                NextContext
            }
        )
    } }
}


pub trait MessageReceiver<M> {
    fn message(&mut self, msg: M)-> anyhow::Result<Option<game_stages::StageEvent>>;
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


