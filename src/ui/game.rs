

use ratatui::text::{Span};
use ratatui::{ 
    layout::{ Constraint, Direction, Layout, Alignment, Rect},
    widgets::{Block, Borders, Paragraph, Padding, Clear, Gauge},
    style::{Style, Modifier, Color},
    Frame,
};
use crate::game::{Card, Suit, Rank};
use crate::protocol::{GamePhaseKind, client::Game, TurnStatus};
use super::Drawable;
use super::Backend;
use crate::ui::details::{StatefulList, Statefulness};


const CARD_WIDTH : u16 = 45 + 1;
const CARD_HEIGHT: u16 = 30 + 1;

 



impl Drawable for Game {
    fn draw(&mut self, f: &mut Frame<Backend>, area: Rect){
        let main_layout = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints(
                            [
                                Constraint::Percentage(99),
                                Constraint::Length(1),
                            ]
                            .as_ref(),
                        )
                        .split(area);


        use crate::input::{GAME_KEYS, CHAT_KEYS, MAIN_KEYS, MainCmd, InputMode};
        use crate::ui::{KeyHelp, DisplayAction};
        
        macro_rules! help {
            ($iter: expr) => {
                KeyHelp($iter.chain(
                        MAIN_KEYS.iter().filter(|(_, cmd)| *cmd != MainCmd::NextContext)
                        .map(|(k, cmd)| Span::from(DisplayAction(k, *cmd))) 
                     ))
                    .draw(f, main_layout[1])
            }
        }
        match self.app.chat.input_mode {
            InputMode::Editing => { 
                help!(CHAT_KEYS.iter().map(|(k, cmd)| 
                         Span::from(DisplayAction(k , *cmd)))
                      );
        }
            InputMode::Normal =>  {
                help!(GAME_KEYS.iter().map(|(k, cmd)| 
                         Span::from(DisplayAction(k , 
                                    DisplayGameAction(*cmd, self.phase)))
                         )
                    );
            }

        };
        
       let screen_layout = Layout::default()
				.direction(Direction::Horizontal)
				.constraints(
					[
						Constraint::Percentage(65),
						Constraint::Percentage(3),
						Constraint::Percentage(30),
					]
					.as_ref(),
				)
				.split(main_layout[0]);
        let viewport_layout = Layout::default()
	        .direction(Direction::Vertical)
				.constraints(
					[
						Constraint::Max(31),
						Constraint::Min(6),
					]
					.as_ref(),
				)
				.split(screen_layout[0]);
        
            
          
        let chat_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                      	Constraint::Max(31),
						Constraint::Min(6),
                    ].as_ref()
                    )
                .split(screen_layout[2]);
        
        self.app.chat.draw(f,  chat_layout[0]);
        // TODO username
        Hud::new("Ig", (self.health, 36)).draw(f, chat_layout[1]);

        Abilities(self.role, &self.abilities, self.phase).draw(f, viewport_layout [1]);
        Monsters(&self.monsters, self.phase, self.attack_monster).draw(f, viewport_layout[0]);

    }
}

use crate::input::GameCmd;
pub struct DisplayGameAction(pub GameCmd, pub TurnStatus);
impl TryFrom<DisplayGameAction> for &'static str {
    type Error = ();
    fn try_from(value: DisplayGameAction) -> Result<Self, Self::Error> {
        use GameCmd as Cmd;
        use GamePhaseKind as Phase;
        match value.1 {
            TurnStatus::Wait => Err(()),
            TurnStatus::Ready(phase) => {
                Ok(
                    match (value.0, phase) {
                       (Cmd::SelectPrev, Phase::DropAbility | Phase::SelectAbility | Phase::AttachMonster) => "SelectPrev",
                       (Cmd::SelectNext, Phase::DropAbility | Phase::SelectAbility | Phase::AttachMonster) => "SelectNext",
                       (Cmd::ConfirmSelected, Phase::DropAbility) => "DropAbility",
                       (Cmd::ConfirmSelected, Phase::SelectAbility) => "SelectAbility",
                       (Cmd::ConfirmSelected, Phase::AttachMonster) => "Attack",
                       (Cmd::ConfirmSelected, Phase::Defend) => "Continue",
                       (Cmd::EnterChat, _) => "Chat",
                       _ => return Err(()),
                    }
                )
            }
        }
    }
}


