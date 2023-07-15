
use crossterm::event::{ Event, KeyEventKind, KeyCode};
use crate::protocol::{client::{ClientGameContext, Intro, Home, Game}, server, client, encode_message};
use crate::client::Chat;
use enum_dispatch::enum_dispatch;
use tracing::{debug, info, warn, error};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Editing,
}

#[enum_dispatch(ClientGameContext)]
pub trait Inputable {
    fn handle_input(&mut self, event: &Event) -> anyhow::Result<()>;
}

impl Inputable for Intro {
    fn handle_input(&mut self, event: &Event) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Enter => {
                    println!("press enter");
                    self.tx.send(encode_message(client::Msg::App(client::AppEvent::NextContext)))?;
                } _ => ()
            }
        }
        Ok(())
    }
}

impl Inputable for Home {
    fn handle_input(&mut self, event: &Event) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            
            match self.chat.input_mode {
                InputMode::Normal => {
                    match key.code {
                        KeyCode::Enter => {
                            self.app.tx.send(encode_message(client::Msg::App(
                                        client::AppEvent::NextContext
                                        )))?;
                            //if state.can_play {
                                // TODO separate next chain from stages
                             //   state.tx.send(encode_message(ClientMessage::StartGame))?;
                              //  return Ok(StageEvent::Next)
                           // } else {
                                // TODO message about wait a second player
                           // }
                        },
                        KeyCode::Char('e') => {
                            self.chat.input_mode = InputMode::Editing;
                        },
                        _ => ()
                    }
                },
                InputMode::Editing => { 
                    match key.code {
                        // TODO move to chat?
                        KeyCode::Enter => {
                            self.app.tx.send(encode_message(client::Msg::Home(
                                    client::HomeEvent::Chat(String::from(self.chat.input.value())))))?;
                        },
                        _ => ()
                    }
                    self.chat.handle_input(event)?; 
                        

                }
            }
            
        }
        
        Ok(())
    }
}
impl Inputable for Game {
    fn handle_input(&mut self,  event: &Event) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match self.chat.input_mode {
                InputMode::Normal => {
                    match key.code {
                        KeyCode::Enter => { 
                        },
                        KeyCode::Char('e') => { self.chat.input_mode = InputMode::Editing; },
                        _ => ()
                    }
                },
                InputMode::Editing => { 
                    self.chat.handle_input(event)?; 
                }
            }
        }
        Ok(())
    }
}

impl Inputable for Chat {
    fn handle_input(&mut self, event: &Event) -> anyhow::Result<()> {
        assert_eq!(self.input_mode, InputMode::Editing);
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Enter => {
                    self.messages.push(server::ChatLine::Text(format!("(me): {}", std::mem::take(&mut self.input))));
                } 
               KeyCode::Esc => {
                            self.input_mode = crate::input::InputMode::Normal;
                        },
                _ => {
                    self.input.handle_event(&Event::Key(*key));
                }
            }   
        }
        Ok(())
    }
}
