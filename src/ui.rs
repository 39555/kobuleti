use std::{
    io,
    sync::{Arc, Mutex, Once, Weak},
};

use anyhow::Context as _;
use crossterm::{event::KeyCode, execute};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Scrollbar, ScrollbarOrientation, Wrap},
    Frame, Terminal,
};
use tracing::{debug, error};

use crate::{
    client::Chat,
    details::dispatch_trait,
    input::InputMode,
    protocol::{
        client::{ClientGameContext, Intro},
        server::ChatLine,
        GameContext,
    },
};

pub mod details;
pub mod game;
pub mod home;
pub mod roles;
type Backend = CrosstermBackend<io::Stdout>;

pub struct TerminalHandle {
    pub terminal: Terminal<CrosstermBackend<io::Stdout>>,
}
impl TerminalHandle {
    pub fn new() -> anyhow::Result<Self> {
        let mut t = TerminalHandle {
            terminal: Terminal::new(CrosstermBackend::new(io::stdout()))
                .context("Creating terminal failed")?,
        };
        t.setup_terminal().context("Unable to setup terminal")?;
        Ok(t)
    }
    fn setup_terminal(&mut self) -> anyhow::Result<()> {
        crossterm::terminal::enable_raw_mode().context("Failed to enable raw mode")?;
        execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen)
            .context("Unable to enter alternate screen")?;
        self.terminal.hide_cursor().context("Unable to hide cursor")
    }
    fn restore_terminal(&mut self) -> anyhow::Result<()> {
        crossterm::terminal::disable_raw_mode().context("failed to disable raw mode")?;
        execute!(
            self.terminal.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen
        )
        .context("unable to switch to main screen")?;
        self.terminal
            .show_cursor()
            .context("unable to show cursor")?;
        debug!("The terminal has been restored");
        Ok(())
    }
    // invoke as `TerminalHandle::chain_panic_for_restore(Arc::downgrade(&term_handle));
    pub fn chain_panic_for_restore(this: Weak<Mutex<Self>>) {
        static HOOK_HAS_BEEN_SET: Once = Once::new();
        HOOK_HAS_BEEN_SET.call_once(|| {
            let original_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |panic| {
                match this.upgrade() {
                    None => {
                        error!("Unable to upgrade a weak terminal handle while panic")
                    }
                    Some(arc) => match arc.try_lock() {
                        Err(e) => {
                            error!("Unable to lock terminal handle: {}", e);
                        }
                        Ok(mut t) => {
                            let _ = t.restore_terminal().map_err(|err| {
                                error!("Unable to restore terminal while panic: {}", err);
                            });
                        }
                    },
                }
                original_hook(panic);
            }));
        });
    }
}

impl Drop for TerminalHandle {
    fn drop(&mut self) {
        // a panic hook call its own restore before drop
        if !std::thread::panicking() {
            if let Err(e) = self.restore_terminal().context("Restore terminal failed") {
                error!("Drop terminal error: {e}");
            };
        };
        debug!("TerminalHandle has been dropped");
    }
}

pub trait Drawable {
    fn draw(&mut self, f: &mut Frame<Backend>, area: ratatui::layout::Rect);
}

impl Drawable for ClientGameContext {
    dispatch_trait! {
        Drawable fn draw(&mut self, f: &mut Frame<Backend>, area: ratatui::layout::Rect, ) {
                GameContext =>
                            Intro
                            Home
                            Roles
                            Game
            }
    }
}

