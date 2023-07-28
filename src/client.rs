
use anyhow::{anyhow,  
            Context as _};
use std::net::SocketAddr;
use std::io::ErrorKind;
use futures::{ SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_util::codec::{ LinesCodec, Framed,  FramedRead, FramedWrite};
use tracing::{debug, info, warn, error};
use crate::protocol::{ server::ChatLine, GameContextId, MessageReceiver, ToContext,
    MessageDecoder, encode_message, client::{ Connection, ClientGameContext, Intro, Home, Game, App, SelectRole}};
use crate::protocol::{server, client};
use crate::ui::{ self, TerminalHandle};

use std::sync::{Arc, Mutex};
type Tx = tokio::sync::mpsc::UnboundedSender<String>;

use crossterm::event::{ Event,  KeyCode, KeyModifiers};
use crate::input::InputMode;
use tui_input::Input;
use crate::protocol::MessageError;
#[derive(Default, Debug)]
pub struct Chat {
        pub input_mode: InputMode,
         /// Current value of the input box
        pub input: Input,
        /// History of recorded messages
        pub messages: Vec<server::ChatLine>,
    }



impl MessageReceiver<server::IntroEvent, &client::Connection> for Intro {
    fn message(&mut self, msg: server::IntroEvent, state: &client::Connection) -> Result<(), MessageError>{
        use server::{IntroEvent::*, LoginStatus::*};
        match msg {
            LoginStatus(status) => {
                self.status = Some(status);
                match status { 
                    Logged => {
                        info!("Successfull login to the game");
                        Ok(()) 
                    },
                    InvalidPlayerName => {
                        Err(MessageError::LoginRejected{
                            reason: format!(
                                        "Invalid player name: '{}'", 
                                        state.username)})
                    },
                    PlayerLimit => {
                        Err(MessageError::LoginRejected{
                            reason: format!(
                    "Player '{}' has tried to login but the player limit has been reached"
                                    , state.username)})
                    },
                    AlreadyLogged => {
                        Err(MessageError::LoginRejected{
                            reason: format!(
                            "User with name '{}' already logged", 
                            state.username)})
                    },
                }
            }
            ,
            ChatLog(log) => {
                    self.chat_log = Some(log);
                    Ok(())
            },
        }
    }
}
impl MessageReceiver<server::HomeEvent, &Connection> for Home {
    fn message(&mut self, msg: server::HomeEvent, _: &Connection) -> Result<(), MessageError>{
        use server::HomeEvent::*;
        match msg {
                Chat(line) => {
                    self.app.chat.messages.push(line);
                }
        }
        Ok(())
    }
}
impl MessageReceiver<server::SelectRoleEvent, &Connection> for SelectRole {
    fn message(&mut self, msg: server::SelectRoleEvent, _: &Connection) -> Result<(), MessageError>{
        use server::SelectRoleEvent::*;
        match msg {
                Chat(line) => {
                    self.app.chat.messages.push(line);
                }
        }
        Ok(())
    }
}
impl MessageReceiver<server::GameEvent, &Connection> for Game {
    fn message(&mut self, msg: server::GameEvent, _: &Connection) -> Result<(), MessageError>{
         use server::GameEvent::*;
        match msg {
                Chat(line) => {
                    self.app.chat.messages.push(line);
                }
        }
        Ok(())
    }
}


use crate::input::Inputable;



pub async fn connect(username: String, host: SocketAddr,) -> anyhow::Result<()> {
    let stream = TcpStream::connect(host)
        .await.with_context(|| format!("Failed to connect to address {}", host))?;
    run(username, stream).await.context("failed to process messages from the server")
}
async fn run(username: String, mut stream: TcpStream
                                   ) -> anyhow::Result<()>{
    let (r, w) = stream.split();
    let mut socket_writer = FramedWrite::new(w, LinesCodec::new());
    let mut socket_reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));
    let mut input_reader  = crossterm::event::EventStream::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let connection = Connection::new(tx, username);
    let mut current_game_context = ClientGameContext::new();
    let terminal =   Arc::new(Mutex::new(
                                    TerminalHandle::new()
                                    .context("Failed to create a terminal for game")?));
    TerminalHandle::chain_panic_for_restore(Arc::downgrade(&terminal));
    loop {
        tokio::select! {
            input = input_reader.next() => {
                match  input {
                    None => break,
                    Some(Err(e)) => {
                        warn!("IO error on stdin: {}", e);
                    }, 
                    Some(Ok(event)) => { 
                        // should quit?
                        if let Event::Key(key) = &event {
                            if KeyCode::Char('c') == key.code && key.modifiers.contains(KeyModifiers::CONTROL) {
                                info!("Closing the client user interface");
                                socket_writer.send(encode_message(
                                    client::Msg::App(client::AppEvent::Logout))).await?;
                                break
                            }
                            }
                        current_game_context.handle_input(&event, &connection)
                           .context("failed to process an input event in the current game stage")?;
                        ui::draw_context(&terminal, &mut current_game_context);
                        
                    }
                }
            }
            Some(msg) = rx.recv() => {
                socket_writer.send(&msg).await.context("failed to send a message to the socket")?;
            }

            r = socket_reader.next::<server::Msg>() => match r { 
                Ok(msg) => {
                    use server::{Msg, AppEvent};
                    match msg {
                        Msg::App(e) => {
                            match e {
                                AppEvent::Logout =>  {
                                    info!("Logout");
                                    break  
                                },
                                AppEvent::NextContext(n) => {
                                    current_game_context.to(n, &connection);
                                },
                            }
                        },
                        _ => {
                            current_game_context.message(msg, &connection)
                                .or_else(|e| 
                                     match e {
                                        MessageError::LoginRejected{reason} =>{ 
                                            std::mem::drop(terminal.lock().unwrap());
                                            println!("{}", reason);
                                            Ok(())
                                        },
                                        _ => Err(e),

                                     }
                                )
                                .with_context(|| format!("current context {:?}"
                                              , GameContextId::from(&current_game_context) ))?;
                        }
                    }
                    ui::draw_context(&terminal, &mut current_game_context);
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


