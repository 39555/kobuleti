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
use crate::server::peer::ServerGameContextHandle;
use thiserror::Error;

/// Shorthand for the transmit half of the message channel.
pub type Tx = tokio::sync::mpsc::UnboundedSender<String>;
/// Shorthand for the receive half of the message channel.
pub type Rx = tokio::sync::mpsc::UnboundedReceiver<String>;  

#[derive(Debug, Clone, PartialEq, Copy, Serialize, Deserialize)]
pub enum GameContext<I,
                     H,
                     S,
                     G> {
    Intro     (I),
    Home      (H),
    SelectRole(S),
    Game      (G),
}


/// A lightweight id for ServerGameContext, ClientGameContext
pub type GameContextId = GameContext::<(), (), (), ()>;
impl Default for GameContextId {
    fn default() -> Self {
        GameContextId::Intro(())
    }
}

pub trait TryNextContext {
    fn try_next_context(source: Self) -> anyhow::Result<Self>
        where Self: Sized;
}
macro_rules! impl_next {
($type: ty, $( $src: ident => $next: ident $(,)?)+) => {
    impl TryNextContext for $type {
        fn try_next_context(source: Self) -> anyhow::Result<Self> {
            use GameContext::*;
            match source {
                $(
                    $src(_) => { Ok(Self::$next(())) },
                )*
                _ => { 
                    Err(anyhow!("unsupported switch to the next game context from {:?}",source))
                }
            }
        }
    }
    };
}
impl_next!(  GameContextId,
             Intro      => Home
             Home       => SelectRole
             SelectRole => Game
          );


