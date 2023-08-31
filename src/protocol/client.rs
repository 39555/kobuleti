use tracing::warn;

use crate::{
    client::{
        states::Chat,
        ui::details::{StatefulList, Statefulness},
    },
    protocol::{server, GameContextKind, ToContext},
};
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::{
    game::{Card, Rank, Role, Suit},
    protocol::{GameContext, TurnStatus, Username},
};
pub struct Connection {
    pub tx: UnboundedSender<Msg>,
    pub username: Username,
    pub cancel: CancellationToken,
}
impl Connection {
    pub fn new(
        to_socket: UnboundedSender<Msg>,
        username: Username,
        cancel: CancellationToken,
    ) -> Self {
        Connection {
            tx: to_socket,
            username,
            cancel,
        }
    }
    pub fn login(self) -> Self {
        self.tx
            .send(Msg::Intro(IntroMsg::Login(self.username.clone())))
            .expect("failed to send a login request to the socket");
        self
    }
}

#[derive(Debug, Default)]
pub struct Intro {
    pub status: Option<server::LoginStatus>,
    pub chat_log: Option<Vec<server::ChatLine>>,
}

#[derive(Debug, Default)]
pub struct App {
    pub chat: Chat,
}

#[derive(Debug, Default)]
pub struct Home {
    pub app: App,
}

#[derive(PartialEq, Eq, Copy, Clone, Serialize, Deserialize, Debug)]
pub enum RoleStatus {
    NotAvailable(Role),
    Available(Role),
}
impl RoleStatus {
    pub fn role(&self) -> Role {
        match *self {
            RoleStatus::NotAvailable(r) => r,
            RoleStatus::Available(r) => r,
        }
    }
}

#[derive(Debug)]
pub struct Roles {
    pub app: App,
    pub roles: StatefulList<RoleStatus, [RoleStatus; 4]>,
}

impl Roles {
    pub fn new(app: App) -> Self {
        Roles {
            app,
            roles: StatefulList::with_items(Role::all().map(RoleStatus::Available)),
        }
    }
}

impl Statefulness for StatefulList<RoleStatus, [RoleStatus; 4]> {
    type Item<'a> = RoleStatus;
    fn next(&mut self) {
        self.active = {
            if self.active.is_none() || self.active.unwrap() >= self.items.as_ref().len() - 1 {
                Some(0)
            } else {
                Some(self.active.expect("Must be Some here") + 1)
            }
        };
    }
    fn prev(&mut self) {
        self.active = {
            if self.active.is_none() || self.active.unwrap() == 0 {
                Some(self.items.as_ref().len() - 1)
            } else {
                Some(self.active.expect("Must be Some here") - 1)
            }
        };
    }

    fn active(&self) -> Option<Self::Item<'_>> {
        self.active.map(|i| self.items.as_ref()[i])
    }
    fn selected(&self) -> Option<Self::Item<'_>> {
        self.selected.map(|i| self.items.as_ref()[i])
    }
}

#[derive(Debug)]
pub struct Game {
    pub role: Suit,
    pub phase: TurnStatus,
    pub attack_monster: Option<usize>,
    pub health: u16,
    pub abilities: StatefulList<Option<Rank>, [Option<Rank>; 3]>,
    pub monsters: StatefulList<Option<Card>, [Option<Card>; 2]>,
    pub app: App,
}

