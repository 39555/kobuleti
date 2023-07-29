


macro_rules! fn_send {
    ($cmd: expr => $sink: expr => $( $fname: ident($($vname:ident : $type: ty $(,)?)*); )+) => {
        paste::item! {
            $(pub fn $fname(&self, $($vname: $type,)*){
                let _ = self.$sink.send($cmd::[<$fname:camel>]($($vname, )*));
            }
            )*
        }
    }
}
pub(crate) use fn_send;

macro_rules! fn_send_and_wait_responce {
    ($cmd: expr => $sink: expr => $( $fname: ident($($vname:ident : $type: ty $(,)?)*) -> $ret: ty; )+) => {
        paste::item! {
            $( pub async fn $fname(&self, $($vname: $type,)*) -> $ret {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let _ = self.$sink.send($cmd::[<$fname:camel>]($($vname, )* tx));
                rx.await.expect(concat!("failed to process ", stringify!($fname)))
            }
            )*
        }
    }
}
pub(crate) use fn_send_and_wait_responce;