impl Drawable for Intro {
    fn draw(&mut self, f: &mut Frame<Backend>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Percentage(50),
                    Constraint::Min(15),
                    Constraint::Length(1),
                ]
                .as_ref(),
            )
            .split(area);
        let intro = Paragraph::new(include_str!("assets/intro"))
            .style(Style::default().add_modifier(Modifier::BOLD))
            .block(Block::default().padding(Padding::new(4, 4, 4, 4)))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });

        f.render_widget(intro, chunks[0]);
        let title = Paragraph::new(include_str!("assets/title"))
            .style(Style::default().add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
        f.render_widget(title, chunks[1]);

        use crate::input::MAIN_KEYS;
        KeyHelp(
            MAIN_KEYS
                .iter()
                .flat_map(|a| Vec::<Span<'_>>::from(DisplayIntroAction(&a.0, a.1))),
        )
        .draw(f, chunks[2]);
    }
}
struct DisplayIntroAction<'a>(&'a KeyEvent, MainCmd);
impl<'a> From<DisplayIntroAction<'a>> for Vec<Span<'a>> {
    fn from(value: DisplayIntroAction) -> Self {
        value
            .1
            .try_into()
            .map(|d: &'static str| {
                vec![
                    Span::raw("Press"),
                    Span::styled(
                        format!(" <{}> ", DisplayKey(value.0)),
                        Style::default()
                            .add_modifier(Modifier::BOLD)
                            .fg(Color::Cyan),
                    ),
                    Span::raw(format!("to {}  ", d)),
                ]
            })
            .unwrap_or(Vec::default())
    }
}

impl Drawable for Chat {
    fn draw(&mut self, f: &mut Frame<Backend>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)].as_ref())
            .split(area);
        let messages = self
            .messages
            .iter()
            .map(|message| {
                Line::from(match &message {
                    ChatLine::Disconnection(user) => Span::styled(
                        format!("{} has left the game", user),
                        Style::default().fg(Color::Red),
                    ),

                    ChatLine::Connection(user) => Span::styled(
                        format!("{} join to the game", user),
                        Style::default().fg(Color::Green),
                    ),

                    ChatLine::Reconnection(user) => Span::styled(
                        format!("{} reconnected", user),
                        Style::default().fg(Color::Green),
                    ),

                    ChatLine::Text(msg) => Span::styled(msg, Style::default().fg(Color::White)),

                    ChatLine::GameEvent(msg) => {
                        Span::styled(msg, Style::default().fg(Color::LightYellow))
                    }
                })
            })
            .collect::<Vec<_>>();

        self.scroll_state = self.scroll_state.content_length(messages.len() as u16);

        let messages_panel = Paragraph::new(messages)
            .block(Block::default().borders(Borders::NONE))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll as u16, 0));

        f.render_widget(messages_panel, chunks[0]);
        f.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            chunks[0],
            &mut self.scroll_state,
        );

        let input = Paragraph::new(self.input.value())
            .style(match self.input_mode {
                InputMode::Normal => Style::default().fg(Color::DarkGray),
                InputMode::Editing => Style::default(),
            })
            .block(Block::default().borders(Borders::ALL).title("Your Message"));

        f.render_widget(input, chunks[1]);

        // cursor
        //
        match self.input_mode {
            crate::input::InputMode::Normal =>
                // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
                {}
            crate::input::InputMode::Editing => {
                // Make the cursor visible and ask ratatui to put it at the specified coordinates after rendering
                f.set_cursor(
                    // Put cursor past the end of the input text
                    chunks[1].x + (self.input.visual_cursor()) as u16 + 1,
                    // Move one line down, from the border to the input line
                    chunks[1].y + 1,
                );
            }
        }
    }
}

use crossterm::event::KeyModifiers;

pub struct DisplayKey<'a>(pub &'a KeyEvent);

