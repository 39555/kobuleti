use anyhow::anyhow;
use crate::client::Start;

use crate::protocol::{NextGameContext, server, GameContextId, MessageReceiver};
use enum_dispatch::enum_dispatch;
use crate::client::Chat;
use crate::ui::terminal::TerminalHandle;
use std::sync::{Arc, Mutex};
use super::details::unwrap_enum;
type Tx = tokio::sync::mpsc::UnboundedSender<String>;

use serde::{Serialize, Deserialize};


pub struct Intro{
    pub username: String,
    pub tx: Tx,
    pub _terminal_handle: Option<Arc<Mutex<TerminalHandle>>>,
}


pub struct Home{
    pub tx: Tx,
    pub _terminal_handle: Arc<Mutex<TerminalHandle>>,
    pub chat: Chat,
}


pub struct Game{
    pub tx: Tx,
    pub _terminal_handle: Arc<Mutex<TerminalHandle>>,
    pub chat: Chat,
}


#[enum_dispatch]
pub enum GameContext {
    Intro  ,
    Home ,
    Game   ,
}



impl NextGameContext for GameContext {
    fn to(& mut self, next: GameContextId){
         take_mut::take(self, |s| {
            use GameContextId as Id;
            use GameContext as C;
             match s {
                GameContext::Intro(mut i) => {
                    match next {
                        Id::Intro => C::Intro(i),
                        Id::Home => {
                            C::from(Home{
                                tx: i.tx, _terminal_handle: i._terminal_handle.take().unwrap(), chat: Chat::default()})
                        },
                        Id::Game => { todo!() }
                    }
                },
                C::Home(h) => {
                     match next {
                        Id::Intro => unimplemented!(),
                        Id::Home =>  C::Home(h),
                        Id::Game => { 
                            C::from(Game{
                                tx: h.tx, _terminal_handle: h._terminal_handle, chat: h.chat})
                        },
                    }
                },
                C::Game(_) => {
                        todo!()
                },

            }
         });
    }
}

impl MessageReceiver<server::Msg> for GameContext {
fn message(&mut self, msg: server::Msg) -> anyhow::Result<()> {
    use server::Msg;
    let cur_ctx = GameContextId::from(&*self);
    let msg_ctx = GameContextId::from(&msg);
    if cur_ctx != msg_ctx{
        return Err(anyhow!("a wrong message type for current stage {:?} was received: {:?}", cur_ctx, msg_ctx));
    }else {
        use GameContext::*;
        match self {
            Intro(i) => i.message(unwrap_enum!(msg,  Msg::Intro).unwrap()) ,
            Home(h)  =>  h.message(unwrap_enum!(msg, Msg::Home).unwrap())   ,
            Game(g)  =>  g.message(unwrap_enum!(msg, Msg::Game).unwrap())   ,
        }
    }
}
}

structstruck::strike! {
#[strikethrough[derive(Deserialize, Serialize, Clone, Debug)]]
pub enum Msg {
    Intro(
        pub enum IntroEvent {
            AddPlayer(String)
        }
    ),
    Home(
        pub enum HomeEvent {
            GetChatLog,
            Chat(String),
            StartGame
        }
    ),
    Game(
        pub enum GameEvent {
            Chat(String)
        }
    ),
    Main(
        pub enum MainEvent {
            RemovePlayer,
            NextContext
        }
    )
} }




