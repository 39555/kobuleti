use std::io::{Error, ErrorKind};

use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_util::codec::{Decoder, LinesCodec};

#[macro_use]
pub mod details;
pub mod client;
pub mod server;
use arraystring::{typenum::U20, ArrayString};
use derive_more::{Debug, From};

#[repr(transparent)]
#[derive(
    Default,
    derive_more::Debug,
    Clone,
    Deserialize,
    Serialize,
    PartialEq,
    Eq,
    derive_more::Display,
    derive_more::Deref,
)]
pub struct Username(
    #[display(forward)]
    #[debug("{_0}")]
    #[deref(forward)]
    ArrayString<U20>,
);
#[derive(thiserror::Error, Debug)]
#[error("Invalid username lenght. expected in range 2-20, found = {0} ")]
pub struct UsernameError(pub usize);
impl Username {
    pub fn new(value: ArrayString<U20>) -> Result<Self, UsernameError> {
        (value.len() > 1)
            .then(|| Username(value))
            .ok_or(UsernameError(value.len() as usize))
    }
}

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
/// A lightweight id for GameContext
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

pub trait MessageReceiver<M, S> {
    fn reduce(&mut self, msg: M, state: S) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
pub trait AsyncMessageReceiver<M, S> {
    async fn reduce(&'_ mut self, msg: M, state: S) -> anyhow::Result<()>
    where
        S: 'async_trait;
}

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

// Allow more control for automatic constructing different Msg in macros
pub(crate) trait With<T, R> {
    fn with(value: T) -> R;
}
macro_rules! impl_my_from_state_msg {
    ($client_or_server:ident => $($ty: ident $(,)?)*) => {
        $(
            impl With<$client_or_server::$ty,  Msg<$client_or_server::SharedMsg, $client_or_server::$ty>> for Msg<$client_or_server::SharedMsg, $client_or_server::$ty> {
                fn with(value: $client_or_server::$ty) -> Msg<$client_or_server::SharedMsg, $client_or_server::$ty> {
                    crate::protocol::Msg::State(value)
                }
            }
        )*
    };
}

impl_my_from_state_msg! {client => IntroMsg, HomeMsg, RolesMsg, GameMsg}
impl_my_from_state_msg! {server => IntroMsg, HomeMsg, RolesMsg, GameMsg}
impl<M> With<client::SharedMsg, Msg<client::SharedMsg, M>> for Msg<client::SharedMsg, M> {
    fn with(value: client::SharedMsg) -> Msg<client::SharedMsg, M> {
        Msg::Shared(value)
    }
}
impl<M> With<server::SharedMsg, Msg<server::SharedMsg, M>> for Msg<server::SharedMsg, M> {
    fn with(value: server::SharedMsg) -> Msg<server::SharedMsg, M> {
        Msg::Shared(value)
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

#[inline]
pub fn encode_message<M>(message: M) -> String
where
    M: for<'b> serde::Serialize,
{
    serde_json::to_string(&message).expect("Failed to serialize a message to json")
}

/*
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
*/
