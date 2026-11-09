//! A VM code unit's global operands (#836; `PRD/GC_PRD.md` §11.6).
//!
//! The VM's compiler (pass 5) gives each distinct global spelling a unit
//! names an index, a [`LinkIdx`], in the order it first emits one, and the
//! instructions that read, write or define a global, or call a primitive
//! through one, carry that index in place of the name. The spellings sit in
//! the unit's [`LinkTable`], one per unit, shared by every member of the unit
//! through one `Rc`. Anything that needs an operand's name reads it there:
//! a definition, the cold read, the errors, the disassembler and the tracer.
//!
//! Beside each spelling the table has an entry for the binding record the
//! operand names, which `VmState::load_unit` links in the namespace the unit
//! was compiled in, before any read goes through it
//! ([`LinkTable::link_in`]): the namespace's own record, a host namespace's
//! read-through record, a placeholder for a name the namespace does not
//! bind yet (`Environment::link_record`), or the heap's by-name record for a
//! name it reaches by an alias or a visible scoped definition. A table the
//! VM compiles and never loads (the disassembler's) stays unlinked, its
//! entries `UNBOUND`, an immediate, and serves its names alone.
//!
//! A linked table is read through [`LinkTable::get`], two loads and a test
//! for `UNBOUND`, which answers every own and imported binding; a
//! placeholder, a read-through record and the by-name record answer
//! `UNBOUND` there and are read by [`LinkTable::read_cold`], which reads a
//! read-through record's parent and otherwise the name, as
//! `Environment::get` does. A store resolves its cell the same way
//! ([`LinkTable::assignee`]), and a definition goes by name in the table's
//! namespace. So the VM's reads and writes of a global key on the binding a
//! unit linked, never on the spelling or on the frame that runs it.
//!
//! The table is traced only by its unit's code id (`VmState::trace_code_ids`),
//! never from the code store, and with it the records, placeholders and weak
//! read-through records it links and the namespace it holds: a placeholder
//! lives exactly as long as code that links it can run.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use crate::Trace;
use crate::binding_record::{BindingCell, Record};
use crate::census::Site;
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
/// one live. It also shows the collector the namespace the table was linked
/// in, which the unit's code reads through it. The VM traces a unit's table
/// by its code id, once a collection (`VmState::trace_code_ids`), when a
/// marked closure, a frame or the dispatch loop's running code names the
/// unit; the code store never does (`CodeObject::links`).
#[derive(Debug, Trace)]
pub struct LinkTable {
    /// Each operand's record word, home (a′), once linked: one of the
    /// namespace's own (in a slot, flags 0, its own or an import's), a host
    /// namespace's read-through record `(unbound . parent)`, a placeholder
    /// `(cell . PLACEHOLDER)` over `(UNBOUND . owner)`, or, for a name the
    /// namespace reaches by an alias or a visible scoped definition, which
    /// is read by name, the heap's by-name record `(cell . BY_NAME)` over a
    /// free cell holding `UNBOUND`. `UNBOUND` until linked. Cells, so the
    /// collector is shown each where it lives (#682).
    entries: Box<[Cell<TaggedValue>]>,
    /// The spellings, each once, in the order pass 5 first emitted an
    /// operand naming it.
    names: Box<[Rc<str>]>,
    /// The namespace the table was linked in (#836's question 3 (i)), set
    /// by the first [`LinkTable::link_in`]: the unit's edge to it, reached
    /// only through a traced table, so the namespace lives while code that
    /// reads its records can run. Unset, and tracing nothing, until linked.
    namespace: OnceCell<Rc<Environment>>,
    /// The `env_id` of the namespace the unit was compiled in, set by the
    /// VM's compile pipeline, or 0 for code compiled with none (ids start
    /// at 1): [`LinkTable::link_in`] refuses, in a debug build, a link in
    /// any other namespace, since pass 5 resolved the unit's primitive
    /// calls and macro-introduced names in that one. An id, so that the
    /// table holds no namespace before it is linked.
    #[trace(skip, reason = "an environment id, for a debug check")]
    compiled_in: Cell<u64>,
    /// The VM's collection epoch this table was last traced in, for
    /// [`LinkTable::trace_once`]: the unit's members share the table, and
    /// each reports its own code id.
    #[trace(
        skip,
        reason = "a collection count: the epoch the table was last traced in"
    )]
    traced: Cell<u64>,
}

