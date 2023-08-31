use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Context as _};
use futures::{SinkExt, StreamExt};
use tokio::{io::AsyncWriteExt, net::TcpStream};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use tracing::{error, info, warn};
use ui::{details::Statefulness, TerminalHandle};

use crate::protocol::{
    client,
    client::{ClientGameContext, Connection, Game, Home, Intro, Roles},
    encode_message, server, ContextConverter, GameContext, GameContextKind, MessageDecoder,
    MessageReceiver, ToContext, TurnStatus, Username,
};
type Tx = tokio::sync::mpsc::UnboundedSender<String>;

use ratatui::widgets::ScrollbarState;
use tokio_util::sync::CancellationToken;
use tui_input::Input;

pub mod input;
pub mod states;
pub mod ui;

use input::{InputMode, Inputable};

pub async fn connect(username: String, host: SocketAddr) -> anyhow::Result<()> {
    let stream = TcpStream::connect(host)
        .await
        .with_context(|| format!("Failed to connect to address {}", host))?;
    states::run(Username(username), stream, CancellationToken::new())
        .await
        .context("failed to process messages from the server")
}
/*
macro_rules! game_event {
    ($self:ident.$msg:literal $(,$args:expr)*) => {
        $self.app.chat.messages.push(server::ChatLine::GameEvent(format!($msg, $($args,)*)))
    }
}
pub(crate) use game_event;

impl MessageReceiver<server::IntroMsg, &client::Connection> for Intro {
    fn reduce(&mut self, msg: server::IntroMsg, state: &client::Connection) -> anyhow::Result<()> {
        use server::{IntroMsg::*, LoginStatus::*};
        match msg {
            LoginStatus(status) => {
                self.status = Some(status);
                match status {
                    Logged => {
                        info!("Successfull login to the game");
                        state
                            .tx
                            .send(client::Msg::Intro(client::IntroMsg::GetChatLog))
                            .expect("failed to request a chat log");
                        Ok(())
                    }
                    Reconnected => {
                        info!("Reconnected!");
                        Ok(())
                    }
                    InvalidPlayerName => Err(anyhow!("Invalid player name: '{}'", state.username)),
                    PlayerLimit => Err(anyhow!(
                        "Player '{}' has tried to login but the player limit has been reached",
                        state.username
                    )),
                    AlreadyLogged => Err(anyhow!(
                        "User with name '{}' already logged",
                        state.username
                    )),
                }
            }
        }
        .context("Failed to join to the game")
    }
}
impl MessageReceiver<server::HomeMsg, &Connection> for Home {
    fn reduce(&mut self, _msg: server::HomeMsg, _: &Connection) -> anyhow::Result<()> {
        Ok(())
    }
}
impl MessageReceiver<server::RolesMsg, &Connection> for Roles {
    fn reduce(&mut self, msg: server::RolesMsg, _: &Connection) -> anyhow::Result<()> {
        use server::RolesMsg::*;
        match msg {
            SelectedStatus(status) => {
                // TODO may be Result  with custom error
                if let Ok(role) = status {
                    game_event!(self."You select {:?}", role);
                    self.roles.selected = Some(
                        self.roles
                            .items
                            .iter()
                            .position(|r| r.role() == role)
                            .expect("Must be present in roles"),
                    )
                }
            }
            AvailableRoles(roles) => {
                self.roles.items = roles;
            }
        }
        Ok(())
    }
}

macro_rules! turn {
    ($self:ident, $turn:expr => |$ability:ident|$block:block) => {
        // TODO ?
        #[allow(clippy::redundant_closure_call)]
        match $turn {
            Ok(turn) => {
                (|$ability|{
                    $block
                })(turn);
            }
            Err(username) => game_event!($self."It's not you turn now. {} should make a turn", username),
        }
    }
}

impl MessageReceiver<server::GameMsg, &Connection> for Game {
    fn reduce(&mut self, msg: server::GameMsg, _: &Connection) -> anyhow::Result<()> {
        use server::GameMsg as Msg;

        use crate::protocol::GamePhaseKind;
        match msg {
            Msg::Turn(s) => {
                self.phase = s;
                if let TurnStatus::Ready(phase) = s {
                    match phase {
                        GamePhaseKind::DropAbility => {
                            //self.attack_monster = None;
                            game_event!(self."Your Turn! You can drop any ability")
                        }
                        GamePhaseKind::SelectAbility => {
                            game_event!(self."You can select ability for attack")
                        }
                        GamePhaseKind::AttachMonster => {
                            game_event!(self."Now You can attach a monster")
                        }
                        GamePhaseKind::Defend => (),
                    }
                };
            }
            Msg::Continue(_) => {}
            Msg::DropAbility(turn) => {
                turn!(self, turn => |ability|{
                    game_event!(self."You discard {:?}", ability);
                    // * self.abilities.items
                    //    .iter_mut()
                     //   .find(|r| r.is_some_and(|r| r == ability))
                     //   .expect("Must be exists") = None;
                });
            }
            Msg::SelectAbility(turn) => {
                turn!(self, turn => |ability| {
                    game_event!(self."You select {:?}", ability);
                    self.abilities.selected = Some(self.abilities.items
                        .iter()
                        .position(|i| i.is_some_and(|i| i == ability))
                        .expect("Must be exists"));
                    // TODO ability description

                });
            }

            Msg::Attack(turn) => {
                turn!(self, turn => |monster| {
                    self.monsters.selected = Some(
                        self.monsters.items
                        .iter()
                        .position(|i| i.is_some_and(|i| i == monster))
                        .expect("Must exists"));
                    game_event!(self."You attack {:?}", self.monsters.active().unwrap());
                    self.abilities.selected = None;
                    self.monsters.selected  = None;
                });
            }
            Msg::Defend(monster) => {
                match monster {
                    Some(m) => {
                        self.attack_monster = Some(
                            self.monsters
                                .items
                                .iter()
                                .position(|i| i.is_some_and(|i| i == m))
                                .expect("Must be Some"),
                        );
                        game_event!(self."A {:?} Attack You!",
                                        self.monsters.items[self.attack_monster
                                                            .expect("Must attack")]
                                                            .expect("Must be Some"));
                        game_event!(self."You get damage from {:?}", self.monsters.items[
                            self.attack_monster.expect("Must attack")]);
                    }
                    None => {
                        game_event!(self."You defend");
                    }
                };
                self.monsters.selected = None;
            }
            Msg::UpdateGameData((monsters, abilities)) => {
                self.monsters.items = monsters;
                self.abilities.items = abilities;
            }
        }
        Ok(())
    }
}
*/

