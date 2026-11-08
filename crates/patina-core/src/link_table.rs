//! A VM code unit's global operands (#836; `PRD/GC_PRD.md` §11.6).
//!
//! The VM's compiler (pass 5) gives each distinct global spelling a unit
//! names an index, a [`LinkIdx`], in the order it first emits one, and the
//! instructions that read, write or define a global, or call a primitive
//! through one, carry that index in place of the name. The spellings sit in
//! the unit's [`LinkTable`], one per unit, shared by every member of the unit
//! through one `Rc`. Anything that needs an operand's name reads it there:
//! the global lookups on a cache miss, a definition, a shadowed primitive's
//! deoptimization, the errors, the disassembler and the tracer.
//!
//! Beside each spelling the table has an entry for the binding record the
//! operand names once the table is linked in a namespace
//! ([`LinkTable::link_in`]): the namespace's own record, a host namespace's
//! read-through record, or a placeholder for a name the namespace does not
//! bind yet (`Environment::link_record`). The VM does not link its units
//! yet, so until it does an entry stays `UNBOUND`, an immediate, and only
//! the tests link a table.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::Trace;
use crate::binding_record::Record;
use crate::environment::Environment;
use crate::heap::Heap;
use crate::tagged_value::TaggedValue;

/// An index into a unit's [`LinkTable`]: the global operand of `LoadGlobal`,
/// `StoreGlobal`, `Define`, `CallPrimitive` and the 20 inline opcodes.
pub type LinkIdx = u32;

/// A code unit's global spellings, by [`LinkIdx`]: made by the VM's pass 5
/// for the unit it compiles, and held by every member of the unit through
/// one `Rc`; and, once linked in a namespace ([`LinkTable::link_in`]), the
/// binding record each operand names there.
///
/// Its derived trace marks each linked entry, as a slot is marked, and so
/// keeps the record, its cell and the value, and a placeholder or a weak
/// read-through record that nothing else marks (#836): that is what makes
/// one live. Nothing traces a table yet but the tests; the code store never
/// will (`CodeObject::links`).
#[derive(Debug, Trace)]
pub struct LinkTable {
    /// Each operand's record word, home (a′), once linked: one of the
    /// namespace's own (in a slot, flags 0, its own or an import's), a host
    /// namespace's read-through record `(unbound . parent)`, or a
    /// placeholder `(cell . PLACEHOLDER)` over `(UNBOUND . owner)`. `UNBOUND`
    /// until linked, and for a name the namespace reaches by an alias or a
    /// visible scoped definition, which is read by name. Cells, so the
    /// collector is shown each where it lives (#682).
    entries: Box<[Cell<TaggedValue>]>,
    /// The spellings, each once, in the order pass 5 first emitted an
    /// operand naming it.
    names: Box<[Rc<str>]>,
}

impl LinkTable {
    /// The table of a unit whose operands name `names`, operand `k` naming
    /// `names[k]`, its entries unlinked.
    pub fn new(names: Box<[Rc<str>]>) -> Self {
        let entries = names
            .iter()
            .map(|_| Cell::new(TaggedValue::UNBOUND))
            .collect();
        LinkTable { entries, names }
    }

    /// A table with no names, for code with no global operand: the runtime's
    /// stubs and code built by hand.
    pub fn empty() -> Rc<Self> {
        Rc::new(LinkTable::new(Box::default()))
    }

    /// Every spelling, by index.
    #[inline]
    pub fn names(&self) -> &[Rc<str>] {
        &self.names
    }

    /// The spelling operand `k` names. Panics if the table has no entry `k`,
    /// which pass 5 never emits.
    #[inline]
    pub fn name(&self, k: LinkIdx) -> &Rc<str> {
        &self.names[k as usize]
    }

    /// Link the table in `namespace` (#836): each entry becomes the record
    /// `namespace` links for its name (`Environment::link_record`), made
    /// there the first time and the same one after, or stays `UNBOUND` for a
    /// name read by name. A placeholder or a weak read-through record it
    /// links lives while this table is traced. Linking twice links the same
    /// records. Under `heap`'s write step for each name.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the VM links each unit's table at load (#836); until then only the tests do"
        )
    )]
    pub(crate) fn link_in(&self, heap: &RefCell<Heap>, namespace: &Rc<Environment>) {
        debug_assert!(
            namespace.is_namespace(),
            "a unit is compiled and linked in a namespace, never a frame (#853, #854)"
        );
        for (entry, name) in self.entries.iter().zip(self.names.iter()) {
            entry.set(
                namespace
                    .link_record(heap, name)
                    .map_or(TaggedValue::UNBOUND, Record::word),
            );
        }
    }

    /// Operand `k`'s entry: a record's word once linked, else `UNBOUND`.
    #[cfg(test)]
    pub(crate) fn entry(&self, k: LinkIdx) -> TaggedValue {
        self.entries[k as usize].get()
    }
}

