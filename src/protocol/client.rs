use tracing::warn;

use crate::{
    client::Chat,
    protocol::{server, GameContextKind, ToContext},
    ui::details::{StatefulList, Statefulness},
};
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use derive_more::Debug;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::{
    details::impl_try_from_for_inner,
    game::{Card, Rank, Role, Suit},
    protocol::{DataForNextContext, GameContext, TurnStatus, Username},
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
            .send(Msg::Intro(IntroMsg::AddPlayer(self.username.clone())))
            .expect("failed to send a login request to the socket");
        self
    }
}

#[derive(Default)]
pub struct Intro {
    pub status: Option<server::LoginStatus>,
    pub chat_log: Option<Vec<server::ChatLine>>,
}

pub struct App {
    pub chat: Chat,
}

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

pub struct Game {
    pub role: Suit,
    //pub phase: GamePhase,
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
            //phase: GamePhase::DropAbility,
        }
    }
}

// implement GameContextId::from( {{context struct}} )
impl_id_from_context_struct! { Intro Home Roles Game }

impl_try_from_for_inner! {
pub type ClientGameContext = GameContext<
    self::Intro => Intro,
    self::Home => Home,
    self::Roles => Roles,
    self::Game => Game,
>;
}

use super::details::impl_from_inner;
impl_from_inner! {
    Intro, Home, Roles, Game  => ClientGameContext
}

impl ClientGameContext {
    pub fn new() -> Self {
        ClientGameContext::from(Intro::default())
    }
}

pub type ClientNextContextData = DataForNextContext<
    Option<Role>,        // Roles (for the reconnection purpose)
    ClientStartGameData, // Game
