use anyhow::anyhow;
use anyhow::Context as _;
use tracing::{info, trace, warn, error};
use crate::protocol::{ToContext, server, GameContextId};
use crate::client::Chat;
use crate::ui::details::StatefulList;
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use crate::protocol::details::impl_try_from_for_inner;
use crate::protocol::{GameContext, DataForNextContext};
use crate::game::{Card, Rank, Role};
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
         to_socket.send(
             encode_message(Msg::Intro(IntroEvent::GetChatLog)))
            .expect("failed to request a chat log");
        Connection{tx: to_socket, username}
    }
}


pub struct Intro{
    pub status   : Option<server::LoginStatus>,
    pub chat_log : Option<Vec<server::ChatLine>>
}
impl Default for Intro {
    fn default() -> Self {
        Intro{status: Default::default(), chat_log: Default::default()}
    }
}
pub struct App {
    pub chat: Chat,
}

pub struct Home{
    pub app:  App,
}

pub struct SelectRole {
    pub app: App,
    pub selected: Option<Role>,
    pub roles:    StatefulList<Role>,
}

pub struct Game{
    pub app : App,
    pub role: Role,
    pub abilities:  [Option<Rank>; 3],
    pub monsters    : [Option<Card>; 4],
}





// implement GameContextId::from( {{context struct}} )
impl_id_from_context_struct!{ Intro Home SelectRole Game }


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
        ClientGameContext::from(Intro::default())
    }
}


pub type ClientNextContextData = DataForNextContext<
                                 /*game: */ ClientStartGameData
                                 >;


#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ClientStartGameData {
    pub abilities  : [Option<Rank>; 3],
    pub monsters    :[Option<Card>; 4],
}
impl ToContext for ClientGameContext {
    type Next = ClientNextContextData;
    type State = Connection;
    fn to(& mut self, next: ClientNextContextData, _: &Connection) -> anyhow::Result<()> {
         macro_rules! strange_next_to_self {
             (ClientGameContext::$self_ctx_type:ident($self_ctx:expr) ) => {
                 {
                    warn!(
                        concat!("Strange next context requested: from ", 
                                stringify!( ClientGameContext::$self_ctx_type), 
                                " to ", stringify!($self_ctx_type), )
                        );
                    ClientGameContext::$self_ctx_type($self_ctx) 
                 }
             }
         }
         macro_rules! unexpected {
             ($next:ident for $ctx: expr) => ( 
                 unimplemented!("wrong next context request ({:?} for {:?})",
                                GameContextId::from(&$next) , GameContextId::from(&$ctx)) 
            )
         }
        {
            // conversion on the client side can panic because reasons of a panic 
            // are development mistakes
            take_mut::take_or_recover(self, 
             | | /*unused recover value for panic case*/ 
             ClientGameContext::from(Intro::default()),
             |this| {
                use ClientNextContextData as Data;
                use ClientGameContext as C;
                 match this {
                    C::Intro(i) => {
                        assert!(i.status.is_some() 
                                && matches!(i.status.unwrap(), server::LoginStatus::Logged)
                                , "A client should be logged before make a next context request");
                        let get_chat = |i: Intro| { 
                                let mut chat = Chat::default();
                                chat.messages = i.chat_log
                                    .expect("chat log is None, it had not been requested");
                                chat
                        };
                        match next {
                            Data::Intro(_) => 
                                strange_next_to_self!(ClientGameContext::Intro(i) ),
                            Data::Home (_) => {
                                C::from(Home{
                                    app: App{ chat: get_chat(i) }})
                            },
                            Data::SelectRole(_) => {
                                C::from(
                                    SelectRole{app: App{chat: get_chat(i) }, 
                                            roles: StatefulList::<Role>::default(), 
                                            selected: None
                                    }
                                )
                            }
                            _ => unexpected!(next for i),
                        }
                    },
                    C::Home(h) => {
                         match next {
                            Data::Home(_) => 
                                strange_next_to_self!(ClientGameContext::Home(h) ),
                            Data::SelectRole(_) =>{ 
                                C::from(
                                    SelectRole{
                                        app: h.app, 
                                        roles: StatefulList::<Role>::default(), 
                                        selected: None
                                    }
                                )
                             },
                            _ => unexpected!(next for h),
                        }
                    },
                    C::SelectRole(r) => {
                         match next {
                            Data::SelectRole(_) => 
                                strange_next_to_self!(ClientGameContext::SelectRole(r) ),
                            Data::Game(data) => {
                                C::from(Game{
                                    app: r.app, 
                                    role: r.roles.items[r.roles.state.selected().unwrap()],
                                    abilities: data.abilities,
                                    monsters : data.monsters
                                })
                            }
                            _ => unexpected!(next for r),
                         }
                    },
                    C::Game(g) => {
                         match next {
                            Data::Game(_) => 
                                strange_next_to_self!(ClientGameContext::Game(g)),
                            _ => unexpected!(next for g),
                         }
                    },

                }
             });
        }
       Ok(())
    }
}


