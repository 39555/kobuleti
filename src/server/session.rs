
use tokio::sync::mpsc::UnboundedSender;
use async_trait::async_trait;
use crate::{
    game::{Card, Deck, MonsterDeck},
    server::Answer,
    protocol::{AsyncMessageReceiver, MessageError},
};

pub struct GameSession{}

pub enum ToSession {
    GetMonsters(Answer<[Option<Card>; 4]>)

}

#[async_trait]
impl<'a> AsyncMessageReceiver<ToSession, &'a mut GameSessionState> for GameSession {
    async fn message(&mut self, 
                     msg: ToSession, 
                     state:  &'a mut GameSessionState) -> Result<(), MessageError>{
        match msg {
            ToSession::GetMonsters(to) => {
                let _ = to.send(*state.monsters());
            }
    
        }
        Ok(())
    }
}

pub struct GameSessionState {
    _monsters : Deck ,
    monster_line : [Option<Card>; 4],
    //pub to_server: UnboundedSender<ToServer> ,
}

impl GameSessionState {
    pub fn monsters(&self) -> &[Option<Card>; 4] {
        &self.monster_line
    }
    pub fn update_monsters(&mut self){
       self.monster_line.iter_mut().filter(|m| m.is_none() ).for_each( |m| {
           *m = self._monsters.cards.pop();
       }); 
    }
}

impl GameSessionState { 
    pub fn new() -> Self{ //tx: UnboundedSender<ToServer> ) -> Self {
        let mut s = GameSessionState{ _monsters: Deck::new_monster_deck(), monster_line: Default::default() };
        s.update_monsters();
        s
    }
}





#[derive(Clone, Debug)]
pub struct GameSessionHandle{
    pub to_session: UnboundedSender<ToSession>
}

use crate::server::details::fn_send;
use crate::server::details::fn_send_and_wait_responce;
impl GameSessionHandle {
    pub fn for_tx(tx: UnboundedSender<ToSession>) -> Self{
        GameSessionHandle{to_session: tx}
    } 
    fn_send_and_wait_responce!(
        ToSession => to_session =>
        get_monsters() -> [Option<Card>; 4];
    );
}



