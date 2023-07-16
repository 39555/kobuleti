

use anyhow::anyhow;
use serde::{Serialize, Deserialize};
use enum_dispatch::enum_dispatch;
use std::net::SocketAddr;
use crate::server::State;
use crate::protocol::{To, client, Role, GameContextId, MessageReceiver };

use super::details::unwrap_enum;
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use std::sync::{Arc, Mutex};

    pub struct Conn{
         pub state: Arc<Mutex<State>>,
        pub addr : SocketAddr,
        pub tx: Tx
    }
    pub struct Intro{
        pub app: Conn 
    }
    pub struct Home{
        pub app: Conn 

    }
    pub struct SelectRole{
        pub app: Conn,
        pub role: Option<Role>

    }
    pub struct Game{
        pub role: Role,
        pub app: Conn 

    }

    #[enum_dispatch]
    pub enum ServerGameContext {
        Intro ,
        Home  ,
        SelectRole,
        Game  ,
    }
  
    impl To for ServerGameContext {
        fn to(&mut self, next: GameContextId) -> &mut Self {
            take_mut::take(self, |s| {
            use GameContextId as Id;
            use ServerGameContext as C;
            match s {
                    C::Intro(i) => {
                        match next {
                            Id::Intro => C::Intro(i),
                            Id::Home => {
                                C::Home(Home{app: i.app})
                            },
                            Id::SelectRole => { todo!() }
                            Id::Game => { todo!() }
                        }
                    },
                    C::Home(h) => {
                         match next {
                            Id::Home =>  C::Home(h),
                            Id::SelectRole => { 
                               C::SelectRole(SelectRole{app: h.app, role: None})
                            },
                            _ => unimplemented!(),
                        }
                    },
                    C::SelectRole(r) => {
                         match next {
                            Id::SelectRole => C::SelectRole(r),
                            Id::Game => { 
                               C::Game(Game{app: r.app, role: r.role.unwrap()})
                            },
                            _ => unimplemented!(),
                         }
                    }
                    C::Game(_) => {
                            todo!()
                    },

                }

        });
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
                )
            }
        ),

        Home(
            pub enum HomeEvent {
                 ChatLog(Vec<ChatLine>),
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
                NextContext(GameContextId)
            }
        )
    } }


    