/*
async fn run(
    username: Username,
    mut stream: TcpStream,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let (r, w) = stream.split();
    let mut socket_writer = FramedWrite::new(w, LinesCodec::new());
    let mut socket_reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));
    let mut input_reader = crossterm::event::EventStream::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<client::Msg>();

    let connection = Connection::new(tx, username, cancel.clone()).login();
    let mut current_game_context = ClientGameContext::default();
    let terminal = Arc::new(Mutex::new(
        TerminalHandle::new().context("Failed to create a terminal for the game")?,
    ));
    TerminalHandle::chain_panic_for_restore(Arc::downgrade(&terminal));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("Closing the client user interface");
                socket_writer.send(encode_message(
                    client::Msg::App(client::SharedMsg::Logout))).await?;
                break
            }

            input = input_reader.next() => {
                match  input {
                    None => break,
                    Some(Err(e)) => {
                        warn!("IO error on stdin: {}", e);
                    },
                    Some(Ok(event)) => {
                        current_game_context.handle_input(&event, &connection)
                           .context("failed to process an input event in the current game stage")?;
                        ui::draw(&terminal, &mut current_game_context);

                    }
                }
            }
            Some(msg) = rx.recv() => {
                socket_writer.send(encode_message(msg)).await
                    .context("failed to send a message to the socket")?;
            }

            r = socket_reader.next::<server::Msg>() => match r {
                Some(Ok(msg)) => {
                    use server::{Msg, SharedMsg};
                    match msg {
                        Msg::App(e) => {
                            match e {
                                SharedMsg::Pong => {

                                }
                                SharedMsg::Logout =>  {
                                    std::mem::drop(terminal);
                                    info!("Logout");
                                    break
                                },
                                SharedMsg::Chat(line) => {
                                     match &mut current_game_context.as_inner_mut(){
                                        GameContext::Home(h) => {
                                         h.app.chat.messages.push(line);
                                        },
                                        GameContext::Roles(r) => {
                                         r.app.chat.messages.push(line);
                                        },
                                        GameContext::Game(g) => {
                                         g.app.chat.messages.push(line);
                                        },
                                        _ => (),
                                    }
                                },
                                SharedMsg::NextContext(n) => {
                                    take_mut::take_or_recover(&mut current_game_context, ||  ClientGameContext::default() , |this| {
                                        ClientGameContext::try_from(ContextConverter(this, n))
                                            .expect("Should switch")
                                    });
                                },
                                SharedMsg::ChatLog(log) => {
                                    match &mut current_game_context.as_inner_mut(){
                                        GameContext::Intro(i) => {
                                            i.chat_log = Some(log);
                                        },
                                        GameContext::Home(h) => {
                                         h.app.chat.messages = log;
                                        },
                                        GameContext::Roles(r) => {
                                         r.app.chat.messages = log;
                                        },
                                        GameContext::Game(g) => {
                                         g.app.chat.messages = log;

                                        },
                                    }
                                }
                            }
                        },
                        _ => {
                            if let Err(e) = current_game_context.reduce(msg, &connection).map_err(|e| anyhow!("{:?}", e))
                                .with_context(|| format!("current context {:?}"
                                              , GameContextKind::from(&current_game_context) )){
                                     std::mem::drop(terminal);
                                     warn!("Error: {}", e);
                                     break
                                };
                        }
                    }
                    ui::draw(&terminal, &mut current_game_context);
                }
                ,
                Some(Err(e)) => {
                    error!("{}", e);
                }
                None => {
                    break;
                }
            }
        }
    }
    stream.shutdown().await?;
    Ok(())
}
*/
/*
use futures::{future, TryFutureExt};

pub type SharedMsg = server::SharedMsg;
#[derive(serde::Serialize, serde::Deserialize)]
enum Msg2<T> {
    Shared(SharedMsg),
    State(T),
}

// state machine
#[derive(Default)]
struct Context<C> {
    state: C,
}

struct StartHome;
struct StartGame;

impl From<ContextConverter<Context<Intro>, StartHome>> for Context<Home> {
    fn from(value: ContextConverter<Context<Intro>, StartHome>) -> Self {
        todo!()
    }
}
impl From<ContextConverter<Context<Home>, StartGame>> for Context<Game> {
    fn from(value: ContextConverter<Context<Home>, StartGame>) -> Self {
        todo!()
    }
}
impl MessageReceiver<server::IntroMsg, &Connection> for Context<Intro> {
    fn reduce(&mut self, msg: server::IntroMsg, state: &Connection) -> anyhow::Result<()> {
        todo!()
    }
}
impl MessageReceiver<server::HomeMsg, &Connection> for Context<Home> {
    fn reduce(&mut self, msg: server::HomeMsg, state: &Connection) -> anyhow::Result<()> {
        todo!()
    }
}
impl MessageReceiver<server::GameMsg, &Connection> for Context<Game> {
    fn reduce(&mut self, msg: server::GameMsg, state: &Connection) -> anyhow::Result<()> {
        todo!()
    }
}
impl Inputable for Context<Intro> {
    type State<'a> = &'a Connection;
    fn handle_input(
        &mut self,
        event: &crossterm::event::Event,
        state: &Connection,
    ) -> anyhow::Result<()> {
        todo!()
    }
}
impl Inputable for Context<Home> {
    type State<'a> = &'a Connection;
    fn handle_input(
        &mut self,
        event: &crossterm::event::Event,
        state: &Connection,
    ) -> anyhow::Result<()> {
        todo!()
    }
}
impl Inputable for Context<Game> {
    type State<'a> = &'a Connection;
    fn handle_input(
        &mut self,
        event: &crossterm::event::Event,
        state: &Connection,
    ) -> anyhow::Result<()> {
        todo!()
    }
}

use tokio::net::tcp::{ReadHalf, WriteHalf};

struct Client<'a> {
    //stream: TcpStream,
    writer: FramedWrite<WriteHalf<'a>, LinesCodec>,
    reader: MessageDecoder<FramedRead<ReadHalf<'a>, LinesCodec>>,
    input: crossterm::event::EventStream,
    //rx    : tokio::sync::mpsc::UnboundedReceiver<client::Msg>,
    terminal: Arc<Mutex<TerminalHandle>>,
}

pub type ClientSharedMsg = client::SharedMsg;

#[derive(serde::Serialize, serde::Deserialize)]
enum ClientMsg<T> {
    Shared(ClientSharedMsg),
    State(T),
}
trait MessageSender {
    type MsgType;
}
impl MessageSender for Intro {
    type MsgType = ClientMsg<client::IntroMsg>;
}
impl MessageSender for Home {
    type MsgType = ClientMsg<client::HomeMsg>;
}
impl MessageSender for Game {
    type MsgType = ClientMsg<client::GameMsg>;
}
impl MessageSender for Roles {
    type MsgType = ClientMsg<client::RolesMsg>;
}

impl Client<'_> {
    #[async_recursion::async_recursion]
    async fn run(
        &mut self,
        mut context: ClientGameContext,
        connection: &Connection,
    ) -> anyhow::Result<()> {
        macro_rules! unwrap_or_return_ok {
            ($option:expr) => {
                match $option {
                    None => return Ok(()),
                    Some(x) => x,
                }
            };
        }
        let next = match context.as_inner_mut() {
            GameContext::Intro(intro) => {
                unwrap_or_return_ok!(self.run_as(intro, connection).await?)
            }
            GameContext::Home(home) => unwrap_or_return_ok!(self.run_as(home, connection).await?),
            GameContext::Roles(roles) => {
                unwrap_or_return_ok!(self.run_as(roles, connection).await?)
            }
            GameContext::Game(game) => unwrap_or_return_ok!(self.run_as(game, connection).await?),
        };
        self.run(
            ClientGameContext::try_from(ContextConverter(context, next))?,
            connection,
        )
        .await
    }

    async fn run_as<State, M>(
        &mut self,
        visitor: &mut State,
        state: &Connection,
    ) -> anyhow::Result<Option<client::NextContext>>
    where
        for<'a> State: MessageReceiver<M, &'a Connection>
            + Inputable<State<'a> = &'a Connection>
            + MessageSender
            + crate::ui::Drawable,
        for<'a> Msg2<M>: serde::Deserialize<'a>,
        <State as MessageSender>::MsgType: serde::Serialize,
    {
        let (tx, mut rx) =
            tokio::sync::mpsc::unbounded_channel::<<State as MessageSender>::MsgType>();
        loop {
            tokio::select! {
                input = self.input.next() => {
                    match  input {
                        None => break,
                        Some(Err(e)) => {
                            warn!("IO error on stdin: {}", e);
                        },
                        Some(Ok(event)) => {
                            visitor.handle_input(&event, state)
                               .context("failed to process an input event in the current game stage")?;
                            ui::draw(&self.terminal, visitor);
                        }
                    }
                }
                Some(msg) = rx.recv() => {
                    self.writer.send(encode_message(msg)).await
                        .context("failed to send a message to the socket")?;
                }

                msg = self.reader.next::<Msg2<M>>() => match msg {
                    Some(Ok(msg)) =>{ match msg {
                            Msg2::Shared(msg) => match msg {
                                SharedMsg::NextContext(ctx) => {
                                    return Ok(Some(ctx));


                                }
                                _ => todo!()


                            },
                            Msg2::State(state_msg) => {
                                if let Err(e) = visitor.reduce(state_msg, state).map_err(|e| anyhow!("{:?}", e)){
                                    //.with_context(|| format!("current context {:?}"
                                    //              , GameContextKind::from(&visitor.state) ))
                                    //              {
                                         //std::mem::drop(terminal);
                                         warn!("Error: {}", e);
                                         break
                                    };
                            }
                        //ui::draw_context(&terminal, &mut current_game_context);
                    }}
                    ,
                    Some(Err(e)) => {
                        error!("{}", e);
                    }
                    None => {
                        break;
                    }
                }

            }
        }

        Ok(None)
    }
}

*/
#[cfg(test)]
mod tests {
    use super::*;

    fn foo(cn: &mut ClientGameContext) -> Result<(), ()> {
        Ok(())
    }

    #[test]
    fn test_refcell() {
        let mut cn = ClientGameContext::default();
        foo(&mut cn).and_then(|_| {
            match cn.as_inner() {
                _ => (),
            };
            Ok(())
        });
    }
}
