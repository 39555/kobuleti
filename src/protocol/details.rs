macro_rules! impl_GameContextKind_from_state {
    ($($ty: ty => $state:ident $(,)?)*) => {
        $(
            impl From<&$ty> for crate::protocol::GameContextKind {
                fn from(_: &$ty) -> Self {
                    crate::protocol::GameContextKind::$state
                }
            }
        )*
    };
    ($($ty: ident $(,)?)*) => {
        $(
            impl From<&$ty> for crate::protocol::GameContextKind {
                #[inline]
                fn from(_: &$ty) -> Self {
                    crate::protocol::GameContextKind::$ty
                }
            }
        )*
    }
}
pub(crate) use impl_GameContextKind_from_state;

use crate::{
    client::ui::details::{StatefulList, Statefulness},
    protocol::RoleStatus,
};

impl Statefulness for StatefulList<RoleStatus, [RoleStatus; 4]> {
    type Item<'a> = RoleStatus;
    fn next(&mut self) {
        self.active = {
            if self.active.is_none() || self.active.unwrap() >= self.items.as_ref().len() - 1 {
                Some(0)
            } else {
                Some(self.active.expect("Must be Some here") + 1)
            }
        };
    }
    fn prev(&mut self) {
        self.active = {
            if self.active.is_none() || self.active.unwrap() == 0 {
                Some(self.items.as_ref().len() - 1)
            } else {
                Some(self.active.expect("Must be Some here") - 1)
            }
        };
    }

    fn active(&self) -> Option<Self::Item<'_>> {
        self.active.map(|i| self.items.as_ref()[i])
    }
    fn selected(&self) -> Option<Self::Item<'_>> {
        self.selected.map(|i| self.items.as_ref()[i])
    }
}

impl<E, T: AsRef<[Option<E>]> + AsMut<[Option<E>]>> Statefulness for StatefulList<Option<E>, T>
where
    for<'a> E: 'a,
    for<'a> T: 'a,
{
    type Item<'a> = &'a E;
    fn next(&mut self) {
        self.active = {
            if self.items.as_ref().iter().all(|i| i.is_none()) {
                None
            } else {
                let active = self.active.expect("Must be Some");
                if active >= self.items.as_ref().len() - 1 {
                    let mut next = 0;
                    while self.items.as_ref()[next].is_none() {
                        next += 1;
                    }
                    Some(next)
                } else {
                    let mut next = active + 1;
                    while self.items.as_ref()[next].is_none() {
                        if next >= self.items.as_ref().len() - 1 {
                            next = 0;
                        } else {
                            next += 1;
                        }
                    }
                    Some(next)
                }
            }
        };
    }
    fn prev(&mut self) {
        self.active = {
            if self.items.as_ref().iter().all(|i| i.is_none()) {
                None
            } else {
                let active = self.active.expect("Must be Some");
                if active == 0 {
                    let mut next = self.items.as_ref().len() - 1;
                    while self.items.as_ref()[next].is_none() {
                        next -= 1;
                    }
                    Some(next)
                } else {
                    let mut next = active - 1;
                    while self.items.as_ref()[next].is_none() {
                        if next == 0 {
                            next = self.items.as_ref().len() - 1;
                        } else {
                            next -= 1;
                        }
                    }
                    Some(next)
                }
            }
        };
    }
    fn active(&self) -> Option<Self::Item<'_>> {
        if let Some(i) = self.active {
            Some(self.items.as_ref()[i].as_ref().expect("Must be Some"))
        } else {
            None
        }
    }

    fn selected(&self) -> Option<Self::Item<'_>> {
        if let Some(i) = self.selected {
            Some(self.items.as_ref()[i].as_ref().expect("Must be Some"))
        } else {
            None
        }
    }
}
