

use anyhow::{self, Context};

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
        //let mut listener = TcpListener::bind(&self.listen_address).await?;
        //let mut incoming = listener.incoming();
        //let connect_address = self.connect_address;

        //while let Some(Ok(stream)) = incoming.next().await {
           // spawn(async move {
             //   if let Err(e) = process(stream, connect_address).await {
              //      println!("failed to process connection; error = {}", e);
               // }
           // });
       // }
        Ok(())
    }
}


