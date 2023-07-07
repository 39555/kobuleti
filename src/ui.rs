
use anyhow::{anyhow,  Context};
use std::sync::{Arc, Mutex, Weak, atomic::{AtomicBool, Ordering}, mpsc};
use std::sync::Once;
type Tx = tokio::sync::mpsc::UnboundedSender<String>;
use ratatui::{
    backend::{CrosstermBackend},
    layout::{self, Constraint, Direction, Layout, Alignment, Rect},
    widgets::{Block, Borders, Paragraph, Wrap},
    style::{Style, Modifier, Color},
    Frame, Terminal
};
use ratatui::text::{Span, Line};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEvent, KeyEventKind, KeyCode}
    , execute
    , terminal 
};
use std::{
    io::{self, Stdout}
    , time::Duration
    , error::Error
    };
use tracing::{debug, info, warn, error};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use crate::shared::{ClientMessage, ServerMessage, LoginStatus, MessageDecoder, ChatType, encode_message};

type Backend = CrosstermBackend<io::Stdout>;
struct TerminalHandle {
    terminal: Terminal<CrosstermBackend<io::Stdout>>
} 
impl TerminalHandle {
    fn new() -> anyhow::Result<Self> {
        let mut t = TerminalHandle{
            terminal : Terminal::new(CrosstermBackend::new(io::stdout())).context("Creating terminal failed")?
        };
        t.setup_terminal().context("Unable to setup terminal")?;
        Ok(t)
    }
    fn setup_terminal(&mut self) -> anyhow::Result<()> {
        terminal::enable_raw_mode().context("Failed to enable raw mode")?;
        execute!(io::stdout(), terminal::EnterAlternateScreen).context("Unable to enter alternate screen")?;
        self.terminal.hide_cursor().context("Unable to hide cursor")

    }
    fn restore_terminal(&mut self) -> anyhow::Result<()> {
        terminal::disable_raw_mode().context("failed to disable raw mode")?;
        execute!(self.terminal.backend_mut(), terminal::LeaveAlternateScreen)
            .context("unable to switch to main screen")?;
        self.terminal.show_cursor().context("unable to show cursor")?;
        debug!("The terminal has been restored");
        Ok(())
    }
    // invoke as `TerminalHandle::chain_panic_for_restore(Arc::downgrade(&term_handle));
    fn chain_panic_for_restore(this: Weak<Mutex<Self>>){
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


trait DrawableComponent {
	fn draw(&self,
		state: &State,
		f: &mut Frame<Backend>,
        area: Rect
	) -> anyhow::Result<()>;
}

trait InputComponent {
    fn input(&self, state: &mut State, event: &Event) -> anyhow::Result<StageEvent>;

}
trait Stage: DrawableComponent + InputComponent {}

enum StageEvent {
    None,
    Next(GameStage)

}
enum GameStage {
    Intro(Intro),
    Home(Home),
    Game(Game)
}
impl GameStage {
    fn get_stage(&self) -> &dyn Stage {
        match self {
            Self::Intro(v) => v,
            Self::Home(v)  => v,
            Self::Game(v)  => v,
        }
    }
}
 



struct Intro;
struct Home;
struct Game;


impl Stage for Intro{}
impl DrawableComponent for Intro {
    fn draw(&self, state: &State, f: &mut Frame<Backend>, area: Rect) -> anyhow::Result<()>{
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
            
            let messages = //if !self.state.server.is_connected()
                //|| !self.state.server.has_compatible_version()
                {
                    vec![
                        Line::from(vec![Span::raw("Press"), enter, Span::raw("to continue...")]),
                        Line::from(vec![Span::raw("Press"), esc, Span::raw("to exit from Ascension")]),
                    ]
                };
        Ok(())
    }
}
impl InputComponent for Intro {
    fn input(&self, state: &mut State, event: &Event) -> anyhow::Result<StageEvent> {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Enter => {
                    return Ok(StageEvent::Next(GameStage::Home(Home{})))
                } _ => ()
            }
        }
        Ok(StageEvent::None)
    }
}


impl Stage for Home{}
impl DrawableComponent for Home {
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
}
impl InputComponent for Home {

    fn input(&self, state: &mut State, event: &Event) -> anyhow::Result<StageEvent> {
        if let Event::Key(key) = event {
                    match key.code {
                        KeyCode::Enter => {
                            return Ok(StageEvent::Next(GameStage::Game(Game{})))
                        } _ => ()
                    }
                }
        Ok(StageEvent::None)
    }
}


impl Stage for Game{}
impl DrawableComponent for Game {
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
}
impl InputComponent for Game {
    fn input(&self, state: &mut State, event: &Event) -> anyhow::Result<StageEvent> {
        if let Event::Key(key) = event {
                    match key.code {
                        KeyCode::Enter => {
                            return Ok(StageEvent::Next(GameStage::Home(Home{})))
                        } _ => ()
                    }
                   
                }
        Ok(StageEvent::None)
    }
}


