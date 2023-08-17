use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    game::{Card, Deck, MonsterDeck},
    protocol::{server::PlayerId, AsyncMessageReceiver, GamePhaseKind},
    server::{
        Handle,
        details::{Stateble, StatebleItem, EndOfItems, api},
        Answer,  ServerHandle,
    },
};

api!{ 
    impl Handle<SessionCmd> {
        pub async fn get_monsters(&self)          -> [Option<Card>; 2];
        pub async fn get_active_player(&self)     -> PlayerId;
        pub async fn get_game_phase(&self)        -> GamePhaseKind ;
        pub async fn drop_monster(&self, monster: Card) -> anyhow::Result<()> ;
        pub async fn switch_to_next_player(&self) -> PlayerId ;
        pub async fn next_monsters(&self)         -> Result<(), EndOfItems>;
        pub async fn continue_game_cycle(&self)   -> ();
    }
}

pub type GameSessionHandle = Handle<SessionCmd>;

impl GameSessionHandle {
    pub fn for_tx(tx: UnboundedSender<SessionCmd>) -> Self {
        GameSessionHandle { tx }
    }
}



#[derive(Default)]
pub struct GameSession {}


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
            SessionCmd::ContinueGameCycle(tx) => {
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
    }

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
