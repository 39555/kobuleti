use anyhow::anyhow;
use anyhow::Context as _;
use tokio::sync::mpsc::UnboundedSender;
use async_trait::async_trait;
use crate::{
    game::{Card, Deck, MonsterDeck},
    server::Answer,
    protocol::{ AsyncMessageReceiver,
        server::PlayerId,
        GamePhaseKind
    },

};

pub struct GameSession{}

use ascension_macro::DisplayOnlyIdents;
use std::fmt::Display;

#[derive(Debug, DisplayOnlyIdents)]
pub enum SessionCmd {
    GetMonsters(Answer<[Option<Card>; 2]>),
    GetActivePlayer(Answer<PlayerId>),
    SetActivePlayer(PlayerId, Answer<()>),
    GetGamePhase(Answer<GamePhaseKind>),
    NextGamePhase(Answer<GamePhaseKind>),
    SetGamePhase(GamePhaseKind, Answer<()>),
    DropMonster(Card, Answer<anyhow::Result<()>>),

}

#[async_trait]
impl<'a> AsyncMessageReceiver<SessionCmd, &'a mut GameSessionState> for GameSession {
    async fn message(&mut self, 
                     msg: SessionCmd, 
                     state:  &'a mut GameSessionState) -> anyhow::Result<()> {
        use SessionCmd as Cmd;
        match msg {
            Cmd::GetMonsters(to) => {
                let _ = to.send(state.monsters());
            }
            Cmd::SetGamePhase(phase, to) => {
                state.phase = phase;
                let _ = to.send(());

            }
            Cmd::GetActivePlayer(to) => {
                let _ = to.send(state.active_player);
            }
            Cmd::SetActivePlayer(player, to) => {
                if state.active_player != player {
                    state.player_cursor += 1;
                    if state.player_cursor % state.player_count == 0 {
                        state.player_cursor = 0;
                    }
                    state.active_player = player;
                }
                let _ = to.send(());
            }
            Cmd::GetGamePhase(to) => {
                let _ = to.send(state.phase);
            }
            SessionCmd::NextGamePhase(to) => {
                let _ = to.send(state.next_game_phase());
            }
            SessionCmd::DropMonster(monster, to) => {
                let _ = to.send(state.drop_monster(monster)
                    );

            }
        }
        Ok(())
    }
}
enum MonsterStatus {
    Alive(usize),
    Killed(usize),
}
impl MonsterStatus {
    fn unwrap_index(&self) -> usize {
        match *self {
            MonsterStatus::Alive(i)  => i,
            MonsterStatus::Killed(i) => i,
        }
    }
}
// TODO cursor
pub struct GameSessionState {
    monsters : Deck ,
    active_monsters: [MonsterStatus; 2],
    //monster_line : [Option<Card>; 2],
    active_player: PlayerId,
    player_count: usize,
    player_cursor: usize,
    phase: GamePhaseKind
    //pub to_server: UnboundedSender<ToServer> ,
}

impl GameSessionState {
    pub fn monsters(&self) -> [Option<Card>; 2] {
        let mut iter = self.active_monsters.iter().map(|s| match s {
            MonsterStatus::Alive(s)  => Some(self.monsters.cards[*s]),
            MonsterStatus::Killed(_) => None,
        });
        core::array::from_fn(|_| iter.next().expect("next must exists"))

    }
   pub fn drop_monster(&mut self, monster: Card) -> anyhow::Result<()>{
        let i = self.monsters.cards.iter().position(|i| *i == monster).ok_or_else(||
            anyhow!("Monster not found")
            )?;
        *self.active_monsters.iter_mut().find(|m| m.unwrap_index() == i).ok_or_else(||
            anyhow!("Monster not active")
        )? = MonsterStatus::Killed(i);
        Ok(())
   }
}

impl GameSessionState { 
    pub fn new(start_player: PlayerId) -> Self{
        GameSessionState{ monsters: Deck::new_monster_deck(), 
        active_player: start_player, phase: GamePhaseKind::DropAbility, player_count: 2, player_cursor:0
            , active_monsters: core::array::from_fn(|i| MonsterStatus::Alive(i))

        }
    }
    fn next_game_phase(&mut self) -> GamePhaseKind{
        self.phase = {
            match self.phase {
                GamePhaseKind::DropAbility   => GamePhaseKind::SelectAbility, 
                GamePhaseKind::SelectAbility => GamePhaseKind::AttachMonster,  
                GamePhaseKind::AttachMonster => GamePhaseKind::Defend, 
                GamePhaseKind::Defend        => GamePhaseKind::DropAbility,

            }

        };
        tracing::trace!("Next game phase {:?}", self.phase);
        self.phase
    }
}


#[derive(Clone, Debug)]
pub struct GameSessionHandle{
    pub tx: UnboundedSender<SessionCmd>
}

use crate::server::details::send_oneshot_and_wait;
impl GameSessionHandle {
    pub fn for_tx(tx: UnboundedSender<SessionCmd>) -> Self{
        GameSessionHandle{tx}
    } 
    pub async fn get_monsters(&self) -> [Option<Card>; 2]{
        send_oneshot_and_wait(&self.tx, |to| SessionCmd::GetMonsters(to)).await
    }
    pub async fn get_active_player(&self) -> PlayerId {
        send_oneshot_and_wait(&self.tx, |to| SessionCmd::GetActivePlayer(to)).await
    }
    pub async fn set_active_player(&self, player: PlayerId){
        send_oneshot_and_wait(&self.tx, |to| SessionCmd::SetActivePlayer( player, to)).await
    }
    pub async fn get_game_phase(&self) -> GamePhaseKind {
        send_oneshot_and_wait(&self.tx, |to| SessionCmd::GetGamePhase(to)).await
    } 
    pub async fn next_game_phase(&self) -> GamePhaseKind{
        send_oneshot_and_wait(&self.tx, |to| SessionCmd::NextGamePhase(to)).await
    }
    pub async fn drop_monster(&self, monster: Card) -> anyhow::Result<()> {
        send_oneshot_and_wait(&self.tx, |to| SessionCmd::DropMonster(monster, to)).await
    }
    pub async fn set_game_phase(&self, phase: GamePhaseKind) {
        send_oneshot_and_wait(&self.tx, |to| SessionCmd::SetGamePhase(phase, to)).await
    }
}



