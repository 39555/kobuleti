
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

use std::io::ErrorKind;
use futures::{future, Sink, SinkExt, Stream, StreamExt};
use tokio::net::{TcpStream, tcp::ReadHalf, tcp::WriteHalf};
//use tokio::io;
use tokio_util::codec::{BytesCodec, LinesCodec,  FramedRead, FramedWrite};
use serde_json;
use serde::{Serialize, Deserialize};

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




pub struct Client {
    username: String,
    host: SocketAddr
}

impl Client {
    pub fn new(host: SocketAddr, username: String) -> Self {
        Self { username, host }
    }
    pub async fn connect(&self) -> anyhow::Result<()> {
        let mut stream = TcpStream::connect(self.host)
        .await.context(format!("Failed to connect to address {}", self.host))?;
        let (r, w) = stream.split();
        let mut writer  = FramedWrite::new(w, LinesCodec::new());
        let reader = MessageDecoder::new(FramedRead::new(r, LinesCodec::new()));
        let msg = ClientMessage::AddPlayer(self.username.clone());
        let json = serde_json::to_string(&msg).unwrap();
        writer.send(json).await?;
        Client::process_incoming_messages(reader).await?;
        //tcp::connect(&self.host).await?;
        // if connected run a ui
        //self.ui().await?;
        Ok(())
    }

    pub async fn process_incoming_messages(mut reader: MessageDecoder<FramedRead<ReadHalf<'_>, LinesCodec>> )
        -> anyhow::Result<()>{
        loop {
            // TODO why select 
            tokio::select! {
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
                    // TODO break only with ErrorKind::ConnectionRejected
                    Err(e) => { 
                        warn!("{}", e);
                        if e.kind() == ErrorKind::ConnectionAborted {
                            break
                        }

                    }
                }
            }
        }
        Ok(())

    }

    pub async fn ui(&self) -> anyhow::Result<()> {
        let mut t = Arc::new(Mutex::new(TerminalHandle::new().context("failed to create a terminal")?));
        TerminalHandle::chain_panic_for_restore(Arc::downgrade(&t));
        hello_room(&mut t)?;
        Ok(())
    }

}



fn hello_room(t: &mut Arc<Mutex<TerminalHandle>>) -> anyhow::Result<()> {
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