// msg to server
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
    } 
}

impl_try_from_msg_for_msg_event!{ 
impl std::convert::TryFrom
    Msg::Intro      for IntroEvent 
    Msg::Home       for HomeEvent 
    Msg::SelectRole for SelectRoleEvent 
    Msg::Game       for GameEvent 
    Msg::App        for AppEvent 

}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::server::LoginStatus;

    // help functions
    fn mock_connection() -> Connection {
        Connection{tx: tokio::sync::mpsc::unbounded_channel().0, username: "Ig".to_string()}
    }
    fn default_intro() -> ClientGameContext {
        ClientGameContext::from(Intro{
            status   : Some(LoginStatus::Logged),
            chat_log : Some(Vec::default())
        })
    }
    fn start_game_data() ->  ClientStartGameData{
        ClientStartGameData{
                    abilities: Default::default(),
                    monsters: Default::default()}
    }
    
    #[test]
    fn shoul_start_from_intro() {
        let  ctx = ClientGameContext::new();
        assert!(matches!(ctx, ClientGameContext::Intro(_)));
        let  id = GameContextId::from(&ctx);
        assert_eq!(id, GameContextId::Intro(()));
    }

    #[test]
    fn client_shoul_correct_next_context_from_next_context_data() {
        let cn = mock_connection();
        let mut ctx = default_intro();
        macro_rules! test_next_ctx {
            ($($data_for_next: expr => $ctx_type: ident,)*) => {
                $(
                    ctx.to($data_for_next, &cn);
                    assert!(matches!(ctx, ClientGameContext::$ctx_type(_)));
                )*
            }
        }
        use ClientNextContextData as Data;
        test_next_ctx!(
                Data::Intro(())                => Intro,
                Data::Home(())                 => Home,
                Data::SelectRole(())           => SelectRole,
                Data::Game(start_game_data())  => Game,
        );

    }

    #[test]
    #[should_panic]
    fn panic_next_context_from_intro_without_login() {
        let cn = mock_connection();
        let mut ctx = ClientGameContext::from(Intro{
            status   : None,
            chat_log : Some(Vec::default())
        });
        ctx.to(ClientNextContextData::Home(()), &cn);
    } 
    #[test]
    #[should_panic]
    fn panic_next_context_from_intro_without_chat_log() {
        let cn = mock_connection();
        let mut ctx = ClientGameContext::from(Intro{
            status   : Some(LoginStatus::Logged),
            chat_log : None
        });
        ctx.to(ClientNextContextData::Home(()), &cn).unwrap();
    } 
    
    #[test]
    fn  client_intro_to_select_role_should_not_panic() {
        let cn = mock_connection();
        let mut ctx = default_intro();
        ctx.to(ClientNextContextData::SelectRole(()), &cn).unwrap();
    } 
    #[test]
    #[should_panic]
    fn  client_intro_to_game_should_panic() {
        let cn = mock_connection();
        let mut ctx = default_intro();
        ctx.to(ClientNextContextData::Game(start_game_data()), &cn).unwrap();
    }

    macro_rules! eq_id_from {
        ($($ctx_type:expr => $ctx:ident,)*) => {
            $(
                assert!(matches!(GameContextId::from(&$ctx_type), GameContextId::$ctx(_)));
            )*
        }
    }

    #[test]
    fn game_context_id_from_client_game_context() {
        let intro = ClientGameContext::from(Intro::default());
        let home =  ClientGameContext::from(Home{
            app: App{chat: Chat::default()}
        });
        let select_role = ClientGameContext::from(SelectRole{
            app: App{chat: Chat::default()}, 
            selected: None, 
            roles: StatefulList::default()
        });
        let game = ClientGameContext::from(Game{
            app:  App{chat: Chat::default()}, 
            role: Role::Mage, 
            abilities: Default::default(), 
            monsters: Default::default()
        });
        eq_id_from!(
            intro       => Intro,
            home        => Home,
            select_role => SelectRole,
            game        => Game,

        );
    }
    #[test]
    fn game_context_id_from_client_msg() {
        let intro = Msg::Intro(IntroEvent::GetChatLog);
        let home =  Msg::Home(HomeEvent::StartGame);
        let select_role = Msg::SelectRole(SelectRoleEvent::Select(Role::Mage));
        let game = Msg::Game(GameEvent::Chat("".into())); 
        eq_id_from!(
            intro       => Intro,
            home        => Home,
            select_role => SelectRole,
            game        => Game,
        );
    } 
    #[test]
    fn game_context_id_from_client_data_for_next_context() {
        use ClientNextContextData as Data;
        eq_id_from!(
           Data::Intro(())               => Intro,
           Data::Home(())                => Home,
           Data::SelectRole(())          => SelectRole,
           Data::Game(start_game_data()) => Game,
        );
    }

}






