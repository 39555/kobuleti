use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context as _};
use futures::{SinkExt, StreamExt};
use ratatui::widgets::ScrollbarState;
use tokio::net::TcpStream;
use tokio_util::{
    codec::{FramedRead, FramedWrite, LinesCodec},
    sync::CancellationToken,
};
use tracing::{error, info, warn};
use tui_input::Input;

use super::{
    input::{InputMode, Inputable},
    ui::{
        self,
        details::{StatefulList, Statefulness},
        TerminalHandle,
    },
};
use crate::{
    game::{Card, Rank, Role, Suit},
    protocol::{
        client, client::RoleStatus, encode_message, server, GameContext, MessageDecoder,
        MessageReceiver, Msg, SendSocketMessage, TurnStatus, Username, With,
    },
};

impl SendSocketMessage for Intro {
    type Msg = Msg<client::SharedMsg, client::IntroMsg>;
}
impl SendSocketMessage for Home {
    type Msg = Msg<client::SharedMsg, client::HomeMsg>;
}
impl SendSocketMessage for Roles {
    type Msg = Msg<client::SharedMsg, client::RolesMsg>;
}
impl SendSocketMessage for Game {
    type Msg = Msg<client::SharedMsg, client::GameMsg>;
}

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
// state machine
pub struct Context<C> {
    pub username: Username,
    pub chat: Chat,
    pub state: C,
}

#[derive(Debug)]
pub struct Intro {
    pub status: Option<server::LoginStatus>,
}
impl Default for Intro {
    fn default() -> Self {
        Intro { status: None }
    }
}

#[derive(Debug, Default)]
pub struct Home {}
#[derive(Debug)]
pub struct Roles {
    pub roles: StatefulList<RoleStatus, [RoleStatus; 4]>,
}
impl Default for Roles {
    fn default() -> Self {
        Roles {
            roles: StatefulList::with_items(Role::all().map(RoleStatus::Available)),
        }
    }
}
#[derive(Debug)]
pub struct Game {
    pub role: Suit,
    pub phase: TurnStatus,
    pub attack_monster: Option<usize>,
    pub health: u16,
    pub abilities: StatefulList<Option<Rank>, [Option<Rank>; 3]>,
    pub monsters: StatefulList<Option<Card>, [Option<Card>; 2]>,
}
impl Game {
    pub fn new(role: Suit, abilities: [Option<Rank>; 3], monsters: [Option<Card>; 2]) -> Self {
        Game {
            role,
            attack_monster: None,
            health: 36,
            phase: TurnStatus::Wait,
            abilities: StatefulList::with_items(abilities),
            monsters: StatefulList::with_items(monsters),
        }
    }
}
use crate::protocol::details::impl_GameContextKind_from_state;
impl_GameContextKind_from_state! {Intro Home Roles Game}

macro_rules! done {
    ($option:expr) => {
        match $option {
            None => return Ok(()),
            Some(x) => x,
        }
    };
}

#[derive(Debug)]
#[repr(transparent)]
pub struct ClientGameContext(pub GameContext<Intro, Home, Roles, Game>);

pub async fn run(
    username: Username,
    mut stream: TcpStream,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let (r, w) = stream.split();
    let terminal = Arc::new(Mutex::new(
        TerminalHandle::new().context("Failed to create a terminal for the game")?,
    ));
    TerminalHandle::chain_panic_for_restore(Arc::downgrade(&terminal));
    let mut io = ClientIO {
        writer: FramedWrite::new(w, LinesCodec::new()),
        reader: MessageDecoder::new(FramedRead::new(r, LinesCodec::new())),
        input: crossterm::event::EventStream::new(),
        terminal,
    };

    let mut context =
        GameContext::<Context<Intro>, Context<Home>, Context<Roles>, Context<Game>>::Intro(
            Context {
                username,
                chat: Chat::default(),
                state: Intro::default(),
            },
        );

    tokio::select! {
        res = async {
            loop {
                context = match context {
                    GameContext::Intro(mut i) => {
                        io.writer.send(encode_message(Msg::with(client::IntroMsg::Login(i.username.clone())))).await?;
                        match done!(run_context(&mut io, &mut i).await?){
                               GameContext::Home(_)     => GameContext::Home(Context::<Home>::from(i)),
                               GameContext::Roles(role) => GameContext::Roles(Context::<Roles>::from((i, role))),
                               GameContext::Game(start) => GameContext::Game(Context::<Game>::from((i, start))),
                               _ => unreachable!(),
                        }
                    }
                    GameContext::Home(mut h) => {
                        let role = done!(run_context(&mut io, &mut h).await?);
                        GameContext::Roles(Context::<Roles>::from((h, role)))
                    }
                    GameContext::Roles(mut r) => {
                        let start = done!(run_context(&mut io, &mut r).await?);
                        GameContext::Game(Context::<Game>::from((r, start)))

                    }
                    GameContext::Game(mut g) => {
                        run_context(&mut io, &mut g).await?;
                        return Ok(())
                    }
                }
            }
        } => {
            res
        },
        _ = cancel.cancelled() => {
            Ok(())
        }
    }
}

