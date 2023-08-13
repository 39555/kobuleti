

macro_rules! dispatch_trait {
    (
       $trait_name:ident 
       fn $trait_func: ident(&$($mut:tt)? self, $($par: ident : $type: ty $(,)?)*) $(-> $ret: ty)?  { 

            $ctx: ident => 
                $($ctx_var: ident)* 
        }
     ) => {
        fn $trait_func(&$($mut)? self, $($par : $type,)*) $(-> $ret)? {
             dispatch_trait!(@call_nested_repeat 
                         $trait_name match self for $ctx {  
                             $($ctx_var),* 
                         }  $trait_func ($($par),*))
        }
    };

    (@call_nested_repeat $trait_name:ident
        match  $self:ident for $ctx:ident {
            $($fun:ident),* 
        } $f: ident  $tuple:tt) => {
        {
            use $ctx::*;
            //use GameContext::*;
            match $self {
                $(
                    $fun(c) =>  dispatch_trait!(@call_function $trait_name c.$f $tuple),
                )*
            }
        }
    };
    (@call_function $trait_name:ident $c:ident.$fun:ident ($($arg:expr),*)) => {
        $trait_name::$fun($c, $($arg,)*)
    };
    
}

pub(crate) use dispatch_trait;



/*
macro_rules! count {
    () => (0usize);
    ( $x:tt $($xs:tt)* ) => (1usize + count!($($xs)*));
}
*/
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
    // macro count!()
    (@count) => (0usize);
    (@count $x:tt $($xs:tt)* ) => (1usize + create_enum_iter!(@count $($xs)*));
}

pub(crate) use create_enum_iter;

/*
macro_rules! impl_from {
    ( impl From ($( $_ref: tt)?) $($src:ident)::+  $(<$($gen: ty $(,)?)*>)?  for $dst: ty,
        $( $id:ident $(($value:tt))? => $dst_id:ident($data:expr) $(,)?
     )+

    ) => {
    impl From< $($_ref)? $($src)::+$(<$($gen,)*>)? > for $dst {
        fn from(src: $($_ref)? $($src)::+$(<$($gen,)*>)?) -> Self {
            use $($src)::+::*;
            #[allow(unreachable_patterns)]
            match src {
                $($id $(($value))? => Self::$dst_id($data),)*
                 _ => unimplemented!("unsupported conversion from {} into {}"
                                     , stringify!($($_ref)? $($src)::+ ), stringify!($dst))
            }
        }
    }
    };
}
*/


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




macro_rules! impl_try_from_for_inner {
    ($vis:vis type $name:ident = $ctx: ident < 
        $( $($self_:ident)?:: $vname:ident => $enum_pat:ident , )*
    >;

    ) => {
        $vis type $name  = $ctx <
            $($vname,)*
        >;
        $(
        impl<'a> std::convert::TryFrom<&'a mut $name> for &'a mut $vname {
            type Error = String;
            fn try_from(other: &'a mut $name) -> Result<Self, Self::Error> {
                    match other {
                        $name::$enum_pat(v) => Ok(v),
                        _ => Err(concat!("The game context must be '", stringify!($enum_pat), "'").into()),
                    }
            }
        }
        impl<'a> std::convert::TryFrom<&'a $name> for &'a $vname {
            type Error = String;
            fn try_from(other: &'a $name) -> Result<Self, Self::Error> {
                    match other {
                        $name::$enum_pat(v) => Ok(v),
                        _ => Err(concat!("The game context must be '", stringify!($enum_pat), "'").into()),
                    }
            }
        }

        )*
    }
}
pub(crate) use  impl_try_from_for_inner;



