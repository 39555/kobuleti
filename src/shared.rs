use serde_json;
use serde::{Serialize, Deserialize};

///Messages that Client sends to Server
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ClientMessage {
    AddPlayer(String),
    RemovePlayer,
    
}


#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ServerMessage {
    LoginStatus(LoginStatus),
    Chat(String)
    
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum LoginStatus {
    Logged,
    InvalidPlayerName,
    AlreadyLogged,
    PlayerLimit,
}






