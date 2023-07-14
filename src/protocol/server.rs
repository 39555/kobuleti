

use anyhow::anyhow;
use serde::{Serialize, Deserialize};
use enum_dispatch::enum_dispatch;
use std::net::SocketAddr;
use crate::server::State;
use crate::protocol::{NextGameContext, client, GameContextId, MessageReceiver };

use super::details::unwrap_enum;
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use std::sync::{Arc, Mutex};

    pub struct Intro{
     pub state: Arc<Mutex<State>>,
     pub addr : SocketAddr,
     pub tx: Tx
    }
    pub struct Home{
        pub state: Arc<Mutex<State>>,
        pub addr : SocketAddr,
        pub tx: Tx
    }
    pub struct Game{
        pub state: Arc<Mutex<State>>,
        pub addr : SocketAddr,
        pub tx: Tx
    }


    #[enum_dispatch]
    pub enum ServerGameContext {
        Intro ,
        Home  ,
        Game  ,
    }

    impl NextGameContext for ServerGameContext {
        fn to(&mut self, next: GameContextId){
            take_mut::take(self, |s| {
            use GameContextId as Id;
            use ServerGameContext as C;
            match s {
                    C::Intro(i) => {
                        match next {
                            Id::Intro => C::Intro(i),
                            Id::Home => {
                                C::Home(Home{state: i.state, addr: i.addr, tx: i.tx})
                            },
                            Id::Game => { todo!() }
                        }
                    },
                    C::Home(h) => {
                         match next {
                            Id::Intro => unimplemented!(),
                            Id::Home =>  C::Home(h),
                            Id::Game => { 
                               C::Game(Game{state: h.state, addr: h.addr, tx: h.tx})
                            },
                        }
                    },
                    C::Game(_) => {
                            todo!()
                    },

                }

        })
        }
    }
    impl MessageReceiver<client::Msg> for ServerGameContext {
    fn message(&mut self, msg: client::Msg) -> anyhow::Result<()> {
        use client::Msg;
        let cur_ctx = GameContextId::from(&*self);
        let msg_ctx = GameContextId::from(&msg);
        if cur_ctx != msg_ctx{
            return Err(anyhow!("a wrong message type for current stage {:?} was received: {:?}", cur_ctx, msg_ctx));
        }else {
            use ServerGameContext::*;
            match self {
                Intro(i) => i.message(unwrap_enum!(msg, Msg::Intro).unwrap()) ,
                Home(h) => h.message(unwrap_enum!(msg, Msg::Home).unwrap())   ,
                Game(g) => g.message(unwrap_enum!(msg, Msg::Game).unwrap())   ,
            }
        }
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
        Game(
            pub enum GameEvent {
                Chat(ChatLine)
            }
        ),
        Main(
            pub enum MainEvent {
                Logout,
                NextContext(GameContextId)
            }
        )
    } }


    



