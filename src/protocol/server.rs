

use anyhow::anyhow;
use serde::{Serialize, Deserialize};
use std::net::SocketAddr;
use crate::server::{room::ServerHandle, peer::PeerHandle};
use crate::game::Role;
use crate::protocol::{ToContext, client, GameContextId, MessageReceiver };
use crate::game::{Card, Rank, Suit, AbilityDeck, Deck, HealthDeck, Deckable };
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use crate::protocol::{ DataForNextContext, client::{ClientNextContextData, ClientStartGameData} };
use crate::server::{ session::GameSessionHandle,
    peer::Connection
};
use crate::protocol::GameContext;


#[derive(Default)]
pub struct Intro{
    pub username : Option<String>,
}

pub struct Home{
    pub username : String,
}
pub struct SelectRole{
    pub username : String,
    pub role: Option<Role>

}

pub struct Game{
    pub username : String,
    pub to_session: GameSessionHandle,
    pub ability_deck: AbilityDeck,
    pub health_deck:  HealthDeck,
}


use crate::details::impl_try_from_for_inner;
impl_try_from_for_inner!{
pub type ServerGameContext = GameContext<
    self::Intro => Intro, 
    self::Home => Home, 
    self::SelectRole => SelectRole, 
    self::Game => Game,
>;
}


use crate::protocol::details::impl_from_inner;
impl_from_inner!{
    Intro, Home, SelectRole, Game  => ServerGameContext
}
// implement GameContextId::from( {{context struct}} )
impl_id_from_context_struct!{ Intro Home SelectRole Game }

pub type ServerNextContextData = DataForNextContext<
                                (), // SelectRoleData
                                ServerStartGameData // GameData
                                >;
#[derive(Debug)]
pub struct ServerStartGameData {
    pub session:   GameSessionHandle,
    pub monsters:  [Option<Card>; 4],
}

