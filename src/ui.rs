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



pub mod terminal {

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
    widgets::{Table, Row, Cell, List, ListItem, Block, Borders, Paragraph, Wrap, Padding},
    text::Text,
    style::{Style, Modifier, Color},
    Frame,
};
//use crossterm::event::{ Event, KeyCode}
use crate::game::{Card, Suit, Rank};

//use crate::ui::{State, Backend, InputMode, theme};
use crate::protocol::{client::{ Intro, Home, Game, SelectRole}, server::ChatLine,  encode_message};
use crate::client::Chat;

use ansi_to_tui::IntoText;

 
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



impl Drawable for Home {
    fn draw(&mut self, f: &mut Frame<Backend>, area: Rect){
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

          let screen_chunks = Layout::default()
				.direction(Direction::Horizontal)
				.constraints(
					[
						Constraint::Percentage(70),
						Constraint::Percentage(30),
					]
					.as_ref(),
				)
				.split(main_layout[0]);
            let viewport = Paragraph::new(include_str!("assets/onelegevil.txt").into_text().unwrap())
                .block(Block::default().borders(Borders::ALL));
            
            if false {
                let viewport_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints( [
                                    Constraint::Min(3),
                                    Constraint::Length(3),
                    ].as_ref(), ).split(screen_chunks[0]);
                 f.render_widget(Paragraph::new(
                        Line::from(vec![
                                     Span::raw("You can play. Press")
                                   , Span::styled(" <Enter> ",
                                        Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
                                     )
                                   , Span::raw("to start a game!")
                        ])), viewport_chunks[1]);
                f.render_widget(viewport, viewport_chunks[0]); 
                f.render_widget(Paragraph::new( Line::from(vec![
                                     Span::raw("Press")
                                   , Span::styled(" <Enter> ",
                                        Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
                                     )
                                   , Span::raw("to start a game!")
                        ])).block(Block::default().borders(Borders::ALL)), viewport_chunks[1]);
            } else {
                f.render_widget(viewport, screen_chunks[0]); 
            }
            self.app.chat.draw(f, screen_chunks[1]);
    }

}

impl Drawable for SelectRole {
    fn draw(&mut self, f: &mut Frame<Backend>, area: Rect){
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

          let screen_chunks = Layout::default()
				.direction(Direction::Horizontal)
				.constraints(
					[
						Constraint::Percentage(70),
						Constraint::Percentage(30),
					]
					.as_ref(),
				)
				.split(main_layout[0]);
            let rows = self.roles.items.iter().map(|role| {
                    Row::new(
                        [Cell::from(
                            Text::from(
                                // TODO wrap and Cow
                                textwrap::fill(role.description(), (screen_chunks[0].width-4) as usize)
                                //.iter()
                                //.map(|s| Line::from(s))
                                )
                        )
                        ]).height(screen_chunks[0].height/4 as u16)//.bottom_margin(1)
                }).collect::<Vec<_>>();
           
             let t = Table::new(rows)
                    .block(Block::default().borders(Borders::ALL).title("Select Role"))
                    .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                    .highlight_symbol(">> ")
                    .widths(&[
                        Constraint::Percentage(100),
                    //    Constraint::Length(30),
                    //    Constraint::Min(10),
                    ])
                    ;
            f.render_stateful_widget(t, screen_chunks[0], &mut self.roles.state);
            self.app.chat.draw(f, screen_chunks[1]);
    }

}