#[cfg(test)]
mod tests {
    use super::LinkTable;
    use crate::binding_record::{PLACEHOLDER, Record};
    use crate::environment::{Environment, RC_COUNTS};
    use crate::heap::new_shared_heap;
    use crate::heap::sentinels::Sentinels;
    use crate::heap::trace::Trace as _;
    use crate::heap::trace_sentinels::collect_only;
    use crate::tagged_value::TaggedValue;
    use std::cell::Cell;
    use std::rc::Rc;

    /// An unlinked table over `names`, as pass 5 makes one.
    fn table(names: &[&str]) -> LinkTable {
        LinkTable::new(names.iter().map(|&name| Rc::from(name)).collect())
    }

    #[test]
    fn an_operand_names_its_entry() {
        let names: Vec<Rc<str>> = vec!["car".into(), "x".into()];
        let table = LinkTable::new(names.into_boxed_slice());
        assert_eq!((&**table.name(0), &**table.name(1)), ("car", "x"));
        assert_eq!(table.names().len(), 2);
        assert_eq!(
            (table.entry(0), table.entry(1)),
            (TaggedValue::UNBOUND, TaggedValue::UNBOUND),
            "unlinked"
        );
        assert!(LinkTable::empty().names().is_empty());
    }

    /// Linked in a parentless namespace (#836), an operand naming a binding
    /// gets the namespace's record, and one naming a name it does not bind a
    /// placeholder: a flagged record over a cell of the namespace's own
    /// holding `UNBOUND`, in its store and in no slot. One per name, however
    /// many tables link it, counted a record and a cell in the heap's sums
    /// by the store, which charges itself; the namespace counts and charges
    /// none of it.
    #[test]
    fn link_in_links_an_own_record_and_one_placeholder_per_name() {
        let heap = new_shared_heap();
        let ns = Rc::new(Environment::with_heap(&heap));
        ns.define(&heap, "x", TaggedValue::fixnum(1));
        let counts =
            |heap: &crate::Heap| (heap.binding_records(), heap.cells(), heap.external_bytes());
        let (records, cells, external) = counts(&heap.borrow());
        let (counted, charged) = (ns.counted(), ns.charged_bytes());

        let first = table(&["later", "x"]);
        first.link_in(&heap, &ns);
        assert_eq!(
            first.entry(1),
            ns.local_record("x").unwrap().word(),
            "x's own record"
        );
        let later = first.entry(0);
        {
            let h = heap.borrow();
            assert_eq!(h.cdr(later), TaggedValue::fixnum(PLACEHOLDER), "flagged");
            let cell = Record::from_word(later).cell(&h);
            assert_eq!(cell.value(&h), TaggedValue::UNBOUND);
            assert!(cell.owned_by(&h, ns.env_id()), "a cell of its own");
            assert!(h.placeholder_list().holds(ns.env_id()), "listed");
        }
        let store = ns.weak_entries().expect("the placeholder's store");
        assert_eq!(store.get("later").map(Record::word), Some(later));

        let second = table(&["later"]);
        second.link_in(&heap, &ns);
        first.link_in(&heap, &ns);
        assert_eq!(second.entry(0), later, "the same placeholder");
        assert_eq!(first.entry(0), later, "linked again, the same");
        let (now_records, now_cells, now_external) = counts(&heap.borrow());
        assert_eq!(
            (now_records, now_cells),
            (records + 1, cells + 1),
            "one record and one cell, once"
        );
        assert_eq!(store.counted(), (1, 1), "counted by the store");
        assert_eq!(store.recount(&heap.borrow()), store.counted());
        assert_eq!(ns.counted(), counted, "the namespace counts none of it");
        assert_eq!(ns.charged_bytes(), charged, "nor charges it");
        assert!(store.charged() > 0);
        assert_eq!(
            now_external - external,
            store.charged(),
            "the store charges itself"
        );

        assert_eq!(ns.local_slot("later"), None, "in no slot");
        assert_eq!(ns.local_record("later"), None);
        assert_eq!(ns.get(&heap, "later"), None);
        assert!(!ns.local_names(&heap).iter().any(|name| name == "later"));
    }

    /// A name a namespace reaches by an alias of its own is read by name
    /// (#836): its entry stays `UNBOUND`, and linking it makes no store.
    #[test]
    fn link_in_leaves_an_alias_unbound_and_makes_no_placeholder() {
        let heap = new_shared_heap();
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "target", TaggedValue::fixnum(100));
        let ns = Rc::new(Environment::with_heap(&heap));
        ns.define_alias("alias", Rc::clone(&library), Rc::from("target"));
        let records = heap.borrow().binding_records();

