//! The literal pool (#705; `PRD/GC_PRD.md` §11.3, and M2 in §17): the data
//! one compilation's `CoreExpr` quotes, in a table the collector traces.
//!
//! A form's literals used to sit in its `CoreExpr` as bare heap words. That
//! was sound only because nothing collects while a form is desugared and
//! compiled, and it left a safe crate no way to read one as a
//! [`Value`](crate::Value): the raw-word API is `unsafe`, and the safe crates
//! forbid it (#698). Now a node holds a [`Lit`]: an immediate inline, which
//! holds nothing on the heap, or an entry of its compilation's
//! [`LiteralPool`].
//!
//! **The pool** is registered with its heap's root set for as long as it
//! lives, so every collection traces its entries, and a safe crate reads one
//! as a `Value` through [`LiteralPool::value`]. Each slot is a `Cell`, so a
//! collector that moves can update it in place.
//!
//! **Scoped to one compilation.** A backend makes a pool for each form it
//! compiles, and drops it once the code holding the form's literals is
//! traced in its own right (#869): the VM's `load_unit`, which takes it,
//! once it has put the unit in the code store, before the form runs; the
//! tree-walker's run, which takes it with the form's `CpsExpr`, once it has
//! returned; and for a `Step::Eval`'s tree, which becomes a step of the
//! running trampoline, the next retirement, the start of the driver's next
//! collection, before it marks, or a run's return. Until then the code is a
//! Rust value nothing traces, and the pool is the only root of the words it
//! copied. So a pool never holds more than one form's text quotes, and
//! nothing it held stays live through it once its form is done (M2).
//!
//! **Links** (#837). A pool's entry may also be a binding record a node
//! links, a [`Link`]: what an early-bound reference is bound to. It is a
//! slot of the same table, so the pool roots the record, its cell and the
//! value as it roots a literal, but it is made only by
//! [`LiteralPool::add_link`], from a [`Linkable`] a context found, and read
//! only by [`LiteralPool::link`], which answers a [`BindingLink`] for a
//! backend to copy into its code. [`LiteralPool::value`] takes a [`Lit`],
//! which no link is, so a record never reaches a program as a value.

use std::cell::Cell;
use std::fmt;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::binding_record::{BindingLink, Linkable, Record};
use crate::heap::Heap;
use crate::heap::gc::{RootProvider, RootToken, SlotVisitor};
use crate::heap::table::HeapTable;
use crate::tagged_value::TaggedValue;
use crate::value::Value;

/// A literal of a `CoreExpr`: an immediate word inline, or an entry of the
/// [`LiteralPool`] of the compilation that made it. Its representation is
/// private, so a heap word reaches a node only through a pool, which roots
/// it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Lit(Repr);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Repr {
    /// A word that holds nothing on the heap.
    Immediate(TaggedValue),
    /// An entry of the pool numbered `pool`.
    Pooled { pool: u32, index: u32 },
}

impl Lit {
    /// The unspecified value, as a literal.
    pub const UNSPECIFIED: Lit = Lit(Repr::Immediate(TaggedValue::UNSPECIFIED));

    /// The empty list, as a literal.
    pub const NULL: Lit = Lit(Repr::Immediate(TaggedValue::NULL));

    /// An immediate as a literal. Panics on a heap word, which only a
    /// [`LiteralPool`] may hold for a node.
    #[track_caller]
    pub fn immediate(word: TaggedValue) -> Lit {
        assert!(
            word.is_immediate(),
            "a heap word is a literal only as an entry of a literal pool (#705)"
        );
        Lit(Repr::Immediate(word))
    }

    /// The word, if the literal is an immediate.
    pub fn as_immediate(self) -> Option<TaggedValue> {
        match self.0 {
            Repr::Immediate(word) => Some(word),
            Repr::Pooled { .. } => None,
        }
    }
}

impl fmt::Debug for Lit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Repr::Immediate(word) => write!(f, "Lit({word:?})"),
            Repr::Pooled { pool, index } => write!(f, "Lit(#{pool}.{index})"),
        }
    }
}