impl Drawable for Game {
    fn draw(&mut self, f: &mut Frame<Backend>, area: Rect){
        let main_layout = Layout::default()
				.direction(Direction::Vertical)
				.constraints(
					[
						Constraint::Ratio(3, 5),
						Constraint::Min(5),
						Constraint::Max(1),
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
            for (i, m) in self.monsters.iter_mut().rev()
                .filter(|m| m.is_some()).map(|m| m.as_mut().unwrap()).enumerate() {
                 m.draw(f, viewport_chunks[i]);
            }
          
        let b_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                      Constraint::Percentage(60)
                    , Constraint::Percentage(40)
                    ].as_ref()
                    )
                .split(main_layout[1]);
        
          self.app.chat.draw(f,  b_layout[1]);


         let inventory_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                     Constraint::Percentage(10)
                      ,Constraint::Percentage(30)
                      ,Constraint::Percentage(30)
                    , Constraint::Percentage(30)
                    ].as_ref()
                    )
                .split(b_layout[0]);

          for (i, a) in self.abilities.iter().filter(|a| a.is_some() ).map(|a| a.unwrap()).enumerate()  {

            f.render_widget(Block::default().borders(Borders::ALL), inventory_layout[i+1]);

            let rect = rect_for_card_sign(inventory_layout[i+1],  SignPosition::new(
            VerticalPosition::Bottom,
            HorizontalPosition::Left
        ), );
            f.render_widget(Clear, rect); //this clears out the background
            f.render_widget(Paragraph::new(char::from(a).to_string()), rect);
            

          }
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
                ChatLine::Disconnection(user) => vec![
                    Span::styled(format!("{} has left the game", user), Style::default().fg(Color::Red)),
                ],
                ChatLine::Connection(user) => vec![
                    Span::styled(format!("{} join to the game", user), Style::default().fg(Color::Green)),
                ],
                ChatLine::Text(msg) => vec![
                        Span::styled(msg, Style::default().fg(Color::White)),
                    ],
                ChatLine::GameEvent(_) => todo!(),
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


macro_rules! include_file_by_rank {
    (match $rank:expr => { $($rank_t:ident)* }, $suit:expr => $suit_tuple:tt ) => {
        match $rank {
           $( 
               Rank::$rank_t => {
                       include_file_by_rank!(@repeat_suit $suit => $rank_t $suit_tuple)
               },
            )*
        }
    };
    (@repeat_suit $suit:expr =>  $rank_t:ident { $($suit_t:ident)* }) => {
        match $suit {
            $(
                Suit::$suit_t => include_str!(concat!("assets/monsters/",
                                            stringify!($rank_t), "_", stringify!($suit_t), ".txt")),
            )*
        }
    };
}
use ratatui::widgets::Clear;

impl Drawable for Card {
    fn draw(&mut self,  f: &mut Frame<Backend>, area: Rect){
        f.render_widget( Paragraph::new(
            include_file_by_rank!(
                match self.rank => {
                        Two 
                        Three
                        Four 
                        Five  
                        Six   
                        Seven 
                        Eight
                        Nine  
                        Ten   
                        Jack  
                        Queen
                        King 
                }, self.suit => {
                        Hearts 
                        Diamonds 
                        Clubs 
                        Spades
                }
            )   
        ).block(Block::default().borders(Borders::ALL)), area);
        self.rank.draw(f, area);
        self.suit.draw(f, area);
    }
}

#[derive(Clone, Copy)]
enum VerticalPosition {
    Top = 1,
    Bottom = 3,
}
#[derive(Clone, Copy)]
enum HorizontalPosition{
    Right = 3,
    Left = 1,
}
#[derive(Clone, Copy)]
struct SignPosition {
    v: VerticalPosition,
    h: HorizontalPosition
}

fn draw_sign(what: Paragraph<'_>, p: SignPosition , card_area: Rect, f: &mut Frame<Backend>){
    let area = rect_for_card_sign(card_area, p);
    f.render_widget(Clear, area); //this clears out the background
    f.render_widget(what, area);
}

impl Drawable for Rank {
    fn draw(&mut self,  f: &mut Frame<Backend>, area: Rect){
        [SignPosition::new(
            VerticalPosition::Top,
            HorizontalPosition::Left
            )
        , SignPosition::new(
              VerticalPosition::Bottom
            , HorizontalPosition::Right
            )]
            .iter()
            .for_each(|p| draw_sign(Paragraph::new(char::from(*self).to_string()), *p, area, f));

    }
}
impl Drawable for Suit {
    fn draw(&mut self,  f: &mut Frame<Backend>, area: Rect) {
        [
         SignPosition::new(
            VerticalPosition::Top,
            HorizontalPosition::Right
        ), 
         SignPosition::new(
            VerticalPosition::Bottom,
            HorizontalPosition::Left
            )]
            .iter()
            .for_each(|p| draw_sign(Paragraph::new(char::from(*self).to_string()), *p, area, f));
    }
}

