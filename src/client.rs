
use anyhow::{anyhow,  Context};

use std::net::{SocketAddr};
use std::{
    io::{self, Stdout}
    , time::Duration
    , error::Error
    };
use std::sync::{Arc, Mutex, Weak, atomic::{AtomicBool, Ordering}, mpsc};
use std::sync::Once;

use ratatui::{
    backend::{CrosstermBackend},
    layout::{self, Constraint, Direction, Layout, Alignment},
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
//use std::sync::mpsc::{channel, Sender, Receiver};
//
use std::rc::Rc;
use std::cell::RefCell;
use tokio::sync;
use std::io::ErrorKind;
use futures::{future, Sink, SinkExt, Stream, StreamExt};
use tokio::net::{TcpStream, tcp::ReadHalf, tcp::WriteHalf};
//use tokio::io;
use tokio_util::codec::{BytesCodec, LinesCodec, Framed,  FramedRead, FramedWrite};
use serde_json;
use serde::{Serialize, Deserialize};
use std::thread;
use tracing::{debug, info, warn, error};
use crate::shared::{ClientMessage, ServerMessage, LoginStatus, MessageDecoder, ChatType, encode_message};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;
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

type Tx = tokio::sync::mpsc::UnboundedSender<String>;
/// Shorthand for the receive half of the message channel.
type Rx = tokio::sync::mpsc::UnboundedReceiver<String>;


pub struct Client {
    username: String,
    host: SocketAddr
}

impl Client {
    pub fn new(host: SocketAddr, username: String) -> Self {
        Self { username, host }
    }
    pub async fn login(&self
        , socket: &mut TcpStream) -> anyhow::Result<()> { 
        let mut stream = Framed::new(socket, LinesCodec::new());
        stream.send(encode_message(ClientMessage::AddPlayer(self.username.clone()))).await?;
        match MessageDecoder::new(stream).next().await?  { 
            ServerMessage::LoginStatus(status) => {
                    match status {
                        LoginStatus::Logged => {
                            info!("Successfull login to the game");
                            Ok(()) 
                        },
                        LoginStatus::InvalidPlayerName => {
                            Err(anyhow!("Invalid player name: '{}'", self.username))
                        },
                        LoginStatus::PlayerLimit => {
                            Err(anyhow!("Player '{}' has tried to login but the player limit has been reached"
                                        , self.username))
                        },
                        LoginStatus::AlreadyLogged => {
                            Err(anyhow!("User with name '{}' already logged", self.username))
                        },
                    }
                },
            _ => Err(anyhow!("not allowed client message, authentification required"))
        } 
    }

    pub async fn connect(&self) -> anyhow::Result<()> {
        let mut stream = TcpStream::connect(self.host)
        .await.context(format!("Failed to connect to address {}", self.host))?;
        self.login(&mut stream).await.context("Failed to join to the game")?;
        self.process_incoming_messages(stream).await?;
        info!("Quit the game");
        Ok(())
    }
    async fn process_incoming_messages(&self, mut stream: TcpStream
                                       ) -> anyhow::Result<()>{
        let (r, w) = stream.split();
        let mut socket_writer = FramedWrite::new(w, LinesCodec::new());
        let mut socket_reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));
        let mut input_reader  = crossterm::event::EventStream::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let mut ui = Ui::new(tx).context("Failed to run a user interface")?;
        ui.show(UiEvent::Draw)?;
        loop {
            tokio::select! {
                input = input_reader.next() => {
                    match  input {
                        None => break,
                        Some(Err(e)) => {
                            warn!("IO error on stdin: {}", e);
                        }, 
                        Some(Ok(event)) => { 
                            ui.show(UiEvent::Input(event))?;
                        }
                    }
                    
                }
                Some(msg) = rx.recv() => {
                    socket_writer.send(&msg).await.context("")?;
                }

                r = socket_reader.next() => match r { 
                    Ok(msg) => match msg {
                        ServerMessage::LoginStatus(_) => unreachable!(),
                        ServerMessage::Chat(msg) => {
                            ui.show(UiEvent::Chat(msg))?;
                        }
                        ServerMessage::Logout => {
                            info!("Logout");
                            break
                        }
                    } 
                    ,
                    Err(e) => { 
                        warn!("Error: {}", e);
                        if e.kind() == ErrorKind::ConnectionAborted {
                            break
                        }

                    }
                }
            }
        }
        Ok(())
    }

}
type UiStage = fn(&mut State, &mut Frame<CrosstermBackend<io::Stdout>>) -> anyhow::Result<()>;
type Backend = CrosstermBackend<io::Stdout>;