impl<E, T: AsRef<[Option<E>]> + AsMut<[Option<E>]>> Statefulness for StatefulList<Option<E>, T>
where
    for<'a> E: 'a,
    for<'a> T: 'a,
{
    type Item<'a> = &'a E;
    fn next(&mut self) {
        self.active = {
            if self.items.as_ref().iter().all(|i| i.is_none()) {
                None
            } else {
                let active = self.active.expect("Must be Some");
                if active >= self.items.as_ref().len() - 1 {
                    let mut next = 0;
                    while self.items.as_ref()[next].is_none() {
                        next += 1;
                    }
                    Some(next)
                } else {
                    let mut next = active + 1;
                    while self.items.as_ref()[next].is_none() {
                        if next >= self.items.as_ref().len() - 1 {
                            next = 0;
                        } else {
                            next += 1;
                        }
                    }
                    Some(next)
                }
            }
        };
    }
    fn prev(&mut self) {
        self.active = {
            if self.items.as_ref().iter().all(|i| i.is_none()) {
                None
            } else {
                let active = self.active.expect("Must be Some");
                if active == 0 {
                    let mut next = self.items.as_ref().len() - 1;
                    while self.items.as_ref()[next].is_none() {
                        next -= 1;
                    }
                    Some(next)
                } else {
                    let mut next = active - 1;
                    while self.items.as_ref()[next].is_none() {
                        if next == 0 {
                            next = self.items.as_ref().len() - 1;
                        } else {
                            next -= 1;
                        }
                    }
                    Some(next)
                }
            }
        };
    }
    fn active(&self) -> Option<Self::Item<'_>> {
        if let Some(i) = self.active {
            Some(self.items.as_ref()[i].as_ref().expect("Must be Some"))
        } else {
            None
        }
    }

    fn selected(&self) -> Option<Self::Item<'_>> {
        if let Some(i) = self.selected {
            Some(self.items.as_ref()[i].as_ref().expect("Must be Some"))
        } else {
            None
        }
    }
}

impl Game {
    pub fn new(
        app: App,
        role: Suit,
        abilities: [Option<Rank>; 3],
        monsters: [Option<Card>; 2],
    ) -> Self {
        Game {
            app,
            role,
            attack_monster: None,
            health: 36,
            phase: TurnStatus::Wait,
            abilities: StatefulList::with_items(abilities),
            monsters: StatefulList::with_items(monsters),
        }
    }
}

// implement GameContextKind::from( {{context struct}} )
impl_GameContextKind_from_context_struct! { Intro Home Roles Game }

#[derive(Debug)]
#[repr(transparent)]
pub struct ClientGameContext(pub GameContext<self::Intro, self::Home, self::Roles, self::Game>);

impl ClientGameContext {
    pub fn as_inner<'a>(&'a self) -> &'a GameContext<Intro, Home, Roles, Game> {
        &self.0
    }
    pub fn as_inner_mut<'a>(&'a mut self) -> &'a mut GameContext<Intro, Home, Roles, Game> {
        &mut self.0
    }
}
impl Default for ClientGameContext {
    fn default() -> Self {
        ClientGameContext::from(Intro::default())
    }
}
impl From<&ClientGameContext> for GameContextKind {
    #[inline]
    fn from(value: &ClientGameContext) -> Self {
        GameContextKind::from(&value.0)
    }
}

macro_rules! impl_from_inner {
($( $src: ident $(,)?)+ => $inner_dst: ty => $dst:ty) => {
    $(
    impl From<$src> for $dst {
        fn from(src: $src) -> Self {
            Self(<$inner_dst>::$src(src))
        }
    }
    )*
    };
}

impl_from_inner! {
    Intro, Home, Roles, Game  => GameContext<Intro, Home, Roles, Game>  => ClientGameContext
}

pub type NextContext = GameContext<
    (),
    (),
    Option<Role>, // Roles (for the reconnection purpose)
    StartGame,    // Game
>;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StartGame {
    pub abilities: [Option<Rank>; 3],
    pub monsters: [Option<Card>; 2],
    pub role: Suit,
}

use crate::protocol::{ContextConverter, NextContextError};

