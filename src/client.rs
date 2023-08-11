
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
use crate::input::Inputable;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
type Tx = tokio::sync::mpsc::UnboundedSender<String>;

use crossterm::event::{ Event,  KeyCode, KeyModifiers};
use crate::input::InputMode;
use tui_input::Input;
use ratatui::widgets::ScrollbarState;

#[derive(Default, Debug)]
pub struct Chat {
        pub input_mode: InputMode,
         /// Current value of the input box
        pub input: Input,
        /// History of recorded messages
        pub messages: Vec<server::ChatLine>,
        pub scroll: usize,
        pub scroll_state: ScrollbarState
    }



impl MessageReceiver<server::IntroMsg, &client::Connection> for Intro {
    fn message(&mut self, msg: server::IntroMsg, state: &client::Connection) -> anyhow::Result<()> {
        use server::{IntroMsg::*, LoginStatus::*};
        match msg {
            LoginStatus(status) => {
                self.status = Some(status);
                match status { 
                    Logged => {
                        info!("Successfull login to the game");
                        state.tx.send(
                         encode_message(client::Msg::Intro(client::IntroMsg::GetChatLog)))
                        .expect("failed to request a chat log");
                        Ok(()) 
                    },
                    Reconnected => {
                        info!("Reconnected!");
                        Ok(())

                    },
                    InvalidPlayerName => {
                        Err(anyhow!(
                            "Invalid player name: '{}'", 
                            state.username))
                    },
                    PlayerLimit => {
                        Err(anyhow!(
        "Player '{}' has tried to login but the player limit has been reached"
                            , state.username))
                    },
                    AlreadyLogged => {
                        Err(anyhow!(
                            "User with name '{}' already logged", 
                            state.username))
                    },
                }
            }
           
            
        }.context("Failed to join to the game")
    }
}
impl MessageReceiver<server::HomeMsg, &Connection> for Home {
    fn message(&mut self, msg: server::HomeMsg, _: &Connection) -> anyhow::Result<()> {
        
        Ok(())
    }
}
impl MessageReceiver<server::SelectRoleMsg, &Connection> for SelectRole {
    fn message(&mut self, msg: server::SelectRoleMsg, _: &Connection) -> anyhow::Result<()> {
        use server::SelectRoleMsg::*;
        match msg {
                
                SelectedStatus(status) => {
                    if let server::SelectRoleStatus::Ok(role) = status {
                        self.roles.selected = Some(self.roles.items.iter().position(|r| *r == role)
                                                   .expect("Must be present"))
                    }
                }
        }
        Ok(())
    }
}
impl MessageReceiver<server::GameMsg, &Connection> for Game {
    fn message(&mut self, msg: server::GameMsg, _: &Connection) -> anyhow::Result<()> {
         
        Ok(())
    }
}





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
    let connection = Connection::new(tx, username).login();
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
                            if KeyCode::Char('d') == key.code && key.modifiers.contains(KeyModifiers::CONTROL) {
                                info!("Closing the client user interface");
                                socket_writer.send(encode_message(
                                    client::Msg::App(client::AppMsg::Logout))).await?;
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
                socket_writer.send(&msg).await
                    .context("failed to send a message to the socket")?;
            }

            r = socket_reader.next::<server::Msg>() => match r { 
                Some(Ok(msg)) => {
                    use server::{Msg, AppMsg};
                    match msg {
                        Msg::App(e) => {
                            match e {
                                AppMsg::Pong => {

                                }
                                AppMsg::Logout =>  {
                                    std::mem::drop(terminal);
                                    info!("Logout");
                                    break  
                                },
                                AppMsg::Chat(line) => {
                                     match &mut current_game_context{
                                        ClientGameContext::Home(h) => {
                                         h.app.chat.messages.push(line);
                                        },
                                        ClientGameContext::SelectRole(r) => {
                                         r.app.chat.messages.push(line);
                                        },
                                        ClientGameContext::Game(g) => {
                                         g.app.chat.messages.push(line);
                                        },
                                        _ => (),
                                    }
                                },
                                AppMsg::NextContext(n) => {
                                    let _ = current_game_context.to(n, &connection);
                                },
                                AppMsg::ChatLog(log) => {
                                    match &mut current_game_context{
                                        ClientGameContext::Intro(i) => {
                                            i.chat_log = Some(log);
                                        }, 
                                        ClientGameContext::Home(h) => {
                                         h.app.chat.messages = log;
                                        },
                                        ClientGameContext::SelectRole(r) => {
                                         r.app.chat.messages = log;
                                        },
                                        ClientGameContext::Game(g) => {
                                         g.app.chat.messages = log;

                                        },
                                    }
                                }
                            }
                        },
                        _ => {
                            if let Err(e) = current_game_context.message(msg, &connection).map_err(|e| anyhow!("{:?}", e))
                                .with_context(|| format!("current context {:?}"
                                              , GameContextId::from(&current_game_context) )){
                                     std::mem::drop(terminal);
                                     warn!("Error: {}", e);
                                     break
                                };
                        }
                    }
                    ui::draw_context(&terminal, &mut current_game_context);
                }
                ,
                Some(Err(e)) => { 
                    error!("{}", e);
                }
                None => {
                    break;
                }
            }
        }
    }
    stream.shutdown().await?;
    Ok(())
}


