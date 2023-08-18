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

macro_rules! impl_id_from_context_struct {
    ($($struct: ident)*) => {
        $(
            impl From<&$struct> for crate::protocol::GameContextKind {
                fn from(_: &$struct) -> Self {
                    crate::protocol::GameContextKind::$struct
                }
            }
        )*
    }
}

macro_rules! nested {
    // a enum with simple variants
    (@sub
        $( #[$meta:meta] )*
        $vis:vis enum $name:ident {
            $(

                $( #[$field_meta:meta] )*
                $field_vis:vis $variant:ident$(($data:ty))? ,

            )* $(,)?
        }
    ) => {
        $( #[$meta] )*
        $vis enum $name {
            $(
                $( #[$field_meta] )*
                $field_vis $variant$(($data))? ,

            )*
        }
    };
    // a enum with nested enums
    (@sub
        $( #[$meta:meta] )*
        $vis:vis enum $name:ident {
            $(

                $( #[$field_meta:meta] )*
                $field_vis:vis $variant:ident(

                    // nested enum
                    $( #[$sub_meta:meta] )*
                    $sub_vis:vis enum $sub_enum_name:ident {
                        $($sub_tt:tt)*
                    }

                ) ,

             )* $(,)?
        }
    ) => {
        // define main enum
        $( #[$meta] )*
        $vis enum $name {
            $(
                $( #[$field_meta] )*
                 $field_vis $variant($sub_enum_name),
            )*
        }
        // define nested
        $(
            nested!{@sub
                $( #[$sub_meta] )*
                $sub_vis enum $sub_enum_name {
                    $($sub_tt)*
                }
            }
        )*
    };
     // entry point
    (
        $( #[$meta:meta] )*
        $vis:vis enum $name:ident {
            $($tt:tt)*
        }


    ) => {
        nested!{@sub
            $( #[$meta] )*
            $vis enum $name {
                $($tt)*
            }
        }
    }
}

pub(crate) use nested;
