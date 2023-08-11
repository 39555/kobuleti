

use ratatui::text::{Span, Line};
use ratatui::{ 
    layout::{ Constraint, Direction, Layout, Alignment, Rect},
    widgets::{Table, Row, Cell, List, ListItem, Block, Borders, Paragraph, Wrap, Padding, Clear},
    text::Text,
    style::{Style, Modifier, Color},
    Frame,
};
use crate::game::{Card, Suit, Rank};
use crate::protocol::client::{Game, GamePhase};
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
        // TODO help message
        f.render_widget(Paragraph::new("Help [h] Scroll Chat [] Quit [q] Message [e] Select [s]"), main_layout[1]);
        
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
        Monsters(&self.monsters, self.phase).draw(f, viewport_layout[0]);
        
            
          
        let chat_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                      	Constraint::Max(31),
						Constraint::Min(6),
                    ].as_ref()
                    )
                .split(screen_layout[2]);
        
        self.app.chat.draw(f,  chat_layout[0]);

        Abilities(Suit::from(self.role), &self.abilities, self.phase).draw(f, viewport_layout [1]);

        if GamePhase::Defend == self.phase {
            let center_v_card_layout = Layout::default().direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(area.height.saturating_sub(CARD_HEIGHT).saturating_div(2)),
                    Constraint::Length(CARD_HEIGHT),
                    Constraint::Length(area.height.saturating_sub(CARD_HEIGHT).saturating_div(2)),

                ]).split(area);
            let center_h_card_layout = Layout::default().direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(area.width.saturating_sub(CARD_WIDTH).saturating_div(2)),
                    Constraint::Length(CARD_WIDTH),
                    Constraint::Length(area.width.saturating_sub(CARD_WIDTH).saturating_div(2)),

                ]).split(center_v_card_layout[1]);
            f.render_widget(Clear, center_h_card_layout[1]);
            Monster(*self.monsters.active().unwrap(), Style::default()).draw(f, center_h_card_layout[1]);
        }
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

struct Monster(Card, Style);
impl Drawable for Monster {
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
                ).block(Block::default()
                    .borders(Borders::ALL))
                    .alignment(Alignment::Center)
                    .style(self.1)
        , area);
        self.0.rank.draw(f, area);
        self.0.suit.draw(f, area);
    }
}
struct Monsters<'a>(&'a StatefulList<Option<Card>,[Option<Card>; 2]>, GamePhase);

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
                if self.1 != GamePhase::Defend || card != *self.0.active()
                    .expect("Some if the container is not empty") {
                   let empty_space = layout[1].height.saturating_sub(CARD_HEIGHT).saturating_div(2);
                   let vertical = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                          Constraint::Length(empty_space),
                          Constraint::Length(CARD_HEIGHT),
                          Constraint::Length(empty_space),
                        ].as_ref()
                        )
                        .split(layout[i]); 
                    let empty_space = layout[1].width.saturating_sub(CARD_WIDTH).saturating_div(2);
                    let horizontal = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                              Constraint::Length(empty_space),
                              Constraint::Length(CARD_WIDTH),
                              Constraint::Length(empty_space),
                            ].as_ref()
                            )
                        .split(vertical[1]);
                    Monster(card, Style::default().fg(
                                if self.0.selected.is_some_and(|s| s == i) && self.1 == GamePhase::SelectMonster {
                                    Color::Red
                                } else if self.1 != GamePhase::SelectMonster || self.0.active.unwrap() == i {
                                    Color::White
                                } else if self.0.selected.is_some_and(|s| s == i) {
                                    Color::Cyan 
                                } else {
                                    Color::DarkGray
                                })
                    ).draw(f,  horizontal[1]);
                   
            
                } 
            });
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

struct Abilities<'a>(Suit, &'a StatefulList<Option<Rank>,[Option<Rank>; 3]>, GamePhase);

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

                let empty_space = layout[i].height.saturating_sub(ABILITY_HEIGHT).saturating_div(2);
                let vertical = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                          Constraint::Length(empty_space),
                          Constraint::Length(ABILITY_HEIGHT),
                          Constraint::Length(empty_space),
                        ].as_ref()
                        )
                    .split(layout[i]);
                let empty_space = layout[i].width.saturating_sub(ABILITY_WIDTH).saturating_div(2);
                let horizontal = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                          Constraint::Length(empty_space),
                          Constraint::Length(ABILITY_WIDTH),
                          Constraint::Length(empty_space),
                        ].as_ref()
                        )
                    .split(vertical[1]);

                Ability(Card::new(ability, self.0),
                        Style::default().fg(
                            if self.1.selected.is_some_and(|s| s == i){
                                Color::Cyan 
                            } else if  ! matches!(self.2, GamePhase::SelectAbility | GamePhase::Discard) 
                            || self.1.active.unwrap() != i {
                                Color::DarkGray
                            } else {
                                Color::White
                            }
                        )
                ).draw(f,  horizontal[1]);
                
           

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
    let popup_layout = Layout::default()
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
        .split(popup_layout[position.v as usize])[position.h as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::client::App;
    use crate::client::Chat;
    use crate::input::{Inputable, InputMode};
    use crate::ui::TerminalHandle;
    use crate::protocol::{
        client::{ ClientGameContext, Connection, GamePhase }
    };
    use crate::ui;
    use std::sync::{Arc, Mutex};
    use crossterm::event::{self, Event, KeyCode};

    fn get_game<'a>(ctx: &'a mut ClientGameContext) -> &'a mut Game {
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
        let mut chat = Chat::default();
        chat.input_mode = InputMode::Editing;
        let mut game = ClientGameContext::from(Game::new( App{chat}, 
                Suit::Clubs, [Some(Rank::Six), Some(Rank::Seven), Some(Rank::Eight)], cards, 
           
        ));
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let state = Connection::new(tx, String::from("Ig"));
        ui::draw_context(&terminal, &mut game);
        loop {
            let event = event::read().expect("failed to read user input");
            match &event {
                Event::Key(key) => {
                    if let KeyCode::Char('q') = key.code {
                        break;
                    }
                                    }
                _ => (),
            }
            let _ = get_game(&mut game).handle_input(&event,  &state);
            
            
            let g = get_game(&mut game);
            if g.phase == GamePhase::SelectMonster {
                if g.monsters.selected.is_some() {
                    ui::draw_context(&terminal, &mut game);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    get_game(&mut game).phase = GamePhase::Defend;
                    ui::draw_context(&terminal, &mut game);
                    continue;
                }
            }
            

            if g.phase == GamePhase::Defend {
                if let Event::Key(k) = &event {
                    if let KeyCode::Char(' ') = k.code{
                        g.phase = GamePhase::Discard;
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


