



 macro_rules! unwrap_enum {
    ($enum:expr, $value:path) => (
        match $enum {
            $value(x) =>Some(x),
            _ => None,
        }
    )
}
pub(crate) use unwrap_enum;


macro_rules! impl_from_inner {
($( $src: ident{} $(,)?)+ => $dst: ty) => {
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

macro_rules! impl_unwrap_to_inner {
    ($(#[$meta:meta])* $vis:vis enum $name:ident {
        $($(#[$vmeta:meta])* $vname:ident $($in:tt)? $(= $val:expr)?,)*
    }) => {
        $(#[$meta])*
        $vis enum $name {
            $($(#[$vmeta])* $vname $($in)? $(= $val)?,)*
        }
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

pub(crate) use impl_unwrap_to_inner;
