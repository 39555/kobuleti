use anyhow::{anyhow,  Context};

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
    widgets::{Block, Borders, Paragraph, Wrap},
    style::{Style, Modifier, Color},
    Frame, Terminal
};
use ratatui::text::{Span, Line};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode}
    , execute
    , terminal 
};
//use std::sync::mpsc::{channel, Sender, Receiver};
//
use tokio::sync::{mpsc};
use std::io::ErrorKind;
use futures::{future, Sink, SinkExt, Stream, StreamExt};
use tokio::net::{TcpStream, tcp::ReadHalf, tcp::WriteHalf};
//use tokio::io;
use tokio_util::codec::{BytesCodec, LinesCodec, Framed,  FramedRead, FramedWrite};
use serde_json;
use serde::{Serialize, Deserialize};
use std::thread;
use tracing::{debug, info, warn, error};
use crate::shared::{ClientMessage, ServerMessage, LoginStatus, MessageDecoder};

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

type Tx = mpsc::UnboundedSender<String>;
/// Shorthand for the receive half of the message channel.
type Rx = mpsc::UnboundedReceiver<String>;


pub struct Client {
    username: String,
    host: SocketAddr
}

impl Client {
    pub fn new(host: SocketAddr, username: String) -> Self {
        Self { username, host }
    }
    pub async fn login(&self
        , stream: &mut TcpStream) -> anyhow::Result<()> { 
        let mut lines = Framed::new(stream, LinesCodec::new());
        lines.send(serde_json::to_string(&ClientMessage::AddPlayer(self.username.clone())).unwrap()).await?;
        let mut stream = MessageDecoder::new(lines);
        match stream.next().await?  { 
            ServerMessage::LoginStatus(status) => {
                    match status {
                        LoginStatus::Logged => {
                            info!("login success");
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
        self.login(&mut stream).await.context("failed to join to the game")?;
        let (tx, rx) = mpsc::unbounded_channel::<String>();
        let handle = thread::spawn(move || {  
            Client::ui(tx).expect("failed to run ui");
        });
        Client::process_incoming_messages(stream, rx).await?;
        handle.join().expect("the ui thread has panicked");
        Ok(())
    }

    pub async fn process_incoming_messages(mut stream: TcpStream
                                           , mut rx: Rx )
        -> anyhow::Result<()>{

        let (r, w) = stream.split();
        let mut writer = FramedWrite::new(w, LinesCodec::new());
        let mut reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));

        loop {
            // TODO writer pipe from rx from ui thread
            // TODO why select 
            tokio::select! {
                Some(msg) = rx.recv() => {
                    writer.send(&msg).await.context("")?;
                }
                r = reader.next() => match r { 
                    Ok(msg) => match msg {
                        ServerMessage::LoginStatus(status) => {
                            match status {
                                LoginStatus::Logged => {
                                    info!("login success")
                                },
                                LoginStatus::InvalidPlayerName => {
                                    error!("invalid player name")
                                },
                                LoginStatus::PlayerLimit => {
                                    error!("server is full ..")
                                },
                                LoginStatus::AlreadyLogged => {
                                    error!("User with name .. already logged")
                                },
                            }
                        },
                        ServerMessage::Chat(msg) => {
                            println!("{}", msg);
                        }
                    } 
                    ,
                    Err(e) => { 
                        warn!("error: {}", e);
                        if e.kind() == ErrorKind::ConnectionAborted {
                            break
                        }

                    }
                }
            }
        }
        Ok(())

    }

    pub fn ui(tx: Tx) -> anyhow::Result<()> {
        let mut t = Arc::new(Mutex::new(TerminalHandle::new().context("failed to create a terminal")?));
        TerminalHandle::chain_panic_for_restore(Arc::downgrade(&t));
        hello_room(&mut t, tx)?;
        Ok(())
    }

}



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



