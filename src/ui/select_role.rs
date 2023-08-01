
use ratatui::text::{Span, Line};
use ratatui::{ 
    layout::{ Constraint, Direction, Layout, Alignment, Rect},
    widgets::{Table, Row, Cell, List, ListItem, Block, Borders, Paragraph, Wrap, Padding},
    text::Text,
    style::{Style, Modifier, Color},
    Frame,
};
use crate::protocol::client::SelectRole;
use super::Drawable;
use super::Backend;

impl Drawable for SelectRole {
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
            let rows = self.roles.items.iter().map(|role| {
                    Row::new(
                        [Cell::from(
                            Text::from(
                                // TODO wrap and Cow
                                textwrap::fill(role.description(), (screen_chunks[0].width-4) as usize)
                                //.iter()
                                //.map(|s| Line::from(s))
                                )
                        )
                        ]).height(screen_chunks[0].height/4 as u16)//.bottom_margin(1)
                }).collect::<Vec<_>>();
           
             let t = Table::new(rows)
                    .block(Block::default().borders(Borders::ALL).title("Select Role"))
                    .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                    .highlight_symbol(">> ")
                    .widths(&[
                        Constraint::Percentage(100),
                    //    Constraint::Length(30),
                    //    Constraint::Min(10),
                    ])
                    ;
            f.render_stateful_widget(t, screen_chunks[0], &mut self.roles.state);
            self.app.chat.draw(f, screen_chunks[1]);
    }

}

