use anyhow::anyhow;

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

/// Shorthand for the transmit half of the message channel.
pub type Tx = tokio::sync::mpsc::UnboundedSender<String>;
/// Shorthand for the receive half of the message channel.
pub type Rx = tokio::sync::mpsc::UnboundedReceiver<String>;  


use ascension_macro::DisplayOnlyIdents;
use std::fmt::Display;


pub type Username = String;

#[derive(DisplayOnlyIdents, Debug, Clone, PartialEq, Copy, Serialize, Deserialize)]
pub enum GameContext<I,
                     H,
                     S,
                     G> {
    Intro     (I),
    Home      (H),
    SelectRole(S),
    Game      (G),
}


/// A lightweight id for ServerGameContext and ClientGameContext
pub type GameContextKind = GameContext::<(), (), (), ()>;
impl Default for GameContextKind {
    fn default() -> Self {
        GameContextKind::Intro(())
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
impl_next!(  GameContextKind,
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
            impl From ( & ) $($type)::+ $(<$($gen,)*>)? for GameContextKind {
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
                           
                            , GameContext <peer::IntroCmd,
                                          peer::HomeCmd, 
                                          peer::SelectRoleCmd, 
                                          peer::GameCmd>
                          //,  client::Msg  
                          //,  server::Msg 
                          ,  DataForNextContext<(), server::ServerStartGameData>           // ServerNextContextData
                          ,  DataForNextContext<Option<Role>, client::ClientStartGameData> // ClientNextContextData
              );

macro_rules! impl_try_from {
    ( 
        impl TryFrom ($( $_ref: tt)?) $($src:ident)::+ $(<$($gen: ty $(,)?)*>)? for $dst: ty {
            $( 
                $id:ident $(($value:tt))? => $dst_id:ident $(($data:expr))? $(,)?
            )+
        }

    ) => {
    impl TryFrom< $($_ref)? $($src)::+$(<$($gen,)*>)? > for $dst {
        type Error = anyhow::Error;
        fn try_from(src: $($_ref)? $($src)::+$(<$($gen,)*>)?) -> Result<Self, anyhow::Error> {
            use $($src)::+::*;
            #[allow(unreachable_patterns)]
            match src {
                $($id $(($value))? => Ok(Self::$dst_id$(($data))?),)*
                 _ => Err(anyhow!("unsupported conversion from {} into {}"
                                     , stringify!($($_ref)?$($src)::+), stringify!($dst)))
            }
        }
    }
    };
}
impl_try_from!{
    impl TryFrom ( & ) server::Msg for GameContextKind {
           Intro(_)      => Intro(())
           Home(_)       => Home(())
           SelectRole(_) => SelectRole(())
           Game(_)       => Game(())

    }
}
impl_try_from!{
    impl TryFrom ( & ) client::Msg for GameContextKind {
           Intro(_)      => Intro(())
           Home(_)       => Home(())
           SelectRole(_) => SelectRole(())
           Game(_)       => Game(())

    }
}

pub trait MessageReceiver<M, S> {
    fn message(&mut self, msg: M, state: S) -> anyhow::Result<()> ;
}


#[async_trait]
pub trait AsyncMessageReceiver<M, S> {
    async fn message(&mut self, msg: M, state: S) -> anyhow::Result<()> 
    where S: 'async_trait;
}



macro_rules! dispatch_msg {
    (/* GameContext enum value */         $ctx: expr, 
     /* {{client|server}}::Msg */         $msg: expr, 
     /* state for MessageReceiver 
      * ({{client|server}}::Connection)*/ $state: expr, 
     // GameContext or ClientGameContext => client::Msg or server::Msg
     $ctx_type:ty => $($msg_type:ident)::*{ 
        // Intro, Home, Game..
         $($ctx_v: ident  $(.$_await:tt)? $(,)?)+ 
     } ) => {
        {
            use $($msg_type)::* as MsgType;
            const MSG_TYPE_NAME: &str = stringify!($($msg_type)::*);
            let msg_ctx = GameContextKind::try_from(&$msg)?;
            match $ctx /*game context*/ {
                $(
                    GameContext::$ctx_v(ctx) => { 
                        ctx.message( match $msg {
                                        MsgType::$ctx_v(x) =>Some(x),
                                        _ => None,
                                    }.ok_or(anyhow!(concat!(
"wrong context requested to unwrap ( expected: {}::", stringify!($ctx_v),
", found {:?})"), MSG_TYPE_NAME, msg_ctx))?, 
                                $state) $(.$_await)? 
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
                           for $ctx_type: ident$(<$lifetime:lifetime>)? $(.$_await:tt)?) 
        => {

        $(#[$m])*
        impl<'a> $msg_receiver<$($msg_type)::*$(<$($gen,)*>)?, $state_type> for $ctx_type$(<$lifetime>)?{
            $($_async)? fn message(&mut self, msg: $($msg_type)::*$(<$($gen,)*>)?, state:  $state_type) -> anyhow::Result<()> {
                let current = GameContextKind::from(&*self);
                let other = GameContextKind::try_from(&msg)?;
                if current != other {
                    return Err(anyhow!(concat!("A message of unexpected context has been received \
for ", stringify!($ctx_type), "(expected {:?}, found {:?})"), current, other));
                } else {
                    dispatch_msg!(self, msg, state ,
                                  $ctx_type => $($msg_type)::*{
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
use  crate::server::peer::{ Connection};
impl_message_receiver_for!(
#[async_trait] 
    async,  impl AsyncMessageReceiver<client::Msg, &'a mut Connection> 
            for ServerGameContextHandle<'_>  .await 
);
use crate::server::peer::ContextCmd;

impl_message_receiver_for!(
#[async_trait] 
    async,  impl AsyncMessageReceiver<ContextCmd , &'a mut Connection> 
            for ServerGameContext  .await 
);



#[derive(Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize, DisplayOnlyIdents)]
pub enum GamePhaseKind {
    DropAbility,
    SelectAbility,
    AttachMonster,
    Defend,
}

#[derive(DisplayOnlyIdents, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnStatus {
    Ready(GamePhaseKind),
    Wait
}
impl TurnStatus{

    #[must_use]
    #[inline]
    pub fn is_ready_and(self, f: impl FnOnce(GamePhaseKind) -> bool) -> bool {
        match self {
            TurnStatus::Wait => false,
            TurnStatus::Ready(phase) => f(phase),
        }
    }
}


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
        assert_ne!(GameContextKind::default(), 
                   GameContextKind::try_next_context(GameContextKind::default())
                   .unwrap())
    } 
    #[test]
    fn should_not_panic_when_switch_to_next_context() {
         assert!(std::panic::catch_unwind(|| {
             let mut ctx = GameContextKind::default();
             for _ in 0..50 {
                ctx = match GameContextKind::try_next_context(GameContextKind::default()){
                    Ok(new_ctx) => new_ctx,
                    Err(_) => ctx
                }
             }
         }).is_ok());
    }

    #[test]
    fn should_dislay_only_enum_idents_without_values() {
        let ctx = GameContextKind::Intro(());
        assert_eq!(ctx.to_string(), "GameContextId::Intro");
    }
}

