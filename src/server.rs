

use anyhow::{self, Context};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use std::net::SocketAddr;

pub struct Server {
    //listen: SocketAddr,
    //connect: IpAddr,
      addr: SocketAddr
    //, udp: u16
}

impl Server {
    pub fn new(addr: SocketAddr) -> Self {
        Self {  addr }
    }

    pub async fn listen(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&self.addr).await?;
        println!("Listening on: {}", &self.addr);
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