/// An immediate writes as its value, an entry as its place in its pool: a
/// `CoreExpr`'s `Display` cannot reach the heap, and never could.
impl fmt::Display for Lit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Repr::Immediate(word) => write!(f, "{word}"),
            Repr::Pooled { index, .. } => write!(f, "#<literal {index}>"),
        }
    }
}

/// A binding record a `CoreExpr` links (#837): an entry of its
/// compilation's [`LiteralPool`], kept apart from [`Lit`] so that
/// [`LiteralPool::value`] can never hand a record to a program as a value.
/// Its representation is private, and only [`LiteralPool::add_link`] makes
/// one. The pool's number is never 0, so an `Option<Link>` is the size of a
/// `Link`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Link {
    pool: NonZeroU32,
    index: u32,
}

/// An entry writes as its place in its pool, as a pooled [`Lit`] does: a
/// `CoreExpr`'s `Display` cannot reach the heap.
impl fmt::Display for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#<link {}>", self.index)
    }
}

/// The literals of one compilation. See the [module docs](self).
pub struct LiteralPool {
    /// This pool's number, which each of its entries carries, so that a
    /// literal or a link read through another pool is caught. Never 0.
    id: NonZeroU32,
    entries: Rc<Entries>,
    /// The registration that has every collection trace the entries.
    _root: RootToken,
}

/// The pool's slots, shared with its registration.
struct Entries(HeapTable<Vec<Cell<TaggedValue>>>);

impl LiteralPool {
    /// An empty pool, traced by every collection of `heap` until it is
    /// dropped.
    pub fn new(heap: &Heap) -> Self {
        static NEXT_ID: AtomicU32 = AtomicU32::new(1);
        // Never 0, the niche `Option<Link>` uses, even once the count wraps.
        let id = loop {
            if let Some(id) = NonZeroU32::new(NEXT_ID.fetch_add(1, Ordering::Relaxed)) {
                break id;
            }
        };
        let entries = Rc::new(Entries(HeapTable::new(Vec::new())));
        let root = heap
            .root_set()
            .register(Box::new(EntriesRoot(Rc::clone(&entries))));
        Self {
            id,
            entries,
            _root: root,
        }
    }

    /// `value` as a literal of this compilation: inline if it is an
    /// immediate, an entry otherwise.
    pub fn add(&self, value: Value<'_>) -> Lit {
        // SAFETY: a value is live while its context is, which the brand
        // ties it to, and from here the pool roots it.
        unsafe { self.add_word(value.tagged()) }
    }

    /// [`LiteralPool::add`] for a word, for a trusted crate that allocated
    /// it outside a context: a backend's quasiquote constructors.
    ///
    /// # Safety
    ///
    /// `word` is live: an immediate, or a heap object no collection has
    /// freed since it was made, on this pool's heap.
    pub unsafe fn add_word(&self, word: TaggedValue) -> Lit {
        if word.is_immediate() {
            return Lit(Repr::Immediate(word));
        }
        Lit(Repr::Pooled {
            pool: self.id.get(),
            index: self.push(word),
        })
    }

    /// A new slot holding `word`, and its index.
    fn push(&self, word: TaggedValue) -> u32 {
        let index = self.entries.0.with_mut(|slots| {
            slots.push(Cell::new(word));
            slots.len() - 1
        });
        u32::try_from(index).expect("a form quotes and links fewer than 2^32 entries")
    }

    /// `link` as an entry of this compilation, rooted from here on (#837).
    /// Safe: the brand ties the handle to a live context, as
    /// [`LiteralPool::add`]'s `Value` is.
    pub fn add_link(&self, link: Linkable<'_>) -> Link {
        Link {
            pool: self.id,
            index: self.push(link.link().record().word()),
        }
    }

    /// The record `link` stands for: what a backend copies into code it
    /// traces. Asserts in every build that `link` is an entry of this pool,
    /// as [`LiteralPool::word`] does, so a link one form's desugaring
    /// remembered is never read through another's pool.
    #[track_caller]
    pub fn link(&self, link: Link) -> BindingLink {
        assert_eq!(
            link.pool, self.id,
            "a link read through another compilation's pool"
        );
        let word = self
            .entries
            .0
            .with(|slots| slots[link.index as usize].get());
        BindingLink::new(Record::from_word(word))
    }