async fn run_context<S>(
    io: &mut ClientIO<'_>,
    visitor: &mut Context<S>,
) -> anyhow::Result<Option<<S as DataForNextState>::Type>>
where
    S: IncomingSocketMessage + SendSocketMessage + DataForNextState,
    for<'a> Context<S>: MessageReceiver<<S as IncomingSocketMessage>::Msg, &'a mut Connection<S>>
        + Inputable<State<'a> = &'a mut Connection<S>>
        + ui::Drawable,
    for<'a> <S as IncomingSocketMessage>::Msg: serde::Deserialize<'a>,
    <S as SendSocketMessage>::Msg: serde::Serialize,
{
    let (cancel, mut cancel_rx) = tokio::sync::oneshot::channel();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<<S as SendSocketMessage>::Msg>();
    let mut state = Connection::<S>::new(tx, cancel);
    ui::draw(&io.terminal, visitor);
    loop {
        tokio::select! {
            done = &mut cancel_rx => {
                return Ok(done?)
            }
            input = io.input.next() => {
                match  input {
                    None => break,
                    Some(Err(e)) => {
                        warn!("IO error on stdin: {}", e);
                    },
                    Some(Ok(event)) => {
                        visitor.handle_input(&event, &mut state)
                           .context("failed to process an input event in the current game stage")?;
                        ui::draw(&io.terminal, visitor);
                    }
                }
            }
            Some(msg) = rx.recv() => {
                io.writer.send(encode_message(msg)).await
                    .context("failed to send a message to the socket")?;
            }
            msg = async {
                if state.cancel.is_some(){
                    io.reader.next::<Msg<server::SharedMsg, <S as IncomingSocketMessage>::Msg>>().await
                } else {
                    // stop receive messages after cancel. Wait for next context
                    let () = futures::future::pending().await;
                    unreachable!();
                }
            }=> match msg {
                Some(Ok(msg)) =>{
                    match msg {
                        Msg::Shared(msg) => {
                            use server::SharedMsg;
                            match msg {
                                SharedMsg::Pong => {

                                }
                                SharedMsg::Logout =>  {
                                        break
                                },
                                SharedMsg::Chat(line) => {
                                    visitor.chat.messages.push(line);

                                }
                                SharedMsg::ChatLog(log) => {
                                    visitor.chat.messages = log;
                                }
                            }
                        }
                        Msg::State(msg) => {
                            visitor.reduce(msg, &mut state)?;
                        }
                    }
                    ui::draw(&io.terminal, visitor);
                }
                ,
                Some(Err(e)) => {
                    error!(cause = %e, "Accept failed");
                }
                None => {
                    break;
                }
            }

        }
    }

    Ok(None)
}

impl From<Context<Intro>> for Context<Home> {
    fn from(intro: Context<Intro>) -> Self {
        assert!(
            intro.state.status.is_some()
                && matches!(intro.state.status.unwrap(), server::LoginStatus::Logged),
            "A client should be logged before make a next context request"
        );
        Context::<Home> {
            username: intro.username,
            chat: intro.chat,
            state: Home::default(),
        }
    }
}
impl From<(Context<Intro>, Option<Role>)> for Context<Roles> {
    fn from((intro, role): (Context<Intro>, Option<Role>)) -> Self {
        assert!(
            intro.state.status.is_some()
                || matches!(
                    intro.state.status.unwrap(),
                    server::LoginStatus::Reconnected
                ),
            "A client should be logged before make a next context request"
        );
        Context::<Roles> {
            username: intro.username,
            chat: intro.chat,
            state: {
                let mut roles = Roles::default();
                roles.roles.selected =
                    role.and_then(|r| roles.roles.items.iter().position(|x| x.role() == r));
                roles
            },
        }
    }
}

impl From<StartGame> for Game {
    #[inline]
    fn from(start: StartGame) -> Self {
        Game::new(start.role, start.abilities, start.monsters)
    }
}

