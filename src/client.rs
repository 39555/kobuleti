
use anyhow::{anyhow,  Context};
use std::net::SocketAddr;
use std::io::ErrorKind;
use futures::{ SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_util::codec::{ LinesCodec, Framed,  FramedRead, FramedWrite};
use tracing::{debug, info, warn, error};
use crate::shared::{ClientMessage, ServerMessage, LoginStatus, MessageDecoder, ChatLine, encode_message};
use crate::ui::{self, UiEvent};
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
        let mut ui = ui::Ui::new(tx).context("Failed to run a user interface")?;
        
        socket_writer.send(encode_message(ClientMessage::GetChatLog)).await?;
        
        ui.draw(UiEvent::Draw)?;
        loop {
            tokio::select! {
                input = input_reader.next() => {
                    match  input {
                        None => break,
                        Some(Err(e)) => {
                            warn!("IO error on stdin: {}", e);
                        }, 
                        Some(Ok(event)) => { 
                            ui.draw(UiEvent::Input(event))?;
                        }
                    }
                    
                }
                Some(msg) = rx.recv() => {
                    socket_writer.send(&msg).await.context("failed to send a message to the socket")?;
                }

                r = socket_reader.next() => match r { 
                    Ok(msg) => match msg {
                        ServerMessage::LoginStatus(_) => unreachable!(),
                        ServerMessage::Chat(msg) => {
                            ui.draw(UiEvent::Chat(msg))?;
                        }
                        ServerMessage::ChatLog(vec) => {
                            ui.upload_chat(vec);
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