        let linked = table(&["alias"]);
        linked.link_in(&heap, &ns);
        assert_eq!(linked.entry(0), TaggedValue::UNBOUND, "read by name");
        assert!(ns.weak_entries().is_none(), "no store");
        assert!(!heap.borrow().placeholder_list().holds(ns.env_id()));
        assert_eq!(heap.borrow().binding_records(), records);
        assert_eq!(ns.get(&heap, "alias"), Some(TaggedValue::fixnum(100)));
    }

    /// The pass at the sweep's start (#836) keeps a placeholder a traced
    /// link table marks, in its store, with its pairs, and drops the ones
    /// nothing marks: out of the store, their counts and charges given back,
    /// and their pairs freed by the sweep. The namespace is rooted too, so a
    /// store or list traced as a root would keep the unmarked ones and fail
    /// this; two are unmarked, so a pass that skipped one leaves it, which a
    /// check build's `check_swept` refuses.
    #[test]
    fn the_pass_keeps_a_marked_placeholder_and_drops_the_unmarked() {
        let heap = new_shared_heap();
        let ns = Rc::new(Environment::with_heap(&heap));
        let linked = table(&["kept"]);
        linked.link_in(&heap, &ns);
        let kept = linked.entry(0);
        let dropped = ["dropped-1", "dropped-2"]
            .map(|name| ns.link_record(&heap, name).expect("a placeholder").word());
        let cell = |record: TaggedValue| heap.borrow().car(record);
        let kept_cell = cell(kept);
        let dropped_cells = dropped.map(cell);
        let store = ns.weak_entries().expect("the store");
        let counts = |heap: &crate::Heap| (heap.binding_records(), heap.cells());
        let (records, cells) = counts(&heap.borrow());
        let (charged, external) = (store.charged(), heap.borrow().external_bytes());
        assert_eq!(store.counted(), (3, 3));

        collect_only(&mut heap.borrow_mut(), |visitor| {
            visitor.env(&ns);
            linked.trace(visitor);
        });
        {
            let h = heap.borrow();
            let free = h.free_pair_slots();
            assert!(!free.contains(&kept.heap_index()), "the linked placeholder");
            assert!(!free.contains(&kept_cell.heap_index()), "and its cell");
            for word in dropped.iter().chain(&dropped_cells) {
                assert!(free.contains(&word.heap_index()), "an unlinked one's pair");
            }
        }
        assert_eq!(
            store.get("kept").map(Record::word),
            Some(kept),
            "the marked one stays in its store"
        );
        assert_eq!(store.get("dropped-1"), None, "the unmarked ones leave it");
        assert_eq!(store.get("dropped-2"), None);
        assert_eq!(store.counted(), (1, 1));
        assert_eq!(store.recount(&heap.borrow()), (1, 1));
        assert_eq!(
            counts(&heap.borrow()),
            (records - 2, cells - 2),
            "their counts given back"
        );
        assert_eq!(
            charged - store.charged(),
            2 * (RC_COUNTS + "dropped-1".len()),
            "and exactly both names' bytes, as the table keeps its capacity"
        );
        assert_eq!(
            heap.borrow().external_bytes(),
            external - (charged - store.charged())
        );
        assert_eq!(
            ns.link_record(&heap, "kept").map(Record::word),
            Some(kept),
            "a link answers the same word"
        );
        linked.link_in(&heap, &ns);
        assert_eq!(linked.entry(0), kept);
    }

    /// The table's sentinel test (#623): built by a struct literal, so a
    /// field added to the table stops this compiling until its tracing is
    /// decided (`docs/GC_DESIGN.md` §5.4, rule 2). Its one traced field,
    /// `entries`, holds a record that only it reaches once the namespace
    /// that bound it is dropped, and a collection with the table as the only
    /// root keeps the record, its cell and the value.
    #[test]
    fn link_table_fields() {
        let heap = new_shared_heap();
        let mut s = Sentinels::new(&mut heap.borrow_mut());
        let value = s.pair(
            &mut heap.borrow_mut(),
            "LinkTable.entries[0]: through x's record and its cell",
        );
        let record = {
            let ns = Rc::new(Environment::with_heap(&heap));
            ns.define(&heap, "x", value);
            ns.local_record("x").expect("x's record").word()
        };
        let linked = LinkTable {
            entries: Box::new([Cell::new(record)]),
            names: Box::new([Rc::from("x")]),
        };
        collect_only(&mut heap.borrow_mut(), |visitor| linked.trace(visitor));
        s.assert_survived(&heap.borrow());
    }
}
