macro_rules! impl_from_inner {
($( $src: ident $(,)?)+ => $dst: ty) => {
    $(
    impl From<$src> for $dst {
        fn from(src: $src) -> Self {
            Self::$src(src)
        }
    }
    )*
    };
}

pub(crate) use impl_from_inner;

macro_rules! impl_try_from_msg_for_msg_event {
    (impl std::convert::TryFrom $($name:ident::$path:ident for $for:ident)*) => {
        $(
            impl std::convert::TryFrom<$name> for $for {
                type Error = $name;
                fn try_from(other: $name) -> Result<Self, Self::Error> {
                    match other {
                        $name::$path(v) => Ok(v),
                        o => Err(o),
                    }
                }
            }
        )*
    }
}

macro_rules! impl_from_msg_event_for_msg {
    (impl std::convert::From $($name:ident => $msg:ident::$path:ident)*) => {
        $(
            impl std::convert::From<$name> for $msg {
                fn from(other: $name) -> Self {
                    $msg::$path(other)
                }
            }
        )*
    }
}

macro_rules! impl_GameContextKind_from_state {
    ($($ty: ty => $state:ident $(,)?)*) => {
        $(
            impl From<&$ty> for crate::protocol::GameContextKind {
                fn from(_: &$ty) -> Self {
                    crate::protocol::GameContextKind::$state
                }
            }
        )*
    };
    ($($ty: ident $(,)?)*) => {
        $(
            impl From<&$ty> for crate::protocol::GameContextKind {
                #[inline]
                fn from(_: &$ty) -> Self {
                    crate::protocol::GameContextKind::$ty
                }
            }
        )*
    }
}
pub(crate) use impl_GameContextKind_from_state;