struct Hud<'a>{
    username: &'a str,
    health: (u16, u16),
}
impl<'a> Hud<'a> {
    fn new(username: &'a str, health: (u16, u16) ) -> Self {
        Hud{
            username, health
        }
    }
}
impl<'a> Drawable for Hud<'a> {
    fn draw(&mut self,  f: &mut Frame<Backend>, area: Rect){
        let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage(30),
                Constraint::Percentage(30),
                Constraint::Percentage(30),
            ]
            .as_ref(),
        )
        .split(area);
        
        f.render_widget(Paragraph::new(self.username)
                        .alignment(Alignment::Center)
        , layout[0]);
        let gauge = Gauge::default()
            .block(Block::default())//.title("Health").borders(Borders::NONE))
            .gauge_style(Style::default().bg(Color::DarkGray))//.add_modifier(Modifier::REVERSED))//.bg(Color::Cyan))
            .percent(self.health.0/100 * self.health.1)
            .label(Span::styled(
                format!("🤍 {}/{}", self.health.0, self.health.1),
                Style::default()
                .fg(Color::White)//.bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)));
        f.render_widget(gauge, Block::default().padding(Padding::new(4, 4, 1, 1)).inner(layout[1]));
    }
}

macro_rules! include_file_by_rank_and_suit {
    (from $folder:literal match $rank:expr => { $($rank_t:ident)* }, $suit:expr => $suit_tuple:tt ) => {
        match $rank {
           $( 
               Rank::$rank_t => {
                       include_file_by_rank_and_suit!(@repeat_suit from $folder match $suit => $rank_t $suit_tuple)
               },
            )*
        }
    };
    (@repeat_suit from $folder:literal match $suit:expr =>  $rank_t:ident { $($suit_t:ident)* }) => {
        match $suit {
            $(
                Suit::$suit_t => include_str!(concat!("../assets/", $folder, "/",
                                            stringify!($rank_t), "_", stringify!($suit_t), ".txt")),
            )*
        }
    };
} 

struct Monster<'a>(Card, Option<Block<'a>>, Style);
impl<'a> Drawable for Monster<'a> {
    fn draw(&mut self,  f: &mut Frame<Backend>, area: Rect){
         f.render_widget( Paragraph::new(
                include_file_by_rank_and_suit!{from "monsters"
                    match self.0.rank => {
                        Six   
                        Seven 
                        Eight
                        Nine  
                        Ten   
                        Jack  
                        Queen
                        King 
                        Ace
                    },  self.0.suit => {
                        Hearts 
                        Diamonds 
                        Clubs 
                        Spades
                    }
                }   
                ).block(self.1.take().unwrap_or_default())
                    .alignment(Alignment::Center)
                    .style(self.2)
        , area);
        self.0.rank.draw(f, area);
        self.0.suit.draw(f, area);
    }
}
struct Monsters<'a>(&'a StatefulList<Option<Card>,[Option<Card>; 2]>, TurnStatus, Option<usize> /*attack monster*/);

