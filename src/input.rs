use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tui_input::backend::crossterm::EventHandler;

use crate::{
    client::Chat,
    protocol::{
        client,
        client::{
            ClientGameContext, Connection, Game, GameMsg, Home, Intro, Msg, RoleStatus, SelectRole,
            SelectRoleMsg,
        },
        server, GamePhaseKind,
    },
    ui::details::Statefulness,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Editing,
}

use crate::{
    details::dispatch_trait,
    protocol::{GameContext, TurnStatus},
};

impl Inputable for ClientGameContext {
    type State<'a> = &'a client::Connection;
    dispatch_trait! {
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

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum MainCmd {
    None,
    Quit,
    NextContext,
}

macro_rules! key {
    ($code:expr) => {
        KeyEvent::new($code, KeyModifiers::NONE)
    };
    ($code:expr, $mods:expr) => {
        KeyEvent::new($code, $mods)
    };
}

pub const MAIN_KEYS: &[(KeyEvent, MainCmd)] = {
    use MainCmd as Cmd;
    &[
        (key!(KeyCode::Enter), Cmd::NextContext),
        (key!(KeyCode::Char('q'), KeyModifiers::CONTROL), Cmd::Quit),
    ]
};

fn handle_main_input(event: &Event, state: &client::Connection) -> anyhow::Result<()> {
    if let Event::Key(key) = event {
        if let Some(a) = MAIN_KEYS.get_action(key) {
            match a {
                MainCmd::NextContext => {
                    state
                        .tx
                        .send(client::Msg::App(client::AppMsg::NextContext))?;
                }
                MainCmd::Quit => {
                    state.cancel.cancel();
                }
                _ => (),
            }
        };
    }
    Ok(())
}

impl Inputable for Intro {
    type State<'a> = &'a client::Connection;
    fn handle_input(&mut self, event: &Event, state: Self::State<'_>) -> anyhow::Result<()> {
        handle_main_input(event, state)
    }
}

#[derive(Copy, Clone)]
pub enum HomeCmd {
    None,
    EnterChat,
}

pub const HOME_KEYS: &[(KeyEvent, HomeCmd)] = {
    use HomeCmd as Cmd;
    &[(key!(KeyCode::Char('e')), Cmd::EnterChat)]
};

trait ActionGetter {
    type Action;
    fn get_action(&self, key: &KeyEvent) -> Option<Self::Action>;
}

impl<A: Copy + Clone> ActionGetter for &[(KeyEvent, A)] {
    type Action = A;
    fn get_action(&self, key: &KeyEvent) -> Option<Self::Action> {
        self.iter().find(|k| k.0 == *key).map(|k| k.1)
    }
}

use crate::client::game_event;

impl Inputable for Home {
    type State<'a> = &'a Connection;
    fn handle_input(&mut self, event: &Event, state: &client::Connection) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    use HomeCmd as Cmd;
                    match HOME_KEYS.get_action(key).unwrap_or(HomeCmd::None) {
                        Cmd::None => {
                            handle_main_input(event, state)?;
                        }
                        Cmd::EnterChat => {
                            self.app.chat.input_mode = InputMode::Editing;
                        }
                    }
                }
                InputMode::Editing => {
                    self.app
                        .chat
                        .handle_input(event, (GameContextKind::from(&*self), state))?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Copy, Clone)]
pub enum SelectRoleCmd {
    None,
    EnterChat,
    SelectPrev,
    SelectNext,
    ConfirmRole,
}

pub const SELECT_ROLE_KEYS: &[(KeyEvent, SelectRoleCmd)] = {
    use SelectRoleCmd as Cmd;
    &[
        (key!(KeyCode::Char('e')), Cmd::EnterChat),
        (key!(KeyCode::Left), Cmd::SelectPrev),
        (key!(KeyCode::Right), Cmd::SelectNext),
        (key!(KeyCode::Char(' ')), Cmd::ConfirmRole),
    ]
};

impl Inputable for SelectRole {
    type State<'a> = &'a Connection;
    fn handle_input(&mut self, event: &Event, state: &client::Connection) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    use SelectRoleCmd as Cmd;
                    match SELECT_ROLE_KEYS
                        .get_action(key)
                        .unwrap_or(SelectRoleCmd::None)
                    {
                        Cmd::None => {
                            handle_main_input(event, state)?;
                        }
                        Cmd::EnterChat => {
                            self.app.chat.input_mode = InputMode::Editing;
                        }
                        Cmd::SelectNext => self.roles.next(),
                        Cmd::SelectPrev => self.roles.prev(),
                        Cmd::ConfirmRole => {
                            if self
                                .roles
                                .active()
                                .is_some_and(|r| matches!(r, RoleStatus::Available(_)))
                            {
                                state.tx.send(Msg::from(SelectRoleMsg::Select(
                                    self.roles.active().unwrap().role(),
                                )))?;
                            } else if self.roles.active != self.roles.selected {
                                game_event!(self."This role is not available");
                            }
                        }
                    }
                }
                InputMode::Editing => {
                    self.app
                        .chat
                        .handle_input(event, (GameContextKind::from(&*self), state))?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
pub enum GameCmd {
    None,
    EnterChat,
    SelectPrev,
    SelectNext,
    ConfirmSelected,
}

pub const GAME_KEYS: &[(KeyEvent, GameCmd)] = {
    use GameCmd as Cmd;
    &[
        (key!(KeyCode::Char('e')), Cmd::EnterChat),
        (key!(KeyCode::Left), Cmd::SelectPrev),
        (key!(KeyCode::Right), Cmd::SelectNext),
        (key!(KeyCode::Char(' ')), Cmd::ConfirmSelected),
    ]
};

impl Inputable for Game {
    type State<'a> = &'a Connection;
    fn handle_input(&mut self, event: &Event, state: &client::Connection) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            match self.app.chat.input_mode {
                InputMode::Normal => {
                    use GameCmd as Cmd;
                    match GAME_KEYS.get_action(key).unwrap_or(GameCmd::None) {
                        Cmd::None => {
                            handle_main_input(event, state)?;
                        }
                        Cmd::ConfirmSelected => {
                            if let TurnStatus::Ready(phase) = self.phase {
                                match phase {
                                    GamePhaseKind::DropAbility => {
                                        state.tx.send(Msg::from(GameMsg::DropAbility(
                                            *self.abilities.active().expect("Must be Some"),
                                        )))?;
                                    }
                                    GamePhaseKind::SelectAbility => {
                                        state.tx.send(Msg::from(GameMsg::SelectAbility(
                                            *self.abilities.active().expect("Must be Some"),
                                        )))?;
                                    }
                                    GamePhaseKind::AttachMonster => {
                                        state.tx.send(Msg::from(GameMsg::Attack(
                                            *self.monsters.active().expect("Must be Some"),
                                        )))?;
                                    }
                                    GamePhaseKind::Defend => {
                                        self.monsters.selected = None;
                                        state.tx.send(Msg::from(GameMsg::Continue))?;
                                    }
                                    _ => (),
                                }
                            }
                        }
                        Cmd::SelectPrev => {
                            if let TurnStatus::Ready(phase) = self.phase {
                                match phase {
                                    GamePhaseKind::SelectAbility | GamePhaseKind::DropAbility => {
                                        self.abilities.prev()
                                    }
                                    GamePhaseKind::AttachMonster => self.monsters.prev(),
                                    _ => (),
                                };
                            }
                        }
                        Cmd::SelectNext => {
                            if let TurnStatus::Ready(phase) = self.phase {
                                match phase {
                                    GamePhaseKind::SelectAbility | GamePhaseKind::DropAbility => {
                                        self.abilities.next()
                                    }
                                    GamePhaseKind::AttachMonster => self.monsters.next(),
                                    _ => (),
                                };
                            }
                        }
                        Cmd::EnterChat => {
                            self.app.chat.input_mode = InputMode::Editing;
                        }
                    }
                }
                InputMode::Editing => {
                    self.app
                        .chat
                        .handle_input(event, (GameContextKind::from(&*self), state))?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
pub enum ChatCmd {
    None,
    SendInput,
    LeaveChatInput,
    ScrollUp,
    ScrollDown,
}
pub const CHAT_KEYS: &[(KeyEvent, ChatCmd)] = &[
    (key!(KeyCode::Enter), ChatCmd::SendInput),
    (key!(KeyCode::Esc), ChatCmd::LeaveChatInput),
    (key!(KeyCode::Up), ChatCmd::ScrollUp),
    (key!(KeyCode::Down), ChatCmd::ScrollDown),
];

use crate::protocol::GameContextKind;
impl Inputable for Chat {
    type State<'a> = (GameContextKind, &'a Connection);
    fn handle_input(
        &mut self,
        event: &Event,
        state: (GameContextKind, &Connection),
    ) -> anyhow::Result<()> {
        assert_eq!(self.input_mode, InputMode::Editing);
        if let Event::Key(key) = event {
            use ChatCmd as Cmd;
            match CHAT_KEYS.get_action(key).unwrap_or(ChatCmd::None) {
                Cmd::None => {
                    self.input.handle_event(&Event::Key(*key));
                }
                Cmd::SendInput => {
                    let input = std::mem::take(&mut self.input);
                    let msg = String::from(input.value());
                    use client::{GameMsg, HomeMsg, Msg, SelectRoleMsg};
                    use GameContextKind as Id;
                    // we can send chat on the server only in specific contexts
                    let msg = match state.0 {
                        Id::Home(_) => Msg::Home(HomeMsg::Chat(msg)),
                        Id::Game(_) => Msg::Game(GameMsg::Chat(msg)),
                        Id::SelectRole(_) => Msg::SelectRole(SelectRoleMsg::Chat(msg)),
                        _ => unreachable!("context {:?} not allows chat messages", state.0),
                    };
                    let _ = state.1.tx.send(msg);
                    self.messages
                        .push(server::ChatLine::Text(format!("(me): {}", input.value())));
                }
                Cmd::LeaveChatInput => {
                    self.input_mode = crate::input::InputMode::Normal;
                }
                Cmd::ScrollDown => {
                    self.scroll = self.scroll.saturating_add(1);
                    self.scroll_state = self.scroll_state.position(self.scroll as u16);
                }
                Cmd::ScrollUp => {
                    self.scroll = self.scroll.saturating_sub(1);
                    self.scroll_state = self.scroll_state.position(self.scroll as u16);
                }
            }
        }
        Ok(())
    }
}
