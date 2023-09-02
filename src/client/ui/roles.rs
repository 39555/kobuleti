use client::states::{Context, Roles};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
    Frame,
};

use super::{details::Statefulness, Backend, Drawable};
use crate::{client, protocol::RoleStatus};

impl Drawable for Context<Roles> {
    fn draw(&mut self, f: &mut Frame<Backend>, area: Rect) {
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(99), Constraint::Length(1)].as_ref())
            .split(area);

        use client::input::{InputMode, CHAT_KEYS, MAIN_KEYS, SELECT_ROLE_KEYS};

        use super::{keys_help, DisplayAction, KeyHelp};

        match self.chat.input_mode {
            InputMode::Editing => {
                KeyHelp(
                    CHAT_KEYS
                        .iter()
                        .map(|(k, cmd)| Span::from(DisplayAction(k, *cmd)))
                        .chain(
                            MAIN_KEYS
                                .iter()
                                .map(|(k, cmd)| Span::from(DisplayAction(k, *cmd))),
                        ),
                )
                .draw(f, main_layout[1]);
            }
            InputMode::Normal => {
                keys_help!(SELECT_ROLE_KEYS).draw(f, main_layout[1]);
            }
        };

        let screen_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
            .split(main_layout[0]);

        const HEIGHT: u16 = 35;
        const WIDTH: u16 = 100;
        let pad_v = screen_chunks[0]
            .height
            .saturating_sub(HEIGHT)
            .saturating_div(2);
        let pad_h = screen_chunks[0]
            .width
            .saturating_sub(WIDTH)
            .saturating_div(2);
        let active = self.state.roles.active().expect("Always active");
        f.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Select Role - {:?}", active)),
            screen_chunks[0],
        );
        f.render_widget(
            Paragraph::new(active.role().description())
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(
                    if self.state.roles.selected().is_some_and(|s| s == active) {
                        Color::Cyan
                    } else if let RoleStatus::NotAvailable(_) = active {
                        Color::DarkGray
                    } else {
                        Color::White
                    },
                )), //.block(Block::default().borders(Borders::ALL)),
            Block::default()
                .padding(Padding::new(pad_h, pad_h, pad_v, pad_v))
                .inner(screen_chunks[0]),
        );
        self.chat.draw(f, screen_chunks[1]);
    }
}

pub struct RolesKeyHelp();
impl Drawable for RolesKeyHelp {
    fn draw(&mut self, f: &mut Frame<Backend>, area: ratatui::layout::Rect) {
        f.render_widget(
            Paragraph::new(Line::from(
                client::input::SELECT_ROLE_KEYS
                    .iter()
                    .map(|(k, cmd)| Span::from(client::ui::DisplayAction(k, *cmd)))
                    .collect::<Vec<_>>(),
            )),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crossterm::event::{self, Event, KeyCode};

    use super::*;
    use crate::{
        client::{
            input::{InputMode, Inputable},
            states::{Chat, Connection, Context, Roles},
            ui,
            ui::TerminalHandle,
        },
        protocol::Username,
    };

    #[test]
    fn show_roles_layout() {
        let terminal = Arc::new(Mutex::new(
            TerminalHandle::new().expect("Failed to create a terminal for game"),
        ));
        TerminalHandle::chain_panic_for_restore(Arc::downgrade(&terminal));

        let chat = Chat {
            input_mode: InputMode::Editing,
            ..Default::default()
        };
        let mut roles = Context::<Roles> {
            chat,
            username: Username::default(),
            state: Roles::default(),
        };
        let (cancel, _) = tokio::sync::oneshot::channel();
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let mut state = Connection::new(tx, cancel);
        ui::draw(&terminal, &mut roles);
        loop {
            let event = event::read().expect("failed to read user input");
            if let Event::Key(key) = &event {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Left => roles.state.roles.prev(),
                    KeyCode::Right => roles.state.roles.next(),
                    KeyCode::Char(' ') => {
                        roles.state.roles.selected = roles.state.roles.active;
                    }
                    _ => (),
                }
            }
            let _ = roles.handle_input(&event, &mut state);
            ui::draw(&terminal, &mut roles);
        }
    }
}
