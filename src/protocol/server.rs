use std::net::SocketAddr;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::{
    game::{AbilityDeck, Card, Deckable, Rank, Role, Suit},
    protocol::{
        client, ContextConverter, GameContext, GameContextKind, NextContextError, ToContext,
        TurnStatus, UnexpectedContext, Username,
    },
};

pub type PlayerId = SocketAddr;

pub const MAX_PLAYER_COUNT: usize = 2;

use crate::server::details::{Stateble, StatebleItem};
impl StatebleItem for AbilityDeck {
    type Item = Rank;
}
impl AsRef<[Rank]> for AbilityDeck {
    fn as_ref(&self) -> &[Rank] {
        &self.ranks
    }
}

pub const ABILITY_COUNT: usize = 3;

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
/*
pub struct ConvertedContext(pub ServerGameContext, pub client::NextContext);

impl<'a> TryFrom<ContextConverter<ServerGameContext, NextContext>> for ConvertedContext {
    type Error = NextContextError;
    fn try_from(
        converter: ContextConverter<ServerGameContext, NextContext>,
    ) -> Result<Self, Self::Error> {
        Ok(match (converter.0 .0, converter.1) {
            (GameContext::Intro(i), NextContext::Home(_)) => ConvertedContext(
                ServerGameContext::from(Home {
                    name: i.name.expect("Username must be exists"),
                }),
                client::NextContext::Home(()),
            ),
            (GameContext::Intro(i), NextContext::Roles(_)) => ConvertedContext(
                ServerGameContext::from(Roles::new(i.name.unwrap())),
                client::NextContext::Roles(None),
            ),

            (GameContext::Home(h), NextContext::Roles(_)) => ConvertedContext(
                ServerGameContext::from(Roles::new(h.name)),
                client::NextContext::Roles(None),
            ),

            (GameContext::Roles(r), NextContext::Game(data)) => {
                if let Some(role) = r.role {
                    let role = Suit::from(role);
                    let game = Game::new(r.name, role, data.session);
                    let client_data =
                        client::NextContext::Game(crate::protocol::client::StartGame {
                            abilities: game.abilities.active_items(),
                            monsters: data.monsters,
                            role,
                        });
                    ConvertedContext(ServerGameContext::from(game), client_data)
                } else {
                    return Err(NextContextError::MissingData(
                        "A player role must be selected in `Roles` context",
                    ));
                }
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
                    return Err(NextContextError::Same(current));
                } else {
                    return Err(NextContextError::Unimplemented { current, requested });
                }
            }
        })
    }
}
*/
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum SelectRoleError {
    Busy,
    AlreadySelected,
}

use derive_more::Debug;

use crate::protocol::{client::RoleStatus, details::nested};

pub type TurnResult<T> = Result<T, Username>;

nested! {
    #[derive(Deserialize, Serialize, Clone, Debug)]
    pub enum Msg {
        Intro (
                //
                #[derive(Deserialize, Serialize, Clone, Debug)]
                pub enum IntroMsg {
                    LoginStatus(LoginStatus),
                    StartHome,
                }
            ),

        Home (
                #[derive(Deserialize, Serialize, Clone, Debug)]
                pub enum HomeMsg {
                    StartRoles(Option<Role>),
                }
             ),

        Roles (
                #[derive(Deserialize, Serialize, Clone, Debug)]
                pub enum RolesMsg {
                    SelectedStatus(Result<Role, SelectRoleError>),
                    AvailableRoles([RoleStatus; Role::count()]),
                    StartGame(client::StartGame),
                }

             ),

        Game (
                #[derive(Deserialize, Serialize, Clone, Debug)]
                pub enum GameMsg {
                    DropAbility(TurnResult<Rank>),
                    SelectAbility(TurnResult<Rank>),
                    Attack(TurnResult<Card>),
                    Defend(Option<Card>),
                    Turn(TurnStatus),
                    Continue(TurnResult<()>),
                    UpdateGameData(([Option<Card>;2], [Option<Rank>;3])),
                }

             ),
        App(
            #[derive(Deserialize, Serialize, Clone, Debug)]
            pub enum SharedMsg {
                Pong,
                Logout,
                //NextContext(client::NextContext),
                ChatLog(Vec<ChatLine>),
                Chat(ChatLine),

            }
        ),
    }

}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum ChatLine {
    Text(String),
    GameEvent(String),
    Connection(Username),
    Reconnection(Username),
    Disconnection(Username),
}

