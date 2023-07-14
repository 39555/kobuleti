
use anyhow::anyhow;
use serde_json;
use serde::{Serialize, Deserialize};
use std::io::Error;
use tokio_util::codec::{ LinesCodec, Decoder};
use futures::{ Stream, StreamExt};
use std::io::ErrorKind;

#[macro_use]
mod details;
pub mod server;
pub mod client;


/// A lightweight id for ServerGameContext, ClientGameContext
#[derive(Debug, Default, Clone, PartialEq, Copy, Serialize, Deserialize)]
pub enum GameContextId{
    #[default]
    Intro,
    Home,
    Game

}
macro_rules! impl_next {
($type: ty, $( $src: ident => $next: ident $(,)?)+) => {
        pub fn next(src: $type) -> Self {
            #[allow(unreachable_patterns)]
            match src {
                $(<$type>::$src => Self::$next,)*
                 _ => unimplemented!("unsupported switch to the next game context")
            }
        }
    };
}
impl GameContextId {
impl_next!(  GameContextId,
             Intro =>   Home ,
             Home  =>   Game  
          );
}

macro_rules! impl_from {
($src: ty, $dst: ty, $( $src_v: ident => $next_v: ident $(,)?)+) => {
    impl From<&$src> for $dst {
        fn from(src: &$src) -> Self {
            use $src::*;
            #[allow(unreachable_patterns)]
            match src {
                $($src_v(_) => Self::$next_v,)*
                 _ => unimplemented!("unsupported conversion to GameContextId")
            }
        }
    }
    };
}
macro_rules! impl_id_from {
    ($($type:ty $(,)?)+) => {
        $(impl_from!{ $type,    GameContextId,
                       Intro  => Intro,
                       Home   => Home,
                       Game   => Game,
        })*
    }
}
impl_id_from!(  server::ServerGameContext
              , client::GameContext
              , client::Message
              , server::Message
              );



pub trait NextGameContext {
    fn to(&mut self, next: GameContextId);
}


pub trait MessageReceiver<M> {
    fn message(&mut self, msg: M)-> anyhow::Result<()>;
}


pub struct MessageDecoder<S> {
    pub stream: S
}
// TODO custom error message type
impl<S> MessageDecoder<S>
where S : Stream<Item=Result<<LinesCodec as Decoder>::Item
                           , <LinesCodec as Decoder>::Error>> 
        + StreamExt 
        + Unpin, {
    pub fn new(stream: S) -> Self {
        MessageDecoder { stream } 
    }
    pub async fn next<M>(&mut self) -> Result<M, Error>
    where
        M: for<'b> serde::Deserialize<'b> {
        match self.stream.next().await  {
            Some(msg) => {
                match msg {
                    Ok(msg) => {
                        serde_json::from_str::<M>(&msg)
                        .map_err(
                            |err| Error::new(ErrorKind::InvalidData, format!(
                                "failed to decode a type {} from the socket stream: {}"
                                    , std::any::type_name::<M>(), err))) 
                    },
                    Err(e) => {
                        Err(Error::new(ErrorKind::InvalidData, format!(
                            "an error occurred while processing messages from the socket: {}", e)))
                    }
                }
            },
            None => { // The stream has been exhausted.
                Err(Error::new(ErrorKind::ConnectionAborted, 
                        "received an unknown message. Connection rejected"))
            }
        }
    }

}


pub fn encode_message<M>(message: M) -> String
where M: for<'b> serde::Serialize {
    serde_json::to_string(&message).expect("failed to serialize a message to json")

}


