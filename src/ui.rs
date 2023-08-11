use anyhow::Context as _;
use std::sync::{Arc, Mutex};
use ratatui::{
    backend::CrosstermBackend,
    //Frame,
};
use crossterm::event::{ Event, KeyEventKind, KeyCode};
use std::io;
use tracing::{debug, info, warn, error};

//use crate::shared::{encode_message};

type Backend = CrosstermBackend<io::Stdout>;
type Tx = tokio::sync::mpsc::UnboundedSender<String>;

use ratatui::{Terminal};
use crate::protocol::client::ClientGameContext;

pub mod game;
pub mod select_role;
pub mod home;
pub mod details;

use std::sync::Once;
use crossterm::execute;
use std::sync::{ Weak};

pub struct TerminalHandle {
    pub terminal: Terminal<CrosstermBackend<io::Stdout>>
} 
impl TerminalHandle {
    pub fn new() -> anyhow::Result<Self> {
        let mut t = TerminalHandle{
            terminal : Terminal::new(CrosstermBackend::new(io::stdout())).context("Creating terminal failed")?
        };
        t.setup_terminal().context("Unable to setup terminal")?;
        Ok(t)
    }
    fn setup_terminal(&mut self) -> anyhow::Result<()> {
        crossterm::terminal::enable_raw_mode().context("Failed to enable raw mode")?;
        execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen).context("Unable to enter alternate screen")?;
        self.terminal.hide_cursor().context("Unable to hide cursor")

    }
    fn restore_terminal(&mut self) -> anyhow::Result<()> {
        crossterm::terminal::disable_raw_mode().context("failed to disable raw mode")?;
        execute!(self.terminal.backend_mut(), crossterm::terminal::LeaveAlternateScreen)
            .context("unable to switch to main screen")?;
        self.terminal.show_cursor().context("unable to show cursor")?;
        debug!("The terminal has been restored");
        Ok(())
    }
    // invoke as `TerminalHandle::chain_panic_for_restore(Arc::downgrade(&term_handle));
    pub fn chain_panic_for_restore(this: Weak<Mutex<Self>>){
        static HOOK_HAS_BEEN_SET: Once = Once::new();
        HOOK_HAS_BEEN_SET.call_once(|| {
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            match this.upgrade(){
                None => { error!("Unable to upgrade a weak terminal handle while panic") },
                Some(arc) => {
                    match arc.try_lock(){
                        Err(e) => { 
                            error!("Unable to lock terminal handle: {}", e);
                            },
                        Ok(mut t) => { 
                            let _ = t.restore_terminal().map_err(|err|{
                                error!("Unable to restore terminal while panic: {}", err);
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
            if let Err(e) = self.restore_terminal().context("Restore terminal failed"){
                error!("Drop terminal error: {e}");
            };
        };
        debug!("TerminalHandle has been dropped");
    }
}




pub mod theme {  
use ratatui::style::{Style, Color, Modifier};
    pub const SEL_BG: Color = Color::Blue;
    pub const SEL_FG: Color = Color::White;
    pub const DIS_FG: Color = Color::DarkGray;
    pub fn border(focused: bool) -> Style {
        	if focused {
			Style::default().add_modifier(Modifier::BOLD)
		} else {
			Style::default().fg(DIS_FG)
		}
    }
    pub fn block(focused: bool) -> Style {
		if focused {
			Style::default()
		} else {
			Style::default().fg(DIS_FG)
		}
	}
    
}

use crate::details::dispatch_trait;

use crate::protocol::GameContext;

pub trait Drawable {
    fn draw(&mut self,f: &mut Frame<Backend>, area: ratatui::layout::Rect);
}


impl Drawable for ClientGameContext {
dispatch_trait!{   
    Drawable fn draw(&mut self, f: &mut Frame<Backend>, area: ratatui::layout::Rect, ) {
            GameContext => 
                        Intro 
                        Home 
                        SelectRole 
                        Game
        }
}
}




use ratatui::text::{Span, Line};
use ratatui::{ 
    layout::{ Constraint, Direction, Layout, Alignment, Rect},
    widgets::{Table, Row, Cell, List, ListItem, Block, Borders, Paragraph, Wrap, Padding, Scrollbar, ScrollDirection, ScrollbarOrientation},
    text::Text,
    style::{Style, Modifier, Color},
    Frame,
};

use crate::protocol::{client::{ Intro, Home, Game, SelectRole}, server::ChatLine,  encode_message};
use crate::client::Chat;

 
impl Drawable for Intro {
    fn draw(&mut self, f: &mut Frame<Backend>, area: Rect){
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                  Constraint::Percentage(50)
                , Constraint::Min(15)
                , Constraint::Length(2)
                ].as_ref()
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
            
            f.render_widget(Paragraph::new(
                    vec![
                        Line::from(vec![
                                   Span::raw("Press")
                                   ,  Span::styled(" <Enter> ",
                                        Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
                                     )
                                   , Span::raw("to continue...")
                        ]),
                        Line::from(vec![
                                   Span::raw("Press")
                                   , Span::styled(" <q> ",
                                        Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow),
                                    )
                                   , Span::raw("to exit from Ascension")
                        ]),
                    ]
                ), chunks[2]);
    }
}



impl Drawable for Chat {
    fn draw(&mut self,  f: &mut Frame<Backend>, area: Rect){
        let chunks = Layout::default()
				.direction(Direction::Vertical)
				.constraints(
					[
						Constraint::Min(3),
						Constraint::Length(3),
					]
					.as_ref(),
				)
				.split(area);
        let messages =  self.messages
            .iter()
            .map(|message| {
               Line::from(match &message {
                ChatLine::Disconnection(user) => //vec![
                    Span::styled(format!("{} has left the game", user), Style::default().fg(Color::Red)),
                
                ChatLine::Connection(user) =>// vec![
                    Span::styled(format!("{} join to the game", user), Style::default().fg(Color::Green)),
                
                ChatLine::Reconnection(user) => //vec![
                    Span::styled(format!("{} reconnected", user), Style::default().fg(Color::Green)),
                
                ChatLine::Text(msg) =>// vec![
                    Span::styled(msg, Style::default().fg(Color::White)),
                    
                ChatLine::GameEvent(msg) => //vec![
                    Span::styled(msg, Style::default().fg(Color::LightYellow)),
            })
        })
        .collect::<Vec<_>>();

        self.scroll_state = self.scroll_state.content_length(messages.len() as u16);

        let messages_panel = Paragraph::new(messages)
        .block(
            Block::default()
                .borders(Borders::NONE)
        )
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
                crate::input::InputMode::Normal  => theme::block(false),
                crate::input::InputMode::Editing => theme::block(true),
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
                    chunks[1].x
                        + (self.input.visual_cursor()) as u16
                        + 1,
                    // Move one line down, from the border to the input line
                    chunks[1].y + 1,
                );
            }
        }
    }
}




#[inline]
pub fn draw_context(t: &Arc<Mutex<TerminalHandle>>, ctx: &mut ClientGameContext){
    let _ = t.try_lock().map(|mut t| {
        t.terminal.draw(
                   |f: &mut Frame<Backend>| {
                       Drawable::draw(ctx, f, f.size());
                   })
                    .expect("Failed to draw a user inteface");
    });
}




