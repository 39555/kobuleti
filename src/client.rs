
use anyhow::{self, Context};
use std::net::{SocketAddr};
use std::{
    io::{self, Stdout}
    , time::Duration
    , error::Error
    };
use std::sync::{Arc, Mutex, Weak};
use std::sync::Once;

use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{self, Constraint, Direction, Layout, Alignment},
    widgets::{Block, Borders, Paragraph},
    style::{Style, Modifier, Color},
    Frame, Terminal
};
use ratatui::text::{Span, Line};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode}
    , execute
    , terminal 
};

struct TerminalHandle {
    terminal: Terminal<CrosstermBackend<io::Stdout>>
} 
impl TerminalHandle {
    fn new() -> anyhow::Result<Self> {
        let mut t = TerminalHandle{
            terminal : Terminal::new(CrosstermBackend::new(io::stdout())).context("creating terminal failed")?
        };
        t.setup_terminal().context("unable to setup terminal")?;
        Ok(t)
    }
    fn setup_terminal(&mut self) -> anyhow::Result<()> {
        terminal::enable_raw_mode().context("failed to enable raw mode")?;
        execute!(io::stdout(), terminal::EnterAlternateScreen).context("unable to enter alternate screen")?;
        self.terminal.hide_cursor().context("unable to hide cursor")

    }
    fn restore_terminal(&mut self) -> anyhow::Result<()> {
        terminal::disable_raw_mode().context("failed to disable raw mode")?;
        execute!(self.terminal.backend_mut(), terminal::LeaveAlternateScreen)
            .context("unable to switch to main screen")?;
        self.terminal.show_cursor().context("unable to show cursor")?;
        #[cfg(debug_assertions)]
        println!("terminal restored..");
        Ok(())
    }
    fn chain_panic_for_restore(terminal_handle: Weak<Mutex<TerminalHandle>>){
        static HOOK_HAS_BEEN_SET: Once = Once::new();
        HOOK_HAS_BEEN_SET.call_once(|| {
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            match terminal_handle.upgrade(){
                None => { println!("unable to upgrade a weak terminal handle while panic") },
                Some(arc) => {
                    match arc.lock(){
                        Err(e) => { 
                            println!("unable to lock terminal handle: {}", e);
                            },
                        Ok(mut t) => { 
                            let _ = t.restore_terminal().map_err(|err|{
                                println!("unaple to restore terminal while panic: {}", err);
                            }); 
                        }
                    }
                }
            }
            original_hook(panic);
        }));
        });
    }
}

impl Drop for TerminalHandle {
    fn drop(&mut self) {
        // a panic hook call its own restore before drop
        if ! std::thread::panicking(){
            if let Err(e) = self.restore_terminal().context("restore terminal failed"){
                eprintln!("Drop terminal error: {e}");
            };
        };
        #[cfg(debug_assertions)]
        println!("TerminalHandle dropped..");
    }
}

pub struct Client {
    host: SocketAddr
}