>;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ClientStartGameData {
    pub abilities: [Option<Rank>; 3],
    pub monsters: [Option<Card>; 2],
    pub role: Suit,
}
impl ToContext for ClientGameContext {
    type Next = ClientNextContextData;
    type State = Connection;
    fn to(&mut self, next: ClientNextContextData, _: &Connection) -> anyhow::Result<()> {
        macro_rules! strange_next_to_self {
            (ClientGameContext::$self_ctx_type:ident($self_ctx:expr) ) => {{
                warn!(concat!(
                    "Strange next context requested: from ",
                    stringify!(ClientGameContext::$self_ctx_type),
                    " to ",
                    stringify!($self_ctx_type),
                ));
                ClientGameContext::$self_ctx_type($self_ctx)
            }};
        }
        macro_rules! unexpected {
            ($next:ident for $ctx: expr) => {
                unimplemented!(
                    "wrong next context request ({:?} for {:?})",
                    GameContextKind::from(&$next),
                    GameContextKind::from(&$ctx)
                )
            };
        }
        {
            // conversion on the client side can panic because reasons of a panic
            // are development mistakes
            take_mut::take_or_recover(
                self,
                || ClientGameContext::from(Intro::default()), // Unused recover value for panic case
                |this| {
                    use ClientGameContext as C;
                    use ClientNextContextData as Data;
                    match this {
                        C::Intro(i) => {
                            assert!(
                                i.status.is_some()
                                    && (matches!(i.status.unwrap(), server::LoginStatus::Logged)
                                        || matches!(
                                            i.status.unwrap(),
                                            server::LoginStatus::Reconnected
                                        )),
                                "A client should be logged before make a next context request"
                            );
                            match next {
                                Data::Intro(_) => {
                                    strange_next_to_self!(ClientGameContext::Intro(i))
                                }
                                Data::Home(_) => C::from(Home {
                                    app: App {
                                        chat: {
                                            Chat {
                                                messages: i.chat_log.expect(
                                                    "chat log is None, it was not been requested",
                                                ),
                                                ..Default::default()
                                            }
                                        },
                                    },
                                }),
                                Data::Roles(r) => {
                                    let mut sr = Roles::new(App {
                                        chat: Chat::default(),
                                    });
                                    sr.roles.selected = r.and_then(|r| {
                                        sr.roles.items.iter().position(|x| x.role() == r)
                                    });
                                    C::from(sr)
                                }
                                Data::Game(g) => C::from(Game::new(
                                    App {
                                        chat: Chat::default(),
                                    },
                                    g.role,
                                    g.abilities,
                                    g.monsters,
                                )),
                            }
                        }
                        C::Home(h) => match next {
                            Data::Home(_) => strange_next_to_self!(ClientGameContext::Home(h)),
                            Data::Roles(_) => C::from(Roles::new(h.app)),
                            _ => unexpected!(next for h),
                        },
                        C::Roles(r) => match next {
                            Data::Roles(_) => {
                                strange_next_to_self!(ClientGameContext::Roles(r))
                            }
                            Data::Game(data) => {
                                C::from(Game::new(r.app, data.role, data.abilities, data.monsters))
                            }
                            _ => unexpected!(next for r),
                        },
                        C::Game(g) => match next {
                            Data::Game(_) => strange_next_to_self!(ClientGameContext::Game(g)),
                            _ => unexpected!(next for g),
                        },
                    }
                },
            );
        }
        Ok(())
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
                AddPlayer(Username),
                GetChatLog,
            }
        ),
        Home(
            #[derive(Deserialize, Serialize, Clone, Debug)]
            pub enum HomeMsg {
                Chat(String),
                StartGame,
            }
        ),
        Roles(
            #[derive(Deserialize, Serialize, Clone, Debug)]
            pub enum RolesMsg {
                Chat(String),
                Select(Role),
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
            pub enum AppMsg {
                Ping,
                Logout,
                NextContext,

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
    Msg::App        for AppMsg

}

impl_from_msg_event_for_msg! {
impl std::convert::From
         IntroMsg      => Msg::Intro
         HomeMsg       => Msg::Home
         RolesMsg => Msg::Roles
         GameMsg       => Msg::Game
         AppMsg        => Msg::App

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
            username: "Ig".to_string(),
            cancel,
        }
    }
    fn default_intro() -> ClientGameContext {
        ClientGameContext::from(Intro {
            status: Some(LoginStatus::Logged),
            chat_log: Some(Vec::default()),
        })
    }
    fn start_game_data() -> ClientStartGameData {
        ClientStartGameData {
            abilities: Default::default(),
            monsters: Default::default(),
            role: Suit::Clubs,
        }
    }

    #[test]
    fn shoul_start_from_intro() {
        let ctx = ClientGameContext::new();
        assert!(matches!(ctx, ClientGameContext::Intro(_)));
        let id = GameContextKind::from(&ctx);
        assert_eq!(id, GameContextKind::Intro(()));
    }

    #[test]
    fn client_shoul_correct_next_context_from_next_context_data() {
        let cn = mock_connection();
        let mut ctx = default_intro();
        macro_rules! test_next_ctx {
            ($($data_for_next: expr => $ctx_type: ident,)*) => {
                $(
                    ctx.to($data_for_next, &cn).expect("Must switch to the next");
                    assert!(matches!(ctx, ClientGameContext::$ctx_type(_)));
                )*
            }
        }
        use ClientNextContextData as Data;
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
        ctx.to(ClientNextContextData::Home(()), &cn)
            .expect("Must switch");
    }
    #[test]
    #[should_panic]
    fn panic_next_context_from_intro_without_chat_log() {
        let cn = mock_connection();
        let mut ctx = ClientGameContext::from(Intro {
            status: Some(LoginStatus::Logged),
            chat_log: None,
        });
        ctx.to(ClientNextContextData::Home(()), &cn)
            .expect("Must switch");
    }

    #[test]
    fn client_intro_to_select_role_should_not_panic() {
        let cn = mock_connection();
        let mut ctx = default_intro();
        ctx.to(ClientNextContextData::Roles(None), &cn)
            .expect("Must switch");
    }
    #[test]
    #[should_panic]
    fn client_intro_to_game_should_panic() {
        let cn = mock_connection();
        let mut ctx = default_intro();
        ctx.to(ClientNextContextData::Game(start_game_data()), &cn)
            .expect("Must switch");
    }

    macro_rules! eq_id_from {
        ($($ctx_type:expr => $ctx:ident,)*) => {
            $(
                assert!(matches!(GameContextKind::try_from(&$ctx_type).unwrap(), GameContextKind::$ctx(_)));
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
        let home = Msg::Home(HomeMsg::StartGame);
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
        use ClientNextContextData as Data;
        eq_id_from!(
           Data::Intro(())               => Intro,
           Data::Home(())                => Home,
           Data::Roles(None)        => Roles,
           Data::Game(start_game_data()) => Game,
        );
    }
}
