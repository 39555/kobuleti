

use anyhow::anyhow;
use serde::{Serialize, Deserialize};
use enum_dispatch::enum_dispatch;
use std::net::SocketAddr;
use crate::server::{WorldHandle, PeerHandle};
use crate::protocol::{To, client, Role, GameContextId, MessageReceiver };

use super::details::unwrap_enum;
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use std::sync::{Arc, Mutex};

    pub struct Connection{
        pub peer: PeerHandle,
        pub tx: Tx,
    }
    pub struct Intro{
        pub world_handle: WorldHandle,
        pub connection: Connection 
    }
    pub struct Home{
        pub world_handle: WorldHandle,
        pub connection: Connection 

    }
    pub struct SelectRole{
        pub world_handle: WorldHandle,
        pub connection: Connection,
        pub role: Option<Role>

    }
    pub struct Game{
        pub role: Role,
        pub connection: Connection 

    }

macro_rules! impl_unwrap_to_inner {
    ($(#[$meta:meta])* $vis:vis enum $name:ident {
        $($(#[$vmeta:meta])* $vname:ident $(= $val:expr)?,)*
    }) => {
        $(#[$meta])*
        $vis enum $name {
            $($(#[$vmeta])* $vname $(= $val)?,)*
        }
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

impl_unwrap_to_inner! {
    #[enum_dispatch]
    pub enum ServerGameContext {
        Intro ,
        Home  ,
        SelectRole,
        Game  ,
    }
}
    impl ServerGameContext {
        pub fn connection(&self) -> &Connection {
            macro_rules! unwrap_connection {
                ($($i: ident)+) => {
                    {
                        use ServerGameContext::*;
                        match *self {
                            $($i(ctx) => &ctx.connection, )*
                        }
                    }
                }
            }
            unwrap_connection!(Intro Home SelectRole Game)
        }
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
                                C::Home(Home{connection: i.connection,  world_handle: i.world_handle,})
                            },
                            Id::SelectRole => { todo!() }
                            Id::Game => { todo!() }
                        }
                    },
                    C::Home(h) => {
                         match next {
                            Id::Home =>  C::Home(h),
                            Id::SelectRole => { 
                               C::SelectRole(SelectRole{connection: h.connection, world_handle: h.world_handle, role: None})
                            },
                            _ => unimplemented!(),
                        }
                    },
                    C::SelectRole(r) => {
                         match next {
                            Id::SelectRole => C::SelectRole(r),
                            Id::Game => { 
                               C::Game(Game{connection: r.connection, role: r.role.unwrap()})
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


    



