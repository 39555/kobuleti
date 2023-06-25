use std::error::Error;
use anyhow::{self, Context};
use clap::{arg, command, Command};
use const_format;
use std::sync::Once;
use std::env;
use std::net::SocketAddr;

mod client;
use client::Client;
mod server;
use server::Server;

pub mod consts {
    pub const AUTHORS    : &str = env!("CARGO_PKG_AUTHORS");
    pub const APPNAME    : &str = env!("CARGO_PKG_NAME");
    pub const VERSION    : &str = env!("CARGO_PKG_VERSION");
    pub const HOMEPAGE   : &str = env!("CARGO_PKG_HOMEPAGE");
    pub const REPOSITORY : &str = env!("CARGO_PKG_REPOSITORY");
    pub const ISSUES     : &str = const_format::formatcp!("{}/issues/new", REPOSITORY);
    pub const DEFAULT_TCP_PORT : &str = "3549";
    pub const DEFAULT_UDP_PORT : &str = "3549";

}


fn chain_panic(){
    static HOOK_HAS_BEEN_SET: Once = Once::new();
    HOOK_HAS_BEEN_SET.call_once(|| {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
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
        , appname=consts::APPNAME
        , issues=consts::ISSUES
        , platform=env::consts::OS, arch=env::consts::ARCH
        , version = consts::VERSION
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

fn port_value_parser(port: &str) -> anyhow::Result<u16> {
    port.parse::<u16>().context(format!("The value must be in range {}-{}"
        , u16::MIN, u16::MAX))
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    chain_panic();
    let matches = command!()
        .help_template(const_format::formatcp!("\
{{before-help}}{{name}} {{version}}
{{author-with-newline}}{{about-with-newline}}
Project home page {}

{{usage-heading}} {{usage}}

{{all-args}}{{after-help}}
", consts::REPOSITORY))
        .propagate_version(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("server")
                .arg_required_else_help(true)
                .about(const_format::formatcp!("run {} server mode", consts::APPNAME))
                .arg_log_file()
                .arg_verbosity()
                .arg(arg!(
                    -t --tcp <PORT> "Set the tcp port for client connections"
                    )
                    .default_value(consts::DEFAULT_TCP_PORT)
                    .value_parser(port_value_parser)
                    )
                .arg(arg!(-u --udp <PORT> "Set the udp port for client connections")
                    .default_value(consts::DEFAULT_UDP_PORT)
                    .value_parser(port_value_parser)
                    )
                ,
        )
        .subcommand(
            Command::new("client")
                .arg_required_else_help(true)
                .about(const_format::formatcp!("run {} client mode", consts::APPNAME))
                .arg(arg!(-o --host <HOST> "Set the server address (ip and port). Format example: 127.0.0.1:3549")
                .value_parser(|host : &str |  -> anyhow::Result<SocketAddr> {
                    host.parse::<SocketAddr>().context("Host must be a valid network address") 
                 })
                .required(true))
                .arg_log_file()
                .arg_verbosity()
                
        )
        .get_matches();

    match matches.subcommand() {
          Some(("client", sub_matches)) => {
            Client::new(
                *sub_matches.get_one::<SocketAddr>("host").expect("required"))
            .connect()
            .await
            .context("failed to connect a client")?;

        }
        , Some(("server", sub_matches)) => {
            Server::new(
                  *sub_matches.get_one::<u16>("tcp").expect("required")
                , *sub_matches.get_one::<u16>("udp").expect("required"))
            .listen()
            .await 
            .context("failed to run a game server")?;
        } 
        , _ => unreachable!("Exhausted list of subcommands and subcommand_required prevents `None`"),
    }
    Ok(())
}

