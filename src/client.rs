use std::net::SocketAddr;

use anyhow::Context as _;
use tokio::net::TcpStream;

use crate::protocol::Username;

pub mod input;
pub mod states;
pub mod ui;

pub async fn connect(username: Username, host: SocketAddr) -> anyhow::Result<()> {
    let stream = TcpStream::connect(host)
        .await
        .with_context(|| format!("Failed to connect to address {}", host))?;
    // A client state machine
    states::run(username, stream, tokio_util::sync::CancellationToken::new()).await
}