impl Client {
    pub fn new(host: SocketAddr) -> Self {
        Self { host }
    }
    pub async fn connect(&self) -> anyhow::Result<()> {
        Ok(())

    }
    pub fn main(&self) -> anyhow::Result<()> {
        let mut t = Arc::new(Mutex::new(TerminalHandle::new().context("failed to create a terminal")?));
        TerminalHandle::chain_panic_for_restore(Arc::downgrade(&t));
        waiting_room(&mut t)?;
        Ok(())
    }

}
/*
fn hello_screen(t: & TerminalHandle) -> anyhow::Result<()> {

    Ok(())
}

struct Title;
impl Title {
    // TODO embed from file
    const MAIN_TITLE: &'static str = "";
    fn dimension() -> (u16, u16) {
        (
            Self::MAIN_TITLE.find('\n').unwrap() as u16,
            Self::MAIN_TITLE.chars().filter(|&c| c == '\n').count() as u16,
        )
    }
}
*/
fn waiting_room(t: &mut Arc<Mutex<TerminalHandle>>) -> anyhow::Result<()> {
     loop {
        t.lock().unwrap().terminal.draw(|f: &mut Frame<CrosstermBackend<io::Stdout>>| {
            let block = Block::default().title("Ascension Waiting Room").borders(Borders::ALL);
            f.render_widget(block, f.size());

            let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(4)
            .constraints([
                  Constraint::Percentage(50)
                , Constraint::Percentage(45)
                , Constraint::Percentage(5)
                ].as_ref()
                )
            .split(f.size());
            let title = Paragraph::new(include_str!("assets/title"))
            .style(Style::default().add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
            f.render_widget(title, chunks[0]);

            let enter = Span::styled(
                " <Enter> ",
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
                );

            let esc = Span::styled(
                " <Esc> ",
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow),
                );
            
            let messages = //if !self.state.server.is_connected()
                //|| !self.state.server.has_compatible_version()
                {
                    vec![
                        Line::from(vec![Span::raw("Press"), enter, Span::raw("to connect to server")]),
                        Line::from(vec![Span::raw("Press"), esc, Span::raw("to exit from Ascension")]),
                    ]
                };
            /*
            else if !self.state.user.is_logged() {
                vec![
                    if self.menu.character_symbol_input.content().is_none() {
                        Spans::from(vec![Span::raw("Choose a character (an ascii uppercase letter)")])
                    }
                    else {
                        Spans::from(vec![
                            Span::raw("Press"),
                            enter,
                            Span::raw("to login with the character"),
                        ])
                    },
                    Spans::from(vec![
                        Span::raw("Press"),
                        esc,
                        Span::raw("to disconnect from the server"),
                    ]),
                ]
            }
            else if let GameStatus::Started = self.state.server.game.status {
                let waiting_secs = match self.state.server.game.next_arena_timestamp {
                    Some(timestamp) => {
                        timestamp.saturating_duration_since(Instant::now()).as_secs() + 1
                    }
                    None => 0,
                };

                let style = Style::default().fg(Color::LightCyan);

                vec![Spans::from(vec![
                    Span::styled("Starting game in ", style),
                    Span::styled(waiting_secs.to_string(), style.add_modifier(Modifier::BOLD)),
                    Span::styled("...", style),
                ])]
            }
            else {
                vec![Spans::from(vec![Span::raw("Press"), esc, Span::raw("to logout the character")])]
            };
            */
            let footer = Paragraph::new(messages).alignment(Alignment::Center);
            f.render_widget(footer, chunks[2]);

            
        })?;

        if let Event::Key(key) = event::read().context("event read failed")? {
            match key.code {
                KeyCode::Char('q') => {
                    break;
                }
                 _ => {
                    continue;
                }

            }
        }
    }

    Ok(())
}
/*
impl Notification {
    const HEIGHT: u16 = 2;
}

impl Widget for Notification {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let enter = Span::styled(
            " <Enter> ",
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
        );

        let esc = Span::styled(
            " <Esc> ",
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow),
        );

        let messages = if !self.state.server.is_connected()
            || !self.state.server.has_compatible_version()
        {
            vec![
                Spans::from(vec![Span::raw("Press"), enter, Span::raw("to connect to server")]),
                Spans::from(vec![Span::raw("Press"), esc, Span::raw("to exit from asciiarena")]),
            ]
        }
        else if !self.state.user.is_logged() {
            vec![
                if self.menu.character_symbol_input.content().is_none() {
                    Spans::from(vec![Span::raw("Choose a character (an ascii uppercase letter)")])
                }
                else {
                    Spans::from(vec![
                        Span::raw("Press"),
                        enter,
                        Span::raw("to login with the character"),
                    ])
                },
                Spans::from(vec![
                    Span::raw("Press"),
                    esc,
                    Span::raw("to disconnect from the server"),
                ]),
            ]
        }
        else if let GameStatus::Started = self.state.server.game.status {
            let waiting_secs = match self.state.server.game.next_arena_timestamp {
                Some(timestamp) => {
                    timestamp.saturating_duration_since(Instant::now()).as_secs() + 1
                }
                None => 0,
            };

            let style = Style::default().fg(Color::LightCyan);

            vec![Spans::from(vec![
                Span::styled("Starting game in ", style),
                Span::styled(waiting_secs.to_string(), style.add_modifier(Modifier::BOLD)),
                Span::styled("...", style),
            ])]
        }
        else {
            vec![Spans::from(vec![Span::raw("Press"), esc, Span::raw("to logout the character")])]
        };

        Paragraph::new(messages).alignment(Alignment::Center).render(area, buffer);
    }
}
*/


fn run(t: &mut Arc<Mutex<TerminalHandle>>) -> anyhow::Result<()> {
    loop {
        t.lock().unwrap().terminal.draw(|f| ui(f))?;
        if let Event::Key(key) = event::read().context("event read failed")? {
            match key.code {
                KeyCode::Char('p') => {
                    panic!("intentional demo panic");
                }

                KeyCode::Char('e') => {
                }

                
                KeyCode::Char('q') => {
                        break;

                }
                _ => {
                    return Ok(());
                }
            }
        }
    }
     Ok(())
}

fn ui<B: Backend>(f: &mut Frame<B>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage(10),
                Constraint::Percentage(80),
                Constraint::Percentage(10),
            ]
            .as_ref(),
        )
        .split(f.size());
    let b = Block::default()
        .title("Panic Handler Demo")
        .borders(Borders::ALL);

    let p = Paragraph::new("Hello").block(b).alignment(Alignment::Center);
    f.render_widget(p, f.size());
    let block = Block::default().title("Block").borders(Borders::ALL);
    f.render_widget(block, chunks[0]);
    let block = Block::default().title("Block 2").borders(Borders::ALL);
    f.render_widget(block, chunks[2]);
}
