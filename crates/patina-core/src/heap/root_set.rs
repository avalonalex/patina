//! The root providers registered with a heap (#682, `PRD/GC_PRD.md` §14):
//! every provider that outlives one collection, traced by each collection of
//! the heap beside the state the running machine lends it.
//!
//! A registration lasts as long as its [`RootToken`]. The token holds the
//! set weakly, so a token kept past its heap holds nothing, and a heap kept
//! past its tokens has no providers left.

use std::fmt;
use std::rc::{Rc, Weak};

use super::gc::RootProvider;
use super::table::HeapTable;

/// The providers registered with one heap. Each collection traces a copy of
/// the list taken as it starts, so a token dropped while one runs, by a
/// payload the sweep drops, takes its provider out of the next collection,
/// not out of this one.
///
/// A table the heap keeps in Rust, behind [`HeapTable`] (#687): a provider's
/// own code — its `ready`, its drop — runs after the access ends.
#[derive(Default)]
pub struct RootSet {
    state: HeapTable<Providers>,
}

#[derive(Default)]
struct Providers {
    /// Each registered provider, with its registration's id.
    list: Vec<(u64, Rc<dyn RootProvider>)>,
    /// The id the next registration takes.
    next_id: u64,
}

impl RootSet {
    /// Register `provider`: every collection of this heap traces it until
    /// the token returned is dropped.
    pub fn register(self: &Rc<Self>, provider: Box<dyn RootProvider>) -> RootToken {
        let provider = Rc::from(provider);
        let id = self.state.with_mut(|state| {
            let id = state.next_id;
            state.next_id += 1;
            state.list.push((id, provider));
            id
        });
        RootToken {
            set: Rc::downgrade(self),
            id,
        }
    }

    /// Whether every registered provider can report its roots now
    /// ([`RootProvider::ready`]). Each is asked outside the table's access,
    /// one at a time, rather than through a copy of the list: a safe point
    /// asks at every poll while a collection waits.
    pub(crate) fn ready(&self) -> bool {
        let mut index = 0;
        while let Some(provider) = self
            .state
            .with(|state| state.list.get(index).map(|(_, p)| Rc::clone(p)))
        {
            if !provider.ready() {
                return false;
            }
            index += 1;
        }
        true
    }

    /// How many providers are registered now: a test's way to see that a
    /// registration lasted no longer than it should (a compilation's literal
    /// pool, #705).
    pub fn registered(&self) -> usize {
        self.state.with(|state| state.list.len())
    }

    /// The providers registered now, for one collection.
    pub(crate) fn snapshot(&self) -> Vec<Rc<dyn RootProvider>> {
        self.state
            .with(|state| state.list.iter().map(|(_, p)| Rc::clone(p)).collect())
    }

    fn unregister(&self, id: u64) {
        let removed = self.state.with_mut(|state| {
            let position = state.list.iter().position(|&(i, _)| i == id);
            position.map(|position| state.list.remove(position))
        });
        // Dropped after the access ends: a provider's drop may unregister
        // another.
        drop(removed);
    }
}

impl fmt::Debug for RootSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let registered = self.state.with(|state| state.list.len());
        write!(f, "RootSet({registered} registered)")
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
