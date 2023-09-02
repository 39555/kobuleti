use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tui_input::backend::crossterm::EventHandler;

use super::{
    states::{Chat, Connection, Context, Game, Home, Intro, Roles},
    ui::details::Statefulness,
};
use crate::protocol::{client, server, GamePhaseKind, Msg, RoleStatus, With};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Editing,
}

use crate::protocol::TurnStatus;

pub trait Inputable {
    type State<'a>;
    fn handle_input(&mut self, event: &Event, state: Self::State<'_>) -> anyhow::Result<()>;
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum MainCmd {
    None,
    Quit,
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
    &[(key!(KeyCode::Char('q'), KeyModifiers::CONTROL), Cmd::Quit)]
};

fn handle_main_input<S>(event: &Event, state: &mut Connection<S>) -> anyhow::Result<()>
where
    S: crate::protocol::SendSocketMessage + crate::client::states::DataForNextState,
{
    if let Event::Key(key) = event {
        if KeyEventKind::Press == key.kind {
            if let Some(MainCmd::Quit) = MAIN_KEYS.get_action(key) {
                state
                    .cancel
                    .take()
                    .expect("Cancel must be valid while Context is running")
                    .send(None)
                    .map_err(|_| anyhow::anyhow!("Failed to quit"))?;
            }
        }
    }
    Ok(())
}
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum IntroCmd {
    None,
    Enter,
}
pub const INTRO_KEYS: &[(KeyEvent, IntroCmd)] = {
    use IntroCmd as Cmd;
    &[(key!(KeyCode::Enter), Cmd::Enter)]
};
impl Inputable for Context<Intro> {
    type State<'a> = &'a mut Connection<Intro>;
    fn handle_input(&mut self, event: &Event, state: Self::State<'_>) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            if KeyEventKind::Press == key.kind {
                use IntroCmd as Cmd;
                match INTRO_KEYS.get_action(key).unwrap_or(IntroCmd::None) {
                    Cmd::None => {
                        handle_main_input(event, state)?;
                    }
                    Cmd::Enter => {
                        state.tx.send(Msg::with(client::IntroMsg::StartHome))?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone)]
pub enum HomeCmd {
    None,
    EnterChat,
    StartRoles,
}

pub const HOME_KEYS: &[(KeyEvent, HomeCmd)] = {
    use HomeCmd as Cmd;
    &[
        (key!(KeyCode::Enter), Cmd::StartRoles),
        (key!(KeyCode::Char('e')), Cmd::EnterChat),
    ]
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

macro_rules! game_event {
    ($self:ident.$msg:literal $(,$args:expr)*) => {
        $self.chat.messages.push(server::ChatLine::GameEvent(format!($msg, $($args,)*)))
    }
}

impl Inputable for Context<Home> {
    type State<'a> = &'a mut Connection<Home>;
    fn handle_input(&mut self, event: &Event, state: &mut Connection<Home>) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            if KeyEventKind::Press == key.kind {
                match self.chat.input_mode {
                    InputMode::Normal => {
                        use HomeCmd as Cmd;
                        match HOME_KEYS.get_action(key).unwrap_or(HomeCmd::None) {
                            Cmd::None => {
                                handle_main_input(event, state)?;
                            }
                            Cmd::EnterChat => {
                                self.chat.input_mode = InputMode::Editing;
                            }
                            Cmd::StartRoles => {
                                state.tx.send(Msg::with(client::HomeMsg::StartRoles))?;
                            }
                        }
                    }
                    InputMode::Editing => {
                        self.chat.handle_input(event, state)?;
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Copy, Clone)]
pub enum RolesCmd {
    None,
    EnterChat,
    SelectPrev,
    SelectNext,
    ConfirmRole,
    StartGame,
}

pub const SELECT_ROLE_KEYS: &[(KeyEvent, RolesCmd)] = {
    use RolesCmd as Cmd;
    &[
        (key!(KeyCode::Enter), Cmd::StartGame),
        (key!(KeyCode::Char('e')), Cmd::EnterChat),
        (key!(KeyCode::Left), Cmd::SelectPrev),
        (key!(KeyCode::Right), Cmd::SelectNext),
        (key!(KeyCode::Char(' ')), Cmd::ConfirmRole),
    ]
};

impl Inputable for Context<Roles> {
    type State<'a> = &'a mut Connection<Roles>;
    fn handle_input(&mut self, event: &Event, state: &mut Connection<Roles>) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            if KeyEventKind::Press == key.kind {
                match self.chat.input_mode {
                    InputMode::Normal => {
                        use RolesCmd as Cmd;
                        match SELECT_ROLE_KEYS.get_action(key).unwrap_or(RolesCmd::None) {
                            Cmd::None => {
                                handle_main_input(event, state)?;
                            }
                            Cmd::EnterChat => {
                                self.chat.input_mode = InputMode::Editing;
                            }
                            Cmd::SelectNext => self.state.roles.next(),
                            Cmd::SelectPrev => self.state.roles.prev(),
                            Cmd::ConfirmRole => {
                                if self
                                    .state
                                    .roles
                                    .active()
                                    .is_some_and(|r| matches!(r, RoleStatus::Available(_)))
                                {
                                    state.tx.send(Msg::with(client::RolesMsg::Select(
                                        self.state.roles.active().unwrap().role(),
                                    )))?;
                                } else if self.state.roles.active != self.state.roles.selected {
                                    game_event!(self."This role is not available");
                                }
                            }
                            Cmd::StartGame => {
                                state.tx.send(Msg::with(client::RolesMsg::StartGame))?;
                            }
                        }
                    }
                    InputMode::Editing => {
                        self.chat.handle_input(event, state)?;
                    }
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

impl Inputable for Context<Game> {
    type State<'a> = &'a mut Connection<Game>;
    fn handle_input(&mut self, event: &Event, state: &mut Connection<Game>) -> anyhow::Result<()> {
        if let Event::Key(key) = event {
            if KeyEventKind::Press == key.kind {
                match self.chat.input_mode {
                    InputMode::Normal => {
                        use GameCmd as Cmd;
                        match GAME_KEYS.get_action(key).unwrap_or(GameCmd::None) {
                            Cmd::None => {
                                handle_main_input(event, state)?;
                            }
                            Cmd::ConfirmSelected => {
                                if let TurnStatus::Ready(phase) = self.state.phase {
                                    match phase {
                                        GamePhaseKind::DropAbility => {
                                            state.tx.send(Msg::with(
                                                client::GameMsg::DropAbility(
                                                    *self
                                                        .state
                                                        .abilities
                                                        .active()
                                                        .expect("Must be Some"),
                                                ),
                                            ))?;
                                        }
                                        GamePhaseKind::SelectAbility => {
                                            state.tx.send(Msg::with(
                                                client::GameMsg::SelectAbility(
                                                    *self
                                                        .state
                                                        .abilities
                                                        .active()
                                                        .expect("Must be Some"),
                                                ),
                                            ))?;
                                        }
                                        GamePhaseKind::AttachMonster => {
                                            state.tx.send(Msg::with(client::GameMsg::Attack(
                                                *self
                                                    .state
                                                    .monsters
                                                    .active()
                                                    .expect("Must be Some"),
                                            )))?;
                                        }
                                        GamePhaseKind::Defend => {
                                            self.state.monsters.selected = None;
                                            state.tx.send(Msg::with(client::GameMsg::Continue))?;
                                        }
                                    }
                                }
                            }
                            Cmd::SelectPrev => {
                                if let TurnStatus::Ready(phase) = self.state.phase {
                                    match phase {
                                        GamePhaseKind::SelectAbility
                                        | GamePhaseKind::DropAbility => self.state.abilities.prev(),
                                        GamePhaseKind::AttachMonster => self.state.monsters.prev(),
                                        _ => (),
                                    };
                                }
                            }
                            Cmd::SelectNext => {
                                if let TurnStatus::Ready(phase) = self.state.phase {
                                    match phase {
                                        GamePhaseKind::SelectAbility
                                        | GamePhaseKind::DropAbility => self.state.abilities.next(),
                                        GamePhaseKind::AttachMonster => self.state.monsters.next(),
                                        _ => (),
                                    };
                                }
                            }
                            Cmd::EnterChat => {
                                self.chat.input_mode = InputMode::Editing;
                            }
                        }
                    }
                    InputMode::Editing => {
                        self.chat.handle_input(event, state)?;
                    }
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

struct ChatMsg(String);
impl From<ChatMsg> for Msg<client::SharedMsg, client::HomeMsg> {
    #[inline]
    fn from(value: ChatMsg) -> Self {
        Msg::with(client::HomeMsg::Chat(value.0))
    }
}
impl From<ChatMsg> for Msg<client::SharedMsg, client::RolesMsg> {
    #[inline]
    fn from(value: ChatMsg) -> Self {
        Msg::with(client::RolesMsg::Chat(value.0))
    }
}
impl From<ChatMsg> for Msg<client::SharedMsg, client::GameMsg> {
    #[inline]
    fn from(value: ChatMsg) -> Self {
        Msg::with(client::GameMsg::Chat(value.0))
    }
}

use crate::protocol::SendSocketMessage;

impl Chat {
    fn handle_input<S>(&mut self, event: &Event, state: &Connection<S>) -> anyhow::Result<()>
    where
        S: SendSocketMessage + crate::client::states::DataForNextState,
        <S as SendSocketMessage>::Msg: From<ChatMsg>,
    {
        assert_eq!(self.input_mode, InputMode::Editing);
        if let Event::Key(key) = event {
            if KeyEventKind::Press == key.kind {
                use ChatCmd as Cmd;
                match CHAT_KEYS.get_action(key).unwrap_or(ChatCmd::None) {
                    Cmd::None => {
                        self.input.handle_event(&Event::Key(*key));
                    }
                    Cmd::SendInput => {
                        let input = std::mem::take(&mut self.input);
                        let msg = String::from(input.value());
                        let _ = state
                            .tx
                            .send(<S as SendSocketMessage>::Msg::from(ChatMsg(msg)));
                        self.messages
                            .push(server::ChatLine::Text(format!("(me): {}", input.value())));
                    }
                    Cmd::LeaveChatInput => {
                        self.input_mode = InputMode::Normal;
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
        }
        Ok(())
    }
}
