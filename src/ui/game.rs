

use ratatui::text::{Span, Line};
use ratatui::{ 
    layout::{ Constraint, Direction, Layout, Alignment, Rect},
    widgets::{Table, Row, Cell, List, ListItem, Block, Borders, Paragraph, Wrap, Padding},
    text::Text,
    style::{Style, Modifier, Color},
    Frame,
};
use crate::game::{Card, Suit, Rank};
use crate::protocol::client::Game;
use super::Drawable;
use super::Backend;

impl Drawable for Game {
    fn draw(&mut self, f: &mut Frame<Backend>, area: Rect){
        let main_layout = Layout::default()
				.direction(Direction::Vertical)
				.constraints(
					[
						Constraint::Max(31),
						Constraint::Min(5),
						Constraint::Max(1),
					]
					.as_ref(),
				)
				.split(area);
        // TODO help message
        f.render_widget(Paragraph::new("Help [h] Scroll Chat [] Quit [q] Message [e] Select [s]"), main_layout[2]);

       let viewport_chunks = Layout::default()
				.direction(Direction::Horizontal)
				.constraints(
					[
						Constraint::Percentage(25),
						Constraint::Percentage(25),
                        Constraint::Percentage(25),
                        Constraint::Percentage(25),
					]
					.as_ref(),
				)
				.split(main_layout[0]);
            for (i, m) in self.monsters.iter_mut().rev()
                .filter(|m| m.is_some()).map(|m| m.as_mut().unwrap()).enumerate() {
                 m.draw(f, viewport_chunks[i]);
            }
          
        let b_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                      Constraint::Percentage(60)
                    , Constraint::Percentage(40)
                    ].as_ref()
                    )
                .split(main_layout[1]);
        
          self.app.chat.draw(f,  b_layout[1]);


         let inventory_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                     Constraint::Percentage(10)
                      ,Constraint::Percentage(30)
                      ,Constraint::Percentage(30)
                    , Constraint::Percentage(30)
                    ].as_ref()
                    )
                .split(b_layout[0]);

          for (i, a) in self.abilities.iter().filter(|a| a.is_some() ).map(|a| a.unwrap()).enumerate()  {

            f.render_widget(Block::default().borders(Borders::ALL), inventory_layout[i+1]);

            let rect = rect_for_card_sign(inventory_layout[i+1],  SignPosition::new(
            VerticalPosition::Bottom,
            HorizontalPosition::Left
        ), );
            f.render_widget(Clear, rect); //this clears out the background
            f.render_widget(Paragraph::new(char::from(a).to_string()), rect);
            

          }
    }
}


macro_rules! include_file_by_rank {
    (match $rank:expr => { $($rank_t:ident)* }, $suit:expr => $suit_tuple:tt ) => {
        match $rank {
           $( 
               Rank::$rank_t => {
                       include_file_by_rank!(@repeat_suit $suit => $rank_t $suit_tuple)
               },
            )*
        }
    };
    (@repeat_suit $suit:expr =>  $rank_t:ident { $($suit_t:ident)* }) => {
        match $suit {
            $(
                Suit::$suit_t => include_str!(concat!("../assets/monsters/",
                                            stringify!($rank_t), "_", stringify!($suit_t), ".txt")),
            )*
        }
    };
}
use ratatui::widgets::Clear;

impl Drawable for Card {
    fn draw(&mut self,  f: &mut Frame<Backend>, area: Rect){
        f.render_widget( Paragraph::new(
            include_file_by_rank!(
                match self.rank => {
                        Two 
                        Three
                        Four 
                        Five  
                        Six   
                        Seven 
                        Eight
                        Nine  
                        Ten   
                        Jack  
                        Queen
                        King 
                }, self.suit => {
                        Hearts 
                        Diamonds 
                        Clubs 
                        Spades
                }
            )   
        ).block(Block::default().borders(Borders::ALL)).alignment(Alignment::Center), area);
        self.rank.draw(f, area);
        self.suit.draw(f, area);
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
    f.render_widget(Clear, area); //this clears out the background
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
            .for_each(|p| draw_sign(Paragraph::new(char::from(*self).to_string()), *p, area, f));

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
                Constraint::Length(area.height-4),
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
                Constraint::Length(area.width-6),
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
    use crate::ui::TerminalHandle;
    use crate::protocol::client::ClientGameContext;
    use crate::game::Role;
    use crate::ui;
    use std::sync::{Arc, Mutex};
    use crossterm::event::{self, Event, KeyCode};

    #[test]
    fn  show_game_layout() {
        let terminal = Arc::new(Mutex::new(TerminalHandle::new()
                                .expect("Failed to create a terminal for game")));
        TerminalHandle::chain_panic_for_restore(Arc::downgrade(&terminal));
        let cards = [
            Some(Card::new(Rank::Two, Suit::Diamonds)),
            Some(Card::new(Rank::Five, Suit::Spades)),
            Some(Card::new(Rank::Two, Suit::Clubs)),
            Some(Card::new(Rank::Two, Suit::Spades))
        ];
        let mut game = ClientGameContext::from(Game{monsters: cards, 
            app: App{chat: Chat::default()}, 
            role: Role::Mage, 
            abilities: [Some(Rank::Two), Some(Rank::Four), Some(Rank::Seven)]
        });

        loop {
            ui::draw_context(&terminal, &mut game);
            if let Event::Key(key) = event::read().expect("failed to read user input") {
                if let KeyCode::Char('q') = key.code {
                    break;
                }
            }
        }
    }
}