    /// The word `lit` stands for: what a backend copies into the code it
    /// runs. A word is safe to hold, but not to read as a value: read one
    /// through [`LiteralPool::value`].
    #[track_caller]
    pub fn word(&self, lit: Lit) -> TaggedValue {
        match lit.0 {
            Repr::Immediate(word) => word,
            Repr::Pooled { pool, index } => {
                assert_eq!(
                    pool,
                    self.id.get(),
                    "a literal read through another compilation's pool"
                );
                self.entries.0.with(|slots| slots[index as usize].get())
            }
        }
    }

    /// The value `lit` stands for, in `cx`. Sound because the pool roots
    /// every entry for its whole life, and the value cannot outlive the
    /// context.
    #[track_caller]
    pub fn value<'gc>(&self, _cx: &crate::Cx<'gc>, lit: Lit) -> Value<'gc> {
        let word = self.word(lit);
        // SAFETY: an immediate is a value anywhere; an entry is rooted by
        // this pool, which is alive, and so live at any point of the
        // context's extent, beneath which nothing collects.
        unsafe { Value::from_tagged(word) }
    }

    /// The pool's entries: the heap literals and the links added to it.
    pub fn len(&self) -> usize {
        self.entries.0.with(Vec::len)
    }

    /// Whether the pool has no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Debug for LiteralPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LiteralPool(#{}, {} entries)", self.id.get(), self.len())
    }
}

/// The registration's provider: the pool's slots, by a second `Rc`, each
/// reported where it lives.
struct EntriesRoot(Rc<Entries>);

impl RootProvider for EntriesRoot {
    fn trace_roots(&self, visitor: &mut dyn SlotVisitor) {
        self.0.0.with(|slots| visitor.slots(slots));
    }
}

#[cfg(test)]
mod tests {
    use super::{Link, Lit, LiteralPool};
    use crate::binding_record::{BindingLink, Linkable};
    use crate::environment::Environment;
    use crate::heap::gc::{Collector, MarkSweepCollector};
    use crate::heap::new_shared_heap;
    use crate::heap::sentinels::Sentinels;
    use crate::heap::{Heap, gc::GcStats};
    use crate::tagged_value::TaggedValue;

    /// A collection with no roots but the heap's own: its registered
    /// providers, among them every live pool.
    fn collect(heap: &mut Heap) -> GcStats {
        MarkSweepCollector::new().collect(heap, &[])
    }

    /// #705's sentinel: with a pool alive, a collection keeps a literal
    /// reachable only through it, and sweeps the decoy beside it.
    #[test]
    fn a_collection_keeps_a_pooled_literal() {
        let heap = new_shared_heap();
        let pool = LiteralPool::new(&heap.borrow());
        let mut sentinels = Sentinels::new(&mut heap.borrow_mut());
        let pair = sentinels.pair(&mut heap.borrow_mut(), "a pooled pair");
        let string = sentinels.string(&mut heap.borrow_mut(), "a pooled string");
        // SAFETY: both were allocated just now, and nothing has collected.
        let lits = unsafe { [pool.add_word(pair), pool.add_word(string)] };
        collect(&mut heap.borrow_mut());
        sentinels.assert_survived(&heap.borrow());
        assert_eq!(pool.word(lits[0]), pair);
        assert_eq!(pool.word(lits[1]), string);
        assert_eq!(pool.len(), 2);
    }