impl<'a> Drawable for Monsters<'a> {
    fn draw(&mut self,  f: &mut Frame<Backend>, area: Rect){
        let layout = Layout::default()
				.direction(Direction::Horizontal)
				.constraints([
						Constraint::Percentage(50),
						Constraint::Percentage(50),
					].as_ref())
				.split(area);

        for (i, card) in self.0.items.iter().enumerate() {
            card.map(|card| {
                if self.1.is_ready_and(|p| p != GamePhaseKind::Defend) || self.2.is_some_and(|i| card != self.0.items[i]
                                                  .expect("Attack monster must be Some")) {
                    let pad_v  = layout[i].height.saturating_sub(CARD_HEIGHT).saturating_div(2);
                    let pad_h = layout[i].width.saturating_sub(CARD_WIDTH).saturating_div(2);
                    Monster(card, 
                            Some(Block::default().borders(Borders::ALL)),
                            Style::default().fg(
                                if self.0.selected.is_some_and(|s| s == i) && self.1.is_ready_and(|p| p == GamePhaseKind::AttachMonster) {
                                    Color::Red
                                } else if self.1.is_ready_and(|p| p != GamePhaseKind::AttachMonster) || self.0.active.unwrap() == i {
                                    Color::White
                                } else if self.0.selected.is_some_and(|s| s == i) {
                                    Color::Cyan 
                                } else {
                                    Color::DarkGray
                                })
                    ).draw(f, Block::default()
                           .padding(Padding::new(pad_h,pad_h, pad_v, pad_v ))
                           .inner(layout[i])
                    );
                   
            
                } 
            });
        }

        // big centered attack monster
        if self.1.is_ready_and(|p| p == GamePhaseKind::Defend) && self.2.is_some() {

            let pad_v = f.size().height.saturating_sub(CARD_HEIGHT).saturating_div(2);
            let pad_h = f.size().width.saturating_sub(CARD_WIDTH).saturating_div(2);
            let area = Block::default()
                           .padding(Padding::new(pad_h, pad_h, pad_v, pad_v ))
                           .inner(f.size());
            f.render_widget(Clear, area);

            Monster(self.0.items[self.2.unwrap()].unwrap(), 
                    Some(Block::default().borders(Borders::ALL).title("You Attacked by ...!")),
                    Style::default(),
            ).draw(f, area);
        }

        
    }
}

struct Ability(Card, Style);
impl Drawable for Ability {
    fn draw(&mut self, f: &mut Frame<Backend>, area: Rect){
        f.render_widget( Paragraph::new(
                include_file_by_rank_and_suit!(from "abilities"
                    match self.0.rank => {
                            Six   
                            Seven 
                            Eight
                            Nine  
                            Ten   
                            Jack  
                            Queen
                            King
                            Ace
                    }, self.0.suit => {
                            Hearts 
                            Diamonds 
                            Clubs 
                            Spades
                    }
                )   
            ).block(Block::default()
                    .borders(Borders::NONE))
                    .alignment(Alignment::Center).style(self.1)
        , area);
        let rect = rect_for_card_sign(area,  SignPosition::new(
                    VerticalPosition::Bottom,
                    HorizontalPosition::Left
                ), );
        f.render_widget(Paragraph::new(String::from(self.0.rank)), rect);
    }
}

struct Abilities<'a>(Suit, &'a StatefulList<Option<Rank>,[Option<Rank>; 3]>, TurnStatus);

impl<'a> Drawable for Abilities<'a> {
    fn draw(&mut self, f: &mut Frame<Backend>, area: Rect){
        let layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                      Constraint::Percentage(33),
                      Constraint::Percentage(33),
                      Constraint::Percentage(33),
                    ].as_ref()
                    )
                .split(area);
        for (i, ability) in self.1.items.iter().enumerate() {
            ability.map(|ability| {
                const ABILITY_WIDTH : u16 = 20;
                const ABILITY_HEIGHT: u16 = 9;
                let pad_v = layout[i].height.saturating_sub(ABILITY_HEIGHT).saturating_div(2);
                let pad_h = layout[i].width.saturating_sub(ABILITY_WIDTH).saturating_div(2);
                Ability(Card::new(ability, self.0),
                        Style::default().fg(
                            if self.1.selected.is_some_and(|s| s == i){
                                Color::Cyan 
                            } else if  ! matches!(self.2, TurnStatus::Ready(GamePhaseKind::SelectAbility)
                                                  | TurnStatus::Ready(GamePhaseKind::DropAbility)) 
                            || self.1.active.unwrap() != i {
                                Color::DarkGray
                            } else {
                                Color::White
                            }
                        )
                ).draw(f,  Block::default()
                           .padding(Padding::new(pad_h,pad_h, pad_v, pad_v))
                           .inner(layout[i])
                );
                
           

            });
        }
    }


}


#[derive(Clone, Copy)]
enum VerticalPosition {
    Top = 1,
    Bottom = 3,
}
#[derive(Clone, Copy)]
enum HorizontalPosition{
    Right = 3,
    Left = 1,
}
#[derive(Clone, Copy)]
struct SignPosition {
    v: VerticalPosition,
    h: HorizontalPosition
}

fn draw_sign(what: Paragraph<'_>, p: SignPosition , card_area: Rect, f: &mut Frame<Backend>){
    let area = rect_for_card_sign(card_area, p);
    f.render_widget(what, area);
}