impl SignPosition {
    fn new(v: VerticalPosition, h: HorizontalPosition) -> Self {
        SignPosition{v, h}
    }
}
fn rect_for_card_sign(area: Rect, position: SignPosition) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(area.height-4),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(area);

      Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Length(area.width-6),
                Constraint::Length(1),
                Constraint::Length(2),
            ]
            .as_ref(),
        )
        .split(popup_layout[position.v as usize])[position.h as usize]
}


/*
/// State holds the state of the ui
pub struct State {
   tx: Tx,    
   /// Current input mode
   input_mode: InputMode,
    
}
impl State {
    fn new(tx: Tx) -> Self {
        State{tx,  input_mode: InputMode::default()  }
  }
}

        match msg {
            UiEvent::Draw => (),
            UiEvent::Chat(msg) => {
              self.state.chat.messages.push(msg);
            },
            UiEvent::CanPlay => { 
                self.state.can_play = true;
                /*
                if let Stage::Home(s) = &mut self.stage{
                     s.can_play = true;   
                } else {
                    unreachable!("message UiEvent::CanPlay not allowed outside of the 'Home' game stage");
                }
                */
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
                        StageEvent::Next => {
                            self.stage = match self.stage {
                                GameStage::Intro(_) => GameStage::from(Home::default()),
                                _ =>  GameStage::from(Home::default())


                            }
                            //if let Stage::Game(_) = &new_stage {
                            //    self.state.tx.send(encode_message(ClientMessage::StartGame))?;
                            //};
                        },
                        _ => ()
                    };
                }
            }
        };
    */

use terminal::TerminalHandle;
pub trait HasTerminal {
    fn get_terminal<'a>(&mut self) -> anyhow::Result<Arc<Mutex<TerminalHandle>>>{
        unimplemented!("get_terminal is not implemented for this game context")
    }
}

//#[enum_dispatch(ClientGameContext)]

pub trait UI: Drawable + HasTerminal + Sized {
    fn draw(&mut self) -> anyhow::Result<()>{
         self.get_terminal()?.lock().unwrap().terminal.draw(|f: &mut Frame<Backend>| {
                              (self as &mut dyn Drawable).draw( f, f.size());
                           })
                .context("Failed to draw a user inteface")?;
        Ok(())
    }
    
}
impl UI for ClientGameContext {
dispatch_trait!{   
    UI fn draw(&mut self,) -> anyhow::Result<()> {
            GameContext => 
                        Intro 
                        Home 
                        SelectRole 
                        Game
        }
}
}


impl HasTerminal for ClientGameContext{}
impl HasTerminal for Home {
    fn get_terminal<'a>(&mut self) -> anyhow::Result<Arc<Mutex<TerminalHandle>>>{
        Ok(self.app.terminal.clone())
    }
}
impl HasTerminal for SelectRole {
    fn get_terminal<'a>(&mut self) -> anyhow::Result<Arc<Mutex<TerminalHandle>>>{
        Ok(self.app.terminal.clone())
    }
}
impl UI for SelectRole{}
impl UI for Home{}

impl HasTerminal for Game {
    fn get_terminal<'a>(&mut self) -> anyhow::Result<Arc<Mutex<TerminalHandle>>> {
        Ok(self.app.terminal.clone())
            
    }
}
impl UI for Game{}



impl HasTerminal for Intro {
    fn get_terminal<'a>(&mut self) -> anyhow::Result<Arc<Mutex<TerminalHandle>>>{
        Err(anyhow::anyhow!("get_terminal is not implemented for this game context"))
    }
}

impl UI for Intro {
    fn draw(&mut self) -> anyhow::Result<()>{
        if let Some(t) = self._terminal.as_mut() {
             Arc::clone(&t)
            .lock().unwrap().terminal.draw(
                                |f: &mut Frame<Backend>| {
                              (self as &mut dyn Drawable).draw( f, f.size());
                           })
                .context("Failed to draw a user inteface")?;
        }
        Ok(())
    }
    
}

