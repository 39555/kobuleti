
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

#[derive(Copy, Clone)]
pub enum MainCmd {
    NextContext
}

pub const MAIN_KEYS : &[(KeyCode, MainCmd)] = {
    use MainCmd as Cmd;
    &[
    ( KeyCode::Enter,     Cmd::NextContext),
    ]
};

fn handle_main_input(event: &Event, state: & client::Connection) -> anyhow::Result<()>{
    if let Event::Key(key) = event {
            if let Some(a) = MAIN_KEYS.get_action(key.code) {
                match a {
                    MainCmd::NextContext => {
                        state.tx.send(encode_message(client::Msg::App(
                            client::AppMsg::NextContext
                            )))?;
                    }
                }};
        }
    Ok(())

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

#[derive(Copy, Clone)]
pub enum HomeCmd{
    None, 
    EnterChat,
}
pub const HOME_KEYS : &[(KeyCode, HomeCmd)] = {
    use HomeCmd as Cmd;
    &[
    ( KeyCode::Char('e'), Cmd::EnterChat),
    ]
};

trait ActionGetter{
    type Action;
    fn get_action(&self, key: KeyCode) -> Option<Self::Action>;
}
impl<A : Copy + Clone> ActionGetter for &[(KeyCode, A)]{
    type Action = A;
    fn get_action(&self, key: KeyCode) -> Option<Self::Action> {
        self.iter().find(|k| k.0 == key).map_or(None, |k| Some(k.1))
    }
}


impl Inputable for Home {
    type State<'a> = &'a Connection;
    fn handle_input(&mut self, event: &Event, state: & client::Connection) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    use HomeCmd as Cmd;
                    match HOME_KEYS.get_action(key.code).unwrap_or(HomeCmd::None){
                        Cmd::None => {
                            handle_main_input(event, state)?; 
                        }
                        Cmd::EnterChat => {
                            self.app.chat.input_mode = InputMode::Editing;
                        },
                        _ => (),
                    }
                },
                InputMode::Editing => { 
                    self.app.chat.handle_input(event, (GameContextKind::from(&*self), state))?; 
                        
                        
                }
            }
            
        }
        
        Ok(())
    }
}

#[derive(Copy, Clone)]
pub enum SelectRoleCmd{
    None, 
    EnterChat,
    SelectNext,
    SelectPrev,
    ConfirmRole,

}
pub const SELECT_ROLE_KEYS : &[(KeyCode,  SelectRoleCmd)] = { 
    use SelectRoleCmd as Cmd;
    &[
        ( KeyCode::Char('e'), Cmd::EnterChat),
        ( KeyCode::Right, Cmd::SelectNext),
        ( KeyCode::Left, Cmd::SelectPrev),
        ( KeyCode::Char(' '), Cmd::ConfirmRole)
    ]
};

impl Inputable for SelectRole {
    type State<'a> =  &'a Connection;
    fn handle_input(&mut self,  event: &Event, state: & client::Connection) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    use SelectRoleCmd as Cmd;
                    match SELECT_ROLE_KEYS.get_action(key.code)
                        .unwrap_or(SelectRoleCmd::None) {
                        Cmd::None => {
                            handle_main_input(event, state)?;
                        }
                        Cmd::EnterChat => { self.app.chat.input_mode = InputMode::Editing; },
                        Cmd::SelectNext=>    self.roles.next(),
                        Cmd::SelectPrev =>   self.roles.prev(),
                        Cmd::ConfirmRole =>   {
                            if self.roles.selected().is_some() {
                                state.tx.send(encode_message(client::Msg::SelectRole(
                                        client::SelectRoleMsg::Select(self.roles.selected().unwrap().0)
                                        )))?;
                            }
                        }
                    }
                },
                InputMode::Editing => {  
                    self.app.chat.handle_input(event, (GameContextKind::from(&*self), state))?; 
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

#[derive(Copy, Clone)]
pub enum GameCmd{
    None, 
    EnterChat,
    SelectNext,
    SelectPrev,
    ConfirmSelected,

}
pub const GAME_KEYS : &[(KeyCode,  GameCmd)] = {
    use GameCmd as Cmd;
    &[
        ( KeyCode::Char('e'), Cmd::EnterChat),
        ( KeyCode::Right, Cmd::SelectNext),
        ( KeyCode::Left, Cmd::SelectPrev),
        ( KeyCode::Char(' '), Cmd::ConfirmSelected)
    ]
};



impl Inputable for Game {
    type State<'a> = &'a Connection;
    fn handle_input(&mut self,  event: &Event, state: &client::Connection) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    use GameCmd as Cmd;
                    match GAME_KEYS.get_action(key.code).unwrap_or(GameCmd::None) {
                        Cmd::None => {
                            handle_main_input(event, state)?;
                        }
                        Cmd::ConfirmSelected => { 
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
                        Cmd::SelectPrev => {
                            match self.phase {
                                GamePhase::SelectAbility | GamePhase::Discard => self.abilities.prev(),
                                GamePhase::AttachMonster => self.monsters.prev(),
                                _ => (),
                            };
                        }
                        Cmd::SelectNext => {
                            match self.phase {
                                GamePhase::SelectAbility | GamePhase::Discard => self.abilities.next(),
                                GamePhase::AttachMonster => self.monsters.next(),
                                _ => (),
                            };
                        }
                        Cmd::EnterChat => { self.app.chat.input_mode = InputMode::Editing; },
                    }
                },
                InputMode::Editing => {  
                    self.app.chat.handle_input(event, (GameContextKind::from(&*self), state))?; 
                }
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
pub enum ChatCmd{
    None, 
    SendInput,
    LeaveInput,
    ScrollUp,
    ScrollDown,

}
pub const CHAT_KEYS : &[(KeyCode,  ChatCmd)] = &[
    ( KeyCode::Enter, ChatCmd::SendInput),
    ( KeyCode::Esc  , ChatCmd::LeaveInput),
    ( KeyCode::Up   , ChatCmd::ScrollUp),
    ( KeyCode::Down , ChatCmd::ScrollDown)
];

use crate::protocol::GameContextKind;
impl Inputable for Chat {
    type State<'a> =  (GameContextKind, &'a Connection);
    fn handle_input(&mut self, event: &Event, state: (GameContextKind, &Connection)) -> anyhow::Result<()> {
        assert_eq!(self.input_mode, InputMode::Editing);
        if let Event::Key(key) = event {
            use ChatCmd as Cmd;
            match CHAT_KEYS.get_action(key.code).unwrap_or(ChatCmd::None) {
                Cmd::None => {
                    self.input.handle_event(&Event::Key(*key));
                },
                Cmd::SendInput => {
                    let input = std::mem::take(&mut self.input);
                    let msg = String::from(input.value());
                    use client::{Msg, HomeMsg, GameMsg, SelectRoleMsg};
                    use GameContextKind as Id;
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
                Cmd::LeaveInput => {
                            self.input_mode = crate::input::InputMode::Normal;
                },
                Cmd::ScrollDown => {
                    self.scroll = self.scroll.saturating_add(1);
                        self.scroll_state = self
                            .scroll_state
                            .position(self.scroll as u16);
                },
                Cmd::ScrollUp  => {
                     self.scroll = self.scroll.saturating_sub(1);
                        self.scroll_state = self
                            .scroll_state
                            .position(self.scroll as u16);
                },
            }   
        }
        Ok(())
    }
}
