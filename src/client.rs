
use anyhow::{anyhow,  Context};
use std::net::SocketAddr;
use std::io::ErrorKind;
use futures::{ SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_util::codec::{ LinesCodec, Framed,  FramedRead, FramedWrite};
use tracing::{debug, info, warn, error};
use crate::shared::{ MessageReceiver, MessageDecoder, encode_message,  game_stages::{ GameStage, Intro, Home, Game}};
use crate::shared::{server, client};
use crate::ui::{ UI, terminal};

use std::sync::{Arc, Mutex};
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use enum_dispatch::enum_dispatch;

use crossterm::event::{ Event,  KeyCode, KeyModifiers};



impl MessageReceiver<server::IntroStageEvent> for Intro {
    fn message(&mut self, msg: server::IntroStageEvent)-> anyhow::Result<Option<StageEvent>>{
        use server::{IntroStageEvent, LoginStatus};
        match msg {
            IntroStageEvent::LoginStatus(status) => {
                match status { 
                    LoginStatus::Logged => {
                        info!("Successfull login to the game");
                        // start ui
                        self._terminal_handle  = Some(Arc::new(Mutex::new(
                                    terminal::TerminalHandle::new()
                                    .context("Failed to create a terminal for game")?)));
                        terminal::TerminalHandle::chain_panic_for_restore(Arc::downgrade(&self._terminal_handle.as_ref().unwrap()));
                        Ok(None) 
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
        }
       //.context("Failed to join to the game")?;
    }
}
impl MessageReceiver<server::HomeStageEvent> for Home {
    fn message(&mut self, msg: server::HomeStageEvent) -> anyhow::Result<Option<StageEvent>>{
        use server::HomeStageEvent;
        match msg {
                HomeStageEvent::ChatLog(log) => {
                    self.chat.messages = log
                },
                HomeStageEvent::Chat(line) => {
                    self.chat.messages.push(line);
                }
        }
        Ok(None)
    }
}
impl MessageReceiver<server::GameStageEvent> for Game {
    fn message(&mut self, msg: server::GameStageEvent) -> anyhow::Result<Option<StageEvent>>{
        Ok(None)
    }
}

impl MessageReceiver<server::Message> for GameStage {
    fn message(&mut self, msg: server::Message) -> anyhow::Result<Option<StageEvent>> {
        macro_rules! stage_msg {
            ($e:expr, $p:path) => {
                match $e {
                    $p(value) => Ok(value),
                    _ => Err(anyhow!("a wrong message type for current stage was received: {:?}", $e )),
                }
            };
        }
        match self {
            GameStage::Intro(i) => { 
                i.message(stage_msg!(msg, server::Message::IntroStage)?)?;
            },
            GameStage::Home(h) =>{
                h.message(stage_msg!(msg, server::Message::HomeStage)?)?;
            },
            GameStage::Game(g) => {
                g.message(stage_msg!(msg, server::Message::GameStage)?)?;
            },
        }
        Ok(None)
}
}

#[enum_dispatch(GameStage)]
pub trait Start {
    fn start(&mut self);
}

impl Start for Intro {
    fn start(&mut self) {
        self.tx.send(encode_message(client::Message::IntroStage(client::IntroStageEvent::AddPlayer(self.username.clone()))))
            .context("failed to send a message to the socket").unwrap();
    }
}
impl Start for Home {
    fn start(&mut self) {

    }
}
impl Start for Game {
    fn start(&mut self) {

    }
}


use crate::input::Inputable;

use crate::shared::game_stages::StageEvent;

type Rx = tokio::sync::mpsc::UnboundedReceiver<String>;

pub struct Client {
    context: GameStage,
    app_rx: Rx
}

impl Client {
    pub fn new( username: String) -> Self {
        let (tx, mut app_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        Self { context:GameStage::from(Intro{username,  tx, _terminal_handle: None}), app_rx }
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
        //let mut ui = Ui::new().context("Failed to run a user interface")?;
        //socket_writer.send(encode_message(ClientMessage::GetChatLog)).await.context("failed to request a chat log")?; 
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
                            if self.should_quit(&event) {
                                     info!("Closing the client user interface");
                                     socket_writer.send(encode_message(
                                            client::Message::Common(client::CommonEvent::RemovePlayer))).await?;
                                     break
                                     
                            } else {
                                self.context.handle_input(&event)
                                    .context("failed to process an input event in the current game stage")?
                                    .map(|e| self.process_context_event(e).unwrap() );
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
                            server::Message::Common(e) => {
                                match e {
                                    server::CommonEvent::Logout =>  {
                                        info!("Logout");
                                        break  
                                    },
                                }
                            },
                            _ => {
                                self.context.message(msg)?
                                    //.with_context(|| format!("current context {:?}", self.context ))?
                                    .map(|e| self.process_context_event(e).unwrap() );
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
    fn should_quit(&self, e: &Event) -> bool {
         if let Event::Key(key) = e {
            if KeyCode::Char('c') == key.code && key.modifiers.contains(KeyModifiers::CONTROL) {
                 return true;
            }
        } 
        false

    } 
    fn process_context_event(&mut self, e: StageEvent) -> anyhow::Result<()> {
        match e {
            StageEvent::Next => {
                self.context = self.context.next();
            },
            _ => ()
        }
        Ok(())
    }
   

}

