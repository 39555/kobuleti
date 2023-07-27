use anyhow::anyhow;
use anyhow::Context as _;
use crate::protocol::{ToContext, server, Role, GameContextId};
use crate::client::Chat;
use crate::ui::TerminalHandle;
use std::sync::{Arc, Mutex};
type Tx = tokio::sync::mpsc::UnboundedSender<String>;

use serde::{Serialize, Deserialize};

pub struct Connection {
    pub tx: Tx,
    pub username: String
}
use crate::protocol::encode_message;
impl Connection {
    pub fn new(to_socket: Tx, username: String) -> Self {
        to_socket.send(
            encode_message(Msg::Intro(IntroEvent::AddPlayer(username.clone()))))
            .expect("failed to send a login request to the socket");
         to_socket.send(encode_message(Msg::Intro(IntroEvent::GetChatLog)))
            .expect("failed to request a chat log");
        Connection{tx: to_socket, username}
    }
}


pub struct Intro{
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
    pub roles:    StatefulList<Role>,
    pub selected: Option<Role>
}
impl Default for StatefulList<Role>{
    fn default() -> Self {
        let mut l = StatefulList::with_items(Role::all().to_vec());
        l.state.select(Some(0));
        l

    }
}

//use arrayvec::ArrayVec;
use crate::game::{Card, Rank};

pub struct Game{
    pub app : App,
    pub role: Role,
    pub abilities:  [Option<Rank>; 3],
    pub monsters    : [Option<Card>; 4],
}

use crate::protocol::details::impl_try_from_for_inner;
use crate::protocol::GameContext;


impl_try_from_for_inner!{
pub type ClientGameContext = GameContext<
    self::Intro, 
    self::Home, 
    self::SelectRole, 
    self::Game,
>;
}


use super::details::impl_from_inner;
impl_from_inner!{
    Intro, Home, SelectRole, Game  => ClientGameContext
}

impl ClientGameContext {
    pub fn new() -> Self {
        ClientGameContext::from(Intro{_terminal: None, chat_log: None})
    }
}

impl ToContext for ClientGameContext {
    type Next = crate::protocol::ClientNextContextData;
    type State = Connection;
    fn to(& mut self, next: crate::protocol::ClientNextContextData, state: &Connection) -> &mut Self{
         take_mut::take(self, |s| {
            use crate::protocol::ClientNextContextData as Next;
            use ClientGameContext as C;
             match s {
                C::Intro(mut i) => {
                    match next {
                        Next::Intro => C::Intro(i),
                        Next::Home => {
                            let mut chat = Chat::default();
                            chat.messages = i.chat_log.expect("chat log not requested");
                            C::from(Home{
                                app: App{terminal: i._terminal.take().unwrap(), chat}})
                        },
                        Next::SelectRole => { todo!() }
                        Next::Game(_) => { todo!() }
                    }
                },
                C::Home(h) => {
                     match next {
                        Next::Home{..} =>  C::Home(h),
                        Next::SelectRole =>{ 
                            C::from(SelectRole{
                                 app: h.app, roles: StatefulList::<Role>::default(), selected: None})
                         },
                        _ => unimplemented!(),
                    }
                },
                C::SelectRole(r) => {
                     match next {
                        Next::SelectRole =>  C::SelectRole(r),
                        Next::Game(data) => {
                            C::from(Game{
                                app: r.app, role: r.roles.items[r.roles.state.selected().unwrap()],
                                abilities: data.abilities, monsters: data.monsters

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




