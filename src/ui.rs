use anyhow::Context;
use std::sync::{Arc, Mutex};
use ratatui::{
    backend::CrosstermBackend,
    Frame,
};
use crossterm::event::{ Event, KeyEventKind, KeyCode};
use std::io;
use tracing::{debug, info, warn, error};
use tui_input::Input;

use crate::shared::{ClientMessage, ChatType, encode_message};

type Backend = CrosstermBackend<io::Stdout>;
type Tx = tokio::sync::mpsc::UnboundedSender<String>;

mod terminal {

use tracing::{debug,  error};
use anyhow::Context;
use std::sync::Once;
use std::io;
use ratatui::{Terminal, backend::CrosstermBackend};
use crossterm::execute;
use std::sync::{Mutex, Weak};

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
                    match arc.lock(){
                        Err(e) => { 
                            error!("Unable to lock terminal handle: {}", e);
                            },
                        Ok(mut t) => { 
                            let _ = t.restore_terminal().map_err(|err|{
                                error!("Unaple to restore terminal while panic: {}", err);
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



mod stages {

use ratatui::text::{Span, Line};
use ratatui::{ 
    layout::{ Constraint, Direction, Layout, Alignment, Rect},
    widgets::{Block, Borders, Paragraph, Wrap},
    style::{Style, Modifier, Color},
    Frame,
};
use crossterm::event::{ Event, KeyCode}
;
use tui_input::backend::crossterm::EventHandler;
use enum_dispatch::enum_dispatch;

use crate::ui::{State, Backend, InputMode, theme};
use crate::shared::{ClientMessage, ChatType, encode_message};


pub enum StageEvent {
    None,
    Next(Stage)

}


pub struct Intro;
pub struct Home;
pub struct Game;
#[enum_dispatch]
pub enum Stage {
    Intro,
    Home,
    Game,
}

impl Default for Stage {
    fn default() -> Self {
        Stage::from(Intro{})
    }
}

#[enum_dispatch(Stage)]
pub trait UIble {
	fn draw(&self, state: &State, f: &mut Frame<Backend>, area: Rect) -> anyhow::Result<()>;
    fn handle_input(&self, state: &mut State, event: &Event) -> anyhow::Result<StageEvent>;
}

 
impl UIble for Intro {
    fn draw(&self, _: &State, f: &mut Frame<Backend>, area: Rect) -> anyhow::Result<()>{
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(4)
            .constraints([
                  Constraint::Percentage(50)
                , Constraint::Percentage(45)
                , Constraint::Percentage(5)
                ].as_ref()
                )
            .split(area);
            let intro = Paragraph::new(include_str!("assets/intro"))
            .style(Style::default().add_modifier(Modifier::BOLD))
            .alignment(Alignment::Left) 
            .wrap(Wrap { trim: true });

            //let title = Paragraph::new(include_str!("assets/title"))
            //.style(Style::default().add_modifier(Modifier::BOLD))
            //.alignment(Alignment::Center);
            f.render_widget(intro, chunks[0]);

            let enter = Span::styled(
                " <Enter> ",
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
                );

            let esc = Span::styled(
                " <q> ",
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow),
                );
            
            let messages = 
                {
                    vec![
                        Line::from(vec![Span::raw("Press"), enter, Span::raw("to continue...")]),
                        Line::from(vec![Span::raw("Press"), esc, Span::raw("to exit from Ascension")]),
                    ]
                };
            f.render_widget(Paragraph::new(messages), chunks[2]);
        Ok(())
    }

    fn handle_input(&self, _: &mut State, event: &Event) -> anyhow::Result<StageEvent> {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Enter => {
                    return Ok(StageEvent::Next(Stage::from(Home{})))
                } _ => ()
            }
        }
        Ok(StageEvent::None)
    }
}


impl UIble for Home {
    fn draw(&self, state: & State, f: &mut Frame<Backend>, area: Rect) -> anyhow::Result<()>{
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

          let viewport_chunks = Layout::default()
				.direction(Direction::Horizontal)
				.constraints(
					[
						Constraint::Percentage(70),
						Constraint::Percentage(30),
					]
					.as_ref(),
				)
				.split(main_layout[0]);

            let screen = Paragraph::new(include_str!("assets/waiting_screen.txt")); //.block(Block::default().borders(Borders::ALL));
            f.render_widget(screen, viewport_chunks[0]); 
            Chat{}.draw(state, f, viewport_chunks[1])?;
            Ok(())
    }

    fn handle_input(&self, state: &mut State, event: &Event) -> anyhow::Result<StageEvent> {
        if let Event::Key(key) = event {
            match state.input_mode {
                InputMode::Normal => {
                    match key.code {
                        KeyCode::Enter => {
                            return Ok(StageEvent::Next(Stage::from(Game{})))
                        },
                        KeyCode::Char('e') => {
                            state.input_mode = InputMode::Editing;
                        },
                        _ => ()
                    }
                },
                InputMode::Editing => { Chat{}.handle_input(state, event)?; }
            }
        }
        Ok(StageEvent::None)
    }
}


impl UIble for Game {
    fn draw(&self, state: & State, f: &mut Frame<Backend>, area: Rect) -> anyhow::Result<()>{
        let main_layout = Layout::default()
				.direction(Direction::Vertical)
				.constraints(
					[
						Constraint::Percentage(60),
						Constraint::Percentage(39),
						Constraint::Length(1),
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
        
            let enemy = Paragraph::new(include_str!("assets/monster.txt")).block(Block::default().borders(Borders::ALL));
                                                                                // .style(Style::default().fg(Theme::DIS_FG)));
            f.render_widget(enemy, viewport_chunks[0]);  
            let enemy = Paragraph::new(include_str!("assets/monster1.txt")).block(Block::default().borders(Borders::ALL));
                                                                                // .style(Style::default().fg(Theme::DIS_FG)));
            f.render_widget(enemy, viewport_chunks[1]);
            let enemy = Paragraph::new(include_str!("assets/monster2.txt")).block(Block::default().borders(Borders::ALL));
                                                                                // .style(Style::default().fg(Theme::DIS_FG)));
            f.render_widget(enemy, viewport_chunks[2]);
            let enemy = Paragraph::new(include_str!("assets/monster3.txt")).block(Block::default().borders(Borders::ALL));
                                                                                // .style(Style::default().fg(Theme::DIS_FG)));
            f.render_widget(enemy, viewport_chunks[3]);
        let b_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                      Constraint::Percentage(55)
                    , Constraint::Percentage(45)
                    ].as_ref()
                    )
                .split(main_layout[1]);
        
          Chat{}.draw(state, f, b_layout[1])?;

          let inventory = Block::default()
                .borders(Borders::ALL)
                .title(Span::styled("Inventory", Style::default().add_modifier(Modifier::BOLD)));
          f.render_widget(inventory, b_layout[0]);

        Ok(())
    }

    fn handle_input(&self, state: &mut State, event: &Event) -> anyhow::Result<StageEvent> {
        if let Event::Key(key) = event {
            match state.input_mode {
                InputMode::Normal => {
                    match key.code {
                        KeyCode::Enter => { 
                            return Ok(StageEvent::Next(Stage::Home(Home{}))) 
                        },
                        KeyCode::Char('e') => { state.input_mode = InputMode::Editing; },
                        _ => ()
                    }
                },
                InputMode::Editing => { Chat{}.handle_input(state, event)?; }
            }
        }
        Ok(StageEvent::None)
    }
}


struct Chat;
impl UIble for Chat {
    fn draw(&self, state: & State, f: &mut Frame<Backend>, area: Rect) -> anyhow::Result<()>{
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
        let messages =  state.chat.messages
            .iter()
            .map(|message| {
               Line::from(match &message {
                ChatType::Disconnection(user) => vec![
                    Span::styled(format!("{} has left the game", user), Style::default().fg(Color::Red)),
                ],
                ChatType::Connection(user) => vec![
                    Span::styled(format!("{} join to the game", user), Style::default().fg(Color::Green)),
                ],
                ChatType::Text(msg) => vec![
                        Span::styled(msg, Style::default().fg(Color::White)),
                    ],
                ChatType::GameEvent(_) => todo!(),
            })
            })
            //let color = if let Some(id) = state.users_id().get(&message.user) {
           //     message_colors[id % message_colors.len()]
           // }
            //else {
            //    theme.my_user_color
            //};
        .collect::<Vec<_>>();
        let messages_panel = Paragraph::new(messages)
        .block(
            Block::default()
                .borders(Borders::ALL)
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false });

        f.render_widget(messages_panel, chunks[0]); 
        let input = Paragraph::new(state.chat.input.value())
            .style(match state.input_mode {
                InputMode::Normal  => theme::block(false),
                InputMode::Editing => theme::block(true),
            })
            .block(Block::default().borders(Borders::ALL).title("Your Message"));
        
        f.render_widget(input, chunks[1]);
        // cursor
        match state.input_mode {
            InputMode::Normal =>
                // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
                {}
            InputMode::Editing => {
                // Make the cursor visible and ask ratatui to put it at the specified coordinates after rendering
                f.set_cursor(
                    // Put cursor past the end of the input text
                    chunks[1].x
                        + (state.chat.input.visual_cursor()) as u16
                        + 1,
                    // Move one line down, from the border to the input line
                    chunks[1].y + 1,
                );
            }
        }

        Ok(())
    }

    fn handle_input(&self, state: &mut State, event: &Event) -> anyhow::Result<StageEvent> {
        assert_eq!(state.input_mode, InputMode::Editing);
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Enter => {
                    let input = std::mem::take(&mut state.chat.input);
                    state.chat.messages.push(ChatType::Text(input.value().into()));
                    state.tx.send(encode_message(ClientMessage::Chat(String::from(input))))?;
                } 
               KeyCode::Esc => {
                            state.input_mode = InputMode::Normal;
                        },
                _ => {
                    state.chat.input.handle_event(&Event::Key(*key));
                }
            }   
        }
        Ok(StageEvent::None)
    }
}

}
use stages::UIble;

