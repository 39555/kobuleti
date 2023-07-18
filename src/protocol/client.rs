use anyhow::anyhow;
use crate::client::Start;

use crate::protocol::{To, server, Role, GameContextId, MessageReceiver};
use enum_dispatch::enum_dispatch;
use crate::client::Chat;
use crate::ui::terminal::TerminalHandle;
use std::sync::{Arc, Mutex};
use super::details::unwrap_enum;
type Tx = tokio::sync::mpsc::UnboundedSender<String>;

use serde::{Serialize, Deserialize};

pub struct Connection {
    pub tx: Tx,
}



pub struct Intro{
    pub username: String,
    pub _terminal: Option<Arc<Mutex<TerminalHandle>>>,
    pub chat_log : Option<Vec<server::ChatLine>>
}

pub struct App {
    pub terminal: Arc<Mutex<TerminalHandle>>,
    pub chat: Chat,
}
pub struct Home{
    pub app:  App,
}

use ratatui::widgets::TableState;
pub struct StatefulList<T> {
    pub state: TableState,
    pub items: Vec<T>,
}
impl<T> StatefulList<T> {
    pub fn with_items(items: Vec<T>) -> StatefulList<T> {
        StatefulList {
            state: TableState::default(),
            items,
        }
    }
    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

}


pub struct SelectRole {
    pub app: App,
    pub roles: StatefulList<Role>,
    pub selected: Option<Role>
}
impl Default for StatefulList<Role>{
    fn default() -> Self {
        let mut l = StatefulList::with_items(Role::all_variants().into());
        l.state.select(Some(0));
        l

    }
}

pub struct Game{
    pub app: App,
    pub role: Role,
}


#[enum_dispatch]
pub enum ClientGameContext {
    Intro ,
    Home ,
    SelectRole,
    Game ,
}

impl ClientGameContext {
    pub fn new(username: String) -> Self {
        ClientGameContext::from(Intro{username, _terminal: None, chat_log: None})
    }
}

impl To for ClientGameContext {
    fn to(& mut self, next: GameContextId) -> &mut Self{
         take_mut::take(self, |s| {
            use GameContextId as Id;
            use ClientGameContext as C;
             match s {
                C::Intro(mut i) => {
                    match next {
                        Id::Intro => C::Intro(i),
                        Id::Home => {
                            let mut chat = Chat::default();
                            chat.messages = i.chat_log.unwrap();
                            C::from(Home{
                                app: App{terminal: i._terminal.take().unwrap(), chat}})
                        },
                        Id::SelectRole => { todo!() }
                        Id::Game => { todo!() }
                    }
                },
                C::Home(h) => {
                     match next {
                        Id::Home =>  C::Home(h),
                        Id::SelectRole =>{ 
                            C::from(SelectRole{
                                 app: h.app, roles: StatefulList::<Role>::default(), selected: None})
                         },
                        _ => unimplemented!(),
                    }
                },
                C::SelectRole(r) => {
                     match next {
                        Id::SelectRole =>  C::SelectRole(r),
                        Id::Game => {
                            C::from(Game{
                                app: r.app, role: r.roles.items[r.roles.state.selected().unwrap()]

                            })

                        }
                        _ => unimplemented!(),
                     }
                },
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
            AddPlayer(String),
            GetChatLog,
        }
    ),
    Home(
        pub enum HomeEvent {
            Chat(String),
            StartGame
        }
    ),
    SelectRole(
        pub enum SelectRoleEvent {
            Chat(String),
            Select(Role)
        }
    ),
    Game(
        pub enum GameEvent {
            Chat(String)
        }
    ),
    App(
        pub enum AppEvent {
            Logout,
            NextContext
        }
    )
} }




