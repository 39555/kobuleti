
use crossterm::event::{ Event, KeyEventKind, KeyCode};
use crate::protocol::{client::{ClientGameContext, Intro, Home, Game, SelectRole}, server, client, encode_message};
use crate::client::Chat;
use enum_dispatch::enum_dispatch;
use tracing::{debug, info, warn, error};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;
use lazy_static::lazy_static;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Editing,
}



 macro_rules! make_key_list {
        ($name: ident {$($field:ident = $value:expr)*}) => {
            pub mod $name {
                use super::*;
                $(pub const $field : KeyCode = $value);*;
            }
        }
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
                    self.tx.send(encode_message(client::Msg::App(client::AppEvent::NextContext)))?;
                } _ => ()
            }
        }
        Ok(())
    }
}

enum HomeAction{
    NextContext,
    EnterChat,
}
lazy_static! {
static ref HOME_KEYS : HashMap<KeyCode, HomeAction> = HashMap::from([
        ( KeyCode::Enter,     HomeAction::NextContext),
        ( KeyCode::Char('e'), HomeAction::EnterChat)
    ]);
}
impl Inputable for Home {
    fn handle_input(&mut self, event: &Event) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    match HOME_KEYS.get(&key.code) {
                        Some(HomeAction::NextContext) => {
                            self.app.tx.send(encode_message(client::Msg::App(
                                        client::AppEvent::NextContext
                                        )))?;
                        },
                        Some(HomeAction::EnterChat) => {
                            self.app.chat.input_mode = InputMode::Editing;
                        },
                        _ => ()
                    }
                },
                InputMode::Editing => { 
                    match key.code {
                        // TODO move to chat?
                        KeyCode::Enter => {
                            self.app.tx.send(encode_message(client::Msg::Home(
                                    client::HomeEvent::Chat(String::from(self.app.chat.input.value())))))?;
                        },
                        _ => ()
                    }
                    self.app.chat.handle_input(event)?; 
                        

                }
            }
            
        }
        
        Ok(())
    }
}
enum SelectRoleAction{
    NextContext,
    EnterChat,
    SelectNext,
    SelectPrev

}
// TODO array [] not HashMap
lazy_static! {
static ref SELECT_ROLE_KEYS : HashMap<KeyCode, SelectRoleAction> = HashMap::from([
        ( KeyCode::Enter,     SelectRoleAction::NextContext),
        ( KeyCode::Char('e'), SelectRoleAction::EnterChat),
        ( KeyCode::Down, SelectRoleAction::SelectNext),
        ( KeyCode::Up, SelectRoleAction::SelectPrev)
    ]);
}
impl Inputable for SelectRole {
    fn handle_input(&mut self,  event: &Event) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    match SELECT_ROLE_KEYS.get(&key.code) {
                        Some(SelectRoleAction::NextContext)  => {  
                            self.app.tx.send(encode_message(client::Msg::App(
                                        client::AppEvent::NextContext
                                        )))?;
                        },
                        Some(SelectRoleAction::EnterChat) => { self.app.chat.input_mode = InputMode::Editing; },
                        Some(SelectRoleAction::SelectNext)=>    self.roles.next(),
                        Some(SelectRoleAction::SelectPrev) =>   self.roles.previous(),
                        _ => ()
                    }
                },
                InputMode::Editing => { 
                    self.app.chat.handle_input(event)?; 
                }
            }
        }
        Ok(())
    }
}

impl Inputable for Game {
    fn handle_input(&mut self,  event: &Event) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    match key.code {
                        KeyCode::Enter => {  
                            self.app.tx.send(encode_message(client::Msg::App(
                                        client::AppEvent::NextContext
                                        )))?;
                        },
                        KeyCode::Char('e') => { self.app.chat.input_mode = InputMode::Editing; },
                        _ => ()
                    }
                },
                InputMode::Editing => { 
                    self.app.chat.handle_input(event)?; 
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
