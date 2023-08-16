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
pub async fn send_oneshot_and_wait<Cmd, F, R>(
    tx: &tokio::sync::mpsc::UnboundedSender<Cmd>,
    cmd_factory: F,
) -> R
where
    F: FnOnce(tokio::sync::oneshot::Sender<R>) -> Cmd,
{
    let (one_tx, rx) = tokio::sync::oneshot::channel::<R>();
    let _ = tx.send(cmd_factory(one_tx));
    rx.await.expect(concat!("failed to process api request"))
}



use anyhow::anyhow;

#[derive(Clone, Copy)]
pub enum ActiveState {
    Alive(usize),
    Dead(usize),
}
impl ActiveState {
    pub fn unwrap_index(&self) -> usize {
        match *self {
            ActiveState::Alive(i) => i,
            ActiveState::Dead(i)  => i,
        }
    }
}
use std::marker::PhantomData;
pub struct StatebleArray<A, T, const ACTIVE_COUNT: usize>
where A: AsRef<[T]>,
      T: Copy + Clone
{
    items : A,
    actives: [ActiveState; ACTIVE_COUNT],
    __ : PhantomData<T>
}

impl<A, T, const ACTIVE_COUNT: usize> StatebleArray<A, T, ACTIVE_COUNT> 
where A: AsRef<[T]> + AsMut<[T]>,
      T: Copy + Clone + PartialEq

{
    pub fn with_items(items: A) -> Self {
        StatebleArray::<A, T, ACTIVE_COUNT>{
            items, 
            actives: core::array::from_fn(|i| ActiveState::Alive(i)),
            __ : PhantomData
        }
    }

    pub fn active_items(&self) -> [Option<T>; ACTIVE_COUNT]{
        let mut iter = self.actives.iter().map(|s| match s {
            ActiveState::Alive(s) => Some(self.items.as_ref()[*s]),
            ActiveState::Dead(_)  => None,
        });
        core::array::from_fn(|_| iter.next().expect("next must exists"))
    }

    pub fn drop_item(&mut self, item: T) -> anyhow::Result<()> {
        let i = self
            .items.as_ref()
            .iter()
            .position(|i| *i == item)
            .ok_or_else(|| anyhow!("Item not found"))?;
        *self
            .actives
            .iter_mut()
            .find(|m| m.unwrap_index() == i)
            .ok_or_else(|| anyhow!("Item is not active"))? = ActiveState::Dead(i);
        Ok(())
    }
    pub fn is_all_dead(&self) -> bool {
        self.actives.iter().all(|i| matches!(i, ActiveState::Dead(_)))
    }
    pub fn next_actives(&mut self) -> anyhow::Result<()> {
        let mut new_actives = self.actives.iter().enumerate().map(|(i, a)| {
            if let ActiveState::Dead(d) = a{
                *d + i + 1
            } else {
                a.unwrap_index()
            }
        });
        if new_actives.all(|i| self.items.as_ref().get(i).is_some()) {
            let mut iter = new_actives.map(|a| ActiveState::Alive(a));
            self.actives = core::array::from_fn(|_| iter.next().expect("Index must exists"));
            Ok(())
        } else {
            // TODO errorkind
            Err(anyhow!("EOF"))
            
        }
    }

}

