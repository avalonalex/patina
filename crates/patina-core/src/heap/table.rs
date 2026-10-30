//! The one wrapper every table the heap keeps in Rust takes (#687, addition
//! C3; `PRD/GC_PRD.md` §18.2, rule 9, and §18.6).
//!
//! The redesign gives each heap tables that Rust owns and more than one
//! owner reaches: the handle table, its standard port parameters, the root
//! set, the literal pool, the `PortTable` and the `FinalRegistry` (stage 4a,
//! #785; the registry's young and old lists at 5a) now, then the
//! `CodeStore` (4e), the `HostPayloadTable` (4f) and the thread table (9).
//! Under N carriers each needs a lock. Behind one wrapper from the start,
//! that is one change in one place, not a hunt through every table for the
//! cells it picked.

#[cfg(not(feature = "threaded"))]
use std::cell::{RefCell, RefMut};
#[cfg(feature = "threaded")]
use std::sync::{Mutex, MutexGuard, PoisonError};

/// A table the heap keeps in Rust, behind the wrapper every such table takes:
/// plain in the default build, and the one place a `threaded` build puts a
/// lock (`PRD/GC_PRD.md` §18.2, rule 9).
///
/// Access is scoped to a closure and hands out nothing that holds the table,
/// so a lock could never be held across a safe point, a callback or a return
/// to Scheme. Code that runs someone else's — a provider's `ready`, the drop
/// of what an entry held — runs it after the access ends.
///
/// One access at a time: an access inside another panics, a read inside a
/// read as well. A `RefCell` would allow that read and a lock would deadlock
/// on it, so the default build fails where a threaded one would hang.
///
/// Under `threaded` the table is behind a `Mutex` (#711): the lock body only,
/// which no lane runs before stage 9. An access waits for one another
/// carrier holds; the default build's panic is what finds a nested one.
#[derive(Default)]
pub struct HeapTable<T> {
    #[cfg(not(feature = "threaded"))]
    state: RefCell<T>,
    #[cfg(feature = "threaded")]
    state: Mutex<T>,
}

impl<T> HeapTable<T> {
    pub const fn new(state: T) -> Self {
        Self {
            #[cfg(not(feature = "threaded"))]
            state: RefCell::new(state),
            #[cfg(feature = "threaded")]
            state: Mutex::new(state),
        }
    }

    /// Read the table for the length of `f`.
    #[inline]
    #[track_caller]
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.access())
    }

    /// Change the table for the length of `f`.
    #[inline]
    #[track_caller]
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        f(&mut self.access())
    }

    #[cfg(feature = "threaded")]
    #[inline]
    #[track_caller]
    fn access(&self) -> MutexGuard<'_, T> {
        // A panic inside an access leaves the table as it was written, as
        // the default build's `RefCell` does.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(not(feature = "threaded"))]
    #[inline]
    #[track_caller]
    fn access(&self) -> RefMut<'_, T> {
        match self.state.try_borrow_mut() {
            Ok(state) => state,
            Err(_) => panic!(
                "a heap table accessed while an access to it is open: under a lock this nested \
                 access would deadlock (#687)"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HeapTable;

    #[test]
    fn an_access_reads_and_changes_the_table() {
        let table = HeapTable::new(vec![1, 2]);
        table.with_mut(|list| list.push(3));
        assert_eq!(table.with(|list| list.iter().sum::<i32>()), 6);
    }

    // The default build's: under `threaded`, a nested access waits on the
    // lock its own access holds.
    #[cfg(not(feature = "threaded"))]
    #[test]
    #[should_panic(expected = "accessed while an access to it is open")]
    fn a_read_inside_a_read_panics() {
        let table = HeapTable::new(0);
        table.with(|_| table.with(|_| ()));
    }

    // The default build's: under `threaded`, a nested access waits on the
    // lock its own access holds.
    #[cfg(not(feature = "threaded"))]
    #[test]
    #[should_panic(expected = "accessed while an access to it is open")]
    fn a_change_inside_a_read_panics() {
        let table = HeapTable::new(0);
        table.with(|_| table.with_mut(|n| *n += 1));
    }

    #[test]
    fn an_access_after_another_ends_is_fine() {
        let table = HeapTable::new(0);
        table.with_mut(|n| *n += 1);
        table.with_mut(|n| *n += 1);
        assert_eq!(table.with(|n| *n), 2);
    }
}
