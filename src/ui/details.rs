use std::{
    convert::{AsMut, AsRef},
    marker::PhantomData,
};

pub struct StatefulList<E, T>
where
    T: AsRef<[E]> + AsMut<[E]>,
{
    pub selected: Option<usize>,
    pub active: Option<usize>,
    pub items: T,
    __: PhantomData<E>,
}

pub trait Statefulness
where
    for<'a> Self: 'a,
{
    type Item<'a>;
    fn next(&mut self);
    fn prev(&mut self);
    fn active(&self) -> Option<Self::Item<'_>>;
    fn selected(&self) -> Option<Self::Item<'_>>;
}

impl<E, T: AsRef<[E]> + AsMut<[E]>> StatefulList<E, T> {
    pub fn with_items(items: T) -> Self {
        StatefulList {
            active: Some(0),
            selected: None,
            items,
            __: PhantomData,
        }
    }
}

impl<E, T: AsRef<[E]> + AsMut<[E]> + Default> Default for StatefulList<E, T> {
    fn default() -> Self {
        StatefulList::with_items(Default::default())
    }
}
