use clap::{arg, command};
use std::{
    io::{self, Stdout}
    , time::Duration
    , error::Error
    }

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode}
    , execute
    , terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen}
}
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Alignment},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};

fn main() {
    let matches = command!()
        // Args here
        /*        .arg(
            arg!(
                -c --config <FILE> "Sets a custom config file"
            )
            .required(false)
        )
        .arg(arg!(
            -t --text <TEXT> "Text to print"
        ))
        */ 
        .get_matches();

    

}
