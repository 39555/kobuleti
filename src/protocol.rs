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
use client::ClientGameContext;
use server::ServerGameContext;
pub trait IsGameContext{}

macro_rules! all_contexts {
    ($name: ident) => {
        pub enum $name {
            Intro,
            Home,
            SelectRole,
            Game
        }

    }
}
//#[derive(Debug,Clone, PartialEq, Copy, Serialize, Deserialize)]
all_contexts!{TestContextId}

/// A lightweight id for ServerGameContext, ClientGameContext
#[derive(Debug, Default, Clone, PartialEq, Copy, Serialize, Deserialize)]
pub enum GameContextId{
    #[default]
    Intro,
    Home,
    SelectRole,
    Game

}
pub trait To {
    fn to(&mut self, next: GameContextId) -> &mut Self;
}
pub trait Next {
    fn next(next: Self) -> Self;
}
macro_rules! impl_next {
($type: ty, $( $src: ident => $next: ident $(,)?)+) => {
    impl Next for $type {
        fn next(next: Self) -> Self {
            match next {
                $(<$type>::$src => { Self::$next },)*
                _ => unimplemented!("unsupported switch to the next game context")
            }
        }
    }
    };
}
impl_next!(  GameContextId,
             Intro =>   Home
             Home  =>   SelectRole
            SelectRole => Game
          );

macro_rules! impl_from {
($src: ty => $dst: ty, $( $id: ident $(,)?)+) => {
    impl From<&$src> for $dst {
        fn from(src: &$src) -> Self {
            use $src::*;
            #[allow(unreachable_patterns)]
            match src {
                $($id(_) => Self::$id,)*
                 _ => unimplemented!("unsupported conversion into GameContextId")
            }
        }
    }
    };
}
macro_rules! impl_id_from {
    ($($type:ty $(,)?)+) => {
        $(impl_from!{ $type => GameContextId,
                       Intro
                       Home
                       SelectRole
                       Game 
        })*
    }
}
impl_id_from!(  server::ServerGameContext
              , client::ClientGameContext
              , client::Msg
              , server::Msg
              );


#[derive(Debug, Clone, PartialEq, Copy, Serialize, Deserialize)]
pub enum Role {
    Warrior,
    Rogue,
    Paladin,
    Mage,
}
impl Role {
    pub fn all_variants() -> [Role; 4] {
        [ Role::Warrior,
          Role::Rogue,
          Role::Paladin,
          Role::Mage,
        ]
    }
    pub fn description(&self) -> &'static str {
        match self {
            Role::Warrior => include_str!("assets/roles/warrior/description.txt"),
            Role::Rogue => include_str!("assets/roles/rogue/description.txt"),
            Role::Paladin => include_str!("assets/roles/paladin/description.txt"),
            Role::Mage => include_str!("assets/roles/mage/description.txt"),
        }
    }

}


macro_rules! dispatch_msg {
    ($ctx: expr, $msg: expr, $state: expr, $ctx_type:ty => $msg_type: ty { $($ctx_v: ident  $(.$_await:tt)? $(,)?)+ } ) => {
        {
            // avoid <type>::pat(_) ->"usage of qualified paths in this context is experimental..."
            use $ctx_type::{$($ctx_v,)*};
            match $ctx {
                $($ctx_v(ctx) => { 
                    use $msg_type::*;
                    ctx.message(unwrap_enum!($msg, $ctx_v).unwrap(), $state)$(.$_await)?
                 } 
                ,)*
            }
        }
    }
}
pub trait MessageReceiver<M, S> {
    fn message(&mut self, msg: M, state: S)-> anyhow::Result<()>;
}


#[async_trait]
pub trait AsyncMessageReceiver<M, S> {
    async fn message(&mut self, msg: M, state: S)-> anyhow::Result<()> where S: 'async_trait;
}

macro_rules! impl_message_receiver_for {
    ($(#[$m:meta])* $($_async:ident)?, impl $msg_receiver: ident<$msg_type: ty, $state_type: ty> for $ctx_type: ident $(.$_await:tt)?) => {
        $(#[$m])*
        impl<'a> $msg_receiver<$msg_type, $state_type> for $ctx_type{
            $($_async)? fn message(&mut self, msg: $msg_type, state:  $state_type) -> anyhow::Result<()> {
                let cur_ctx = GameContextId::from(&*self);
                let msg_ctx = GameContextId::from(&msg);
                if cur_ctx != msg_ctx{
                    return Err(anyhow!(
                            "a wrong message type for current stage '{:?}' was received: {:?}
                            , message {:?}", cur_ctx, msg_ctx, msg));
                }else {
                    dispatch_msg!(self, msg, state ,
                                  $ctx_type=> $msg_type {
                                                    Intro $(.$_await)?,
                                                    Home $(.$_await)?,
                                                    SelectRole $(.$_await)?,
                                                    Game $(.$_await)?,
                                                }
                    )
                }
            }
        }
    }
}
use async_trait::async_trait;

impl_message_receiver_for!(,
            impl MessageReceiver<server::Msg, &client::Connection> for ClientGameContext
);
impl_message_receiver_for!(
#[async_trait] 
    async,  impl AsyncMessageReceiver<client::Msg, &'a server::Connection> for ServerGameContext  .await 
);




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