impl<'a> TryFrom<ContextConverter<ClientGameContext, NextContext>> for ClientGameContext {
    type Error = NextContextError;
    fn try_from(
        converter: ContextConverter<ClientGameContext, NextContext>,
    ) -> Result<Self, Self::Error> {
        match converter.0.as_inner() {
            GameContext::Intro(i) => {
                assert!(
                    i.status.is_some()
                        && (matches!(i.status.unwrap(), server::LoginStatus::Logged)
                            || matches!(i.status.unwrap(), server::LoginStatus::Reconnected)),
                    "A client should be logged before make a next context request"
                );
            }
            _ => (),
        }

        Ok(match (converter.0 .0, converter.1) {
            (GameContext::Intro(i), NextContext::Home(_)) => ClientGameContext::from(Home {
                app: App {
                    chat: {
                        Chat {
                            messages: i
                                .chat_log
                                .expect("chat log is None, it was not been requested"),
                            ..Default::default()
                        }
                    },
                },
            }),
            (GameContext::Intro(_), NextContext::Roles(r)) => {
                let mut sr = Roles::new(App {
                    chat: Chat::default(),
                });
                sr.roles.selected =
                    r.and_then(|r| sr.roles.items.iter().position(|x| x.role() == r));
                ClientGameContext::from(sr)
            }
            (GameContext::Intro(_), NextContext::Game(g)) => ClientGameContext::from(Game::new(
                App {
                    chat: Chat::default(),
                },
                g.role,
                g.abilities,
                g.monsters,
            )),
            (GameContext::Home(h), NextContext::Roles(_)) => {
                ClientGameContext::from(Roles::new(h.app))
            }
            (GameContext::Roles(r), NextContext::Game(data)) => {
                ClientGameContext::from(Game::new(r.app, data.role, data.abilities, data.monsters))
            }
            (from, to) => {
                let current = GameContextKind::from(&from);
                let requested = GameContextKind::from(&to);
                if current == requested {
                    tracing::warn!(
                        "Strange next context request = {:?} -> {:?}",
                        current,
                        requested
                    );
                    ClientGameContext(from)
                } else {
                    return Err(NextContextError::Unimplemented { current, requested });
                }
            }
        })
    }
}

// msg to server
use crate::protocol::details::nested;
nested! {
    #[derive(Deserialize, Serialize, Clone, Debug)]
    pub enum Msg {
        Intro(
            #[derive(Deserialize, Serialize, Clone, Debug)]
            pub enum IntroMsg {
                Login(Username),
                GetChatLog,
                Continue,
            }
        ),
        Home(
            #[derive(Deserialize, Serialize, Clone, Debug)]
            pub enum HomeMsg {
                Chat(String),
                EnterRoles,
            }
        ),
        Roles(
            #[derive(Deserialize, Serialize, Clone, Debug)]
            pub enum RolesMsg {
                Chat(String),
                Select(Role),
                StartGame,
            }
        ),
        Game(
            #[derive(Deserialize, Serialize, Clone, Debug)]
            pub enum GameMsg {
                Chat         (String),
                DropAbility  (Rank),
                SelectAbility(Rank),
                Attack(Card),
                Continue,
            }
        ),
        App(
            #[derive(Deserialize, Serialize, Clone, Debug)]
            pub enum SharedMsg {
                Ping,
                Logout,
                //NextContext,

            }
        ),
    }
}

impl_try_from_msg_for_msg_event! {
impl std::convert::TryFrom
    Msg::Intro      for IntroMsg
    Msg::Home       for HomeMsg
    Msg::Roles for RolesMsg
    Msg::Game       for GameMsg
    Msg::App        for SharedMsg

}

