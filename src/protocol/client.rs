use tracing::warn;

use crate::{
    client::{
        states::Chat,
        ui::details::{StatefulList, Statefulness},
    },
    protocol::{server, GameContextKind},
};
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::{
    game::{Card, Rank, Role, Suit},
    protocol::{GameContext, TurnStatus, Username},
};

// messages to server per state
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum IntroMsg {
    Login(Username),
    GetChatLog,
    Continue,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum HomeMsg {
    Chat(String),
    EnterRoles,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum RolesMsg {
    Chat(String),
    Select(Role),
    StartGame,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum GameMsg {
    Chat         (String),
    DropAbility  (Rank),
    SelectAbility(Rank),
    Attack(Card),
    Continue,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum SharedMsg {
    Ping,
    Logout,
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


#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StartGame {
    pub abilities: [Option<Rank>; 3],
    pub monsters: [Option<Card>; 2],
    pub role: Suit,
}







/*
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
*/
