use thiserror::Error;

#[inline]
pub async fn send_oneshot_and_wait<Cmd, F, R>(
    tx: &super::Tx<Cmd>,
    cmd_factory: F,
) -> Result<R, tokio::sync::oneshot::error::RecvError>
where
    F: FnOnce(tokio::sync::oneshot::Sender<R>) -> Cmd,
{
    let (one_tx, rx) = tokio::sync::oneshot::channel::<R>();
    tx.send(cmd_factory(one_tx)).await.unwrap();
    rx.await
}

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

#[derive(Debug)]
pub struct Stateble<A, const ACTIVE_COUNT: usize>
where
    A: AsRef<[<A as StatebleItem>::Item]> + StatebleItem,
    <A as StatebleItem>::Item: PartialEq + Eq,
{
    pub items: A,
    pub actives: [ActiveState; ACTIVE_COUNT],
}

#[derive(Error, Debug)]
#[error("Active items reach end of all items")]
pub struct EndOfItems(usize);

#[derive(Error, Debug)]
pub enum DeactivateItemError {
    #[error("Item not found")]
    NotFound,
    #[error("Item is already not active")]
    AlreadyNotActive,
}

impl<A, const ACTIVE_COUNT: usize> Stateble<A, ACTIVE_COUNT>
where
    A: AsRef<[<A as StatebleItem>::Item]> + StatebleItem,
    <A as StatebleItem>::Item: PartialEq + Eq,
{
    pub fn with_items(items: A) -> Self {
        Stateble::<A, ACTIVE_COUNT> {
            items,
            actives: core::array::from_fn(ActiveState::Enable),
        }
    }

    pub fn active_items(&self) -> [Option<&<A as StatebleItem>::Item>; ACTIVE_COUNT] {
        self.actives.map(|s| match s {
            ActiveState::Enable(s) => Some(&self.items.as_ref()[s]),
            ActiveState::Disable(_) => None,
        })
    }
    pub fn deactivate_item_by_index(&mut self, i: usize) -> Result<(), DeactivateItemError> {
        *self
            .actives
            .iter_mut()
            .find(|m| m.unwrap_index() == i)
            .ok_or(DeactivateItemError::AlreadyNotActive)? = ActiveState::Disable(i);
        Ok(())
    }

    pub fn deactivate_item(
        &mut self,
        item: &<A as StatebleItem>::Item,
    ) -> Result<(), DeactivateItemError> {
        let i = self
            .items
            .as_ref()
            .iter()
            .position(|i| *i == *item)
            .ok_or(DeactivateItemError::NotFound)?;
        *self
            .actives
            .iter_mut()
            .find(|m| m.unwrap_index() == i)
            .ok_or(DeactivateItemError::AlreadyNotActive)? = ActiveState::Disable(i);
        Ok(())
    }

    pub fn next_actives(&mut self) -> Result<(), EndOfItems> {
        for (i, a) in self.actives.iter_mut().enumerate() {
            let new_index = if let ActiveState::Disable(d) = a {
                *d + ACTIVE_COUNT
            } else {
                a.unwrap_index()
            };
            if self.items.as_ref().get(new_index).is_some() {
                *a = ActiveState::Enable(new_index)
            } else {
                return Err(EndOfItems(i));
            }
        }
        Ok(())
    }

    pub fn repeat_after_eof(&mut self, eof: EndOfItems) {
        for i in 0..(ACTIVE_COUNT - eof.0) {
            self.actives[i] = ActiveState::Enable(i);
        }
    }
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.actives = core::array::from_fn(ActiveState::Enable);
    }
}

macro_rules! actor_api {

    // entry with SharedCmd
    (impl<M> Handle<Msg<SharedCmd, M>> {$($input:tt)*}) => {
        actor_api!{@impl_enum SharedCmd { $($input)* }}
        #[allow(dead_code)]
        impl<M> Handle<Msg<SharedCmd, M>> {
            actor_api!{@impl_api SharedCmd { $($input)* } }
        }

    };
    // entry with any context cmd
    (impl Handle<Msg<SharedCmd, $cmd:ident>> {$($input:tt)*}) => {
        actor_api!{@impl_enum $cmd { $($input)* }}

        #[allow(dead_code)]
        impl Handle<Msg<SharedCmd, $cmd>> {
            actor_api!{@impl_api $cmd { $($input)* } }
        }

    };
    // entry with full template cmd for Intro context
    (impl Handle<$cmd:ident> {$($input:tt)*}) => {
        actor_api!{@impl_enum $cmd { $($input)* }}

        #[allow(dead_code)]
        impl Handle<$cmd> {
            actor_api!{@impl_api $cmd { $($input)* } }
        }

    };

    (@impl_enum $cmd:ident {
        $($vis:vis $(async)? fn $fname:ident(&self $(,$_:ident : $ty:ty)*) $( -> Result<$ret:ty, RecvError> )?;)*

    }) => {

        paste::item!{
            #[derive(derive_more::Debug)]
            pub enum $cmd {
                $(
                [<$fname:camel>] ($($ty,)* $( #[debug(skip)] Answer<$ret>)?),
            )*

            }
        }
    };

    // End of tt muncher
    (@impl_api $cmd: ident {} ) => ();

    (
        @impl_api $cmd: ident {
             $vis:vis async fn $fname: ident(&self$(,)? $($vname:ident : $type: ty $(,)?)*); $($tail:tt)*
        }

    ) => {
        paste::item! {
            #[inline]
            $vis async fn $fname(&self, $($vname: $type,)*){
                self.tx.send(<Msg<_, _> as crate::protocol::With<_, _>>::with($cmd::[<$fname:camel>]($($vname, )*))).await.unwrap();
            }

        }
        actor_api!{ @impl_api $cmd { $($tail)* }}
    };
    (
        @impl_api $cmd: ident {
             $vis:vis async fn $fname: ident(&self$(,)? $($vname:ident : $type: ty $(,)?)*) -> $ret: ty; $($tail:tt)*
        }

    ) => {
        paste::item! {
            #[must_use = "Unused api responce"]
            #[inline]
            $vis async fn $fname(&self, $($vname: $type,)*) -> $ret {
                crate::server::details::send_oneshot_and_wait(&self.tx, |tx| <Msg<_, _> as crate::protocol::With<_, _>>::with($cmd::[<$fname:camel>]($($vname, )* tx))).await
            }

        }
        actor_api!{ @impl_api $cmd { $($tail)* }}
    };



}
pub(crate) use actor_api;