impl Drawable for Rank {
    fn draw(&mut self,  f: &mut Frame<Backend>, area: Rect){
        [SignPosition::new(
            VerticalPosition::Top,
            HorizontalPosition::Left
            )
        , SignPosition::new(
              VerticalPosition::Bottom
            , HorizontalPosition::Right
            )]
            .iter()
            .for_each(|p| draw_sign(Paragraph::new(String::from(*self)), *p, area, f));

    }
}
impl Drawable for Suit {
    fn draw(&mut self,  f: &mut Frame<Backend>, area: Rect) {
        [
         SignPosition::new(
            VerticalPosition::Top,
            HorizontalPosition::Right
        ), 
         SignPosition::new(
            VerticalPosition::Bottom,
            HorizontalPosition::Left
            )]
            .iter()
            .for_each(|p| draw_sign(Paragraph::new(char::from(*self).to_string()), *p, area, f));
    }
}

impl SignPosition {
    fn new(v: VerticalPosition, h: HorizontalPosition) -> Self {
        SignPosition{v, h}
    }
}
fn rect_for_card_sign(area: Rect, position: SignPosition) -> Rect {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(area.height.saturating_sub(4)),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(area);

      Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Length(area.width.saturating_sub(6)),
                Constraint::Length(1),
                Constraint::Length(2),
            ]
            .as_ref(),
        )
        .split(layout[position.v as usize])[position.h as usize]
}





#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::client::App;
    use crate::client::Chat;
    use crate::input::{Inputable, InputMode};
    use crate::ui::TerminalHandle;
    use crate::protocol::{ GamePhaseKind, 
        client::{ ClientGameContext, Connection }}
    ;
    use crate::ui;
    use std::sync::{Arc, Mutex};
    use crossterm::event::{self, Event, KeyCode};
    use tokio_util::sync::CancellationToken;

    fn get_game(ctx: &mut ClientGameContext) -> &mut Game {
        <&mut Game>::try_from(ctx).unwrap() 
    }
    #[test]
    fn  show_game_layout() {
        let terminal = Arc::new(Mutex::new(TerminalHandle::new()
                                .expect("Failed to create a terminal for game")));
        TerminalHandle::chain_panic_for_restore(Arc::downgrade(&terminal));
        let cards = [
            Some(Card::new(Rank::Queen, Suit::Diamonds)),
            Some(Card::new(Rank::Eight, Suit::Diamonds)),
        ];
        let chat = Chat{ 
            input_mode : InputMode::Editing, ..Default::default()
        };
        let mut game = ClientGameContext::from(Game::new( App{chat}, 
                Suit::Clubs, [Some(Rank::Six), Some(Rank::Seven), Some(Rank::Eight)], cards, 
           
        ));
        let cancel = CancellationToken::new();
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let state = Connection::new(tx, String::from("Ig"), cancel);
        ui::draw_context(&terminal, &mut game);
        loop {
            let event = event::read().expect("failed to read user input");
            if let Event::Key(key) = &event {
                 if let KeyCode::Char('q') = key.code {
                     break;
                 }
             }
            let _ = get_game(&mut game).handle_input(&event,  &state);
            
            
            let g = get_game(&mut game);
            if g.phase == TurnStatus::Ready(GamePhaseKind::AttachMonster) && g.monsters.selected.is_some() {
                ui::draw_context(&terminal, &mut game);
                std::thread::sleep(std::time::Duration::from_secs(1));
                get_game(&mut game).phase = TurnStatus::Ready(GamePhaseKind::Defend);
                get_game(&mut game).attack_monster = Some(0);
                ui::draw_context(&terminal, &mut game);
                continue;
            }
            

            if g.phase == TurnStatus::Ready(GamePhaseKind::Defend) {
                if let Event::Key(k) = &event {
                    if let KeyCode::Char(' ') = k.code{
                        g.phase = TurnStatus::Ready(GamePhaseKind::DropAbility);
                        g.abilities.items.iter_mut().filter(|i| i.is_none()).for_each(|i| {
                            *i =  Some(Rank::Six);
                        });
                    }
                }

            }
            ui::draw_context(&terminal, &mut game);

        }
    }
}


