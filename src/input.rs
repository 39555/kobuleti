
use crossterm::event::{ Event, KeyEventKind, KeyCode};
use crate::shared::{game_stages::{GameStage, Intro, Home, Game}, client, encode_message, game_stages::StageEvent};
use enum_dispatch::enum_dispatch;
use tracing::{debug, info, warn, error};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum InputMode {
    #[default]
    Normal,
    Editing,
}

#[enum_dispatch(GameStage)]
pub trait Inputable {
    fn handle_input(&mut self, event: &Event) -> anyhow::Result<Option<StageEvent>>;
}

impl Inputable for Intro {
    fn handle_input(&mut self, event: &Event) -> anyhow::Result<Option<StageEvent>> {
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Enter => {
                    return Ok(Some(StageEvent::Next))
                } _ => ()
            }
        }
        Ok(None)
    }
}

impl Inputable for Home {
    fn handle_input(&mut self, event: &Event) -> anyhow::Result<Option<StageEvent>> {
        if let Event::Key(key) = event {
            
            match self.chat.input_mode {
                InputMode::Normal => {
                    match key.code {
                        KeyCode::Enter => {
                            //if state.can_play {
                                // TODO separate next chain from stages
                             //   state.tx.send(encode_message(ClientMessage::StartGame))?;
                              //  return Ok(StageEvent::Next)
                           // } else {
                                // TODO message about wait a second player
                           // }
                        },
                        KeyCode::Char('e') => {
                            self.chat.input_mode = InputMode::Editing;
                        },
                        _ => ()
                    }
                },
                InputMode::Editing => { 
                    self.chat.handle_input(event)?; 
                }
            }
            
        }
        Ok(None)
    }
}
impl Inputable for Game {
    fn handle_input(&mut self,  event: &Event) -> anyhow::Result<Option<StageEvent>> {
        if let Event::Key(key) = event {
            match self.chat.input_mode {
                InputMode::Normal => {
                    match key.code {
                        KeyCode::Enter => { 
                            return Ok(Some(StageEvent::Next)) 
                        },
                        KeyCode::Char('e') => { self.chat.input_mode = InputMode::Editing; },
                        _ => ()
                    }
                },
                InputMode::Editing => { self.chat.handle_input(event)?; }
            }
        }
        Ok(None)
    }
}

