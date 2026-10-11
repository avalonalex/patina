//! The root providers registered with a heap (#682, `PRD/GC_PRD.md` §14):
//! every provider that outlives one collection, traced by each collection of
//! the heap beside the state the running machine lends it.
//!
//! A registration lasts as long as its [`RootToken`]. The token holds the
//! set weakly, so a token kept past its heap holds nothing, and a heap kept
//! past its tokens has no providers left.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};

use super::gc::RootProvider;

/// The providers registered with one heap. Each collection traces a copy of
/// the list taken as it starts, so a token dropped while one runs, by a
/// payload the sweep drops, takes its provider out of the next collection,
/// not out of this one.
#[derive(Default)]
pub struct RootSet {
    providers: RefCell<Vec<(u64, Rc<dyn RootProvider>)>>,
    next_id: Cell<u64>,
}

impl RootSet {
    /// Register `provider`: every collection of this heap traces it until
    /// the token returned is dropped.
    pub fn register(self: &Rc<Self>, provider: Box<dyn RootProvider>) -> RootToken {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.providers.borrow_mut().push((id, Rc::from(provider)));
        RootToken {
            set: Rc::downgrade(self),
            id,
        }
    }

    /// Whether every registered provider can report its roots now
    /// ([`RootProvider::ready`]).
    pub(crate) fn ready(&self) -> bool {
        self.providers.borrow().iter().all(|(_, p)| p.ready())
    }

    /// The providers registered now, for one collection.
    pub(crate) fn snapshot(&self) -> Vec<Rc<dyn RootProvider>> {
        self.providers
            .borrow()
            .iter()
            .map(|(_, p)| Rc::clone(p))
            .collect()
    }

    fn unregister(&self, id: u64) {
        self.providers.borrow_mut().retain(|&(i, _)| i != id);
    }
}

impl fmt::Debug for RootSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RootSet({} registered)", self.providers.borrow().len())
    }
}

/// A provider's registration with a heap's [`RootSet`]. Dropping it
/// unregisters the provider.
#[must_use = "dropping the token unregisters its provider at once"]
pub struct RootToken {
    set: Weak<RootSet>,
    id: u64,
}

impl Drop for RootToken {
    fn drop(&mut self) {
        if let Some(set) = self.set.upgrade() {
            set.unregister(self.id);
        }
    }
}

impl fmt::Debug for RootToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RootToken({})", self.id)
    }
}