impl LinkTable {
    /// The table of a unit whose operands name `names`, operand `k` naming
    /// `names[k]`, its entries unlinked.
    pub fn new(names: Box<[Rc<str>]>) -> Self {
        let entries = names
            .iter()
            .map(|_| Cell::new(TaggedValue::UNBOUND))
            .collect();
        LinkTable {
            entries,
            names,
            namespace: OnceCell::new(),
            compiled_in: Cell::new(0),
            traced: Cell::new(0),
        }
    }

    /// Record that the unit was compiled in the namespace numbered `env_id`,
    /// which [`LinkTable::link_in`] checks in a debug build.
    pub fn set_compiled_in(&self, env_id: u64) {
        self.compiled_in.set(env_id);
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
    /// there the first time and the same one after, or for a name read by
    /// name the heap's by-name record (`Heap::by_name_record`), so that
    /// once linked every entry is a record. A placeholder or a weak
    /// read-through record it links lives while this table is traced, and
    /// so does `namespace`, which the table holds from now on. Linking twice
    /// links the same records. Under `heap`'s write step for each name, and
    /// one more for the by-name record the first time a table needs it.
    ///
    /// A table is linked in one namespace: the first link sets it, and a
    /// later link in another relinks against the first, so that the table
    /// never names one namespace's records while it holds another; a debug
    /// build then refuses it. Refused after the relink, so that the relink a
    /// release build relies on runs, and is tested, in every build. A debug
    /// build refuses at once, before anything links, a namespace other than
    /// the one the unit was compiled in ([`LinkTable::set_compiled_in`]).
    pub fn link_in(&self, heap: &RefCell<Heap>, namespace: &Rc<Environment>) {
        debug_assert!(
            namespace.is_namespace(),
            "a unit is compiled and linked in a namespace, never a frame (#853, #854)"
        );
        debug_assert!(
            self.compiled_in.get() == 0 || self.compiled_in.get() == namespace.env_id(),
            "a unit is linked in the namespace it was compiled in (#836)"
        );
        let linked = self.namespace.get_or_init(|| Rc::clone(namespace));
        let mut by_name = None;
        for (entry, name) in self.entries.iter().zip(self.names.iter()) {
            // `link_record` takes its own write step, so the by-name record
            // is asked for only once it has returned.
            let word = match linked.link_record(heap, name) {
                Some(record) => record.word(),
                None => *by_name.get_or_insert_with(|| heap.borrow_mut().by_name_record()),
            };
            entry.set(word);
        }
        debug_assert!(
            Rc::ptr_eq(linked, namespace),
            "a unit's table is linked in one namespace (#836)"
        );
    }

    /// The namespace the table was linked in, or `None` before it is: the
    /// one a `Define` defines in and a cold read reads by name.
    #[inline]
    pub fn namespace(&self) -> Option<&Rc<Environment>> {
        self.namespace.get()
    }

    /// What operand `k` names in a linked table, read through its record:
    /// `link[k] → record → cell → value`, two loads and a test, with `heap`
    /// borrowed (#836). `None` where that cell holds `UNBOUND`: a
    /// placeholder, a host namespace's read-through record, whose own cell
    /// names no binding, and the by-name record, each of which
    /// [`LinkTable::read_cold`] reads. Every binding of the namespace's own
    /// and every import answers here. `UNBOUND` is never a program's value,
    /// so the test cannot take a binding for a miss.
    ///
    /// The table must be linked: an unlinked entry is no record.
    #[inline(always)]
    pub fn get(&self, heap: &Heap, k: LinkIdx) -> Option<TaggedValue> {
        let record = Record::from_word(self.entries[k as usize].get());
        let value = record.car_cell(heap).value(heap);
        (value != TaggedValue::UNBOUND).then_some(value)
    }

    /// What operand `k` names where [`LinkTable::get`] answered `None`, as
    /// `Environment::get` would read its name in the namespace the table
    /// was linked in (#836): a host namespace's read-through record answers
    /// its parent's binding, unless the host now reaches the name by an
    /// alias or a visible scoped definition of its own, which `get` finds
    /// first; a placeholder, the by-name record, a read-through record over
    /// a parent's placeholder and a parent's binding that holds `UNBOUND`
    /// are read by name, so an alias, a scoped definition, or a binding a
    /// route made without adopting the record is seen. `None` is unbound,
    /// and is not remembered: the next read asks again.
    ///
    /// In a check build, a placeholder read here is asserted to be still
    /// its namespace's store's entry for the name: a kept unit's table is
    /// traced (`VmState::release_dead_code`), so the pass at the sweep's
    /// start keeps the placeholders it links, and only an adoption, which
    /// clears the flag, takes one out of the store.
    #[cold]
    #[inline(never)]
    pub fn read_cold(&self, heap: &RefCell<Heap>, k: LinkIdx) -> Option<TaggedValue> {
        let ns = self.namespace.get()?;
        let name = &self.names[k as usize];
        {
            let heap = heap.borrow();
            let record = Record::from_word(self.entries[k as usize].get());
            if crate::heap::GC_CHECK && record.is_placeholder(&heap) {
                let held = ns.weak_entries().and_then(|store| store.get(name));
                if held != Some(record) {
                    linked_placeholder_out_of_its_store(name);
                }
            }
            if let Some(parent) = record.parent_record(&heap)
                && !ns.read_by_name(name)
            {
                let value = parent.value(&heap);
                if value != TaggedValue::UNBOUND {
                    return Some(value);
                }
            }
        }
        ns.get(heap, name)
    }

    /// The cell a store to operand `k` writes, and what it holds now, with
    /// `heap` borrowed (#836): the record's own cell, its own or an
    /// import's, when it holds a binding; for a host namespace's
    /// read-through record over a bound parent, while the host reaches the
    /// name by no alias or visible scoped definition of its own, the
    /// parent's cell ([`Record::resolved_cell`]), as `Environment::set`
    /// writes it. `None` for every other case, a placeholder, the by-name
    /// record and a read-through record over a parent's placeholder among
    /// them, which a store resolves by name. Never a cell that holds
    /// `UNBOUND`, so a store never binds a placeholder's cell or a host's
    /// `unbound` cell.
    #[inline(always)]
    pub fn assignee(&self, heap: &Heap, k: LinkIdx) -> Option<(Assignee, TaggedValue)> {
        let record = Record::from_word(self.entries[k as usize].get());
        let cell = record.car_cell(heap);
        let old = cell.value(heap);
        if old != TaggedValue::UNBOUND {
            return Some((Assignee(cell), old));
        }
        self.assignee_cold(heap, k)
    }

    /// [`LinkTable::assignee`] for a record whose own cell holds `UNBOUND`.
    #[cold]
    #[inline(never)]
    fn assignee_cold(&self, heap: &Heap, k: LinkIdx) -> Option<(Assignee, TaggedValue)> {
        let record = Record::from_word(self.entries[k as usize].get());
        record.parent_record(heap)?;
        let ns = self.namespace.get()?;
        if ns.read_by_name(&self.names[k as usize]) {
            return None;
        }
        let cell = record.resolved_cell(heap);
        let old = cell.value(heap);
        (old != TaggedValue::UNBOUND).then_some((Assignee(cell), old))
    }

    /// Store `value` into the cell [`LinkTable::assignee`] resolved, logged
    /// as a store through a namespace's slot (`Site::EnvSetSlot`), as
    /// `Environment::set` logs the same assignment by name.
    #[inline(always)]
    pub fn assign(heap: &mut Heap, to: Assignee, value: TaggedValue) {
        to.0.store(heap, value, Site::EnvSetSlot);
    }

    /// Mark the table traced in collection `epoch`, and answer whether it
    /// already was: the VM traces a unit's table once a collection however
    /// many of its members' code ids marking reached. A table belongs to one
    /// machine, whose epoch it records.
    #[inline]
    pub fn trace_once(&self, epoch: u64) -> bool {
        self.traced.replace(epoch) == epoch
    }

    /// Whether the table was traced in collection `epoch`: what a check
    /// build asserts of every unit a collection kept, once it has released
    /// the rest (`VmState::release_dead_code`, #836). Read only there.
    #[inline]
    pub fn traced_in(&self, epoch: u64) -> bool {
        self.traced.get() == epoch
    }

    /// Operand `k`'s entry: a record's word once linked, else `UNBOUND`.
    #[cfg(test)]
    pub(crate) fn entry(&self, k: LinkIdx) -> TaggedValue {
        self.entries[k as usize].get()
    }
}

/// The cell a linked store writes (#836): made only by
/// [`LinkTable::assignee`], and handed back to [`LinkTable::assign`], so
/// that a store through a link table writes no other cell.
#[derive(Clone, Copy, Debug)]
pub struct Assignee(BindingCell);

/// A check build's failure: a placeholder a link table reads is no longer
/// its namespace's store's entry for its name, which only a pass that
/// dropped a placeholder a traced table links would leave (#836).
#[cold]
#[inline(never)]
fn linked_placeholder_out_of_its_store(name: &str) -> ! {
    panic!(
        "a linked placeholder for {name:?} is no longer its namespace's store's entry: \
         the sweep's pass dropped a placeholder a traced link table links (#836)"
    );
}

#[cfg(test)]
mod tests {
    use super::LinkTable;
    use crate::binding_record::{BY_NAME, PLACEHOLDER, Record};
    use crate::environment::{Environment, RC_COUNTS};
    use crate::heap::new_shared_heap;
    use crate::heap::sentinels::Sentinels;
    use crate::heap::trace::Trace as _;
    use crate::heap::trace_sentinels::collect_only;
    use crate::tagged_value::TaggedValue;
    use std::cell::{Cell, OnceCell};
    use std::rc::Rc;

