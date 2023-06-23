use anyhow::{Context};
use clap::{arg, command, Command, ArgAction};
use std::{
    io::{self, Stdout}
    , time::Duration
    , error::Error
    };
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode}
    , execute
    , terminal 
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Alignment},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal
};
use const_format;
use std::sync::{Arc, Mutex, Weak};
use std::sync::Once;
use std::env;
use std::net::{SocketAddr};


pub mod my_consts {
    use std::ops::RangeInclusive;
    pub const AUTHORS    : &str = env!("CARGO_PKG_AUTHORS");
    pub const APPNAME    : &str = env!("CARGO_PKG_NAME");
    pub const VERSION    : &str = env!("CARGO_PKG_VERSION");
    pub const HOMEPAGE   : &str = env!("CARGO_PKG_HOMEPAGE");
    pub const REPOSITORY : &str = env!("CARGO_PKG_REPOSITORY");
    pub const ISSUES     : &str = const_format::formatcp!("{}/issues/new", REPOSITORY);
    pub const PORT_RANGE : RangeInclusive<usize> = 1..=65535;

}

struct TerminalHandle{
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

fn chain_panic(terminal_handle: Weak<Mutex<TerminalHandle>>){
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
        const PANIC_MSG : &str = const_format::formatcp!("
        ============================================================
        \"{appname}\" has panicked. This is a bug in \"{appname}\". Please report this
        at {issues}. 
        If you can reliably reproduce this panic, include the
        reproduction steps and re-run with the RUST_BACKTRACE=1 env
        var set and include the backtrace in your report.\n
        Platform: {platform} {arch}
        Version: {version}
        "
        , appname=my_consts::APPNAME
        , issues=my_consts::ISSUES
        , platform=env::consts::OS, arch=env::consts::ARCH
        , version = my_consts::VERSION
        );
        eprint!("{}", PANIC_MSG);
        original_hook(panic);
    }));
    });
}

pub trait LogArg {
    fn arg_log_file(self) -> Self;
    fn arg_verbosity(self) -> Self;
}
impl LogArg for Command {
    fn arg_log_file(self) -> Self {
        self.arg(arg!(-l --log <FILE> "specify a log file")
            .required(false))
    }
    fn arg_verbosity(self) -> Self {
        self.arg(arg!(-v --verbosity <TYPE> "set log level of verbosity")
                    .required(false))
    }
}

fn main() -> Result<(), Box<dyn Error>> {

    let matches = command!()
        .help_template(const_format::formatcp!("\
{{before-help}}{{name}} {{version}}
{{author-with-newline}}{{about-with-newline}}
Project home page {}

{{usage-heading}} {{usage}}

{{all-args}}{{after-help}}
", my_consts::REPOSITORY))
        .propagate_version(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("server")
                .arg_required_else_help(true)
                .about(const_format::formatcp!("run {} server mode", my_consts::APPNAME))
                .arg_log_file()
                .arg_verbosity()
                .arg(arg!(
                    -t --tcp <PORT> "Set the tcp port for client connections"
                )
                .value_parser(|port : &str| -> anyhow::Result<u16> {
                    port.parse::<u16>().context(format!("The value must be in range {}-{}"
                        , my_consts::PORT_RANGE.start(), my_consts::PORT_RANGE.end()))
                        })
                    )
                .arg(arg!(-u --udp <PORT> "Set the udp port for client connections"))
                ,
        )
        .subcommand(
            Command::new("client")
                .arg_required_else_help(true)
                .about(const_format::formatcp!("run {} client mode", my_consts::APPNAME))
                .arg(arg!(-o --host <HOST> "Set the server address (ip and port). Format example: 192.168.0.56:3549")
                .value_parser(|host : &str |  -> anyhow::Result<SocketAddr> {
                    host.parse::<SocketAddr>().context("Host must be a valid network address") 
                 })
                .required(true))
                .arg_log_file()
                .arg_verbosity()
                
        )
        .get_matches();
    // TODO for client tui
    //let mut t = Arc::new(Mutex::new(TerminalHandle::new().context("failed to create a terminal")?));
    //chain_panic(Arc::downgrade(&t));

    match matches.subcommand() {
          Some(("server", sub_matches)) => println!(
            "'myapp server",
        )
        , Some(("client", sub_matches)) => println!("my app client")
        , _ => unreachable!("Exhausted list of subcommands and subcommand_required prevents `None`"),
    }

    //run(&mut t).context("application loop failed")?;
    Ok(())
}

fn run(t: &mut Arc<Mutex<TerminalHandle>>) -> anyhow::Result<()> {
    loop {
        t.lock().unwrap().terminal.draw(|f| ui(f))?;
        if let Event::Key(key) = event::read().context("event read failed")? {
            match key.code {
                KeyCode::Char('p') => {
                    panic!("intentional demo panic");
                }

                KeyCode::Char('e') => {
                }

                
                KeyCode::Char('q') => {
                        break;

                }
                _ => {
                    return Ok(());
                }
            }
        }
    }
     Ok(())
}

fn ui<B: Backend>(f: &mut Frame<B>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage(10),
                Constraint::Percentage(80),
                Constraint::Percentage(10),
            ]
            .as_ref(),
        )
        .split(f.size());
    let b = Block::default()
        .title("Panic Handler Demo")
        .borders(Borders::ALL);

    let p = Paragraph::new("Hello").block(b).alignment(Alignment::Center);
    f.render_widget(p, f.size());
    let block = Block::default().title("Block").borders(Borders::ALL);
    f.render_widget(block, chunks[0]);
    let block = Block::default().title("Block 2").borders(Borders::ALL);
    f.render_widget(block, chunks[2]);
}
