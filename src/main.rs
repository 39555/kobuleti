use std::{
    env,
    error::Error,
    net::{IpAddr, SocketAddr},
    path::Path,
    sync::Once,
};

use anyhow::{self, Context as _};
use clap::{self, arg, command};
use tokio::signal;
use tracing_subscriber::{self, filter::LevelFilter, prelude::*, EnvFilter};

mod client;
mod details;
mod game;
mod protocol;
mod server;

mod consts {
    macro_rules! make_pub_and_const {
        ( {$($field:ident = $value:expr;)*}) => {
            $(pub const $field : &str = $value);*;
        }
    }
    make_pub_and_const!({
        APPNAME = env!("CARGO_PKG_NAME");
        VERSION = env!("CARGO_PKG_VERSION");
        REPOSITORY = env!("CARGO_PKG_REPOSITORY");
        ISSUES = const_format::formatcp!("{}/issues/new", REPOSITORY);
        DEFAULT_TCP_PORT = "8000";
        DEFAULT_LOCALHOST = "127.0.0.1";
        LOG_ENV_VAR = const_format::concatcp!(
            const_format::map_ascii_case!(const_format::Case::Upper, APPNAME),
            "_LOG"
        );
    });
}

fn chain_panic() {
    static HOOK_HAS_BEEN_SET: Once = Once::new();
    HOOK_HAS_BEEN_SET.call_once(|| {
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            const PANIC_MSG: &str = const_format::formatcp!(
                "
        ============================================================
        \"{appname}\" has panicked. This is a bug in \"{appname}\". Please report this
        at {issues}. 
        If you can reliably reproduce this panic, include the
        reproduction steps and re-run with the RUST_BACKTRACE=1 env
        var set and include the backtrace in your report.\n
        Platform: {platform} {arch}
        Version: {version}
        ",
                appname = consts::APPNAME,
                issues = consts::ISSUES,
                platform = env::consts::OS,
                arch = env::consts::ARCH,
                version = consts::VERSION
            );
            eprint!("{}", PANIC_MSG);
            original_hook(panic);
        }));
    });
}

use commands::Command;
pub mod commands {
    use std::net::IpAddr;

    use anyhow::{self, Context as _};
    use clap::{self, arg};
    use const_format;

    use crate::{consts, protocol::Username};

    pub trait Command {
        const NAME: &'static str;
        fn new_command() -> clap::Command;
    }

    pub struct Server;
    impl Command for Server {
        const NAME: &'static str = "server";
        fn new_command() -> clap::Command {
            clap::Command::new(Server::NAME)
                .about(const_format::formatcp!(
                    "run {} dedicated server",
                    consts::APPNAME
                ))
                .arg(address())
                .arg(tcp())
        }
    }
    pub struct Client;
    impl Command for Client {
        const NAME: &'static str = "client";
        fn new_command() -> clap::Command {
            clap::Command::new(Client::NAME)
                .about("connect to the server and start a game")
                .arg(address())
                .arg(tcp())
                .arg(
                    arg!(
                        -n --name <USERNAME> "Your username in game"
                    )
                    .required(true)
                    .value_parser(username_parser),
                )
        }
    }
    fn address() -> clap::Arg {
        arg!(<HOST> "Set the IPv4 or IPv6 network address or use 'localhost' keyword")
            .default_value(consts::DEFAULT_LOCALHOST)
            .required(false)
            .value_parser(|addr: &str| -> anyhow::Result<IpAddr> {
                if addr == "localhost" {
                    consts::DEFAULT_LOCALHOST
                        .parse::<IpAddr>()
                        .context("[!] dev: failed to parse a default host address")
                } else {
                    addr.parse::<IpAddr>()
                        .context("Address must be a valid IPv4 or IPv6 address")
                }
            })
    }
    fn port_parser(port: &str) -> anyhow::Result<u16> {
        port.parse::<u16>().context(const_format::formatcp!(
            "The value must be in range {}-{}",
            u16::MIN,
            u16::MAX
        ))
    }
    fn tcp() -> clap::Arg {
        arg!(<PORT> "Set the TCP port for client connections")
            .default_value(consts::DEFAULT_TCP_PORT)
            .required(false)
            .value_parser(port_parser)
    }

    fn username_parser(name: &str) -> Result<Username, crate::protocol::UsernameError> {
        Username::new(
            arraystring::ArrayString::try_from_str(name)
                .map_err(|_| crate::protocol::UsernameError(name.len()))?,
        )
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    chain_panic();

    use tracing_subscriber::fmt::format::FmtSpan;
    let log = tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_env(consts::LOG_ENV_VAR).unwrap_or_else(|_| {
                EnvFilter::new(format!("{}={}", consts::APPNAME, LevelFilter::TRACE))
            }),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stdout)
                .with_ansi(!cfg!(feature = "console-subscriber"))
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .without_time(),
        );

    #[cfg(feature = "console-subscriber")]
    let log = log.with(console_subscriber::spawn());

    let matches = command!()
        .help_template(const_format::formatcp!(
            "\
{{before-help}}{{name}} {{version}}
{{author-with-newline}}{{about-with-newline}}
Project home page {}

{{usage-heading}} {{usage}}

{{all-args}}{{after-help}}
",
            consts::REPOSITORY
        ))
        .propagate_version(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(commands::Server::new_command())
        .subcommand(commands::Client::new_command())
        .arg(arg!( -l --log <FILE> "specify a log file").required(false))
        .get_matches();

    let (non_blocking, _guard);
    if let Some(file) = matches.get_one::<String>("log") {
        let file = Path::new(file);
        let file_appender =
            tracing_appender::rolling::never(file.parent().unwrap(), file.file_name().unwrap());
        (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
        log.with(tracing_subscriber::fmt::layer().with_writer(non_blocking))
            .init();
    } else {
        log.init();
    }

    let get_addr = |matches: &clap::ArgMatches| {
        SocketAddr::new(
            *matches.get_one::<IpAddr>("HOST").expect("Required"),
            *matches.get_one::<u16>("PORT").expect("Required"),
        )
    };
    match matches.subcommand() {
        Some((commands::Client::NAME, sub_matches)) => {
            client::connect(
                sub_matches
                    .get_one::<crate::protocol::Username>("name")
                    .expect("Required")
                    .to_owned(),
                get_addr(sub_matches),
            )
            .await
            .context("Error while run a client")?;
            tracing::info!("Quit the game");
        }
        Some((commands::Server::NAME, sub_matches)) => {
            println!(include_str!("assets/server_intro"));
            server::listen(get_addr(sub_matches), signal::ctrl_c())
                .await
                .context("Error while run a game server")?;
            tracing::info!("Close a game server");
        }
        _ => unreachable!("Exhausted list of subcommands.."),
    }
    Ok(())
}