    /// An unlinked table over `names`, as pass 5 makes one.
    fn table(names: &[&str]) -> LinkTable {
        LinkTable::new(names.iter().map(|&name| Rc::from(name)).collect())
    }

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    /// The message of a panic `catch_unwind` caught.
    fn panic_message(panic: &(dyn std::any::Any + Send)) -> Option<&str> {
        (panic.downcast_ref::<&str>().copied())
            .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
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
    /// (#836): its entry is the heap's by-name record, a record flagged
    /// `BY_NAME` over a cell holding `UNBOUND`, made once and the same word
    /// for every name and every table after, and counted nowhere. Linking
    /// it makes no store and no placeholder; the linked read finds
    /// `UNBOUND` and the cold read reads the name, afresh each time, so the
    /// alias's target defined later is seen.
    #[test]
    fn link_in_links_the_by_name_record_for_an_alias_and_makes_no_placeholder() {
        let heap = new_shared_heap();
        let library = Rc::new(Environment::with_heap(&heap));
        let ns = Rc::new(Environment::with_heap(&heap));
        ns.define_alias("alias", Rc::clone(&library), Rc::from("target"));
        ns.define_alias("other", Rc::clone(&library), Rc::from("target"));
        let counts = |heap: &crate::Heap| (heap.binding_records(), heap.cells());
        let before = counts(&heap.borrow());

        let linked = table(&["alias"]);
        linked.link_in(&heap, &ns);
        let word = linked.entry(0);
        assert!(word.is_pair(), "a record, not `UNBOUND`");
        {
            let h = heap.borrow();
            assert_eq!(h.cdr(word), n(BY_NAME), "flagged by-name");
            assert_eq!(h.car(h.car(word)), TaggedValue::UNBOUND);
        }
        let second = table(&["other", "alias"]);
        second.link_in(&heap, &ns);
        linked.link_in(&heap, &ns);
        assert_eq!(
            (second.entry(0), second.entry(1), linked.entry(0)),
            (word, word, word),
            "one record for every name, table and link"
        );
        assert_eq!(heap.borrow_mut().by_name_record(), word, "the heap's");
        assert!(ns.weak_entries().is_none(), "no store");
        assert!(!heap.borrow().placeholder_list().holds(ns.env_id()));
        assert_eq!(counts(&heap.borrow()), before, "counted nowhere");

        assert_eq!(linked.get(&heap.borrow(), 0), None, "unbound by the link");
        assert_eq!(linked.read_cold(&heap, 0), None, "the target is unbound");
        library.define(&heap, "target", n(100));
        assert_eq!(linked.get(&heap.borrow(), 0), None);
        assert_eq!(linked.read_cold(&heap, 0), Some(n(100)), "read by name");
        assert_eq!(ns.get(&heap, "alias"), Some(n(100)));
    }

    /// The linked read (#836): `get` answers every binding of the
    /// namespace's own and every import, the exporter's value as it is now,
    /// and `None` for a placeholder, which the cold read reads by name; a
    /// define adopts the placeholder, keeping its word, and `get` answers
    /// it from then on.
    #[test]
    fn get_answers_an_own_or_imported_binding_and_an_adopted_placeholder() {
        let heap = new_shared_heap();
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "count", n(7));
        let ns = Rc::new(Environment::with_heap(&heap));
        ns.define(&heap, "own", n(1));
        let count = library.cell_of(&heap, "count").expect("count's cell");
        ns.import_cell(&heap, "imported", count);
        let linked = table(&["own", "imported", "later"]);
        linked.link_in(&heap, &ns);
        let get = |k| linked.get(&heap.borrow(), k);

        assert_eq!(get(0), Some(n(1)), "its own");
        assert_eq!(get(1), Some(n(7)), "the exporter's");
        assert_eq!(get(2), None, "a placeholder");
        assert_eq!(linked.read_cold(&heap, 0), Some(n(1)));
        assert_eq!(linked.read_cold(&heap, 2), None, "unbound");
        library.define(&heap, "count", n(8));
        assert_eq!(get(1), Some(n(8)), "the exporter's value now");

        let placeholder = linked.entry(2);
        ns.define(&heap, "later", n(5));
        assert_eq!(linked.entry(2), placeholder, "adopted, the same word");
        assert_eq!(get(2), Some(n(5)));
        assert_eq!(linked.read_cold(&heap, 2), Some(n(5)));
    }

