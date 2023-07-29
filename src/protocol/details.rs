




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




macro_rules! impl_try_from_for_inner {
    ($vis:vis type $name:ident = $ctx: ident < 
        $( $($self_:ident)?:: $vname:ident, )*
    >;

    ) => {
        $vis type $name  = $ctx <
            $($vname,)*
        >;
        $(
        impl std::convert::TryFrom<$name> for $vname {
            type Error = $name;
            fn try_from(other: $name) -> Result<Self, Self::Error> {
                    match other {
                        $name::$vname(v) => Ok(v),
                        o => Err(o),
                    }
            }
        }
        )*
    }
}

pub(crate) use  impl_try_from_for_inner;




macro_rules! impl_id_from_context_struct {
    ($($struct: ident)*) => {
        $(
            impl From<&$struct> for GameContextId {
                fn from(_: &$struct) -> Self {
                    GameContextId::$struct(())
                }
            }
        )*
    }
}