use crate::protocol::client::StartGame;
impl From<(Context<Intro>, StartGame)> for Context<Game> {
    fn from((intro, start_game): (Context<Intro>, StartGame)) -> Self {
        assert!(
            intro.state.status.is_some()
                || matches!(
                    intro.state.status.unwrap(),
                    server::LoginStatus::Reconnected
                ),
            "A client should be logged before make a next context request"
        );
        Context::<Game> {
            username: intro.username,
            chat: intro.chat,
            state: Game::from(start_game),
        }
    }
}

impl From<(Context<Home>, Option<Role>)> for Context<Roles> {
    fn from((home, role): (Context<Home>, Option<Role>)) -> Self {
        Context::<Roles> {
            username: home.username,
            chat: home.chat,
            state: {
                let mut roles = Roles::default();
                roles.roles.selected =
                    role.and_then(|r| roles.roles.items.iter().position(|x| x.role() == r));
                roles
            },
        }
    }
}
impl From<(Context<Roles>, StartGame)> for Context<Game> {
    fn from((roles, start_game): (Context<Roles>, StartGame)) -> Self {
        Context::<Game> {
            username: roles.username,
            chat: roles.chat,
            state: Game::from(start_game),
        }
    }
}

use crate::protocol::IncomingSocketMessage;

impl IncomingSocketMessage for Intro {
    type Msg = server::IntroMsg;
}
impl IncomingSocketMessage for Home {
    type Msg = server::HomeMsg;
}
impl IncomingSocketMessage for Roles {
    type Msg = server::RolesMsg;
}
impl IncomingSocketMessage for Game {
    type Msg = server::GameMsg;
}

use tokio::net::tcp::{ReadHalf, WriteHalf};
pub struct ClientIO<'a> {
    writer: FramedWrite<WriteHalf<'a>, LinesCodec>,
    reader: MessageDecoder<FramedRead<ReadHalf<'a>, LinesCodec>>,
    input: crossterm::event::EventStream,
    terminal: Arc<Mutex<TerminalHandle>>,
}

use crate::server::Answer;
pub type Tx<T> = tokio::sync::mpsc::UnboundedSender<T>;
pub type Rx<T> = tokio::sync::mpsc::UnboundedReceiver<T>;

pub trait DataForNextState {
    type Type;
}
impl DataForNextState for Intro {
    type Type = GameContext<(), (), Option<Role>, StartGame>;
}
impl DataForNextState for Home {
    type Type = Option<Role>;
}
impl DataForNextState for Roles {
    type Type = StartGame;
}
impl DataForNextState for Game {
    type Type = ();
}

pub struct Connection<S>
where
    S: SendSocketMessage + DataForNextState,
{
    pub tx: Tx<<S as SendSocketMessage>::Msg>,
    pub cancel: Option<Answer<Option<<S as DataForNextState>::Type>>>,
}
impl<S> Connection<S>
where
    S: SendSocketMessage + DataForNextState,
{
    pub fn new(
        to_socket: Tx<<S as SendSocketMessage>::Msg>,
        cancel: Answer<Option<<S as DataForNextState>::Type>>,
    ) -> Self {
        Connection {
            tx: to_socket,
            cancel: Some(cancel),
        }
    }
}

impl MessageReceiver<server::IntroMsg, &mut Connection<Intro>> for Context<Intro> {
    fn reduce(
        &mut self,
        msg: server::IntroMsg,
        state: &mut Connection<Intro>,
    ) -> anyhow::Result<()> {
        use server::IntroMsg;
        match msg {
            IntroMsg::LoginStatus(status) => {
                self.state.status = Some(status);
                use crate::protocol::server::LoginStatus;
                return match status {
                    LoginStatus::Logged => {
                        state
                            .tx
                            .send(Msg::with(client::IntroMsg::GetChatLog))
                            .expect("failed to request a chat log");
                        Ok(())
                    }
                    LoginStatus::Reconnected => {
                        info!("Reconnected!");
                        Ok(())
                    }
                    LoginStatus::InvalidPlayerName => {
                        Err(anyhow!("Invalid player name: '{}'", self.username))
                    }
                    LoginStatus::PlayerLimit => Err(anyhow!(
                        "Player '{}' has tried to login but the player limit has been reached",
                        self.username
                    )),
                    LoginStatus::AlreadyLogged => {
                        Err(anyhow!("User with name '{}' already logged", self.username))
                    }
                }
                .context("Failed to join to the game");
            }
            IntroMsg::StartHome => {
                state
                    .cancel
                    .take()
                    .unwrap()
                    .send(Some(GameContext::Home(())))
                    .map_err(|_| anyhow!("Failed done"))?;
            }
            IntroMsg::ReconnectRoles(role) => {
                state
                    .cancel
                    .take()
                    .unwrap()
                    .send(Some(GameContext::Roles(role)))
                    .map_err(|_| anyhow!("Failed done"))?;
            }
            IntroMsg::ReconnectGame(game) => {
                state
                    .cancel
                    .take()
                    .unwrap()
                    .send(Some(GameContext::Game(game)))
                    .map_err(|_| anyhow!("Failed done"))?;
            }
        }
        Ok(())
    }
}
macro_rules! game_event {
    ($self:ident.$msg:literal $(,$args:expr)*) => {
        $self.chat.messages.push(server::ChatLine::GameEvent(format!($msg, $($args,)*)))
    }
}
impl MessageReceiver<server::HomeMsg, &mut Connection<Home>> for Context<Home> {
    fn reduce(&mut self, msg: server::HomeMsg, state: &mut Connection<Home>) -> anyhow::Result<()> {
        use server::HomeMsg;
        match msg {
            HomeMsg::StartRoles(role) => {
                state
                    .cancel
                    .take()
                    .unwrap()
                    .send(Some(role))
                    .map_err(|_| anyhow!("Failed done"))?;
            }
        }
        Ok(())
    }
}