impl ToContext for ServerGameContext {
    type Next = ServerNextContextData;
    type State = Connection; 
    fn to(&mut self, next: ServerNextContextData, state: &Connection) -> anyhow::Result<()> {
        macro_rules! strange_next_to_self {
             (ServerGameContext::$self_ctx_type:ident($self_ctx:expr) ) => {
                 {
                    tracing::warn!(
                        concat!("Strange next context requested: from ", 
                                stringify!( ServerGameContext::$self_ctx_type), 
                                " to ", stringify!($self_ctx_type), )
                        );
                    ServerGameContext::from($self_ctx) 
                 }
             }
         }
        macro_rules! unexpected {
            ($next:ident for $ctx:expr) => {
                Err(anyhow!("Unimplemented {:?} to {:?}",
                   GameContextId::from(&$next) , GameContextId::from(&$ctx)))

            }
        }
        // server must never panic. Just return result and close connection
        let mut conversion_result = Ok(());
        {
            take_mut::take(self, |this| {
            use ServerNextContextData as Id;
            use ServerGameContext as C;
            match this {
                    C::Intro(i) => {
                        match next {
                            Id::Intro(_) => strange_next_to_self!(ServerGameContext::Intro(i) ),
                            Id::Home(_) => { 
                                let _ = state.socket.as_ref().unwrap().send(crate::protocol::encode_message(Msg::App(
                                AppMsg::NextContext(ClientNextContextData::Home(())))));
                                C::from(Home{username: i.username.expect("Username must be exists")})
                            },
                            Id::SelectRole(_) => { 
                                let _ = state.socket.as_ref().unwrap().send(crate::protocol::encode_message(Msg::App(
                                AppMsg::NextContext(ClientNextContextData::SelectRole(None)))));
                                C::from(SelectRole{username: i.username.unwrap(), role: None})

                            }
                            _ => {
                                conversion_result = unexpected!(next for i);
                                C::from(i)
                            },
                        }
                    },
                    C::Home(h) => {
                         match next {
                            Id::Home(_) =>  strange_next_to_self!(ServerGameContext::Home(h) ),
                            Id::SelectRole(_) => { 
                               let _ = state.socket.as_ref().unwrap().send(crate::protocol::encode_message(Msg::App(
                               AppMsg::NextContext(ClientNextContextData::SelectRole(None)))));
                               C::from(SelectRole{ username: h.username, role: None})
                            },
                            _ => {
                                conversion_result = unexpected!(next for h);
                                C::from(h)
                            },
                        }
                    },
                    C::SelectRole(r) => {
                         match next {
                            Id::SelectRole(_) => strange_next_to_self!(ServerGameContext::SelectRole(r) ),
                            Id::Game(data) => { 
                                if r.role.is_none(){
                                    conversion_result = Err(anyhow!(
                                            "a role must be selected at the start of the game"));
                                    C::from(r)
                                } else {
                                   let mut ability_deck = AbilityDeck::new(Suit::from(r.role.unwrap()));
                                   ability_deck.shuffle();
                                   let mut health_deck = HealthDeck::default(); 
                                   health_deck.shuffle();
                                   let mut abilities :[Option<Rank>; 3] = Default::default();
                                   ability_deck.ranks.drain(..3)
                                       .map(|r| Some(r) ).zip(abilities.iter_mut()).for_each(|(r, a)| *a = r );

                                   let _ = state.socket.as_ref().unwrap().send(crate::protocol::encode_message(Msg::App(
                                   AppMsg::NextContext(ClientNextContextData::Game(
                                         ClientStartGameData{
                                                abilities,
                                                monsters : data.monsters,
                                         }
                                    )
                                   ))));
                                   C::from(Game{username: r.username,
                                        health_deck, ability_deck, to_session: data.session})
                                }
                            },
                            _ =>{
                                conversion_result = unexpected!(next for r);
                                C::from(r)
                            },
                         }
                    }
                    C::Game(g) => {
                         match next {
                            Id::Game(_) => strange_next_to_self!(ServerGameContext::Game(g) ),
                            _ => {
                                conversion_result = unexpected!(next for g);
                                C::from(g)
                            },
                         }
                    },
                }
            });
        }
        conversion_result
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum SelectRoleStatus{
    Busy,
    Ok(Role),
    AlreadySelected,

}
use ascension_macro::DisplayOnlyIdents;
use std::fmt::Display;
use crate::protocol::details::nested;
nested! {
    #[derive(DisplayOnlyIdents, Deserialize, Serialize, Clone, Debug)]
    pub enum Msg {
        Intro (
                //
                #[derive(DisplayOnlyIdents, Deserialize, Serialize, Clone, Debug)]
                pub enum IntroMsg {
                    LoginStatus(LoginStatus),
                }
            ),
        Home (
                #[derive(DisplayOnlyIdents, Deserialize, Serialize, Clone, Debug)]
                pub enum HomeMsg {
                    Chat(ChatLine),
                }
             ),
        SelectRole (
                #[derive(DisplayOnlyIdents, Deserialize, Serialize, Clone, Debug)]
                pub enum SelectRoleMsg {
                    Chat(ChatLine),
                    SelectedStatus(SelectRoleStatus),
                }
               
             ),
        Game (
                #[derive(DisplayOnlyIdents, Deserialize, Serialize, Clone, Debug)]
                pub enum GameMsg {
                    Chat(ChatLine),
                }
                
             ), 
        App(
            #[derive(DisplayOnlyIdents, Deserialize, Serialize, Clone, Debug)]
            pub enum AppMsg {
                Pong,
                Logout,
                NextContext(ClientNextContextData),
                ChatLog(Vec<ChatLine>),

            }
        ),
    }

}


#[derive(Deserialize, Serialize, Clone, Debug)]
 pub enum ChatLine {
    Text          (String),
    GameEvent     (String),
    Connection    (String),
    Disconnection (String),
}


#[derive(DisplayOnlyIdents, PartialEq, Copy,Clone, Debug, Deserialize, Serialize)]
pub enum LoginStatus {
    Logged,
    Reconnected,
    InvalidPlayerName,
    AlreadyLogged,
    PlayerLimit,
}




impl_try_from_msg_for_msg_event!{ 
impl std::convert::TryFrom
    Msg::Intro      for IntroMsg 
    Msg::Home       for HomeMsg 
    Msg::SelectRole for SelectRoleMsg 
    Msg::Game       for GameMsg 
    Msg::App        for AppMsg 

}

impl_from_msg_event_for_msg!{ 
impl std::convert::From
         IntroMsg      => Msg::Intro
         HomeMsg       => Msg::Home
         SelectRoleMsg => Msg::SelectRole
         GameMsg       => Msg::Game
         AppMsg        => Msg::App
             
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::peer::{ServerGameContextHandle};
    use tokio_util::sync::CancellationToken;
    
    // mock
    fn game_session() -> GameSessionHandle {
        GameSessionHandle{
                to_session: tokio::sync::mpsc::unbounded_channel().0}
    }
    fn game() -> Game {
        Game{
            username: "Ig".into(), 
            to_session: game_session(), 
            ability_deck: AbilityDeck::new(Suit::Hearts),
            health_deck: HealthDeck::default()
        }
    }
    fn intro() -> Intro {
        Intro{username: Some("Ig".into())}//, peer_handle: PeerHandle{tx: to_peer}}

    }
    fn home() -> Home {
        Home{username: "Ig".into()}
    }
    fn select_role() -> SelectRole {
        SelectRole{username: "Ig".into(), role: Some(Role::Mage)}
    }
    fn start_game_data() -> ServerStartGameData{
        ServerStartGameData {
             session:   game_session(),
             monsters:  [None; 4]
        }

    }
    fn connection() -> Connection {
        let (to_socket, _) = tokio::sync::mpsc::unbounded_channel();
        let (to_world, _) = tokio::sync::mpsc::unbounded_channel(); 
        use std::net::{IpAddr, Ipv4Addr};

        Connection{//status: ConnectionStatus::Connected,
                   addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(000, 0, 0, 0)), 0000), 
                   socket: Some(to_socket), server: ServerHandle{tx: to_world}}
    }

