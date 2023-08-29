use std::io::{Error, ErrorKind};

use anyhow::anyhow;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_util::codec::{Decoder, LinesCodec};

#[macro_use]
mod details;
pub mod client;
pub mod server;
use client::ClientGameContext;
use derive_more::{Debug, From, TryUnwrap};


#[repr(transparent)]
#[derive(
    Debug, Clone, Deserialize, Serialize, PartialEq, Eq, derive_more::Display, derive_more::Deref,
)]
pub struct Username(
    #[display(forward)]
    #[deref(forward)]
    pub String,
);

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum Msg<SharedMsg, StateMsg> {
    Shared(SharedMsg),
    State(StateMsg),
}

pub trait IncomingSocketMessage
where
    for<'a> Self::Msg: serde::Deserialize<'a> + core::fmt::Debug,
{
    type Msg;
}
pub trait SendSocketMessage
where
    Self::Msg: serde::Serialize + core::fmt::Debug,
{
    type Msg;
}
pub trait NextState
where
    Self::Next: Send,
{
    type Next;
}
/// A lightweight id for ServerGameContext and ClientGameContext
macro_rules! kind {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident<$($gen:ident $(,)?)*> {
            $(
                $variant:ident($gen2:expr),
                )*
        }
        ) => {
        $(#[$meta])*
        $vis enum $name<$($gen,)*> {
            $(
                $variant($gen),

                )*
        }

        paste::item!{
            $(#[$meta])*
            $vis enum [<$name Kind>] {
                $(
                    $variant,

                    )*
            }
        }

        paste::item! {
            impl<$($gen ,)*> From<&$name<$($gen,)*>> for [<$name Kind>] {
                fn from(value: &$name<$($gen,)*>) -> Self {
                    match value {
                        $(
                            $name::$variant(_) => Self::$variant,
                        )*

                    }

                }
            }
        }
    }
}

kind! {
    #[derive(Debug, Clone, PartialEq, Copy, Serialize, Deserialize)]
    pub enum GameContext<I, H, S, G> {
        Intro(I),
        Home(H),
        Roles(S),
        Game(G),
    }
}

#[derive(thiserror::Error, Debug)]
#[error("unexpected context (expected = {expected:?}, found = {found:?})")]
pub struct UnexpectedContext {
    expected: GameContextKind,
    found: GameContextKind,
}

pub struct ContextConverter<From, To>(pub From, pub To);

#[derive(thiserror::Error, Debug)]
pub enum NextContextError {
    #[error("Data is missing = {0}")]
    MissingData(&'static str),
    #[error("Unimplemented next context ({current:?} -> {requested:?})")]
    Unimplemented {
        current: GameContextKind,
        requested: GameContextKind,
    },
    #[error("Requested the same context = ({0:?} -> {0:?})")]
    Same(GameContextKind),
}

impl Default for GameContextKind {
    fn default() -> Self {
        GameContextKind::Intro
    }
}
impl Iterator for GameContextKind {
    type Item = GameContextKind;
    fn next(&mut self) -> Option<Self::Item> {
        use GameContextKind::*;
        Some(match self {
            Intro => Home,
            Home => Roles,
            Roles => Game,
            Game => return None,
        })
    }
}

pub trait ToContext {
    type Next<'a>;
    fn to(&mut self, next: Self::Next<'_>) -> anyhow::Result<()>;
}

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
impl_try_from! {
    impl TryFrom ( & ) server::Msg for GameContextKind {
           Intro(_)      => Intro
           Home(_)       => Home
           Roles(_) => Roles
           Game(_)       => Game

    }
}
impl_try_from! {
    impl TryFrom ( & ) client::Msg for GameContextKind {
           Intro(_)      => Intro
           Home(_)       => Home
           Roles(_) => Roles
           Game(_)       => Game

    }
}

pub trait MessageReceiver<M, S> {
    fn reduce(&mut self, msg: M, state: S) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
pub trait AsyncMessageReceiver<M, S> {
    async fn reduce(&'_ mut self, msg: M, state: S) -> anyhow::Result<()>
    where
        S: 'async_trait;
}
/*
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
            match &mut $ctx.0 /*game context*/ {
                $(
                    GameContext::$ctx_v(ctx) => {
                        ctx.reduce( match $msg {
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
            $($_async)? fn reduce(&mut self, msg: $($msg_type)::*$(<$($gen,)*>)?, state:  $state_type) -> anyhow::Result<()> {
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
                                        Roles $(.$_await)?,
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
*/

/*
impl_message_receiver_for!(
#[async_trait]
    async,  impl AsyncMessageReceiver<client::Msg, &'a mut Connection>
            for ServerGameContextHandle<'a>  .await
);

macro_rules! try_unwrap {
    ($expr:expr, $($pat:ident)::*) => {
        match $expr {
             $($pat)::*(i) =>  i,
            _ => return Err(anyhow!(""))
        }
    }
}
*/
/*
#[async_trait]
impl<'a> AsyncMessageReceiver<client::Msg, &'a mut Connection> for ServerGameContextHandle<'a>{
    async fn reduce(&mut self, msg: client::Msg, state: &'a mut  Connection) -> anyhow::Result<()> {
            let current = GameContextKind::from(&*self);
                let other = GameContextKind::try_from(&msg)?;
                if current != other {
                    Err(anyhow!(""))
                } else {
                    match &mut self.0 {
                        GameContext::Intro(i) => i.reduce(try_unwrap!(msg, client::Msg::Intro), state).await,
                        GameContext::Home(h) =>  h.reduce(try_unwrap!(msg, client::Msg::Home), state).await,
                        GameContext::Roles(r) => r.reduce(try_unwrap!(msg, client::Msg::Roles), state).await,
                        GameContext::Game(g) =>  g.reduce(try_unwrap!(msg, client::Msg::Game), state).await,
                    }

                }
    }
}
*/

/*
impl_message_receiver_for!(
#[async_trait]
    async,  impl AsyncMessageReceiver<ContextCmd , &'a mut Connection>
            for ServerGameContext  .await
);
*/
#[derive(Default, Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize)]
pub enum GamePhaseKind {
    #[default]
    DropAbility,
    SelectAbility,
    AttachMonster,
    Defend,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnStatus {
    Ready(GamePhaseKind),
    Wait,
}
impl TurnStatus {
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
    stream: S,
}

impl<S> MessageDecoder<S>
where
    S: Stream<Item = Result<<LinesCodec as Decoder>::Item, <LinesCodec as Decoder>::Error>>
        + StreamExt
        + Unpin,
{
    pub fn new(stream: S) -> Self {
        MessageDecoder { stream }
    }
    pub async fn next<M>(&mut self) -> Option<Result<M, Error>>
    where
        M: for<'b> serde::Deserialize<'b>,
    {
        match self.stream.next().await {
            Some(msg) => {
                match msg {
                    Ok(msg) => Some(serde_json::from_str::<M>(&msg).map_err(|err| {
                        Error::new(
                            ErrorKind::InvalidData,
                            format!(
                                "Failed to decode a type {} from the socket stream: {}",
                                std::any::type_name::<M>(),
                                err
                            ),
                        )
                    })),
                    Err(e) => Some(Err(Error::new(
                        ErrorKind::InvalidData,
                        format!(
                            "An error occurred while processing messages from the socket: {}",
                            e
                        ),
                    ))), /*
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
            }
            None => {
                // The stream has been exhausted.
                None
            }
        }
    }
}

pub fn encode_message<M>(message: M) -> String
where
    M: for<'b> serde::Serialize,
{
    serde_json::to_string(&message).expect("Failed to serialize a message to json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_context_should_has_next_context() {
        let mut c = GameContextKind::default();
        assert_ne!(
            GameContextKind::default(),
            GameContextKind::default().next().unwrap()
        )
    }
    #[test]
    fn should_not_panic_when_switch_to_next_context() {
        assert!(std::panic::catch_unwind(|| {
            let mut ctx = GameContextKind::default();
            for _ in 0..50 {
                ctx = match GameContextKind::default().next() {
                    Some(new_ctx) => new_ctx,
                    None => ctx,
                }
            }
        })
        .is_ok());
    }
}
