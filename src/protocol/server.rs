

use anyhow::anyhow;
use serde::{Serialize, Deserialize};
use enum_dispatch::enum_dispatch;
use std::net::SocketAddr;
use crate::server::{ServerHandle, PeerHandle};
use crate::protocol::{ToContext, client, Role, GameContextId, MessageReceiver };
use crate::game::{Card, Rank, Suit, AbilityDeck, Deck, HealthDeck, Deckable };
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use std::sync::{Arc, Mutex};

    pub struct Connection {
        pub addr     : SocketAddr,
        pub to_socket: Tx,
        pub world: ServerHandle,
    }
impl Connection {
    pub fn new(addr: SocketAddr, socket_tx: Tx, world_handle: ServerHandle) -> Self {
        Connection{addr, to_socket: socket_tx, world: world_handle}
    }
}
    pub struct Intro{
        pub username : Option<String>,
        pub peer_handle : PeerHandle,
    }
    impl Intro {
        pub fn new(peer_handle: PeerHandle) -> Self{
            Intro{username: None, peer_handle}
        }
    }
    pub struct Home{
        pub username : String,
    }
    pub struct SelectRole{
        pub username : String,
        pub role: Option<Role>

    }
use crate::server::GameSessionHandle;
    pub struct Game{
        pub username : String,
        pub to_session: GameSessionHandle,
        pub ability_deck: AbilityDeck,
        pub health_deck:  HealthDeck,
    }

use crate::protocol::GameContext;

macro_rules! impl_try_from_for_inner {
    ($vis:vis type $name:ident = $ctx: ident < 
        $( $($self_:ident)?:: $vname:ident, )*
    >;

    ) => {
        $vis type $name  = $ctx <
            $($vname,)*
        >;
        $(
        impl std::convert::TryFrom<$name> for $vname {
            type Error = $name;
            fn try_from(other: $name) -> Result<Self, Self::Error> {
                    match other {
                        $name::$vname(v) => Ok(v),
                        o => Err(o),
                    }
            }
        }
        )*
    }
}
impl_try_from_for_inner!{
pub type ServerGameContext = GameContext<
    self::Intro, 
    self::Home, 
    self::SelectRole, 
    self::Game,
>;
}


//use crate::protocol::details::impl_unwrap_to_inner;
use super::details::impl_from_inner;


impl_from_inner!{
    Intro, Home, SelectRole, Game  => ServerGameContext
}



use crate::protocol::ServerNextContextData;

    impl ToContext for ServerGameContext {
        type Next = ServerNextContextData;
        type State = Connection; 
        fn to(&mut self, next: ServerNextContextData, state: &Connection) -> &mut Self {

            take_mut::take(self, |this| {
            use ServerNextContextData as Id;
            use ServerGameContext as C;
            match this {
                    C::Intro(i) => {
                        match next {
                            Id::Intro => C::Intro(i),
                            Id::Home => { 
                                let _ = state.to_socket.send(crate::protocol::encode_message(Msg::App(
                                AppEvent::NextContext(crate::protocol::ClientNextContextData::Home))));
                                C::Home(Home{username: i.username.unwrap()})
                            },
                            Id::SelectRole => { todo!() }
                            Id::Game(_) => { todo!() }
                        }
                    },
                    C::Home(h) => {
                         match next {
                            Id::Home =>  C::Home(h),
                            Id::SelectRole => { 
                               let _ = state.to_socket.send(crate::protocol::encode_message(Msg::App(
                               AppEvent::NextContext(crate::protocol::ClientNextContextData::SelectRole))));
                               C::SelectRole(SelectRole{ username: h.username, role: None})
                            },
                            _ => unimplemented!(),
                        }
                    },
                    C::SelectRole(r) => {
                         match next {
                            Id::SelectRole => C::SelectRole(r),
                            Id::Game(data) => { 
                               let mut ability_deck = AbilityDeck::new(Suit::from(r.role
                                        .expect("a role must br selected on the game start
                                                                 ")));
                               ability_deck.shuffle();
                               let mut health_deck = HealthDeck::default(); 
                               health_deck.shuffle();
                               state.to_socket.send(crate::protocol::encode_message(Msg::App(
                               AppEvent::NextContext(crate::protocol::ClientNextContextData::Game(
                                     crate::protocol::ClientStartGameData{
                                            card : Card{rank: *ability_deck.ranks.last().unwrap()
                                            , suit: ability_deck.suit},
                                            monsters : data.monsters
                                     }

                                )
                               )))).unwrap();
                               C::Game(Game{username: r.username,
                                health_deck, ability_deck, to_session: data.session})


                            },
                            _ => unimplemented!(),
                         }
                    }
                    C::Game(_) => {
                            todo!()
                    },

                }

        });
        //tracing::info!("new ctx {:?}", GameContextId::from(&*self));
        self
        }
    }

    structstruck::strike! {
    #[strikethrough[derive(Deserialize, Serialize, Clone, Debug)]]
    pub enum Msg {
        Intro(
            pub enum IntroEvent {
                LoginStatus( 
                    pub enum LoginStatus {
                        #![derive(PartialEq, Copy)]
                        Logged,
                        InvalidPlayerName,
                        AlreadyLogged,
                        PlayerLimit,
                    }
                ),
                ChatLog(Vec<ChatLine>),
            }
        ),

        Home(
            pub enum HomeEvent {
                 Chat(
                        pub enum ChatLine {
                            Text          (String),
                            GameEvent     (String),
                            Connection    (String),
                            Disconnection (String),
                        }
                 ),
            }
        ),
        SelectRole(
            pub enum SelectRoleEvent {
                Chat(ChatLine)
            }
        ),
        Game(
            pub enum GameEvent {
                Chat(ChatLine)
            }
        ),
        App(
            pub enum AppEvent {
                Logout,
                NextContext(crate::protocol::ClientNextContextData),

            }
        )
    } }


    



