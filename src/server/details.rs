use thiserror::Error;

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

#[derive(Debug, Clone, Copy)]
pub enum ActiveState {
    Enable(usize),
    Disable(usize),
}
impl ActiveState {
    pub fn unwrap_index(&self) -> usize {
        match *self {
            ActiveState::Enable(i) => i,
            ActiveState::Disable(i) => i,
        }
    }
}

pub trait StatebleItem {
    type Item;
}

pub struct Stateble<A, const ACTIVE_COUNT: usize>
where
    A: AsRef<[<A as StatebleItem>::Item]> + StatebleItem,
    <A as StatebleItem>::Item: Copy + Clone + PartialEq,
{
    pub items: A,
    pub actives: [ActiveState; ACTIVE_COUNT],
}

#[derive(Error, Debug)]
#[error("Active items reach end of all items")]
pub struct EndOfItems(usize);

impl<A, const ACTIVE_COUNT: usize> Stateble<A, ACTIVE_COUNT>
where
    A: AsRef<[<A as StatebleItem>::Item]> + StatebleItem,
    <A as StatebleItem>::Item: Copy + Clone + PartialEq,
{
    pub fn with_items(items: A) -> Self {
        Stateble::<A, ACTIVE_COUNT> {
            items,
            actives: core::array::from_fn(|i| ActiveState::Enable(i)),
        }
    }

    pub fn active_items(&self) -> [Option<<A as StatebleItem>::Item>; ACTIVE_COUNT] {
        let mut iter = self.actives.iter().map(|s| match s {
            ActiveState::Enable(s) => Some(self.items.as_ref()[*s]),
            ActiveState::Disable(_) => None,
        });
        core::array::from_fn(|_| iter.next().expect("next must exists"))
    }

    pub fn drop_item(&mut self, item: <A as StatebleItem>::Item) -> anyhow::Result<()> {
        let i = self
            .items
            .as_ref()
            .iter()
            .position(|i| *i == item)
            .ok_or_else(|| anyhow!("Item not found"))?;
        *self
            .actives
            .iter_mut()
            .find(|m| m.unwrap_index() == i)
            .ok_or_else(|| anyhow!("Item is not active"))? = ActiveState::Disable(i);
        Ok(())
    }

    pub fn next_actives(&mut self) -> Result<(), EndOfItems> {
        for (i, a) in self.actives.iter_mut().enumerate() {
            let new_index = if let ActiveState::Disable(d) = a {
                *d + ACTIVE_COUNT
            } else {
                a.unwrap_index()
            };
            if self.items.as_ref().get(new_index).is_some(){
                *a = ActiveState::Enable(new_index)
            } else {
                return Err(EndOfItems(i))
            }
        };
        Ok(())
        
    }

    pub fn repeat_after_eof(&mut self, eof: EndOfItems){
        for i in  0..(ACTIVE_COUNT - eof.0) {
            self.actives[i] = ActiveState::Enable(i);
        }

    }
    pub fn reset(&mut self) {
        self.actives = core::array::from_fn(|i| ActiveState::Enable(i));
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Deck, Deckable};

    fn stateble() -> Stateble::<Deck, 3>{
        Stateble::<Deck, 3>::with_items(Deck::default())
    }

    #[test]
    fn should_repeat_after_eof(){
        let mut st = stateble();

        //st.actives.iter().for_each(|i| println!("{:?}", i));
        for _ in 0..Deck::DECK_SIZE {
            for i in 0..3 {
                let _ = st.drop_item(st.active_items()[i].unwrap());
            }
            let _ = st.next_actives().map_err(|e| {
                st.repeat_after_eof(e);
            });
           //st.actives.iter().for_each(|i| println!("{:?}", i));
            //println!("---");
            assert!(st.active_items().iter().all(|i| i.is_some()))
        }
    }
}