impl_from_msg_event_for_msg! {
impl std::convert::From
         IntroMsg      => Msg::Intro
         HomeMsg       => Msg::Home
         RolesMsg => Msg::Roles
         GameMsg       => Msg::Game
         SharedMsg        => Msg::App

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::server::LoginStatus;

    // help functions
    fn mock_connection() -> Connection {
        let cancel = tokio_util::sync::CancellationToken::new();
        Connection {
            tx: tokio::sync::mpsc::unbounded_channel().0,
            username: Username("Ig".to_string()),
            cancel,
        }
    }
    fn default_intro() -> ClientGameContext {
        ClientGameContext::from(Intro {
            status: Some(LoginStatus::Logged),
            chat_log: Some(Vec::default()),
        })
    }
    fn start_game_data() -> StartGame {
        StartGame {
            abilities: Default::default(),
            monsters: Default::default(),
            role: Suit::Clubs,
        }
    }

    #[test]
    fn shoul_start_from_intro() {
        let ctx = ClientGameContext::default();
        assert!(matches!(ctx.as_inner(), GameContext::Intro(_)));
        let id = GameContextKind::from(&ctx);
        assert_eq!(id, GameContextKind::Intro);
    }

    #[test]
    fn client_shoul_correct_next_context_from_next_context_data() {
        let cn = mock_connection();
        let mut ctx = default_intro();
        macro_rules! test_next_ctx {
            ($($data_for_next: expr => $ctx_type: ident,)*) => {
                $(
                    take_mut::take_or_recover(&mut ctx, || ctx , |this| {
                        ClientGameContext::try_from(ContextConverter(this, $data_for_next))
                            .expect("Must switch to the next")
                    });
                    assert!(matches!(ctx.as_inner(), GameContext::$ctx_type(_)));
                )*
            }
        }
        use NextContext as Data;
        test_next_ctx!(
                Data::Intro(())                => Intro,
                Data::Home(())                 => Home,
                Data::Roles(None)           => Roles,
                Data::Game(start_game_data())  => Game,
        );
    }

    #[test]
    #[should_panic]
    fn panic_next_context_from_intro_without_login() {
        let cn = mock_connection();
        let mut ctx = ClientGameContext::from(Intro {
            status: None,
            chat_log: Some(Vec::default()),
        });
        take_mut::take_or_recover(
            &mut ctx,
            || ctx,
            |this| {
                ClientGameContext::try_from(ContextConverter(this, NextContext::Home(()))).unwrap()
            },
        );
    }
    #[test]
    #[should_panic]
    fn panic_next_context_from_intro_without_chat_log() {
        let cn = mock_connection();
        let mut ctx = ClientGameContext::from(Intro {
            status: Some(LoginStatus::Logged),
            chat_log: None,
        });
        take_mut::take_or_recover(
            &mut ctx,
            || ctx,
            |this| {
                ClientGameContext::try_from(ContextConverter(this, NextContext::Home(())))
                    .expect("Should switch")
            },
        );
    }

    #[test]
    fn client_intro_to_select_role_should_not_panic() {
        let cn = mock_connection();
        let mut ctx = default_intro();
        take_mut::take_or_recover(
            &mut ctx,
            || ctx,
            |this| {
                ClientGameContext::try_from(ContextConverter(this, NextContext::Roles(None)))
                    .expect("Should switch")
            },
        );
    }

    #[test]
    #[should_panic]
    fn client_intro_to_game_should_panic() {
        let cn = mock_connection();
        let mut ctx = default_intro();
        take_mut::take_or_recover(
            &mut ctx,
            || ctx,
            |this| {
                ClientGameContext::try_from(ContextConverter(
                    this,
                    NextContext::Game(start_game_data()),
                ))
                .expect("Should switch")
            },
        );
    }

    macro_rules! eq_id_from {
        ($($ctx_type:expr => $ctx:ident,)*) => {
            $(
                assert!(matches!(GameContextKind::try_from(&$ctx_type).unwrap(), GameContextKind::$ctx));
            )*
        }
    }

    #[test]
    fn game_context_id_from_client_game_context() {
        let intro = ClientGameContext::from(Intro::default());
        let home = ClientGameContext::from(Home {
            app: App {
                chat: Chat::default(),
            },
        });
        let select_role = ClientGameContext::from(Roles::new(App {
            chat: Chat::default(),
        }));
        let game = ClientGameContext::from(Game::new(
            App {
                chat: Chat::default(),
            },
            Suit::Clubs,
            Default::default(),
            Default::default(),
        ));
        eq_id_from!(
            intro       => Intro,
            home        => Home,
            select_role => Roles,
            game        => Game,

        );
    }
    #[test]
    fn game_context_id_from_client_msg() {
        let intro = Msg::Intro(IntroMsg::GetChatLog);
        let home = Msg::Home(HomeMsg::EnterRoles);
        let select_role = Msg::Roles(RolesMsg::Select(Role::Mage));
        let game = Msg::Game(GameMsg::Chat("".into()));
        eq_id_from!(
            intro       => Intro,
            home        => Home,
            select_role => Roles,
            game        => Game,
        );
    }
    #[test]
    fn game_context_id_from_client_data_for_next_context() {
        use NextContext as Data;
        eq_id_from!(
           Data::Intro(())               => Intro,
           Data::Home(())                => Home,
           Data::Roles(None)        => Roles,
           Data::Game(start_game_data()) => Game,
        );
    }
}
