use serde::{Deserialize, Serialize};

use crate::{
    game::{Card, Rank, Role, Suit},
    protocol::Username,
};

// Sent by the client to the server per context

#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum IntroMsg {
    Login(Username),
    GetChatLog,
    StartHome,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum HomeMsg {
    Chat(String),
    StartRoles,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum RolesMsg {
    Chat(String),
    Select(Role),
    StartGame,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum GameMsg {
    Chat(String),
    DropAbility(Rank),
    SelectAbility(Rank),
    Attack(Card),
    Continue,
}
#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum SharedMsg {
    Ping,
    Logout,
}

// Initial data for start or reconnect to the Game State
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StartGame {
    pub abilities: [Option<Rank>; 3],
    pub monsters: [Option<Card>; 2],
    pub role: Suit,
}
