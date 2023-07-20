

use anyhow::anyhow;
use serde::{Serialize, Deserialize};
use enum_dispatch::enum_dispatch;
use std::net::SocketAddr;
use crate::server::{WorldHandle, PeerHandle};
use crate::protocol::{To, client, Role, GameContextId, MessageReceiver };

type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use std::sync::{Arc, Mutex};

    pub struct Connection {
        pub addr     : SocketAddr,
        pub to_socket: Tx,
        pub world: WorldHandle,
    }
impl Connection {
    pub fn new(addr: SocketAddr, socket_tx: Tx, world_handle: WorldHandle) -> Self {
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
    pub struct Game{
        pub username : String,
        pub role: Role,
    }


use crate::protocol::details::impl_unwrap_to_inner;
impl_unwrap_to_inner! {
    pub enum ServerGameContext {
        Intro (Intro),
        Home (Home)  ,
        SelectRole(SelectRole),
        Game(Game)  ,
    }
}

use super::details::impl_from_inner;

impl_from_inner!{
    Intro{}, Home{}, SelectRole{}, Game{}  => ServerGameContext
}

    impl To for ServerGameContext {
        type Next = GameContextId;
        fn to(&mut self, next: GameContextId) -> &mut Self {
            take_mut::take(self, |s| {
            use GameContextId as Id;
            use ServerGameContext as C;
            match s {
                    C::Intro(i) => {
                        match next {
                            Id::Intro => C::Intro(i),
                            Id::Home => {
                                C::Home(Home{username: i.username.unwrap()})
                            },
                            Id::SelectRole => { todo!() }
                            Id::Game => { todo!() }
                        }
                    },
                    C::Home(h) => {
                         match next {
                            Id::Home =>  C::Home(h),
                            Id::SelectRole => { 
                               C::SelectRole(SelectRole{ username: h.username, role: None})
                            },
                            _ => unimplemented!(),
                        }
                    },
                    C::SelectRole(r) => {
                         match next {
                            Id::SelectRole => C::SelectRole(r),
                            Id::Game => { 
                               C::Game(Game{username: r.username, role: r.role.unwrap()})
                            },
                            _ => unimplemented!(),
                         }
                    }
                    C::Game(_) => {
                            todo!()
                    },

                }

        });
        tracing::info!("new ctx {:?}", GameContextId::from(&*self));
        self
        }
    }
use arrayvec::ArrayVec;
use crate::game::Card;

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
                NextContext(pub enum NextContextData {
                    Intro,
                    Home{chat_log: Vec<ChatLine>},
                    SelectRole,
                    Game(pub struct StartGameData {
                             pub current_card: Card,
                             pub monsters    :[Card; 4],
                    })


                })
            }
        )
    } }


    



