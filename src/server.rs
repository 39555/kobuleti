

use anyhow::{self, Context};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct Server {
    //listen: SocketAddr,
    //connect: IpAddr,
      tcp: u16
    , udp: u16
}

impl Server {
    pub fn new(tcp: u16, udp: u16) -> Self {
        Self { tcp, udp }
    }

    pub async fn listen(&self) -> anyhow::Result<()> {
        // TODO what address?
        let address = format!("127.0.0.1:{}", self.tcp);
        let listener = TcpListener::bind(&address).await?;
        println!("Listening on: {}", address);

        loop {
            let (mut socket, _) = listener.accept().await?;
            tokio::spawn(async move {
                let mut buf = vec![0; 1024];
                loop {
                    let n = socket
                        .read(&mut buf)
                        .await
                        .expect("failed to read data from socket");

                    if n == 0 {
                        return;
                    }

                    socket
                        .write_all(&buf[0..n])
                        .await
                        .expect("failed to write data to socket");
                }
            });
        }
        //let mut incoming = listener.incoming();
        //let connect_address = self.connect_address;

        //while let Some(Ok(stream)) = incoming.next().await {
           // spawn(async move {
             //   if let Err(e) = process(stream, connect_address).await {
              //      println!("failed to process connection; error = {}", e);
               // }
           // });
       // }
        //Ok(())
    }
}


