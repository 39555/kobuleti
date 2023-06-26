use std::error::Error;
use anyhow::{self, Context};
use clap::command;
use const_format;
use std::sync::Once;
use std::env;
use std::net::SocketAddr;

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
    use std::net::SocketAddr;
    use crate::consts;

    pub trait Command {
        const NAME: &'static str;
        fn new() -> clap::Command;
    }

    pub struct Server;
    impl Command for Server {
        const NAME : &'static str = "server";
        fn new() -> clap::Command {
             let port_parser = |port: &str| -> anyhow::Result<u16> {
                port.parse::<u16>().context(
                const_format::formatcp!("The value must be in range {}-{}"
                    , u16::MIN, u16::MAX))
            };
             clap::Command::new(Server::NAME)
            .arg_required_else_help(false)
            .about(const_format::formatcp!("run {} server mode", consts::APPNAME))
            .arg(arg!(
                -t --tcp <PORT> "Set the tcp port for client connections"
                )
                .default_value(consts::DEFAULT_TCP_PORT)
                .value_parser(port_parser)
            )
            .arg(arg!(
                -u --udp <PORT> "Set the udp port for client connections"
                )
                .default_value(consts::DEFAULT_UDP_PORT)
                .value_parser(port_parser)
            )
            .arg(log_file())
            .arg(verbosity())
        }
    }
    pub struct Client;
    impl Command for Client {
        const NAME : &'static str = "client";
        fn new() -> clap::Command {
           clap::Command::new(Client::NAME)
            .arg_required_else_help(false)
            .about(const_format::formatcp!("run {} client mode", consts::APPNAME))
            .arg(arg!(-o --host <HOST> "Set the server address (ip and port). Format example: 127.0.0.1:3549")
            .value_parser(|host : &str |  -> anyhow::Result<SocketAddr> {
                host.parse::<SocketAddr>().context("Host must be a valid network address") 
             })
            .required(true))
            .arg(log_file())
            .arg(verbosity())
        }
    }

    fn log_file() -> clap::Arg {
        arg!(-l --log <FILE> "specify a log file").required(false)
    }
    fn verbosity() -> clap::Arg {
        arg!(-v --verbosity <TYPE> "set log level of verbosity").required(false)
    }
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
        .subcommand(commands::Server::new())
        .subcommand(commands::Client::new())
        .get_matches();
    match matches.subcommand() {
          Some((commands::Client::NAME , sub_matches) ) => {
            Client::new(
                *sub_matches.get_one::<SocketAddr>("host").expect("required"))
            .main()
            .await
            .context("failed to run a client")?;
        }
        , Some((commands::Server::NAME , sub_matches)) => {
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

