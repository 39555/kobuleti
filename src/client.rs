
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
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEvent, KeyCode}
    , execute
    , terminal 
};
//use std::sync::mpsc::{channel, Sender, Receiver};
//
use std::rc::Rc;
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
use crate::shared::{ClientMessage, ServerMessage, LoginStatus, MessageDecoder, encode_message};

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
type UiStage = fn(Rc<Tx>, &mut Frame<CrosstermBackend<io::Stdout>>) -> anyhow::Result<()>;
type Backend = CrosstermBackend<io::Stdout>;
struct Ui {
    tx: Rc<Tx>,
    terminal_handle: Arc<Mutex<TerminalHandle>>,
    current_stage: UiStage
}
pub enum UiEvent {
    Chat(String),
    Input(Event),
    Draw,
}
impl Ui {
    pub fn new(tx: Tx) -> anyhow::Result<Self> { 
        let terminal_handle = Arc::new(Mutex::new(TerminalHandle::new().context("Failed to create a terminal")?));
        TerminalHandle::chain_panic_for_restore(Arc::downgrade(&terminal_handle));
        let current_ui_stage : UiStage = Ui::intro ;
        Ok(Ui{tx: Rc::new(tx), terminal_handle, current_stage: current_ui_stage})
    }
   
    pub fn show(&mut self, msg: UiEvent) -> anyhow::Result<()> { 
        match msg {
            UiEvent::Draw => (),
            UiEvent::Chat(msg) => {
              self.current_stage = Ui::intro ;
            },
            UiEvent::Input(event) => { 
                if let Event::Key(key) = event {
                    match key.code {
                        KeyCode::Char('f') => {
                             self.tx.send(encode_message(ClientMessage::Chat("Hello, game".to_owned())))
                                .expect("Unable to send a message into the mpsc channel");
                        }
                        KeyCode::Char('e') => {
                            self.current_stage = Ui::menu ;
                        },
                       
                        KeyCode::Char('q') => {
                             info!("Closing the client user interface");
                                self.tx.send(encode_message(ClientMessage::RemovePlayer)).expect("");
                        }
                         _ => {
                            ();
                        }
                    }
                }
            }
        };
            let t = Rc::clone(&self.tx);
            self.terminal_handle.lock().unwrap()
                .terminal.draw(|f: &mut Frame<Backend>| {
                                   if let Err(e) = (self.current_stage)(t, f){
                                        error!("A user interface error: {}", e)
                                   } 
                               })
                .context("Failed to draw a user inteface")?;
        Ok(())
    }
    fn menu(tx: Rc<Tx>,  f: &mut Frame<Backend> ) -> anyhow::Result<()>
    {    
        let title = Paragraph::new(include_str!("assets/title"))
            .style(Style::default().add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
            f.render_widget(title, f.size());
        Ok(())
    }
  
    fn intro(tx: Rc<Tx>, f: &mut Frame<Backend>) -> anyhow::Result<()> {
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

