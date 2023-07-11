use serde_json;
use serde::{Serialize, Deserialize};
use std::io::Error;
use tokio_util::codec::{ LinesCodec, Decoder};
use futures::{ Stream, StreamExt};
use std::io::ErrorKind;

pub mod game_stages {
    use super::{server, client};
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
    pub enum GameStage {
        Intro,
        Home,
        Game,
    }

    impl GameStage {
        pub fn next(&mut self) -> Self {
            match self {
                GameStage::Intro(intro) => {
                    GameStage::from(Home{tx: intro.tx.clone(), _terminal_handle:  
                        std::mem::replace(&mut intro._terminal_handle, Option::default()).unwrap(), chat: Chat::default()})
                        
                },
                GameStage::Home(_) => {
                    todo!()
                    //GameStage::from(Game{tx: h.tx, _terminal_handle: h._terminal_handle, chat: Chat::default()})
                },
                GameStage::Game(_) => {
                    todo!()
                },

            }

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
        tx: Tx
    }

    #[enum_dispatch]
    //#[derive(Debug)]
    pub enum GameContext {
        Intro,
        Home,
        Game,
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
                Logout
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
                GetGhatLog,
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
                RemovePlayer
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


