
use crossterm::event::{ Event, KeyEventKind, KeyCode};
use crate::protocol::{client::{ClientGameContext, Intro, Home, Game, SelectRole}, server, client, encode_message};
use crate::client::Chat;
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

use crate::details::dispatch_trait;
use crate::protocol::GameContext;

impl Inputable for ClientGameContext {
     type State<'a> = &'a client::Connection;
dispatch_trait!{
        Inputable fn handle_input(&mut self, event: &Event, state: Self::State<'_>,) -> anyhow::Result<()>  {
            GameContext => 
                        Intro 
                        Home 
                        SelectRole 
                        Game
        }
}
}


pub trait Inputable {
    type State<'a>;
    fn handle_input(&mut self, event: &Event, state: Self::State<'_>) -> anyhow::Result<()>;
}


impl Inputable for Intro {
    type State<'a> = &'a client::Connection;
    fn handle_input(&mut self, event: &Event, state: Self::State<'_>) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Enter => {
                    state.tx.send(encode_message(client::Msg::App(client::AppEvent::NextContext)))?;
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
    type State<'a> = &'a client::Connection;
    fn handle_input(&mut self, event: &Event, state: & client::Connection) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    match HOME_KEYS.get(&key.code) {
                        Some(HomeAction::NextContext) => {
                            state.tx.send(encode_message(client::Msg::App(
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
                    self.app.chat.handle_input(event,  StateForChat{ctx: GameContextId::Home(()), cn: state})?; 
                        

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
    SelectPrev,
    ConfirmRole,

}
// TODO array [] not HashMap
lazy_static! {
static ref SELECT_ROLE_KEYS : HashMap<KeyCode, SelectRoleAction> = HashMap::from([
        ( KeyCode::Enter,     SelectRoleAction::NextContext),
        ( KeyCode::Char('e'), SelectRoleAction::EnterChat),
        ( KeyCode::Down, SelectRoleAction::SelectNext),
        ( KeyCode::Up, SelectRoleAction::SelectPrev),
        ( KeyCode::Char(' '), SelectRoleAction::ConfirmRole)
    ]);
}
impl Inputable for SelectRole {
    type State<'a> =  &'a client::Connection;
    fn handle_input(&mut self,  event: &Event, state: & client::Connection) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    match SELECT_ROLE_KEYS.get(&key.code) {
                        Some(SelectRoleAction::NextContext)  => { 
                            if self.selected.is_some() {
                                state.tx.send(encode_message(client::Msg::App(
                                        client::AppEvent::NextContext
                                        )))?;
                            }
                        },
                        Some(SelectRoleAction::EnterChat) => { self.app.chat.input_mode = InputMode::Editing; },
                        Some(SelectRoleAction::SelectNext)=>    self.roles.next(),
                        Some(SelectRoleAction::SelectPrev) =>   self.roles.previous(),
                        Some(SelectRoleAction::ConfirmRole) =>   {
                            if self.roles.state.selected().is_some() {
                                self.selected = Some(self.roles.items[self.roles.state.selected().unwrap()]);
                                state.tx.send(encode_message(client::Msg::SelectRole(
                                        client::SelectRoleEvent::Select(self.selected.unwrap())
                                        )))?;
                            }
                        }
                        ,
                        _ => ()
                    }
                },
                InputMode::Editing => {  
                    self.app.chat.handle_input(event, StateForChat{ctx: GameContextId::SelectRole(()), cn: state})?; 
                }
            }
        }
        Ok(())
    }
}

impl Inputable for Game {
    type State<'a> = &'a client::Connection;
    fn handle_input(&mut self,  event: &Event, state: &client::Connection) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    match key.code {
                        KeyCode::Enter => {  
                            state.tx.send(encode_message(client::Msg::App(
                                        client::AppEvent::NextContext
                                        )))?;
                        },
                        KeyCode::Char('e') => { self.app.chat.input_mode = InputMode::Editing; },
                        _ => ()
                    }
                },
                InputMode::Editing => {  
                    self.app.chat.handle_input(event, StateForChat{ctx: GameContextId::Game(()), cn: state})?; 
                }
            }
        }
        Ok(())
    }
}
pub struct StateForChat<'a> {
    cn: &'a client::Connection,
    ctx: GameContextId
}

use crate::protocol::GameContextId;
impl Inputable for Chat {
    type State<'a> = StateForChat<'a>;
    fn handle_input(&mut self, event: &Event, state: StateForChat) -> anyhow::Result<()> {
        assert_eq!(self.input_mode, InputMode::Editing);
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Enter => {
                    let input = std::mem::take(&mut self.input);
                    let msg = String::from(input.value());
                    use client::{Msg, HomeEvent, GameEvent, SelectRoleEvent};
                    let msg = match state.ctx {
                        GameContextId::Home(_) => {
                            Msg::Home(HomeEvent::Chat(msg))
                        },
                        GameContextId::Game(_) => {
                            Msg::Game(GameEvent::Chat(msg))
                        },
                        GameContextId::SelectRole(_) => {
                            Msg::SelectRole(SelectRoleEvent::Chat(msg))
                        },
                        _ => unreachable!()

                    };
                    state.cn.tx.send(encode_message(msg))?;
                    self.messages.push(server::ChatLine::Text(format!("(me): {}", input.value())));
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