    /// The cold read through a host namespace's read-through records (#835,
    /// #836). A strong one, over the parent's binding, answers `None` to
    /// `get`, since its own cell names no binding, and the parent's value,
    /// as it is now, to `read_cold`; once the host reaches the name by an
    /// alias of its own, `read_cold` reads the alias, as `Environment::get`
    /// finds it first. A weak one, over the parent's placeholder, answers
    /// `None` to both until the parent defines the name, and then the
    /// parent's value to `read_cold` alone: the parent's define adopts its
    /// placeholder, not the host's record.
    #[test]
    fn read_cold_reads_a_read_through_record_s_parent_unless_the_host_aliases_it() {
        let heap = new_shared_heap();
        let global = Rc::new(Environment::with_heap(&heap));
        global.define(&heap, "x", n(1));
        let host = Rc::new(Environment::host_namespace(Rc::clone(&global)));
        let linked = table(&["x", "y"]);
        linked.link_in(&heap, &host);
        let get = |k| linked.get(&heap.borrow(), k);

        assert_eq!(get(0), None, "a strong child's own cell names no binding");
        assert_eq!(linked.read_cold(&heap, 0), Some(n(1)), "the parent's");
        global.define(&heap, "x", n(2));
        assert_eq!(linked.read_cold(&heap, 0), Some(n(2)), "as it is now");

        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "x", n(100));
        host.define_alias("x", Rc::clone(&library), Rc::from("x"));
        assert_eq!(get(0), None);
        assert_eq!(
            linked.read_cold(&heap, 0),
            Some(n(100)),
            "the host's alias, before the parent"
        );
        assert_eq!(host.get(&heap, "x"), Some(n(100)));

