use anyhow::Context as _;
use std::net::SocketAddr;
use tokio::net::TcpStream;

use crate::protocol::Username;

pub mod input;
pub mod states;
pub mod ui;

pub async fn connect(username: Username, host: SocketAddr) -> anyhow::Result<()> {
    let stream = TcpStream::connect(host)
        .await
        .with_context(|| format!("Failed to connect to address {}", host))?;
    states::run(username, stream, tokio_util::sync::CancellationToken::new())
        .await
        .context("failed to process messages from the server")
}

/*
#[cfg(test)]
mod tests {
    use super::*;

    fn foo(cn: &mut ClientGameContext) -> Result<(), ()> {
        Ok(())
    }

    #[test]
    fn test_refcell() {
        let mut cn = ClientGameContext::default();
        foo(&mut cn).and_then(|_| {
            match cn.as_inner() {
                _ => (),
            };
            Ok(())
        });
    }
}
*/
