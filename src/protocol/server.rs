use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

use crate::{
    game::{AbilityDeck, Card, Rank, Role},
    protocol::{client, RoleStatus, TurnStatus, Username},
};

pub const MAX_PLAYER_COUNT: usize = 2;
pub const ABILITY_COUNT: usize = 3;
pub const MONSTERS_PER_LINE_COUNT: usize = 2;

pub type PlayerId = SocketAddr;

// Sent by the server to give the client
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum IntroMsg {
    LoginStatus(LoginStatus),
    StartHome,
    ReconnectRoles(Option<Role>),
    ReconnectGame(client::StartGame),
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum HomeMsg {
    StartRoles(Option<Role>),
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum RolesMsg {
    SelectedStatus(Result<Role, SelectRoleError>),
    AvailableRoles([RoleStatus; Role::count()]),
    StartGame(client::StartGame),
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum GameMsg {
    DropAbility(TurnResult<Rank>),
    SelectAbility(TurnResult<Rank>),
    Attack(TurnResult<Card>),
    Defend(Option<Card>),
    Turn(TurnStatus),
    Continue(TurnResult<()>),
    UpdateGameData(([Option<Card>; 2], [Option<Rank>; 3])),
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum SharedMsg {
    Pong,
    Logout,
    ChatLog(Vec<ChatLine>),
    Chat(ChatLine),
}

use crate::server::details::StatebleItem;
impl StatebleItem for AbilityDeck {
    type Item = Rank;
}
impl AsRef<[Rank]> for AbilityDeck {
    fn as_ref(&self) -> &[Rank] {
        &self.ranks
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
pub enum SelectRoleError {
    Busy,
    AlreadySelected,
}

pub type TurnResult<T> = Result<T, Username>;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum ChatLine {
    Text(String),
    GameEvent(String),
    Connection(Username),
    Reconnection(Username),
    Disconnection(Username),
}

#[derive(PartialEq, Copy, Clone, Debug, Deserialize, Serialize)]
pub enum LoginStatus {
    Logged,
    Reconnected,
    InvalidPlayerName,
    AlreadyLogged,
    PlayerLimit,
}
