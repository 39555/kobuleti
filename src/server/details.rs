


macro_rules! fn_send {
    ($cmd: expr => $sink: expr => $( $vis:vis $fname: ident($($vname:ident : $type: ty $(,)?)*); )+) => {
        paste::item! {
            $($vis fn $fname(&self, $($vname: $type,)*){
                let _ = self.$sink.send($cmd::[<$fname:camel>]($($vname, )*));
            }
            )*
        }
    }
}
pub(crate) use fn_send;

macro_rules! fn_send_and_wait_responce {
    ($cmd: expr => $sink: expr => $( $vis:vis $fname: ident($($vname:ident : $type: ty $(,)?)*) -> $ret: ty; )+) => {
        paste::item! {
            $($vis async fn $fname(&self, $($vname: $type,)*) -> $ret {
                let (tx, rx) = tokio::sync::oneshot::channel();
                let _ = self.$sink.send($cmd::[<$fname:camel>]($($vname, )* tx));
                rx.await.expect(concat!("failed to process ", stringify!($fname)))
            }
            )*
        }
    }
}
pub(crate) use fn_send_and_wait_responce;


#[inline]
pub async fn oneshot_send_and_wait<Cmd, F,  R>(tx: &tokio::sync::mpsc::UnboundedSender<Cmd>, cmd_factory: F) -> R 
where
 F: Fn(tokio::sync::oneshot::Sender<R>)->Cmd
{
    let (one_tx, rx) = tokio::sync::oneshot::channel::<R>();
    let _ = tx.send(cmd_factory(one_tx));
    rx.await.expect(concat!("failed to process function"))
}