    macro_rules! eq_id_from {
        ($($ctx_type:expr => $ctx:ident,)*) => {
            $(
                assert!(matches!(GameContextId::from(&$ctx_type), GameContextId::$ctx(_)));
            )*
        }
    }

    #[test]
    fn game_context_id_from_server_game_context() {
        use ServerGameContext as C;
        eq_id_from!(
            C::from(intro())       => Intro,
            C::from(home())        => Home,
            C::from(select_role()) => SelectRole,
            C::from(game())        => Game,

        );
    }
    #[test]
    fn game_context_id_from_server_context_struct() {
        eq_id_from!(
            intro()       => Intro,
            home()       => Home,
            select_role() => SelectRole,
            game()      => Game,

        );
    }
    #[test]
    fn game_context_id_from_server_msg() {
        let intro = Msg::Intro(IntroMsg::LoginStatus(LoginStatus::Logged));
        let home =  Msg::Home(HomeMsg::Chat(ChatLine::Text("_".into())));
        let select_role = Msg::SelectRole(SelectRoleMsg::Chat(ChatLine::Text("_".into())));
        let game = Msg::Game(GameMsg::Chat(ChatLine::Text("_".into()))); 
        eq_id_from!(
            intro       => Intro,
            home        => Home,
            select_role => SelectRole,
            game        => Game,
        );
    } 
    #[test]
    fn game_context_id_from_server_data_for_next_context() {
        let intro = ServerNextContextData::Intro(());
        let home =  ServerNextContextData::Home(());
        let select_role = ClientNextContextData::SelectRole(None);
        let game = ServerNextContextData::Game(start_game_data()); 
        eq_id_from!(
            intro       => Intro,
            home        => Home,
            select_role => SelectRole,
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
                    Data::SelectRole(()),  
                    Data::Game(start_game_data())
                ]
            }
        }
        use ServerNextContextData as Data;
        assert!(std::panic::catch_unwind(|| {
        use ServerGameContext as C;
            let cn = connection();
             for mut ctx in [
                 C::from(intro()),
                 C::from(home()),
                 C::from(select_role()),
                 C::from(game()),

             ]{
                for i in data!() {
                    let _ = ctx.to(i, &cn);
                }
             }
         }).is_ok());

    }

}



