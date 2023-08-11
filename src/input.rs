
use crossterm::event::{ Event, KeyEventKind, KeyCode};
use crate::protocol::{client::{Connection, ClientGameContext, Intro, Home, Game, SelectRole, GamePhase}, server, client, encode_message};
use crate::client::Chat;
use tracing::{debug, info, warn, error};
use tui_input::Input;
use tui_input::backend::crossterm::EventHandler;
use lazy_static::lazy_static;
use std::collections::HashMap;
use crate::ui::details::Statefulness;

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
                    state.tx.send(encode_message(client::Msg::App(client::AppMsg::NextContext)))?;
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
    type State<'a> = &'a Connection;
    fn handle_input(&mut self, event: &Event, state: & client::Connection) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    match HOME_KEYS.get(&key.code) {
                        Some(HomeAction::NextContext) => {
                            state.tx.send(encode_message(client::Msg::App(
                                        client::AppMsg::NextContext
                                        )))?;
                        },
                        Some(HomeAction::EnterChat) => {
                            self.app.chat.input_mode = InputMode::Editing;
                        },
                        _ => ()
                    }
                },
                InputMode::Editing => { 
                    self.app.chat.handle_input(event, (GameContextId::from(&*self), state))?; 
                        
                        
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
        ( KeyCode::Right, SelectRoleAction::SelectNext),
        ( KeyCode::Left, SelectRoleAction::SelectPrev),
        ( KeyCode::Char(' '), SelectRoleAction::ConfirmRole)
    ]);
}
impl Inputable for SelectRole {
    type State<'a> =  &'a Connection;
    fn handle_input(&mut self,  event: &Event, state: & client::Connection) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    match SELECT_ROLE_KEYS.get(&key.code) {
                        Some(SelectRoleAction::NextContext)  => { 
                            if self.roles.selected.is_some() {
                                state.tx.send(encode_message(client::Msg::App(
                                        client::AppMsg::NextContext
                                        )))?;
                            }
                        },
                        Some(SelectRoleAction::EnterChat) => { self.app.chat.input_mode = InputMode::Editing; },
                        Some(SelectRoleAction::SelectNext)=>    self.roles.next(),
                        Some(SelectRoleAction::SelectPrev) =>   self.roles.prev(),
                        Some(SelectRoleAction::ConfirmRole) =>   {
                            if self.roles.selected().is_some() {
                                state.tx.send(encode_message(client::Msg::SelectRole(
                                        client::SelectRoleMsg::Select(self.roles.selected().unwrap())
                                        )))?;
                            }
                        }
                        ,
                        _ => ()
                    }
                },
                InputMode::Editing => {  
                    self.app.chat.handle_input(event, (GameContextId::from(&*self), state))?; 
                }
            }
        }
        Ok(())
    }
}



macro_rules! event {
    ($self:ident.$msg:literal $(,$args:expr)*) => {
        $self.app.chat.messages.push(server::ChatLine::GameEvent(format!($msg, $($args,)*)))
    }
}

impl Inputable for Game {
    type State<'a> = &'a Connection;
    fn handle_input(&mut self,  event: &Event, state: &client::Connection) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    match key.code {
                        KeyCode::Char(' ') => { 
                            match self.phase {
                                GamePhase::Discard => {
                                    // TODO ability description
                                    event!(self."You discard {:?}", self.abilities.active());
                                    self.abilities.items[self.abilities.active.unwrap()] = None;
                                    self.phase = GamePhase::SelectAbility;

                                }
                                GamePhase::SelectAbility => {
                                    self.abilities.selected = self.abilities.active;
                                    // TODO ability description
                                    event!(self."You select {:?}", self.abilities.active().unwrap());
                                    self.phase = GamePhase::AttachMonster;
                                    event!(self."You can attach a monster")
                                }
                                GamePhase::AttachMonster => {
                                    self.monsters.selected = Some(self.monsters.active.expect("Must be Some of collection is not empty"));
                                    event!(self."You attack {:?}", self.monsters.active().unwrap());
                                    event!(self."Now selected monster {:?}, active {:?}", self.monsters.selected(), self.monsters.active().unwrap());
                                    //self.phase = GamePhase::Defend;
                                    self.abilities.selected = None;
                                }
                                GamePhase::Defend => {
                                    event!(self."You get damage");
                                    self.monsters.selected  = None;

                                }
                                _ => (),
                            }
                        },
                        KeyCode::Left => {
                            match self.phase {
                                GamePhase::SelectAbility | GamePhase::Discard => self.abilities.prev(),
                                GamePhase::AttachMonster => self.monsters.prev(),
                                _ => (),
                            };
                            
                        }
                        KeyCode::Right => {
                            match self.phase {
                                GamePhase::SelectAbility | GamePhase::Discard => self.abilities.next(),
                                GamePhase::AttachMonster => self.monsters.next(),
                                _ => (),
                            };
                        }
                        KeyCode::Char('e') => { self.app.chat.input_mode = InputMode::Editing; },
                        _ => ()
                    }
                },
                InputMode::Editing => {  
                    self.app.chat.handle_input(event, (GameContextId::from(&*self), state))?; 
                }
            }
        }
        Ok(())
    }
}

use crate::protocol::GameContextId;
impl Inputable for Chat {
    type State<'a> =  (GameContextId, &'a Connection);
    fn handle_input(&mut self, event: &Event, state: (GameContextId, &Connection)) -> anyhow::Result<()> {
        assert_eq!(self.input_mode, InputMode::Editing);
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Enter => {
                    let input = std::mem::take(&mut self.input);
                    let msg = String::from(input.value());
                    use client::{Msg, HomeMsg, GameMsg, SelectRoleMsg};
                    use GameContextId as Id;
                    // we can send chat on the server only in specific contexts
                    let msg = match state.0 {
                        Id::Home(_) => Msg::Home(HomeMsg::Chat(msg)),
                        Id::Game(_) => Msg::Game(GameMsg::Chat(msg)),
                        Id::SelectRole(_) => Msg::SelectRole(SelectRoleMsg::Chat(msg)) ,
                        _ => unreachable!("context {:?} not allows chat messages", state.0)
                    };
                    let _ = state.1.tx.send(encode_message(msg));
                    self.messages.push(server::ChatLine::Text(format!("(me): {}", input.value())));
                }, 
                KeyCode::Esc => {
                            self.input_mode = crate::input::InputMode::Normal;
                },
                KeyCode::Down => {
                    self.scroll = self.scroll.saturating_add(1);
                        self.scroll_state = self
                            .scroll_state
                            .position(self.scroll as u16);
                },
                KeyCode::Up => {
                     self.scroll = self.scroll.saturating_sub(1);
                        self.scroll_state = self
                            .scroll_state
                            .position(self.scroll as u16);
                },
                _ => {
                    self.input.handle_event(&Event::Key(*key));
                }

            }   
        }
        Ok(())
    }
}