pub enum UiEvent {
    Chat(ChatType),
    Input(Event),
    Draw,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum InputMode {
    #[default]
    Normal,
    Editing,
}

#[derive(Default)]
struct ChatState {
     /// Current value of the input box
    input: Input,
   
    /// History of recorded messages
    messages: Vec<ChatType>,
}

/// State holds the state of the ui
pub struct State {
   tx: Tx,    
   /// Current input mode
   input_mode: InputMode,
   chat: ChatState,
    
}
impl State {
    fn new(tx: Tx) -> Self {
        State{tx,  input_mode: InputMode::default(), chat: ChatState::default() }
  }
}


pub struct Ui {
    stage: stages::Stage,
    state: State, 

    _terminal_handle: Arc<Mutex<terminal::TerminalHandle>>,
}
impl Ui {
    pub fn new(tx: Tx) -> anyhow::Result<Self> { 
        let terminal_handle = Arc::new(Mutex::new(terminal::TerminalHandle::new().context("Failed to create a terminal")?));
        terminal::TerminalHandle::chain_panic_for_restore(Arc::downgrade(&terminal_handle));
        Ok(Ui{stage: stages::Stage::default(), state: State::new(tx), _terminal_handle: terminal_handle})
    }
   
    pub fn draw(&mut self, msg: UiEvent) -> anyhow::Result<()> { 
        match msg {
            UiEvent::Draw => (),
            UiEvent::Chat(msg) => {
              self.state.chat.messages.push(msg);
            },
            UiEvent::Input(event) => { 
                if let Event::Key(key) = event {
                    if key.kind == KeyEventKind::Press { match self.state.input_mode {
                        InputMode::Normal => {
                            match key.code {
                                KeyCode::Char('q') => {
                                     info!("Closing the client user interface");
                                       self.state.tx.send(encode_message(ClientMessage::RemovePlayer))?;
                                }
                                 _ => ()
                            }
                        },
                        _ => (),   
                    }};
                    match self.stage.handle_input(&mut self.state, &event)
                        .context("failed to process an input event in current game stage")?{
                        stages::StageEvent::Next(new_stage) => self.stage = new_stage,
                        _ => ()
                    };
                }
            }
        };
        self._terminal_handle.lock().unwrap()
            .terminal.draw(|f: &mut Frame<Backend>| {
                               if let Err(e) = self.stage.draw(& self.state, f, f.size()){
                                    error!("A user interface error: {}", e)
                               } 
                           })
                .context("Failed to draw a user inteface")?;
        Ok(())
    }

}
