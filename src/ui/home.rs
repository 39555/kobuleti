use ratatui::text::{Span, Line};
use ratatui::{ 
    layout::{ Constraint, Direction, Layout, Alignment, Rect},
    widgets::{Table, Row, Cell, List, ListItem, Block, Borders, Paragraph, Wrap, Padding},
    text::Text,
    style::{Style, Modifier, Color},
    Frame,
};
use crate::protocol::client::Home;
use super::Drawable;
use super::Backend;
use ansi_to_tui::IntoText;

impl Drawable for Home {
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
          // TODO help widget
          f.render_widget(Paragraph::new("Help [h] Scroll Chat [] Quit [q] Message [e] Select [s]"), main_layout[1]);

          let screen_chunks = Layout::default()
				.direction(Direction::Horizontal)
				.constraints(
					[
						Constraint::Percentage(70),
						Constraint::Percentage(30),
					]
					.as_ref(),
				)
				.split(main_layout[0]);
            let viewport = Paragraph::new(include_str!("../assets/onelegevil.txt").into_text().unwrap())
                .block(Block::default().borders(Borders::ALL));
            
            if false {
                let viewport_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints( [
                                    Constraint::Min(3),
                                    Constraint::Length(3),
                    ].as_ref(), ).split(screen_chunks[0]);
                 f.render_widget(Paragraph::new(
                        Line::from(vec![
                                     Span::raw("You can play. Press")
                                   , Span::styled(" <Enter> ",
                                        Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
                                     )
                                   , Span::raw("to start a game!")
                        ])), viewport_chunks[1]);
                f.render_widget(viewport, viewport_chunks[0]); 
                f.render_widget(Paragraph::new( Line::from(vec![
                                     Span::raw("Press")
                                   , Span::styled(" <Enter> ",
                                        Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
                                     )
                                   , Span::raw("to start a game!")
                        ])).block(Block::default().borders(Borders::ALL)), viewport_chunks[1]);
            } else {
                f.render_widget(viewport, screen_chunks[0]); 
            }
            self.app.chat.draw(f, screen_chunks[1]);
    }

}


