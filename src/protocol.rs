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
use thiserror::Error;


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

pub trait NextContext {
    fn next_context(next: Self) -> Self;
}
macro_rules! impl_next {
($type: ty, $( $src: ident => $next: ident $(,)?)+) => {
    impl NextContext for $type {
        fn next_context(next: Self) -> Self {
            use GameContext::*;
            match next {
                $(
                    $src(_) => { Self::$next(()) },
                )*
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



pub trait ToContext {
    type Next;
    type State;
    fn to(&mut self, next: Self::Next, state: &Self::State) ;
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub enum DataForNextContext<G>{
    Intro(()),
    Home(()),
    SelectRole(()),
    Game(G)
}


macro_rules! impl_from {
    ( ($( $_ref: tt)?) $($src:ident)::+  $(<$($gen: ident,)*>)? => $dst: ty,
        $( $id:ident $(($value:tt))? => $dst_id:ident $(,)?
     )+

    ) => {
    impl$(<$($gen,)*>)? From< $($_ref)? $($src)::+$(<$($gen,)*>)? > for $dst {
        fn from(src: $($_ref)? $($src)::+$(<$($gen,)*>)?) -> Self {
            use $($src)::+::*;
            #[allow(unreachable_patterns)]
            match src {
                $($id $(($value))? => Self::$dst_id(()),)*
                 _ => unimplemented!("unsupported conversion from {} into {}"
                                     , stringify!($($_ref)? $($src)::+ ), stringify!($dst))
            }
        }
    }
    };
}
macro_rules! impl_game_context_id_from {
    ( $(  $($type:ident)::+ $(<$($gen: ident,)*>)? $(,)?)+) => {
        $(impl_from!{  ( & ) $($type)::+ $(<$($gen,)*>)? => GameContextId,
                       Intro(_)      => Intro
                       Home(_)       => Home
                       SelectRole(_) => SelectRole
                       Game(_)       => Game
        })*
    }
}

impl_game_context_id_from!(  GameContext  <I, H, S, G,>
                          ,  client::Msg  
                          ,  server::Msg 
                          ,  DataForNextContext<T,>
              );






use crate::details::create_enum_iter;
create_enum_iter!{
    #[derive(Debug, Clone, PartialEq, Copy, Serialize, Deserialize)]
    pub enum Role {
        Warrior,
        Rogue,
        Paladin,
        Mage,
    }
}

impl Role {
    pub fn description(&self) -> &'static str {
        match self {
            Role::Warrior => include_str!("assets/roles/warrior/description.txt"),
            Role::Rogue => include_str!("assets/roles/rogue/description.txt"),
            Role::Paladin => include_str!("assets/roles/paladin/description.txt"),
            Role::Mage => include_str!("assets/roles/mage/description.txt"),
        }
    }

}
// 
#[derive(Error, Debug)]
pub enum MessageError {
    #[error("A message of unexpected context has been received
            (expected {current_context:?}, 
             found {message_context:?})")]
    UnexpectedContext{
        current_context: GameContextId,
        message_context: GameContextId
    },

    #[error("Failed to join to the game: {reason:?}")]
    LoginRejected{
        reason: String
    },
    
    // TODO maybe it is not error need work in server
    #[error("Client logout")]
    Logout,

    #[error("Unknown message error: {0:?}")]
    Unknown(String)

}

pub trait MessageReceiver<M, S> {
    fn message(&mut self, msg: M, state: S) -> Result<(), MessageError>;
}


#[async_trait]
pub trait AsyncMessageReceiver<M, S> {
    async fn message(&mut self, msg: M, state: S) -> Result<(), MessageError> 
    where S: 'async_trait;
}

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
        $($_async:ident)?, impl $msg_receiver: ident<$msg_type: ty, $state_type: ty> 
                           for $ctx_type: ident $(.$_await:tt)?) 
        => {

        $(#[$m])*
        impl<'a> $msg_receiver<$msg_type, $state_type> for $ctx_type{
            $($_async)? fn message(&mut self, msg: $msg_type, state:  $state_type) -> Result<(), MessageError> {
                let current_context = GameContextId::from(&*self);
                let message_context = GameContextId::from(&msg);
                if current_context != message_context {
                    return Err(MessageError::UnexpectedContext{
                                current_context,
                                message_context
                            });
                } else {
                    dispatch_msg!(self, msg, state ,
                                  $ctx_type => $msg_type {
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
impl_message_receiver_for!(
#[async_trait] 
    async,  impl AsyncMessageReceiver<client::Msg, &'a server::Connection> 
            for ServerGameContext  .await 
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_ccontext_id_not_equal_to_current() {
        assert_ne!(GameContextId::default(), GameContextId::next_context(GameContextId::default()))
    }
}

