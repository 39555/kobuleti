macro_rules! create_enum_iter {
    (
     $(#[$meta:meta])*
     $vis:vis enum $name:ident {
        $($(#[$vmeta:meta])* $vname:ident $(= $val:expr)?,)*
    }) => {
        $(#[$meta])*
        $vis enum $name {
            $($(#[$vmeta])* $vname $(= $val)?,)*
        }
        impl $name {
            const _ALL: [$name; create_enum_iter!(@count $($vname)*)] = [$($name::$vname,)*];
            pub fn iter() -> std::iter::Copied<std::slice::Iter<'static, $name>> {
                Self::_ALL.iter().copied()
            }
            pub const fn all() -> &'static[$name; create_enum_iter!(@count $($vname)*)]{
                &Self::_ALL
            }
            pub const fn count() -> usize {
                Self::_ALL.len()
            }
        }
    };
    (@count) => (0usize);
    (@count $x:tt $($xs:tt)* ) => (1usize + create_enum_iter!(@count $($xs)*));
}

pub(crate) use create_enum_iter;


macro_rules! impl_from {
    (
        impl From ($( $_ref: tt)?) $($src:ident)::+  $(<$($gen: ty $(,)?)*>)? for $dst: ty {
            $(
                $id:ident $(($value:tt))? => $dst_id:ident $(($data:expr))? $(,)?
            )+
        }

    ) => {
    impl From< $($_ref)? $($src)::+$(<$($gen,)*>)? > for $dst {
        fn from(src: $($_ref)? $($src)::+$(<$($gen,)*>)?) -> Self {
            use $($src)::+::*;
            match src {
                $($id $(($value))? => Self::$dst_id$(($data))?,)*
            }
        }
    }
    };
}

pub(crate) use impl_from;