#[derive(PartialEq, Copy, Clone, Debug, Deserialize, Serialize)]
pub enum LoginStatus {
    Logged,
    Reconnected,
    InvalidPlayerName,
    AlreadyLogged,
    PlayerLimit,
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
/*
#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{commands::ServerHandle, peer::Connection};

    // mock
    fn game_session() -> GameSessionHandle {
        GameSessionHandle {
            tx: tokio::sync::mpsc::unbounded_channel().0,
        }
    }
    fn game() -> Game {
        Game::new(Username("Ig".into()), Suit::Clubs, game_session())
    }
    fn intro() -> Intro {
        Intro {
            name: Some(Username("Ig".into())),
        } //, peer_handle: PeerHandle{tx: to_peer}}
    }
    fn home() -> Home {
        Home {
            name: Username("Ig".into()),
        }
    }
    fn select_role() -> Roles {
        Roles {
            name: Username("Ig".into()),
            role: Some(Role::Mage),
        }
    }
    fn start_game_data() -> StartGame {
        StartGame {
            session: game_session(),
            monsters: [None; 2],
        }
    }
    fn connection() -> Connection {
        let (to_socket, _) = tokio::sync::mpsc::unbounded_channel();
        let (to_world, _) = tokio::sync::mpsc::unbounded_channel();
        use std::net::{IpAddr, Ipv4Addr};

        Connection {
            //status: ConnectionStatus::Connected,
            addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(000, 0, 0, 0)), 0000),
            socket: Some(to_socket),
            server: ServerHandle { tx: to_world },
        }
    }

    macro_rules! eq_id_from {
        ($($ctx_type:expr => $ctx:ident,)*) => {
            $(
                assert!(matches!(GameContextKind::try_from(&$ctx_type).unwrap(), GameContextKind::$ctx));
            )*
        }
    }

    #[test]
    fn game_context_id_from_server_game_context() {
        use ServerGameContext as C;
        eq_id_from!(
            *C::from(intro()).as_inner()       => Intro,
            *C::from(home()).as_inner()        => Home,
            *C::from(select_role()).as_inner() => Roles,
            *C::from(game()).as_inner()        => Game,

        );
    }
    #[test]
    fn game_context_id_from_server_context_struct() {
        eq_id_from!(
            intro()       => Intro,
            home()       => Home,
            select_role() => Roles,
            game()      => Game,

        );
    }
    #[test]
    fn game_context_id_from_server_msg() {
        let intro = Msg::Intro(IntroMsg::LoginStatus(LoginStatus::Logged));
        //let home =  Msg::Home(HomeMsg::Chat(ChatLine::Text("_".into())));
        let select_role = Msg::Roles(RolesMsg::SelectedStatus(Err(SelectRoleError::Busy)));
        //let game = Msg::Game(GameMsg::Chat(ChatLine::Text("_".into())));
        eq_id_from!(
            intro       => Intro,
           // home        => Home,
            select_role => Roles,
            //game        => Game,
        );
    }
    #[test]
    fn game_context_id_from_server_data_for_next_context() {
        let intro = NextContext::Intro(());
        let home = NextContext::Home(());
        let select_role = client::NextContext::Roles(None);
        let game = NextContext::Game(start_game_data());
        eq_id_from!(
            intro       => Intro,
            home        => Home,
            select_role => Roles,
            game        => Game,
        );
    }

    #[test]
    fn server_to_next_context_should_never_panic() {
        macro_rules! data {
            () => {
                [
                    Data::Intro(()),
                    Data::Home(()),
                    Data::Roles(()),
                    Data::Game(start_game_data()),
                ]
            };
        }
        use NextContext as Data;
        assert!(std::panic::catch_unwind(|| {
            use ServerGameContext as C;
            let cn = connection();
            for mut ctx in [
                C::from(intro()),
                C::from(home()),
                C::from(select_role()),
                C::from(game()),
            ] {
                for i in data!() {
                    take_mut::take_or_recover(
                        &mut ctx,
                        || ctx,
                        |this| {
                            ConvertedContext::try_from(ContextConverter(this, i))
                                .unwrap()
                                .0
                        },
                    )
                }
            }
        })
        .is_ok());
    }
}
*/