impl std::fmt::Display for DisplayKey<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        macro_rules! codes {
            (
                $code: expr =>
                    $($name:ident$(($field:expr))?  $fmt:expr $(,)?)*

            ) => {

                match $code {
                    $(
                        KeyCode::$name$(($field))? => $fmt,
                    )*
                    _  => todo!("This key can't display yet")
                }

            }
        }
        let buf;
        write!(
            f,
            "{}{}{}",
            match self.0.modifiers {
                KeyModifiers::NONE => "",
                KeyModifiers::CONTROL => "ctrl",
                _ => todo!("This key modifiers can't display yet"),
            },
            if KeyModifiers::NONE == self.0.modifiers {
                ""
            } else {
                "-"
            },
            match self.0.code {
                KeyCode::Char(' ') => "space",
                KeyCode::Char(c) => {
                    buf = c.to_string();
                    &buf
                }
                KeyCode::F(n) => {
                    buf = n.to_string();
                    &buf
                }
                _ => {
                    codes!(
                    self.0.code =>
                         Enter  "enter",
                         Up  "↑",
                         Down  "↓",
                         Right  "→",
                         Left  "←",
                         Esc  "esc",
                     )
                }
            }
        )
    }
}
macro_rules! str_try_from_context_cmd {
    (
        $cmd:ident {
            $($name:ident $($what:literal)? , )*
        }
    ) => {
        impl TryFrom<$cmd> for &'static str {
             type Error = ();
            fn try_from(value: $cmd) -> Result<Self, Self::Error> {
                match value {
                   $(
                       $cmd::$name => Ok(str_try_from_context_cmd!(@read_name $name => $($what)?)),

                    )*
                   $cmd::None => Err(())
                }
            }
        }
    };
    (@read_name $cmd:ident => $name:literal) => ($name);
    (@read_name $cmd:ident =>) => (stringify!($cmd));
}

use crate::input::{ChatCmd, HomeCmd, MainCmd, RolesCmd};
str_try_from_context_cmd! { MainCmd {
    NextContext "Continue",
    Quit ,
}}
str_try_from_context_cmd! { HomeCmd {
    EnterChat ,
}}
str_try_from_context_cmd! { RolesCmd {
    EnterChat  ,
    SelectPrev  ,
    SelectNext  ,
    ConfirmRole ,

}}
str_try_from_context_cmd! { ChatCmd {
    SendInput  ,
    LeaveChatInput ,
    ScrollUp   ,
    ScrollDown ,

}}

pub struct DisplayAction<'a, A: TryInto<&'a str>>(&'a KeyEvent, A);
impl<'a, A: TryInto<&'a str>> From<DisplayAction<'a, A>> for Span<'a>
where
    <A as TryInto<&'a str>>::Error: std::fmt::Debug,
{
    fn from(value: DisplayAction<'a, A>) -> Self {
        Span::styled(
            value
                .1
                .try_into()
                .map(|d| format!("[{}]{} ", DisplayKey(value.0), d))
                .unwrap_or(String::default()),
            Style::default()
                .fg(Color::White) //.bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
    }
}

use crossterm::event::KeyEvent;

pub struct KeyHelp<'a, Actions>(Actions)
where
    Actions: Iterator<Item = Span<'a>>;

macro_rules! keys_help {
    ($iter: expr) => {
        KeyHelp(
            $iter
                .iter()
                .map(|(k, cmd)| Span::from(DisplayAction(k, *cmd)))
                .chain(
                    MAIN_KEYS
                        .iter()
                        .map(|(k, cmd)| Span::from(DisplayAction(k, *cmd))),
                ),
        )
    };
}
pub(crate) use keys_help;

impl<'a, 'b, Actions> From<&'a mut KeyHelp<'b, Actions>> for Line<'a>
where
    Actions: Iterator<Item = Span<'b>> + Clone,
{
    fn from(value: &'a mut KeyHelp<'b, Actions>) -> Self {
        Line::from(value.0.clone().collect::<Vec<_>>())
    }
}

impl<'a, Actions> Drawable for KeyHelp<'a, Actions>
where
    Actions: Iterator<Item = Span<'a>> + Clone,
{
    fn draw(&mut self, f: &mut Frame<Backend>, area: ratatui::layout::Rect) {
        f.render_widget(Paragraph::new(Line::from(self)), area);
    }
}

#[inline]
pub fn draw(t: &Arc<Mutex<TerminalHandle>>, ctx: &mut impl Drawable) {
    let _ = t.try_lock().map(|mut t| {
        t.terminal
            .draw(|f: &mut Frame<Backend>| {
                Drawable::draw(ctx, f, f.size());
            })
            .expect("Failed to draw a user inteface");
    });
}
