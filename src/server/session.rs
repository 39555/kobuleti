use anyhow::anyhow;
use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    game::{Card, Deck, MonsterDeck},
    protocol::{server::PlayerId, AsyncMessageReceiver, GamePhaseKind},
    server::{ Answer,
        details::{Stateble, StatebleItem},

    },
};

pub struct GameSession {}


use derive_more::Debug;

#[derive(Debug)]
pub enum SessionCmd {
    GetMonsters(
        #[debug(skip)]
        Answer<[Option<Card>; 2]>
        ),

    GetActivePlayer(
        #[debug(skip)]
        Answer<PlayerId>
        ),
    SetActivePlayer(
        PlayerId, 
        #[debug(skip)]
        Answer<()>
                    ),
    GetGamePhase(
        #[debug(skip)]
        Answer<GamePhaseKind>
        ),
    NextGamePhase(
        #[debug(skip)]
        Answer<GamePhaseKind>
        ),

    SetGamePhase(
        GamePhaseKind,
        #[debug(skip)]
        Answer<()>
                 ),

    DropMonster(
        Card, 
        #[debug(skip)]
        Answer<anyhow::Result<()>>
                ),
}

#[async_trait]
impl<'a> AsyncMessageReceiver<SessionCmd, &'a mut GameSessionState> for GameSession {
    async fn reduce(
        &mut self,
        msg: SessionCmd,
        state: &'a mut GameSessionState,
    ) -> anyhow::Result<()> {
        use SessionCmd as Cmd;
        match msg {
            Cmd::GetMonsters(to) => {
                let _ = to.send(state.monsters.active_items());
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
                let _ = to.send(state.monsters.drop_item(monster));
            }
        }
        Ok(())
    }
}

impl AsRef<[Card]> for Deck {
    fn as_ref(&self) -> &[Card] {
        &self.cards
    }
}
impl StatebleItem for Deck{
    type Item = Card;
}

const MONSTER_LINE_LEN: usize = 2;

pub struct GameSessionState {
    pub monsters: Stateble<Deck, MONSTER_LINE_LEN>,
    active_player: PlayerId,
    player_count: usize,
    player_cursor: usize,
    phase: GamePhaseKind, //pub to_server: UnboundedSender<ToServer> ,
}



impl GameSessionState {
    pub fn new(start_player: PlayerId) -> Self {
        GameSessionState {
            monsters: Stateble::with_items(Deck::new_monster_deck()),
            active_player: start_player,
            phase: GamePhaseKind::DropAbility,
            player_count: 2,
            player_cursor: 0,
        }
    }
    fn next_game_phase(&mut self) -> GamePhaseKind {
        self.phase = {
            match self.phase {
                GamePhaseKind::DropAbility => GamePhaseKind::SelectAbility,
                GamePhaseKind::SelectAbility => GamePhaseKind::AttachMonster,
                GamePhaseKind::AttachMonster => GamePhaseKind::Defend,
                GamePhaseKind::Defend => GamePhaseKind::DropAbility,
            }
        };
        tracing::trace!("Next game phase {:?}", self.phase);
        self.phase
    }
}

#[derive(Clone, Debug)]
pub struct GameSessionHandle {
    pub tx: UnboundedSender<SessionCmd>,
}

use crate::server::details::send_oneshot_and_wait;
impl GameSessionHandle {
    pub fn for_tx(tx: UnboundedSender<SessionCmd>) -> Self {
        GameSessionHandle { tx }
    }
    pub async fn get_monsters(&self) -> [Option<Card>; 2] {
        send_oneshot_and_wait(&self.tx, SessionCmd::GetMonsters).await
    }
    pub async fn get_active_player(&self) -> PlayerId {
        send_oneshot_and_wait(&self.tx, SessionCmd::GetActivePlayer).await
    }
    pub async fn set_active_player(&self, player: PlayerId) {
        send_oneshot_and_wait(&self.tx, |to| SessionCmd::SetActivePlayer(player, to)).await
    }
    pub async fn get_game_phase(&self) -> GamePhaseKind {
        send_oneshot_and_wait(&self.tx, SessionCmd::GetGamePhase).await
    }
    pub async fn next_game_phase(&self) -> GamePhaseKind {
        send_oneshot_and_wait(&self.tx, SessionCmd::NextGamePhase).await
    }
    pub async fn drop_monster(&self, monster: Card) -> anyhow::Result<()> {
        send_oneshot_and_wait(&self.tx, |to| SessionCmd::DropMonster(monster, to)).await
    }
    pub async fn set_game_phase(&self, phase: GamePhaseKind) {
        send_oneshot_and_wait(&self.tx, |to| SessionCmd::SetGamePhase(phase, to)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_enum(){
        let (tx, _) = tokio::sync::oneshot::channel::<Result<(), anyhow::Error>>();
        println!("{:?}", SessionCmd::DropMonster(Card::new(crate::game::Rank::Ace, crate::game::Suit::Clubs), tx )); 
        let (tx, _) = tokio::sync::oneshot::channel::<PlayerId>();
        println!("{:?}", SessionCmd::GetActivePlayer( tx ))

    }

}