impl MessageReceiver<server::RolesMsg, &mut Connection<Roles>> for Context<Roles> {
    fn reduce(
        &mut self,
        msg: server::RolesMsg,
        state: &mut Connection<Roles>,
    ) -> anyhow::Result<()> {
        use server::RolesMsg;
        match msg {
            RolesMsg::SelectedStatus(status) => {
                // TODO may be Result  with custom error
                if let Ok(role) = status {
                    game_event!(self."You select {:?}", role);
                    self.state.roles.selected = Some(
                        self.state
                            .roles
                            .items
                            .iter()
                            .position(|r| r.role() == role)
                            .expect("Must be present in roles"),
                    )
                }
            }
            RolesMsg::AvailableRoles(roles) => {
                self.state.roles.items = roles;
            }
            RolesMsg::StartGame(start) => {
                game_event!(self."Start Game!");
                state
                    .cancel
                    .take()
                    .unwrap()
                    .send(Some(start))
                    .map_err(|_| anyhow!("Failed done"))?;
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

impl MessageReceiver<server::GameMsg, &mut Connection<Game>> for Context<Game> {
    fn reduce(
        &mut self,
        msg: server::GameMsg,
        _state: &mut Connection<Game>,
    ) -> anyhow::Result<()> {
        use server::GameMsg;

        use crate::protocol::GamePhaseKind;
        match msg {
            GameMsg::Turn(s) => {
                self.state.phase = s;
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
            GameMsg::Continue(_) => {}
            GameMsg::DropAbility(turn) => {
                turn!(self, turn => |ability|{
                    game_event!(self."You discard {:?}", ability);
                    // * self.abilities.items
                    //    .iter_mut()
                     //   .find(|r| r.is_some_and(|r| r == ability))
                     //   .expect("Must be exists") = None;
                });
            }
            GameMsg::SelectAbility(turn) => {
                turn!(self, turn => |ability| {
                    game_event!(self."You select {:?}", ability);
                    self.state.abilities.selected = Some(self.state.abilities.items
                        .iter()
                        .position(|i| i.is_some_and(|i| i == ability))
                        .expect("Must be exists"));
                    // TODO ability description

                });
            }

            GameMsg::Attack(turn) => {
                turn!(self, turn => |monster| {
                    self.state.monsters.selected = Some(
                        self.state.monsters.items
                        .iter()
                        .position(|i| i.is_some_and(|i| i == monster))
                        .expect("Must exists"));
                    game_event!(self."You attack {:?}", self.state.monsters.active().unwrap());
                    self.state.abilities.selected = None;
                    self.state.monsters.selected  = None;
                });
            }
            GameMsg::Defend(monster) => {
                match monster {
                    Some(m) => {
                        self.state.attack_monster = Some(
                            self.state
                                .monsters
                                .items
                                .iter()
                                .position(|i| i.is_some_and(|i| i == m))
                                .expect("Must be Some"),
                        );
                        game_event!(self."A {:?} Attack You!", 
                                        self.state.monsters.items[self.state.attack_monster
                                                            .expect("Must attack")]
                                                            .expect("Must be Some"));
                        game_event!(self."You get damage from {:?}", self.state.monsters.items[
                            self.state.attack_monster.expect("Must attack")]);
                    }
                    None => {
                        game_event!(self."You defend");
                    }
                };
                self.state.monsters.selected = None;
            }
            GameMsg::UpdateGameData((monsters, abilities)) => {
                self.state.monsters.items = monsters;
                self.state.abilities.items = abilities;
            }
        }

        Ok(())
    }
}
