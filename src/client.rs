use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Context as _};
use futures::{SinkExt, StreamExt};
use tokio::{io::AsyncWriteExt, net::TcpStream};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use tracing::{error, info, warn};

use crate::{
    input::Inputable,
    protocol::{
        client,
        client::{ClientGameContext, Connection, Game, Home, Intro, SelectRole},
        encode_message, server, GameContextKind, MessageDecoder, MessageReceiver, ToContext,
        TurnStatus,
    },
    ui::{self, details::Statefulness, TerminalHandle},
};
type Tx = tokio::sync::mpsc::UnboundedSender<String>;

use ratatui::widgets::ScrollbarState;
use tokio_util::sync::CancellationToken;
use tui_input::Input;

use crate::input::InputMode;

#[derive(Default, Debug)]
pub struct Chat {
    pub input_mode: InputMode,
    /// Current value of the input box
    pub input: Input,
    /// History of recorded messages
    pub messages: Vec<server::ChatLine>,
    pub scroll: usize,
    pub scroll_state: ScrollbarState,
}

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
impl MessageReceiver<server::SelectRoleMsg, &Connection> for SelectRole {
    fn reduce(&mut self, msg: server::SelectRoleMsg, _: &Connection) -> anyhow::Result<()> {
        use server::SelectRoleMsg::*;
        match msg {
            SelectedStatus(status) => {
                if let server::SelectRoleStatus::Ok(role) = status {
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
                    //*self.abilities.items
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
                        self.attack_monster = Some(self
                            .monsters
                            .items
                            .iter()
                            .position(|i| i.is_some_and(|i| i == m)).expect("Must be Some"));
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

pub async fn connect(username: String, host: SocketAddr) -> anyhow::Result<()> {
    let stream = TcpStream::connect(host)
        .await
        .with_context(|| format!("Failed to connect to address {}", host))?;
    run(username, stream, CancellationToken::new())
        .await
        .context("failed to process messages from the server")
}
async fn run(
    username: String,
    mut stream: TcpStream,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let (r, w) = stream.split();
    let mut socket_writer = FramedWrite::new(w, LinesCodec::new());
    let mut socket_reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));
    let mut input_reader = crossterm::event::EventStream::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<client::Msg>();

    let connection = Connection::new(tx, username, cancel.clone()).login();
    let mut current_game_context = ClientGameContext::new();
    let terminal = Arc::new(Mutex::new(
        TerminalHandle::new().context("Failed to create a terminal for the game")?,
    ));
    TerminalHandle::chain_panic_for_restore(Arc::downgrade(&terminal));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("Closing the client user interface");
                socket_writer.send(encode_message(
                    client::Msg::App(client::AppMsg::Logout))).await?;
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
                        ui::draw_context(&terminal, &mut current_game_context);

                    }
                }
            }
            Some(msg) = rx.recv() => {
                socket_writer.send(encode_message(msg)).await
                    .context("failed to send a message to the socket")?;
            }

            r = socket_reader.next::<server::Msg>() => match r {
                Some(Ok(msg)) => {
                    use server::{Msg, AppMsg};
                    match msg {
                        Msg::App(e) => {
                            match e {
                                AppMsg::Pong => {

                                }
                                AppMsg::Logout =>  {
                                    std::mem::drop(terminal);
                                    info!("Logout");
                                    break
                                },
                                AppMsg::Chat(line) => {
                                     match &mut current_game_context{
                                        ClientGameContext::Home(h) => {
                                         h.app.chat.messages.push(line);
                                        },
                                        ClientGameContext::SelectRole(r) => {
                                         r.app.chat.messages.push(line);
                                        },
                                        ClientGameContext::Game(g) => {
                                         g.app.chat.messages.push(line);
                                        },
                                        _ => (),
                                    }
                                },
                                AppMsg::NextContext(n) => {
                                    let _ = current_game_context.to(n, &connection);
                                },
                                AppMsg::ChatLog(log) => {
                                    match &mut current_game_context{
                                        ClientGameContext::Intro(i) => {
                                            i.chat_log = Some(log);
                                        },
                                        ClientGameContext::Home(h) => {
                                         h.app.chat.messages = log;
                                        },
                                        ClientGameContext::SelectRole(r) => {
                                         r.app.chat.messages = log;
                                        },
                                        ClientGameContext::Game(g) => {
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
                    ui::draw_context(&terminal, &mut current_game_context);
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
