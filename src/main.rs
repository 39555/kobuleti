use std::{
    error::Error,
    sync::Once,
    env,
    net::{SocketAddr, IpAddr},
    path::Path
};
use anyhow::{self, Context};
use clap::{self, arg, command};
use const_format;
use tracing_subscriber::{self, prelude::*, EnvFilter};

mod client;
use client::Client;
mod server;
use server::Server;




mod consts {
    macro_rules! pub_and_const {
        ( {$($field:ident = $value:expr;)*}) => {
            $(pub const $field : &str = $value);*;
        }
    }
    pub_and_const!({
    AUTHORS           = env!("CARGO_PKG_AUTHORS");
    APPNAME           = env!("CARGO_PKG_NAME");
    VERSION           = env!("CARGO_PKG_VERSION");
    HOMEPAGE          = env!("CARGO_PKG_HOMEPAGE");
    REPOSITORY        = env!("CARGO_PKG_REPOSITORY");
    ISSUES            = const_format::formatcp!("{}/issues/new", REPOSITORY);
    DEFAULT_TCP_PORT  = "8000";
    DEFAULT_UDP_PORT  = "8000";
    DEFAULT_LOCALHOST = "127.0.0.1";
    LOG_ENV_VAR       = "ASCENSION_LOG";
    });

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


use commands::Command;
pub mod commands {
    use anyhow::{self, Context};
    use const_format;
    use clap::{self, arg};
    use std::net::IpAddr;
    use crate::consts;

    pub trait Command {
        const NAME: &'static str;
        fn new() -> clap::Command;
    }

    pub struct Server;
    impl Command for Server {
        const NAME : &'static str = "server";
        fn new() -> clap::Command {
             clap::Command::new(Server::NAME)
            //.arg_required_else_help(false)
            .about(const_format::formatcp!("run {} dedicated server", consts::APPNAME))
            .arg(address())
            .arg(tcp())
        }
    }
    pub struct Client;
    impl Command for Client {
        const NAME : &'static str = "client";
        fn new() -> clap::Command {
           clap::Command::new(Client::NAME)
            //.arg_required_else_help(false)
            .about("connect to the server and start a game")
            .arg(address())
            .arg(tcp())
        }
    }
    fn address() -> clap::Arg {
        arg!(<HOST> "Set the IPv4 or IPv6 network address or use 'localhost' keyword")
            .default_value(consts::DEFAULT_LOCALHOST)
            .required(false)
            .value_parser(|addr : &str| -> anyhow::Result<IpAddr> {
                if addr == "localhost" {
                    consts::DEFAULT_LOCALHOST.parse::<IpAddr>()
                    .context("[!] dev: failed to parse a default host address")
                } else {
                addr.parse::<IpAddr>()
                .context("Address must be a valid IPv4 or IPv6 address")
                }
        })
    }
    fn port_parser(port: &str) -> anyhow::Result<u16> {
        port.parse::<u16>().context(
            const_format::formatcp!("The value must be in range {}-{}"
            , u16::MIN, u16::MAX))
    }
    fn tcp() -> clap::Arg {
        arg!(<PORT> "Set the TCP port for client connections")
            .default_value(consts::DEFAULT_TCP_PORT)
            .required(false)
            .value_parser(port_parser)

    }
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    chain_panic();

    let log = tracing_subscriber::registry()
        .with(EnvFilter::try_from_env(consts::LOG_ENV_VAR)
            .unwrap_or_else(|_| EnvFilter::new("info")
        ))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout));

    let matches = command!()
        .help_template(const_format::formatcp!("\
{{before-help}}{{name}} {{version}}
{{author-with-newline}}{{about-with-newline}}
Project home page {}

{{usage-heading}} {{usage}}

{{all-args}}{{after-help}}
"           , consts::REPOSITORY))
        .propagate_version(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(commands::Server::new())
        .subcommand(commands::Client::new())
        .arg(arg!( -l --log <FILE> "specify a log file")
            .required(false)
                    )
        .get_matches();

    let (non_blocking, _guard); 
    if let Some(file) =  matches.get_one::<String>("log"){
        let file = Path::new(file);
        let file_appender = tracing_appender::rolling::never(
            file.parent().unwrap(), file.file_name().unwrap());
        (non_blocking,  _guard) = tracing_appender::non_blocking(file_appender);
        log.with(tracing_subscriber::fmt::layer().with_writer(non_blocking)).init();
    } else {
        log.init();
    }

    let get_addr = |matches: & clap::ArgMatches|{
        SocketAddr::new(
              *matches.get_one::<IpAddr>("HOST").expect("required")
            , *matches.get_one::<u16>("PORT").expect("required")
        )
    };
    match matches.subcommand() {
          Some((commands::Client::NAME , sub_matches) ) => {
            Client::new(
                get_addr(&sub_matches)
            )
            .connect()
            .await
            .context("failed to run a client")?;
        }
        , Some((commands::Server::NAME , sub_matches)) => {
            Server::new(
                get_addr(&sub_matches)
            )
            .listen()
            .await 
            .context("failed to run a game server")?;
        } 
        , _ => unreachable!("Exhausted list of subcommands and subcommand_required prevents `None`"),
    }
    Ok(())
}