        assert_eq!(get(1), None, "a weak child");
        assert_eq!(linked.read_cold(&heap, 1), None, "over a placeholder");
        global.define(&heap, "y", n(9));
        assert_eq!(get(1), None, "the parent adopted its own placeholder");
        assert_eq!(linked.read_cold(&heap, 1), Some(n(9)), "read through it");
    }

    /// The cell a linked store writes (#836): an own record's cell; an
    /// import's, the exporter's, so the exporter sees the store; for a
    /// host's read-through record over a bound parent, the parent's cell,
    /// while the host's own cell, which every read-through record of the
    /// host shares, still holds `UNBOUND`, so another name the host reads
    /// through is still unbound by its link; and none for a placeholder or
    /// a name the host reaches by an alias, which a store writes by name.
    #[test]
    fn assignee_resolves_the_cell_a_store_writes() {
        let heap = new_shared_heap();
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "count", n(7));
        let ns = Rc::new(Environment::with_heap(&heap));
        ns.define(&heap, "own", n(1));
        let count = library.cell_of(&heap, "count").expect("count's cell");
        ns.import_cell(&heap, "imported", count);
        let linked = table(&["own", "imported", "later"]);
        linked.link_in(&heap, &ns);
        let store = |table: &LinkTable, k, value| {
            let (to, old) = table.assignee(&heap.borrow(), k).expect("a cell");
            LinkTable::assign(&mut heap.borrow_mut(), to, value);
            old
        };

