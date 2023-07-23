



macro_rules! unwrap_enum {
    ($enum:expr => $value:path) => (
        match $enum {
            $value(x) =>Some(x),
            _ => None,
        }
    )
}
pub(crate) use unwrap_enum;


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
