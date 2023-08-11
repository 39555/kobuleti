
use ratatui::widgets::TableState;
use crate::game::Role;
use std::convert::AsRef;
use std::convert::AsMut;
use std::marker::PhantomData;


pub struct StatefulList3<E, T>
where 
    T : AsRef<[E]> + AsMut<[E]>
{
    pub state: TableState,
    pub items : T,
    __ : PhantomData<E>
}


pub struct StatefulList<E, T>
where 
    T : AsRef<[E]> + AsMut<[E]>
{
    pub selected: Option<usize>,
    pub active: Option<usize>,
    pub items : T,
    __ : PhantomData<E>
}

pub trait Statefulness
where for<'a> Self: 'a{
    type Item<'a>;
    fn next(&mut self);
    fn prev(&mut self);
    fn active(&self) -> Option<Self::Item<'_>>;
    fn selected(&self) -> Option<Self::Item<'_>>;
}

impl<E,  T: AsRef<[E]> + AsMut<[E]>> StatefulList<E, T> 
{
    pub fn with_items(items: T) -> Self {
        StatefulList {
            active: Some(0),
            selected: None,
            items, 
            __: PhantomData,
        }
    }
}



impl<E ,  T: AsRef<[E]> + AsMut<[E]> + Default> Default for StatefulList<E, T> {
    fn default() -> Self {
        StatefulList::with_items(Default::default())
    }
}

impl<E,  T: AsRef<[E]> + AsMut<[E]>> StatefulList3<E, T> 
{
    pub fn with_items(items: T) -> Self {
        StatefulList3 {
            state: TableState::default(),
            items, 
            __: PhantomData,
        }
    }
    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.as_ref().len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
    pub fn prev(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.as_ref().len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

}


impl Default for StatefulList3<Role, Vec<Role>>{
    fn default() -> Self {
        let mut l = StatefulList3::with_items(Role::all().to_vec());
        l.state.select(Some(0));
        l

    }
}
