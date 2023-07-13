
use anyhow::{anyhow,  Context};
use std::net::SocketAddr;
use std::io::ErrorKind;
use futures::{ SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_util::codec::{ LinesCodec, Framed,  FramedRead, FramedWrite};
use tracing::{debug, info, warn, error};
use crate::protocol::{ server::ChatLine, GameContextId, MessageReceiver,
    MessageDecoder, encode_message, client::{  ClientGameContext, Intro, Home, Game}};
use crate::protocol::{server, client};
use crate::ui::{ UI, terminal};

use std::sync::{Arc, Mutex};
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use enum_dispatch::enum_dispatch;

use crossterm::event::{ Event,  KeyCode, KeyModifiers};
use crate::input::InputMode;
use tui_input::Input;
#[derive(Default, Debug)]
pub struct Chat {
        pub input_mode: InputMode,
         /// Current value of the input box
        pub input: Input,
        /// History of recorded messages
        pub messages: Vec<server::ChatLine>,
    }



impl MessageReceiver<server::IntroEvent> for Intro {
    fn message(&mut self, msg: server::IntroEvent)-> anyhow::Result<()>{
        use server::{IntroEvent, LoginStatus};
        let r = match msg {
            IntroEvent::LoginStatus(status) => {
                match status { 
                    LoginStatus::Logged => {
                        info!("Successfull login to the game");
                        // start ui
                        self._terminal_handle  = Some(Arc::new(Mutex::new(
                                    terminal::TerminalHandle::new()
                                    .context("Failed to create a terminal for game")?)));
                        terminal::TerminalHandle::chain_panic_for_restore(Arc::downgrade(&self._terminal_handle.as_ref().unwrap()));
                        Ok(()) 
                    },
                    LoginStatus::InvalidPlayerName => {
                        Err(anyhow!("Invalid player name: '{}'", self.username))
                    },
                    LoginStatus::PlayerLimit => {
                        Err(anyhow!("Player '{}' has tried to login but the player limit has been reached"
                                    , self.username))
                    },
                    LoginStatus::AlreadyLogged => {
                        Err(anyhow!("User with name '{}' already logged", self.username))
                    },

                }
            }
        };
       r.context("Failed to join to the game")
    }
}
impl MessageReceiver<server::HomeEvent> for Home {
    fn message(&mut self, msg: server::HomeEvent) -> anyhow::Result<()>{
        use server::HomeEvent;
        match msg {
                HomeEvent::ChatLog(log) => {
                    self.chat.messages = log
                },
                HomeEvent::Chat(line) => {
                    self.chat.messages.push(line);
                }
        }
        Ok(())
    }
}
impl MessageReceiver<server::GameEvent> for Game {
    fn message(&mut self, msg: server::GameEvent) -> anyhow::Result<()>{
        Ok(())
    }
}


#[enum_dispatch(ClientGameContext)]
pub trait Start {
    fn start(&mut self);
}

impl Start for Intro {
    fn start(&mut self) {
        self.tx.send(encode_message(client::Message::Intro(client::IntroEvent::AddPlayer(self.username.clone()))))
            .context("failed to send a message to the socket").unwrap();
    }
}
impl Start for Home {
    fn start(&mut self) {
        // TODO maybe do it in Intro while chat is invisible
        self.tx.send(encode_message(client::Message::Home(client::HomeEvent::GetChatLog)))
            .context("failed to send a message to the socket").unwrap();
    }
}
impl Start for Game {
    fn start(&mut self) {

    }
}


use crate::input::Inputable;


type Rx = tokio::sync::mpsc::UnboundedReceiver<String>;

pub struct Client {
    context: ClientGameContext,
    app_rx: Rx
}

impl Client {
    pub fn new( username: String) -> Self {
        let (tx, app_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        Self { context: ClientGameContext::from(Intro{username,  tx, _terminal_handle: None}), app_rx }
    }

    pub async fn connect(&mut self, host: SocketAddr,) -> anyhow::Result<()> {
        let stream = TcpStream::connect(host)
            .await.with_context(|| format!("Failed to connect to address {}", host))?;
        self.process_incoming_messages(stream).await.context("failed to process messages from the server")?;
        info!("Quit the game");
        Ok(())
    }
    async fn process_incoming_messages(&mut self, mut stream: TcpStream
                                       ) -> anyhow::Result<()>{
        let (r, w) = stream.split();
        let mut socket_writer = FramedWrite::new(w, LinesCodec::new());
        let mut socket_reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));
        let mut input_reader  = crossterm::event::EventStream::new();
        self.context.start();
        loop {
            tokio::select! {
                input = input_reader.next() => {
                    match  input {
                        None => break,
                        Some(Err(e)) => {
                            warn!("IO error on stdin: {}", e);
                        }, 
                        Some(Ok(event)) => { 
                            // TODO common context input processor
                            if Client::should_quit(&event) {
                                     info!("Closing the client user interface");
                                     socket_writer.send(encode_message(
                                            client::Message::Main(client::MainEvent::RemovePlayer))).await?;
                                     break
                                     
                            } else {
                                self.context.handle_input(&event)
                                    .context("failed to process an input event in the current game stage")?;
                                self.context.draw()?;
                            }
                        }
                    }
                }
                Some(msg) = self.app_rx.recv() => {
                    socket_writer.send(&msg).await.context("failed to send a message to the socket")?;
                }

                r = socket_reader.next::<server::Message>() => match r { 
                    Ok(msg) => {
                        match msg {
                            server::Message::Main(e) => {
                                match e {
                                    server::MainEvent::Logout =>  {
                                        info!("Logout");
                                        break  
                                    },
                                    server::MainEvent::NextContext(n) => {
                                        self.context.next(n);
                                        self.context.start();

                                    },
                                    _ => (),
                                }
                            },
                            _ => {
                                self.context.message(msg)
                                    .with_context(|| format!("current context {:?}", GameContextId::from(&self.context) ))?;
                            }
                        }
                        self.context.draw()?;
                    }
                    ,
                    Err(e) => { 
                        warn!("Error: {}", e);
                        if e.kind() == ErrorKind::ConnectionAborted {
                            break
                        }

                    }
                }
            }
        }
        Ok(())
    }
    fn should_quit(e: &Event) -> bool {
         if let Event::Key(key) = e {
            if KeyCode::Char('c') == key.code && key.modifiers.contains(KeyModifiers::CONTROL) {
                 return true;
            }
        } 
        false

    } 
  
   

}