        assert_eq!(store(&linked, 0, n(2)), n(1), "its old value");
        assert_eq!(ns.get(&heap, "own"), Some(n(2)));
        assert_eq!(store(&linked, 1, n(8)), n(7));
        assert_eq!(
            library.get(&heap, "count"),
            Some(n(8)),
            "the exporter sees it"
        );
        assert!(
            linked.assignee(&heap.borrow(), 2).is_none(),
            "a placeholder"
        );
        assert_eq!(linked.read_cold(&heap, 2), None, "left unbound");

        let global = Rc::new(Environment::with_heap(&heap));
        global.define(&heap, "x", n(1));
        global.define(&heap, "z", n(4));
        let host = Rc::new(Environment::host_namespace(Rc::clone(&global)));
        let hosted = table(&["x", "z", "w"]);
        hosted.link_in(&heap, &host);
        assert_eq!(store(&hosted, 0, n(3)), n(1), "the parent's value");
        assert_eq!(global.get(&heap, "x"), Some(n(3)), "into the parent's cell");
        assert_eq!(
            hosted.get(&heap.borrow(), 1),
            None,
            "the host's own cell still holds `UNBOUND`"
        );
        assert_eq!(hosted.read_cold(&heap, 1), Some(n(4)));
        assert!(hosted.assignee(&heap.borrow(), 2).is_none(), "a weak child");

        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "x", n(100));
        host.define_alias("x", Rc::clone(&library), Rc::from("x"));
        assert!(
            hosted.assignee(&heap.borrow(), 0).is_none(),
            "the host's alias: by name"
        );
    }

    /// A table is linked in the namespace it was compiled in (#836): a debug
    /// build refuses another before anything links, and a table compiled
    /// with no namespace, `compiled_in` 0, links in any.
    #[test]
    fn a_table_is_linked_in_the_namespace_it_was_compiled_in() {
        let heap = new_shared_heap();
        let compiled = Rc::new(Environment::with_heap(&heap));
        let other = Rc::new(Environment::with_heap(&heap));
        other.define(&heap, "x", n(1));
        let linked = table(&["x"]);
        linked.set_compiled_in(compiled.env_id());
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            linked.link_in(&heap, &other);
        }));
        assert_eq!(refused.is_err(), cfg!(debug_assertions));
        match refused {
            Err(panic) => {
                assert_eq!(
                    panic_message(&*panic),
                    Some("a unit is linked in the namespace it was compiled in (#836)")
                );
                assert!(linked.namespace().is_none(), "refused before it links");
                assert_eq!(linked.entry(0), TaggedValue::UNBOUND);
            }
            Ok(()) => assert_eq!(linked.get(&heap.borrow(), 0), Some(n(1))),
        }

        let at_home = table(&["x"]);
        at_home.set_compiled_in(other.env_id());
        at_home.link_in(&heap, &other);
        assert_eq!(at_home.get(&heap.borrow(), 0), Some(n(1)));
        let anywhere = table(&["x"]);
        anywhere.link_in(&heap, &other);
        assert!(Rc::ptr_eq(anywhere.namespace().unwrap(), &other));
        assert_eq!(anywhere.get(&heap.borrow(), 0), Some(n(1)));
    }

    /// The cold read's check (#836): a placeholder a table links is its
    /// namespace's store's entry, which the pass at the sweep's start keeps
    /// while the table is traced. One taken out of the store by hand, as a
    /// pass that dropped it while a traced table linked it would leave it,
    /// is refused at the next cold read, before anything reads its cell.
    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "is no longer its namespace's store's entry")]
    fn a_check_build_refuses_a_linked_placeholder_out_of_its_store() {
        let heap = new_shared_heap();
        let ns = Rc::new(Environment::with_heap(&heap));
        let linked = table(&["later"]);
        linked.link_in(&heap, &ns);
        let store = ns.weak_entries().expect("its store");
        store.take("later").expect("the placeholder");
        linked.read_cold(&heap, 0);
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
    /// decided (`docs/GC_DESIGN.md` §5.4, rule 2). Its traced fields each
    /// hold what only they reach: `entries` a record whose namespace was
    /// dropped, which keeps the record, its cell and the value; `namespace`
    /// another namespace, held by nothing else, whose binding of `y` it
    /// keeps (#836). Two namespaces, so that neither field's sentinel is
    /// reached through the other.
    #[test]
    fn link_table_fields() {
        let heap = new_shared_heap();
        let mut s = Sentinels::new(&mut heap.borrow_mut());
        let value = s.pair(
            &mut heap.borrow_mut(),
            "LinkTable.entries[0]: through x's record and its cell",
        );
        let bound = s.vector(
            &mut heap.borrow_mut(),
            "LinkTable.namespace: y, bound only there",
        );
        let record = {
            let ns = Rc::new(Environment::with_heap(&heap));
            ns.define(&heap, "x", value);
            ns.local_record("x").expect("x's record").word()
        };
        let namespace = Rc::new(Environment::with_heap(&heap));
        namespace.define(&heap, "y", bound);
        let linked = LinkTable {
            entries: Box::new([Cell::new(record)]),
            names: Box::new([Rc::from("x")]),
            namespace: OnceCell::from(namespace),
            compiled_in: Cell::new(0),
            traced: Cell::new(0),
        };
        collect_only(&mut heap.borrow_mut(), |visitor| linked.trace(visitor));
        s.assert_survived(&heap.borrow());
    }

    /// A table is traced once an epoch (#836): the first ask in an epoch
    /// answers that it was not traced yet and records it, every later one
    /// in that epoch that it was, and a new epoch starts again. `traced_in`
    /// answers for the epoch the table was last traced in, and no other:
    /// neither one before it, nor one after.
    #[test]
    fn a_table_is_traced_once_an_epoch() {
        let linked = table(&["x"]);
        assert!(!linked.traced_in(1), "not traced in epoch 1 before it");
        assert!(!linked.trace_once(1), "not yet traced in epoch 1");
        assert!(linked.traced_in(1), "traced in epoch 1 once asked");
        assert!(linked.trace_once(1), "traced in epoch 1 already");
        assert!(linked.trace_once(1));
        assert!(!linked.traced_in(2), "not in an epoch it has not reached");
        assert!(!linked.trace_once(2), "a new epoch");
        assert!(linked.trace_once(2));
        assert!(linked.traced_in(2));
        assert!(!linked.traced_in(1), "nor in an epoch before its last");
    }

    /// Linking sets the namespace the table holds (#836): once the
    /// namespace is dropped, a collection with only the table as a root
    /// keeps `x`, which the table links, and `y`, which it does not and
    /// which only the namespace binds.
    #[test]
    fn a_linked_table_holds_its_namespace() {
        let heap = new_shared_heap();
        let mut s = Sentinels::new(&mut heap.borrow_mut());
        let x = s.pair(&mut heap.borrow_mut(), "x: linked by the table");
        let y = s.vector(
            &mut heap.borrow_mut(),
            "y: bound in the namespace the table was linked in, and not linked",
        );
        let linked = table(&["x"]);
        {
            let ns = Rc::new(Environment::with_heap(&heap));
            ns.define(&heap, "x", x);
            ns.define(&heap, "y", y);
            linked.link_in(&heap, &ns);
        }
        collect_only(&mut heap.borrow_mut(), |visitor| linked.trace(visitor));
        s.assert_survived(&heap.borrow());
    }

    /// A table is linked in one namespace (#836): a second link, in another,
    /// relinks against the first, so that the table never names one
    /// namespace's records while it holds another, and a debug build then
    /// refuses it. One test for every build, so that the debug lane tests the
    /// relink a release build relies on.
    #[test]
    fn a_second_namespace_relinks_in_the_first() {
        let heap = new_shared_heap();
        let linked = table(&["x"]);
        let first = Rc::new(Environment::with_heap(&heap));
        let second = Rc::new(Environment::with_heap(&heap));
        first.define(&heap, "x", TaggedValue::fixnum(1));
        second.define(&heap, "x", TaggedValue::fixnum(2));
        linked.link_in(&heap, &first);
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            linked.link_in(&heap, &second);
        }));
        assert_eq!(
            refused.is_err(),
            cfg!(debug_assertions),
            "a debug build refuses it; a release build relinks silently"
        );
        if let Err(panic) = refused {
            assert_eq!(
                panic_message(&*panic),
                Some("a unit's table is linked in one namespace (#836)")
            );
        }
        assert_eq!(
            linked.entry(0),
            first.local_record("x").unwrap().word(),
            "linked against the first namespace"
        );
    }
}