pub enum UiEvent {
    Chat(ChatType),
    Input(Event),
    Draw,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
enum InputMode {
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
   chat: ChatState
    
}
impl State {
    fn new(tx: Tx) -> Self {
        State{tx, chat: ChatState{ input:  Input::default()
            , input_mode: InputMode::Normal, messages: vec![] }}
    }
}
struct Ui {
    state: State,
    terminal_handle: Arc<Mutex<TerminalHandle>>,
    current_stage: UiStage
}
impl Ui {
    pub fn new(tx: Tx) -> anyhow::Result<Self> { 
        let terminal_handle = Arc::new(Mutex::new(TerminalHandle::new().context("Failed to create a terminal")?));
        TerminalHandle::chain_panic_for_restore(Arc::downgrade(&terminal_handle));
        let current_stage : UiStage = Ui::chat ;
        Ok(Ui{state: State::new(tx), terminal_handle, current_stage})
    }
   
    pub fn show(&mut self, msg: UiEvent) -> anyhow::Result<()> { 
        let input = self.state.chat.input_mode;
        match msg {
            UiEvent::Draw => (),
            UiEvent::Chat(msg) => {
              self.state.chat.messages.push(msg);
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
                               self.state.chat.input_mode = InputMode::Editing;
                            },
                           
                            KeyCode::Char('q') => {
                                 info!("Closing the client user interface");
                                   self.state.tx.send(encode_message(ClientMessage::RemovePlayer)).expect("");
                            }
                             _ => {
                                ();
                            }
                        }},
                    InputMode::Editing => { if key.kind == KeyEventKind::Press { match key.code {
                         KeyCode::Enter => {
                            self.state.chat.messages.push(ChatType::Text(self.state.chat.input.value().into()));
                            self.state.tx.send(encode_message(ClientMessage::Chat(self.state.chat.input.value().into()))).expect("");
                            self.state.chat.input.reset();
                        }
                        KeyCode::Esc => {
                            self.state.chat.input_mode = InputMode::Normal;
                        }
                        _ => {
                            self.state.chat.input.handle_event(&Event::Key(key));
                        }
                    }}},   
            }
                
            }}};
            self.terminal_handle.lock().unwrap()
                .terminal.draw(|f: &mut Frame<Backend>| {
                                   if let Err(e) = (self.current_stage)(&mut self.state, f){
                                        error!("A user interface error: {}", e)
                                   } 
                               })
                .context("Failed to draw a user inteface")?;
        Ok(())
    }
    fn menu(state: &mut State,  f: &mut Frame<Backend> ) -> anyhow::Result<()>
    {    
        let title = Paragraph::new(include_str!("assets/title"))
            .style(Style::default().add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
            f.render_widget(title, f.size());
        Ok(())
    }
    fn chat(state: &mut State,  f: &mut Frame<Backend> ) ->anyhow::Result<()>{
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
				.split(f.size());
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
        let chat_chunks =   Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                      Constraint::Percentage(85)
                    , Constraint::Percentage(15)
                    ].as_ref()
                    )
                .split(b_layout[1]);
          let inventory = Block::default()
                .borders(Borders::ALL)
                .title(Span::styled("Inventory", Style::default().add_modifier(Modifier::BOLD)));
          f.render_widget(inventory, b_layout[0]);

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
        //.scroll((state.chat.scroll_messages_view as u16, 0))
        .wrap(Wrap { trim: false });

    f.render_widget(messages_panel, chat_chunks[0]);
    let input = Paragraph::new(state.chat.input.value())
        .style(match state.chat.input_mode {
            InputMode::Normal  => Theme::block(false),
            InputMode::Editing => Theme::block(true),
        })
        .block(Block::default().borders(Borders::ALL).title("Your Message"));
    

    f.render_widget(input, chat_chunks[1]);
    match state.chat.input_mode {
        InputMode::Normal =>
            // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
            {}

        InputMode::Editing => {
            // Make the cursor visible and ask tui-rs to put it at the specified coordinates after rendering
            f.set_cursor(
                // Put cursor past the end of the input text
                chat_chunks[1].x
                    + (state.chat.input.visual_cursor()) as u16
                    + 1,
                // Move one line down, from the border to the input line
                chat_chunks[1].y + 1,
            );
        }
    };
    Ok(())
    } 
    fn intro(state: &mut State, f: &mut Frame<Backend>) -> anyhow::Result<()> {
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

    /*
fn hello_room(t: &mut Arc<Mutex<TerminalHandle>>, tx: Tx) -> anyhow::Result<()> {
     loop {
        t.lock().unwrap().terminal.draw(|f: &mut Frame<CrosstermBackend<io::Stdout>>| {
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
            /*
            else if !self.state.user.is_logged() {
                vec![
                    if self.menu.character_symbol_input.content().is_none() {
                        Spans::from(vec![Span::raw("Choose a character (an ascii uppercase letter)")])
                    }
                    else {
                        spans::from(vec![
                            span::raw("press"),
                            enter,
                            span::raw("to login with the character"),
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
                KeyCode::Char('f') => {
                     tx.send(serde_json::to_string(&ClientMessage::Chat("Hello, game".to_owned())).unwrap())
                .expect("Unable to send on channel");
                }
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

*/