pub trait ToContext {
    type Next;
    type State;
    fn to(&mut self, next: Self::Next, state: &Self::State)  -> anyhow::Result<()>;
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum DataForNextContext<S, G>{
    Intro(()),
    Home(()),
    SelectRole(S),
    Game(G)
}


use crate::details::impl_from;

macro_rules! impl_game_context_id_from {
    ( $(  $($type:ident)::+ $(<$( $gen:ty $(,)?)*>)? $(,)?)+) => {
        $(impl_from!{ 
            impl From ( & ) $($type)::+ $(<$($gen,)*>)? for GameContextId {
                       Intro(_)      => Intro(())
                       Home(_)       => Home(())
                       SelectRole(_) => SelectRole(())
                       Game(_)       => Game(())
            }
        })*
    }
}
use crate::game::Role;
use crate::server::peer;
impl_game_context_id_from!(  GameContext <client::Intro, client::Home, client::SelectRole, client::Game>
                           , GameContext <server::Intro, server::Home, server::SelectRole, server::Game>
                           , GameContext <peer::IntroHandle,
                                          peer::HomeHandle, 
                                          peer::SelectRoleHandle, 
                                          peer::GameHandle>
                            , GameContext <peer::IntroCmd,
                                          peer::HomeCmd, 
                                          peer::SelectRoleCmd, 
                                          peer::GameCmd>
                          ,  client::Msg  
                          ,  server::Msg 
                          ,  DataForNextContext<(), server::ServerStartGameData>           // ServerNextContextData
                          ,  DataForNextContext<Option<Role>, client::ClientStartGameData> // ClientNextContextData
              );



pub trait MessageReceiver<M, S> {
    fn message(&mut self, msg: M, state: S) -> anyhow::Result<()> ;
}


#[async_trait]
pub trait AsyncMessageReceiver<M, S> {
    async fn message(&mut self, msg: M, state: S) -> anyhow::Result<()> 
    where S: 'async_trait;
}

// like matches!() but return inner value
macro_rules! unwrap_enum {
    ($enum:expr => $value:path) => (
        match $enum {
            $value(x) =>Some(x),
            _ => None,
        }
    )
}
macro_rules! dispatch_msg {
    (/* GameContext enum value */         $ctx: expr, 
     /* {{client|server}}::Msg */         $msg: expr, 
     /* state for MessageReceiver 
      * ({{client|server}}::Connection)*/ $state: expr, 
     // GameContext or ClientGameContext => client::Msg or server::Msg
     $ctx_type:ty => $msg_type: ty { 
        // Intro, Home, Game..
         $($ctx_v: ident  $(.$_await:tt)? $(,)?)+ 
     } ) => {
        {
            use GameContext::*;
            match $ctx /*game context*/ {
                $($ctx_v(ctx) => { 
                    use $msg_type::*;
                    let msg_ctx = GameContextId::from(&$msg);
                    ctx.message(unwrap_enum!($msg => /*Msg::*/$ctx_v)
                        .expect(&format!(concat!("wrong context message requested to unwrap
                                        , msg type: ",   stringify!($msg_type)
                                        , ", msg context {:?}, ",
                                        "game context: ", stringify!($ctx_v)), msg_ctx))
                        , $state)$(.$_await)?
                 } 
                ,)*
            }
        }
    }
}
macro_rules! impl_message_receiver_for {
    (
        $(#[$m:meta])* 
        $($_async:ident)?, impl $msg_receiver: ident<$($msg_type: ident)::* $(<$($gen:ident,)*>)?, $state_type: ty> 
                           for $ctx_type: ident $(.$_await:tt)?) 
        => {

        $(#[$m])*
        impl<'a> $msg_receiver<$($msg_type)::*$(<$($gen,)*>)?, $state_type> for $ctx_type{
            $($_async)? fn message(&mut self, msg: $($msg_type)::*$(<$($gen,)*>)?, state:  $state_type) -> anyhow::Result<()> {
                let current = GameContextId::from(&*self);
                let other = GameContextId::from(&msg);
                if current != other {
                    return Err(anyhow!(concat!("A message of unexpected context has been received \
for ", stringify!($ctx_type), "(expected {:?}, found {:?})"), current, other));
                } else {
                    dispatch_msg!(self, msg, state ,
                                  $ctx_type => $($msg_type)::* {
                                        Intro      $(.$_await)?,
                                        Home       $(.$_await)?,
                                        SelectRole $(.$_await)?,
                                        Game       $(.$_await)?,
                                   }
                    )
                }
            }
        }
    }
}

impl_message_receiver_for!(,
            impl MessageReceiver<server::Msg, &client::Connection> 
            for ClientGameContext
);


use async_trait::async_trait;
use  crate::server::peer::{ PeerHandle, Connection, IntroCmd, HomeCmd, SelectRoleCmd, GameCmd};
impl_message_receiver_for!(
#[async_trait] 
    async,  impl AsyncMessageReceiver<client::Msg, (&'a mut  PeerHandle ,&'a mut Connection)> 
            for ServerGameContextHandle  .await 
);
impl_message_receiver_for!(
#[async_trait] 
    async,  impl AsyncMessageReceiver<GameContext<IntroCmd, HomeCmd, SelectRoleCmd, GameCmd,> , &'a mut Connection> 
            for ServerGameContext  .await 
);


pub struct MessageDecoder<S> {
    stream: S
}

impl<S> MessageDecoder<S>
where S : Stream<Item=Result<<LinesCodec as Decoder>::Item
                           , <LinesCodec as Decoder>::Error>> 
        + StreamExt 
        + Unpin, {
    pub fn new(stream: S) -> Self {
        MessageDecoder { stream } 
    }
    pub async fn next<M>(&mut self) -> Option<Result<M, Error>>
    where
        M: for<'b> serde::Deserialize<'b> {
        match self.stream.next().await  {
            Some(msg) => {
                match msg {
                    Ok(msg) => {
                        Some(serde_json::from_str::<M>(&msg)
                        .map_err(
                            |err| Error::new(ErrorKind::InvalidData, format!(
                                "Failed to decode a type {} from the socket stream: {}"
                                    , std::any::type_name::<M>(), err)))) 
                    },
                    Err(e) => {
                        Some(Err(Error::new(ErrorKind::InvalidData, format!(
                            "An error occurred while processing messages from the socket: {}", e))))
                    }
                    /*
                     *  match e.kind() {
                        std::io::ErrorKind::BrokenPipe => {
                            player_handle.close("Closed by peer".to_string());
                        },
                        _ => {
                            player_handle.close(format!("Due to unknown error: {:?}", e));
                        }
                    }
                },

                     *
                     * */
                }
            },
            None => { 

                // The stream has been exhausted.
                None
             
            }
        }
    }

}


pub fn encode_message<M>(message: M) -> String
where M: for<'b> serde::Serialize {
    serde_json::to_string(&message).expect("Failed to serialize a message to json")

}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_context_should_has_next_context() {
        assert_ne!(GameContextId::default(), 
                   GameContextId::try_next_context(GameContextId::default())
                   .unwrap())
    } 
    #[test]
    fn should_not_panic_when_switch_to_next_context() {
         assert!(std::panic::catch_unwind(|| {
             let mut ctx = GameContextId::default();
             for _ in 0..50 {
                ctx = match GameContextId::try_next_context(GameContextId::default()){
                    Ok(new_ctx) => new_ctx,
                    Err(_) => ctx
                }
             }
         }).is_ok());
    }
}

