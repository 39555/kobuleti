

macro_rules! dispatch_trait {
    (
       $trait_name:ident fn $trait_func: ident(&$($mut:tt)? self, $($par: ident : $type: ty $(,)?)*) $(-> $ret: ty)?  { 
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

macro_rules! impl_from {
    ( ($( $_ref: tt)?)  $src: ty => $dst: ty,
        $( $id:ident $(($value:tt))? => $dst_id:ident $(,)?
     )+

    ) => {
    impl From< $($_ref)? $src> for $dst {
        fn from(src: $($_ref)? $src) -> Self {
            use $src::*;
            #[allow(unreachable_patterns)]
            match src {
                $($id $(($value))? => Self::$dst_id,)*
                 _ => unimplemented!("unsupported conversion from {} into {}"
                                     , stringify!($($_ref)? $src), stringify!($dst))
            }
        }
    }
    };
}
pub(crate) use impl_from;
