use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    game::{Card, Deck, MonsterDeck},
    protocol::{server::PlayerId, AsyncMessageReceiver, GamePhaseKind},
    server::{
        details::{Stateble, StatebleItem, EndOfItems},
        Answer, ServerCmd, ServerHandle,
    },
};

#[derive(Default)]
pub struct GameSession {}

use derive_more::Debug;

pub fn spawn_session(
    players: [PlayerId; MAX_PLAYER_COUNT],
    server: ServerHandle,
) -> GameSessionHandle {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SessionCmd>();
    tokio::spawn(async move {
        let mut state = GameSessionState::new(players, server);
        let mut session = GameSession::default();
        loop {
            if let Some(cmd) = rx.recv().await {
                if let Err(e) = session.reduce(cmd, &mut state).await {
                    tracing::error!("Failed to process SessionCmd : {}", e);
                }
            }
        }
    });
    GameSessionHandle::for_tx(tx)
}

#[derive(Debug)]
pub enum SessionCmd {
    GetMonsters(#[debug(skip)] Answer<[Option<Card>; 2]>),
    // TODO EOF handling custom error
    NextMonsters(#[debug(skip)] Answer<Result<(), EndOfItems>>),
    GetActivePlayer(#[debug(skip)] Answer<PlayerId>),
    GetGamePhase(#[debug(skip)] Answer<GamePhaseKind>),
    DropMonster(Card, #[debug(skip)] Answer<anyhow::Result<()>>),
    Continue(#[debug(skip)] Answer<()>),
    SwitchToNextPlayer(#[debug(skip)] Answer<PlayerId>),
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

            Cmd::GetActivePlayer(to) => {
                let _ = to.send(state.players.active_items()[0].unwrap());
            }

            Cmd::GetGamePhase(to) => {
                let _ = to.send(state.phase);
            }

            SessionCmd::DropMonster(monster, to) => {
                let _ = to.send(state.monsters.drop_item(monster));
            }
            SessionCmd::SwitchToNextPlayer(tx) => {
                let _ = tx.send(state.switch_to_next_player());
            }
            SessionCmd::NextMonsters(tx) => {
                let _ = tx.send(state.monsters.next_actives());
            }
            SessionCmd::Continue(tx) => {
                // TODO end of game here
               
                let _ = tx.send(());

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
impl StatebleItem for Deck {
    type Item = Card;
}

const MONSTER_LINE_LEN: usize = 2;

use crate::protocol::server::MAX_PLAYER_COUNT;
impl StatebleItem for [PlayerId; MAX_PLAYER_COUNT] {
    type Item = PlayerId;
}

pub struct GameSessionState {
    pub monsters: Stateble<Deck, MONSTER_LINE_LEN>,
    players: Stateble<[PlayerId; MAX_PLAYER_COUNT], 1>,
    phase: GamePhaseKind,
    pub server: ServerHandle,
}


impl GameSessionState {
    pub fn new(mut players: [PlayerId; MAX_PLAYER_COUNT], server: ServerHandle) -> Self {
        use rand::{seq::SliceRandom, thread_rng};
        players.shuffle(&mut thread_rng());
        GameSessionState {
            monsters: Stateble::with_items(Deck::new_monster_deck()),
            players: Stateble::with_items(players),
            phase: GamePhaseKind::DropAbility,
            server,
        }
        /*
        let random = session.random_player();
        session.players.actives = [ActiveState::Enable(
            session
                .players
                .items
                .iter()
                .position(|i| *i == random)
                .expect("Must exists"),
        )];
        session
        */
    }

    /*
    fn random_player(&mut self) -> PlayerId {
        use rand::seq::IteratorRandom;
        *self
            .players
            .items
            .iter()
            .choose(&mut rand::thread_rng())
            .expect("players.len() > 0")
    }
    */

    fn switch_to_next_player(&mut self) -> PlayerId {
        match self.phase {
            GamePhaseKind::DropAbility => {
                // TODO errorkind
                tracing::info!("current active {:?}", self.players.active_items()[0]);
                self.players
                    .drop_item(self.players.active_items()[0].unwrap())
                    .expect("Must drop");
                let _ = self.players.next_actives().map_err(|eof| {
                    self.phase = GamePhaseKind::SelectAbility;
                    self.players.repeat_after_eof(eof)
                });
                tracing::info!("next active {:?}", self.players.active_items()[0]);
            }
            GamePhaseKind::SelectAbility => {
                self.phase = GamePhaseKind::AttachMonster;
            }
            GamePhaseKind::AttachMonster => {
                self.players
                    .drop_item(self.players.active_items()[0].unwrap())
                    .expect("Must drop");
                self.phase = self.players.next_actives().map_or_else(
                    |eof| {
                        self.players.repeat_after_eof(eof);
                        GamePhaseKind::Defend
                    },
                    |_| GamePhaseKind::SelectAbility,
                );
            }
            GamePhaseKind::Defend => {
                self.players
                    .drop_item(self.players.active_items()[0].unwrap())
                    .expect("Must drop");
                let _ = self.players.next_actives().map_err(|eof| {
                    // handle game end here?
                    tracing::info!("Next cycle");
                    let _ = self.monsters.next_actives();
                    tracing::info!("next monsters {:?}", self.monsters.active_items());
                    self.phase = GamePhaseKind::DropAbility;
                    self.players.repeat_after_eof(eof);
                });
            }
        };
        self.players.active_items()[0].expect("Not use disable system. Always Some")
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
    pub async fn get_game_phase(&self) -> GamePhaseKind {
        send_oneshot_and_wait(&self.tx, SessionCmd::GetGamePhase).await
    }
    pub async fn drop_monster(&self, monster: Card) -> anyhow::Result<()> {
        send_oneshot_and_wait(&self.tx, |to| SessionCmd::DropMonster(monster, to)).await
    }
    pub async fn switch_to_next_player(&self) -> PlayerId {
        send_oneshot_and_wait(&self.tx, |tx| SessionCmd::SwitchToNextPlayer(tx)).await
    }
    pub async fn next_monsters(&self) -> Result<(), EndOfItems> {
        send_oneshot_and_wait(&self.tx, |tx| SessionCmd::NextMonsters(tx)).await
    }
    pub async fn continue_game_phases(&self) {
        send_oneshot_and_wait(&self.tx, |tx| SessionCmd::Continue(tx)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_enum() {
        let (tx, _) = tokio::sync::oneshot::channel::<Result<(), anyhow::Error>>();
        println!(
            "{:?}",
            SessionCmd::DropMonster(
                Card::new(crate::game::Rank::Ace, crate::game::Suit::Clubs),
                tx
            )
        );
        let (tx, _) = tokio::sync::oneshot::channel::<PlayerId>();
        println!("{:?}", SessionCmd::GetActivePlayer(tx))
    }
}
