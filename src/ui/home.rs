use ansi_to_tui::IntoText;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::{Backend, Drawable};
use crate::protocol::client::Home;

impl Drawable for Home {
    fn draw(&mut self, f: &mut Frame<Backend>, area: Rect) {
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(99), Constraint::Length(1)].as_ref())
            .split(area);
        use crate::{
            input::{InputMode, MainCmd, CHAT_KEYS, HOME_KEYS, MAIN_KEYS},
            ui::{keys_help, DisplayAction, KeyHelp},
        };
        match self.app.chat.input_mode {
            InputMode::Editing => {
                KeyHelp(
                    CHAT_KEYS
                        .iter()
                        .map(|(k, cmd)| Span::from(DisplayAction(k, *cmd)))
                        .chain(
                            MAIN_KEYS
                                .iter()
                                .filter(|(_, cmd)| *cmd != MainCmd::NextContext)
                                .map(|(k, cmd)| Span::from(DisplayAction(k, *cmd))),
                        ),
                )
                .draw(f, main_layout[1]);
            }
            InputMode::Normal => {
                keys_help!(HOME_KEYS).draw(f, main_layout[1]);
            }
        };

        let screen_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
            .split(main_layout[0]);

        let viewport = Paragraph::new(
            include_str!("../assets/onelegevil.txt")
                .into_text()
                .unwrap(),
        )
        .block(Block::default().borders(Borders::ALL));

        if false {
            let viewport_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)].as_ref())
                .split(screen_chunks[0]);
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw("You can play. Press"),
                    Span::styled(
                        " <Enter> ",
                        Style::default()
                            .add_modifier(Modifier::BOLD)
                            .fg(Color::Cyan),
                    ),
                    Span::raw("to start a game!"),
                ])),
                viewport_chunks[1],
            );
            f.render_widget(viewport, viewport_chunks[0]);
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw("Press"),
                    Span::styled(
                        " <Enter> ",
                        Style::default()
                            .add_modifier(Modifier::BOLD)
                            .fg(Color::Cyan),
                    ),
                    Span::raw("to start a game!"),
                ]))
                .block(Block::default().borders(Borders::ALL)),
                viewport_chunks[1],
            );
        } else {
            f.render_widget(viewport, screen_chunks[0]);
        }
        self.app.chat.draw(f, screen_chunks[1]);
    }
}