    /// #837's sentinel: with a pool alive, a collection keeps a binding
    /// record a `Link` names, reachable only through the pool (its
    /// namespace dropped), with its cell and the value the cell holds; and
    /// the pool answers the record back.
    #[test]
    fn a_collection_keeps_a_pooled_links_record() {
        let heap = new_shared_heap();
        let pool = LiteralPool::new(&heap.borrow());
        let mut sentinels = Sentinels::new(&mut heap.borrow_mut());
        let value = sentinels.pair(&mut heap.borrow_mut(), "a linked record's cell's value");
        let record = {
            let ns = std::rc::Rc::new(Environment::with_heap(&heap));
            ns.define(&heap, "x", value);
            ns.local_record("x").expect("x's record")
        };
        let link = pool.add_link(Linkable::new(BindingLink::new(record)));
        assert_eq!(pool.len(), 1);
        collect(&mut heap.borrow_mut());
        sentinels.assert_survived(&heap.borrow());
        let linked = pool.link(link);
        assert_eq!(linked.record(), record);
        assert_eq!(linked.value(&heap.borrow()), Some(value));
        assert_eq!(link.to_string(), "#<link 0>");
        assert_eq!(
            std::mem::size_of::<Option<Link>>(),
            std::mem::size_of::<Link>(),
            "the pool's number leaves a niche"
        );
    }

    /// A link is read through the pool that made it, in every build. The
    /// other pool has an entry at the link's index, so without the check
    /// the read would answer that entry's record rather than fail.
    #[test]
    #[should_panic(expected = "a link read through another compilation's pool")]
    fn a_link_read_through_another_pool_panics() {
        let heap = new_shared_heap();
        let (one, two) = (
            LiteralPool::new(&heap.borrow()),
            LiteralPool::new(&heap.borrow()),
        );
        let ns = std::rc::Rc::new(Environment::with_heap(&heap));
        ns.define(&heap, "x", TaggedValue::fixnum(1));
        ns.define(&heap, "y", TaggedValue::fixnum(2));
        let record = ns.local_record("x").unwrap();
        let link = one.add_link(Linkable::new(BindingLink::new(record)));
        let other = two.add_link(Linkable::new(BindingLink::new(
            ns.local_record("y").unwrap(),
        )));
        assert_eq!(other.index, link.index, "an entry at the same index");
        two.link(link);
    }

    /// Dropping the pool unregisters it: what it held is no longer live
    /// through it, and the next collection frees it (M2).
    #[test]
    fn a_dropped_pool_roots_nothing() {
        let heap = new_shared_heap();
        let registered = || heap.borrow().root_set().registered();
        let before = registered();
        let pool = LiteralPool::new(&heap.borrow());
        assert_ne!(
            registered(),
            before,
            "the pool is registered while it lives"
        );
        let pair = heap
            .borrow_mut()
            .alloc_pair(TaggedValue::fixnum(7), TaggedValue::NULL);
        // SAFETY: allocated just now, and nothing has collected.
        let lit = unsafe { pool.add_word(pair) };
        assert_eq!(pool.word(lit), pair);
        drop(pool);
        assert_eq!(registered(), before, "the pool is gone from the root set");
        let stats = collect(&mut heap.borrow_mut());
        assert_eq!(stats.last_swept.pairs, 1, "{stats:?}");
    }

    /// An immediate needs no entry, and a heap word is refused inline.
    #[test]
    fn an_immediate_stays_inline() {
        let heap = new_shared_heap();
        let pool = LiteralPool::new(&heap.borrow());
        // SAFETY: an immediate is live anywhere.
        let lit = unsafe { pool.add_word(TaggedValue::fixnum(42)) };
        assert_eq!(lit, Lit::immediate(TaggedValue::fixnum(42)));
        assert_eq!(lit.as_immediate(), Some(TaggedValue::fixnum(42)));
        assert!(pool.is_empty());
        let pair = heap
            .borrow_mut()
            .alloc_pair(TaggedValue::NULL, TaggedValue::NULL);
        assert!(std::panic::catch_unwind(|| Lit::immediate(pair)).is_err());
    }

    /// A literal is read through the pool that made it.
    #[test]
    #[should_panic(expected = "another compilation's pool")]
    fn a_literal_read_through_another_pool_panics() {
        let heap = new_shared_heap();
        let (one, two) = (
            LiteralPool::new(&heap.borrow()),
            LiteralPool::new(&heap.borrow()),
        );
        let pair = heap
            .borrow_mut()
            .alloc_pair(TaggedValue::NULL, TaggedValue::NULL);
        // SAFETY: allocated just now, and nothing has collected.
        let lit = unsafe { one.add_word(pair) };
        two.word(lit);
    }
}