struct Chat;
impl Stage for Chat{}
impl DrawableComponent for Chat {
    fn draw(&self, state: & State, f: &mut Frame<Backend>, area: Rect) -> anyhow::Result<()>{
        let chunks = Layout::default()
				.direction(Direction::Vertical)
				.constraints(
					[
						Constraint::Percentage(85),
						Constraint::Percentage(15),
					]
					.as_ref(),
				)
				.split(area);
        let messages =  state.chat.messages
            .iter()
            .map(|message| {
               Line::from(match &message {
                ChatType::Disconnection(user) => vec![
                    Span::styled(user, Style::default().fg(Color::Red)),
                    Span::styled(" has left the game", Style::default().fg(Color::Red)),
                ],
                ChatType::Connection(user) => vec![
                    Span::styled(user, Style::default().fg(Color::Green)),
                    Span::styled(" join to the game", Style::default().fg(Color::Green)),
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
            .style(match state.chat.input_mode {
                InputMode::Normal  => Theme::block(false),
                InputMode::Editing => Theme::block(true),
            })
            .block(Block::default().borders(Borders::ALL).title("Your Message"));
        
        f.render_widget(input, chunks[1]);
        match state.chat.input_mode {
            InputMode::Normal =>
                // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
                {}

        InputMode::Editing => {
            // Make the cursor visible and ask tui-rs to put it at the specified coordinates after rendering
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
}
impl InputComponent for Chat {

    fn input(&self, state: &mut State, event: &Event) -> anyhow::Result<StageEvent> {
        assert_eq!(state.chat.input_mode, InputMode::Editing);
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press { match key.code {
                KeyCode::Enter => {
                    state.chat.messages.push(ChatType::Text(state.chat.input.value().into()));
                    state.tx.send(encode_message(ClientMessage::Chat(state.chat.input.value().into()))).expect("");
                    state.chat.input.reset();
                }
                KeyCode::Esc => {
                    state.chat.input_mode = InputMode::Normal;
                }
                _ => {
                    state.chat.input.handle_event(&Event::Key(*key));
                }
            }}   
            }
        Ok(StageEvent::None)
    }
}




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

struct Theme;
impl Theme {  
    pub const SEL_BG: Color = Color::Blue;
    pub const SEL_FG: Color = Color::White;
    pub const DIS_FG: Color = Color::DarkGray;
    fn border(focused: bool) -> Style {
        	if focused {
			Style::default().add_modifier(Modifier::BOLD)
		} else {
			Style::default().fg(Theme::DIS_FG)
		}
    }
    fn block(focused: bool) -> Style {
		if focused {
			Style::default()
		} else {
			Style::default().fg(Theme::DIS_FG)
		}
	}
    
}


#[derive(Default)]
struct ChatState {
     /// Current value of the input box
    input: Input,
    /// Current input mode
    input_mode: InputMode,
    /// History of recorded messages
    messages: Vec<ChatType>,
}

/// State holds the state of the ui
struct State {
   tx: Tx,
   chat: ChatState,
    
}
impl State {
    fn new(tx: Tx) -> Self {
        State{tx, chat: ChatState::default(),
      }
  }
}


pub struct Ui {
    stage: GameStage,
    state: State,
    terminal_handle: Arc<Mutex<TerminalHandle>>,
}
impl Ui {
    pub fn new(tx: Tx) -> anyhow::Result<Self> { 
        let terminal_handle = Arc::new(Mutex::new(TerminalHandle::new().context("Failed to create a terminal")?));
        TerminalHandle::chain_panic_for_restore(Arc::downgrade(&terminal_handle));
        Ok(Ui{stage: GameStage::Intro(Intro{}), state: State::new(tx),   terminal_handle})
    }
   
    pub fn draw(&mut self, msg: UiEvent) -> anyhow::Result<()> { 
        let input = InputMode::Normal;
        match msg {
            UiEvent::Draw => (),
            UiEvent::Chat(msg) => {
              //self.state.chat.messages.push(msg);
            },
            UiEvent::Input(event) => { 
                if let Event::Key(key) = event {
                match input {
                    InputMode::Normal => {
                        match key.code {
                            KeyCode::Char('f') => {
                                  self.state.tx.send(encode_message(ClientMessage::Chat("Hello, game".to_owned())))
                                    .expect("Unable to send a message into the mpsc channel");
                            }
                            KeyCode::Char('e') => {
                               //self.state.chat.input_mode = InputMode::Editing;
                            },
                           
                            KeyCode::Char('q') => {
                                 info!("Closing the client user interface");
                                   self.state.tx.send(encode_message(ClientMessage::RemovePlayer)).expect("");
                            }
                             _ => {
                                ();
                            }
                        }},
                    InputMode::Editing => { },//self.chat_process_input(event) },   
                }    
            };
            match self.stage.get_stage().input(&mut self.state, &event).context("failed to process an input event in current game stage")?{
                StageEvent::Next(new_stage) => { 
                    self.stage = new_stage;
                }
                _ => ()
            };
            }};
            self.terminal_handle.lock().unwrap()
                .terminal.draw(|f: &mut Frame<Backend>| {
                                   if let Err(e) = self.stage.get_stage().draw(& self.state, f, f.size()){
                                        error!("A user interface error: {}", e)
                                   } 
                               })
                .context("Failed to draw a user inteface")?;
        Ok(())
    }

}

