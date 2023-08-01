
use tokio::sync::mpsc::UnboundedSender;
use async_trait::async_trait;
use crate::{
    game::{Card, Deck, MonsterDeck},
    server::Answer,
    protocol::{AsyncMessageReceiver, MessageError},
};

pub struct GameSession{}

pub enum SessionCmd {
    GetMonsters(Answer<[Option<Card>; 4]>)

}

#[async_trait]
impl<'a> AsyncMessageReceiver<SessionCmd, &'a mut GameSessionState> for GameSession {
    async fn message(&mut self, 
                     msg: SessionCmd, 
                     state:  &'a mut GameSessionState) -> Result<(), MessageError>{
        match msg {
            SessionCmd::GetMonsters(to) => {
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
    pub to_session: UnboundedSender<SessionCmd>
}

use crate::server::details::oneshot_send_and_wait;
impl GameSessionHandle {
    pub fn for_tx(tx: UnboundedSender<SessionCmd>) -> Self{
        GameSessionHandle{to_session: tx}
    } 
    pub async fn get_monsters(&self) -> [Option<Card>; 4]{
        oneshot_send_and_wait(&self.to_session, |to| SessionCmd::GetMonsters(to)).await
    }
    
}



