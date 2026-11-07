//! The one API through which a top-level environment's own bindings are read
//! and written (addition C6, `PRD/GC_PRD.md` §18.6; #830).
//!
//! [`NamespaceView`] is a view over an [`Environment`], by reference and
//! `Copy`, with the same signatures the environment's methods have. What goes
//! through it: the VM's global path and its definitions, the funnels every
//! import and every Rust-built library's definition takes
//! (`Library::import_into`, `define_primitive`, `Cx::define_primitive` and
//! `Cx::lookup`), the writes that go around them (the desugarer's staged
//! import, a host library's exports, the builders' own definitions), the
//! hosts' `define` and `lookup`, and the desugarer's alias installs. The
//! desugarer's and the expander's queries reach namespaces through `Cx`,
//! which passes the context's heap: most through methods over this view
//! (`Cx::binding_location`, `Cx::scoped_binding_of` and the rest), and
//! `Cx::resolve_with_scopes` and `Cx::scoped_macro_binding` through
//! [`Environment`]'s own methods, since they resolve from whatever frame the
//! expansion holds. What stays on [`Environment`]'s methods: the
//! tree-walker's reads and writes by name, which run from whatever frame the
//! step holds; the backends' reading of a top-level form's head
//! (`patina_frontend`'s `top_level_head`, by `get_with_scopes`); and the
//! loaders' export snapshots, which take each export's cell
//! (`Environment::cell_of`, #834). The view, not
//! the type system, holds the discipline: namespaces and frames are one type
//! (4b's start decision 4 on #782).
//!
//! Every read or write of a binding takes the heap its values live on,
//! `heap: &RefCell<Heap>`, which the environment borrows at the namespace
//! step (#834): shared for a lookup's hit, mutable for an insert, a
//! re-point or a store. A frame's own binding takes no borrow; a frame's
//! import borrows it to read or write its cell, and installing one takes
//! none.
//!
//! Its operations come in four groups, implemented over binding records
//! (#834): a namespace's binding is a record in its slot naming a cell, an
//! import is a record naming the exporter's cell. The first three are what a
//! threaded build keeps apart, C6's
//! "lookup, insert, re-point" (`PRD/study/gc/followup/parallelism/itemize.md`,
//! its R3: lookups over a snapshot, inserts under a lock, a re-point as a
//! release store); a store writes a cell, which a threaded build treats as
//! any heap store:
//! - **lookups** read and never write: not a binding, not a table, not a
//!   charge. A lookup of a name that is not bound inserts nothing;
//! - **inserts** give a name a plain slot of this environment's where it has
//!   none, or record what binds no name (a macro-introduced definition's
//!   identity). A name the environment reached only through a
//!   macro-expansion alias, the name-only view of a scoped definition or its
//!   parent is inserted all the same: the new slot shadows what it reached;
//! - **re-points** make a name with a plain slot here mean another location:
//!   a `define` over an import, and an import over a definition or another
//!   import (§11.6's Re-pointing row);
//! - **stores** write a value into the location a name means, which is
//!   where [`Environment::get`] reads it: this environment's own binding;
//!   the library's for a name it imported, since an import is one location,
//!   not a copy (R7RS §5.2, #406); and, for `set!` by name, an alias's
//!   target, a scoped definition or the parent's binding.
//!
//! A `define` and an import each keep one signature, and can insert, re-point
//! or store depending on what the name held; each method says which outcome
//! is which. The view asserts no kind: the VM reads a host environment
//! through it, which is a frame until 4b.8, and the desugarer's relinks write
//! a speculative scan's private frame. Its lookups by name walk to the parent
//! as [`Environment::get`] does.

use crate::binding_record::BindingCell;
use crate::environment::{BindingLocation, Environment, IntroducedDefinition};
use crate::heap::Heap;
use crate::procedure::{Arity, Procedure};
use crate::scope::ScopeSet;
use crate::scope_resolve::AmbiguousReference;
use crate::tagged_value::TaggedValue;
use std::cell::RefCell;
use std::rc::Rc;

/// A top-level environment's own bindings, as the one API the sites in the
/// [module's](self) list read and write them through. See the module for the
/// four groups its operations fall in.
///
/// **The raw layer** (#831), as [`Environment`]'s methods are: outside the
/// embedding API's promise (`docs/README.md`, "Changing the embedding API").
/// Implemented over binding records (#834).
#[derive(Clone, Copy)]
pub struct NamespaceView<'e> {
    env: &'e Environment,
}

impl<'e> NamespaceView<'e> {
    /// The view of `env`: a namespace, or a frame the VM or the desugarer
    /// reads and writes as one (a host's environment, a scan's private frame).
    #[inline(always)]
    pub fn of(env: &'e Environment) -> Self {
        NamespaceView { env }
    }

    // ---------------------------------------------------------------------
    // Lookups: each reads and none writes.
    // ---------------------------------------------------------------------

    /// The environment's identity, which the VM's per-site cache keys on
    /// beside the slot ([`Environment::env_id`]).
    #[inline(always)]
    pub fn env_id(self) -> u64 {
        self.env.env_id()
    }

    /// The value `name` means here, walking to the parent as
    /// [`Environment::get`] does: a plain binding, read through an import to
    /// the library's location, then a macro-expansion alias, then the
    /// name-only view of a scoped definition, then the parent. `None` for a
    /// name bound nowhere on the chain, which inserts nothing. A hit in a
    /// namespace borrows `heap` shared.
    #[inline]
    pub fn get(self, heap: &RefCell<Heap>, name: &str) -> Option<TaggedValue> {
        self.env.get(heap, name)
    }

    /// The slot of `name` in this environment's own plain bindings, its own
    /// or imported, never the parent's ([`Environment::local_slot`]). Stable
    /// for the environment's life, which the VM's per-site cache keys on
    /// until 4b.9's link tables.
    #[inline(always)]
    pub fn local_slot(self, name: &str) -> Option<u32> {
        self.env.local_slot(name)
    }

    /// The value in a slot [`local_slot`](Self::local_slot) answered, read
    /// through an import to the library's location
    /// ([`Environment::slot_value`]). A namespace borrows `heap` shared.
    #[inline(always)]
    pub fn slot_value(self, heap: &RefCell<Heap>, slot: u32) -> TaggedValue {
        self.env.slot_value(heap, slot)
    }

    /// [`slot_value`](Self::slot_value) with the heap already borrowed
    /// ([`Environment::slot_value_in`]).
    #[inline(always)]
    pub fn slot_value_in(self, heap: &Heap, slot: u32) -> TaggedValue {
        self.env.slot_value_in(heap, slot)
    }

    /// The location [`get`](Self::get) reads for `name`, by the same walk, or
    /// `None` where it finds nothing ([`Environment::binding_location`]). A
    /// hit in a namespace borrows `heap` shared.
    #[inline]
    pub fn binding_location(self, heap: &RefCell<Heap>, name: &str) -> Option<BindingLocation> {
        self.env.binding_location(heap, name)
    }

    /// Which scoped binding a reference to `name` at `scopes` denotes, by
    /// the scope set it was bound at ([`Environment::scoped_binding_of`]):
    /// `None` when a read would fall back by name. Each namespace's hit
    /// borrows `heap` shared.
    #[inline]
    pub fn scoped_binding_of(
        self,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<Option<ScopeSet>, Box<AmbiguousReference>> {
        self.env.scoped_binding_of(heap, name, scopes)
    }

    /// Whether the name-only view of `name` is the binding a reference at
    /// `scopes` denotes ([`Environment::name_reaches_binding_of`]). Borrows
    /// `heap` as [`scoped_binding_of`](Self::scoped_binding_of) does.
    #[inline]
    pub fn name_reaches_binding_of(
        self,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<bool, Box<AmbiguousReference>> {
        self.env.name_reaches_binding_of(heap, name, scopes)
    }

    /// The top-level definition a macro introduced that `scopes` selects
    /// for `name`, held by this chain's root
    /// ([`Environment::introduced_definition`]). Borrows `heap` shared at
    /// each namespace's step.
    #[inline]
    pub fn introduced_definition(
        self,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Option<IntroducedDefinition> {
        self.env.introduced_definition(heap, name, scopes)
    }

    /// The cell this environment's own binding of `name` names
    /// ([`Environment::cell_of`]): a namespace's record's, or a frame's
    /// import's. `None` for a frame's own binding, a name reached through an
    /// alias or the parent, or one not bound. A namespace borrows `heap`
    /// shared.
    #[inline]
    pub fn cell_of(self, heap: &RefCell<Heap>, name: &str) -> Option<BindingCell> {
        self.env.cell_of(heap, name)
    }

    /// The names a program bound in this environment only, in slot order
    /// ([`Environment::local_names`]). A namespace holding an import alias
    /// borrows `heap` shared.
    #[inline]
    pub fn local_names(self, heap: &RefCell<Heap>) -> Vec<String> {
        self.env.local_names(heap)
    }

    // ---------------------------------------------------------------------
    // A `define` and an import: an insert, a re-point or a store.
    // ---------------------------------------------------------------------

    /// Define `name` as `value` here ([`Environment::define`]):
    /// - an **insert** when the name has no plain slot here: it gets one of
    ///   its own, which shadows whatever the name reached before (an alias,
    ///   a scoped definition's name-only view, the parent's binding);
    /// - a **store** when its binding here is this environment's own: the
    ///   slot and its location stay, and the value changes (R7RS's "acts like
    ///   `set!`");
    /// - a **re-point** when the name is an import: the slot becomes a
    ///   binding of this environment's own, holding `value`, and the
    ///   library's binding is left alone, as chibi and Gauche leave it. The
    ///   name keeps its slot, and its location changes.
    ///
    /// A namespace borrows `heap` mutably for each.
    #[inline]
    pub fn define(self, heap: &RefCell<Heap>, name: impl Into<Rc<str>>, value: TaggedValue) {
        self.env.define(heap, name, value);
    }

    /// Define `name` at `scopes` here ([`Environment::define_with_scopes`]):
    /// - an **insert** of a scoped binding where the name has none at that
    ///   scope set;
    /// - a **store** over the one it has at that scope set;
    /// - when `scopes` is empty, [`define`](Self::define) by name, so an
    ///   insert, a store or a **re-point** of an import by `define`'s rules.
    ///
    /// A namespace borrows `heap` mutably for each.
    #[inline]
    pub fn define_with_scopes(
        self,
        heap: &RefCell<Heap>,
        name: impl Into<Rc<str>>,
        scopes: ScopeSet,
        value: TaggedValue,
    ) {
        self.env.define_with_scopes(heap, name, scopes, value);
    }

    /// Make `name` here the binding `cell` is, an import
    /// ([`Environment::import_cell`]):
    /// - an **insert** when the name has no plain slot here: a record naming
    ///   the cell;
    /// - a **re-point** when it has one, a definition or another import:
    ///   the slot keeps its record, which names the cell from now on.
    ///
    /// An own cell under another name is copied, by `define`'s rules. A
    /// namespace borrows `heap` mutably; a frame's import takes no borrow
    /// and allocates nothing.
    #[inline]
    pub fn import_cell(self, heap: &RefCell<Heap>, name: impl Into<Rc<str>>, cell: BindingCell) {
        self.env.import_cell(heap, name, cell);
    }

    /// [`import_cell`](Self::import_cell) from a staging environment, which
    /// owns nothing ([`Environment::copy_binding`]): a staged import arrives
    /// as an import of the same cell (an insert or a re-point, as
    /// `import_cell`'s), and a staged value as a definition of it (an
    /// insert, a store or a re-point, as [`define`](Self::define)'s).
    /// Borrows `heap` as they do.
    #[inline]
    pub fn copy_binding(
        self,
        heap: &RefCell<Heap>,
        name: impl Into<Rc<str>>,
        staging: &Rc<Environment>,
        staged_name: &str,
    ) -> bool {
        self.env.copy_binding(heap, name, staging, staged_name)
    }

    // ---------------------------------------------------------------------
    // Inserts.
    // ---------------------------------------------------------------------

    /// Define `name` here as a primitive procedure of `library`, named
    /// `library/name` (`scheme.base/+`). A Rust-built library binds each name
    /// once, so this is an insert; it is [`define`](Self::define) all the
    /// same, so a name bound before is stored or re-pointed as `define`
    /// would. The qualified name is made once here, not on each call.
    pub fn define_primitive(
        self,
        heap: &RefCell<Heap>,
        name: &'static str,
        arity: Arity,
        library: Vec<String>,
    ) {
        let qualified_name: Rc<str> = Rc::from(format!("{}/{}", library.join("."), name));
        let proc = Procedure::primitive(name, arity, qualified_name, None);
        // The allocation's borrow ends with its statement, before `define`
        // takes the namespace step.
        let tv = heap.borrow_mut().alloc_procedure(proc);
        self.define(heap, name, tv);
    }

    /// Record a macro-introduced top-level definition's identity: `name`
    /// introduced at `scopes`, renamed to `renamed_to`
    /// ([`Environment::define_introduced_global`]). It binds no name; the
    /// `Define` of `renamed_to` does.
    #[inline]
    pub fn define_introduced_global(self, name: Rc<str>, scopes: ScopeSet, renamed_to: Rc<str>) {
        self.env.define_introduced_global(name, scopes, renamed_to);
    }

    /// A name of this environment's own for the location `name` imports
    /// ([`Environment::import_alias`]): the one remembered for that
    /// location and spelling while it still is that import, which writes
    /// nothing, or a fresh one `mint` offers, inserted as an import of the
    /// same location. `None` when `name` is not an import here. A namespace
    /// borrows `heap` shared over the lookup and mutably to insert.
    #[inline]
    pub fn import_alias(
        self,
        heap: &RefCell<Heap>,
        name: &str,
        mint: impl FnMut() -> Rc<str>,
    ) -> Option<Rc<str>> {
        self.env.import_alias(heap, name, mint)
    }

    /// An alias here to `name` in `target_env`, under `scopes`
    /// ([`Environment::relink_alias`]): the one remembered for that target,
    /// or the root's when this is a frame, which writes nothing; or a fresh
    /// one `mint` offers, inserted as a macro-expansion alias.
    #[inline]
    pub fn relink_alias(
        self,
        target_env: Rc<Environment>,
        name: Rc<str>,
        scopes: Option<ScopeSet>,
        mention: &ScopeSet,
        mint: impl FnMut() -> Rc<str>,
    ) -> Rc<str> {
        self.env
            .relink_alias(target_env, name, scopes, mention, mint)
    }

    /// [`relink_alias`](Self::relink_alias) to the definition
    /// [`Environment::introduced_definition`] found in `home`
    /// ([`Environment::relink_alias_to_introduced`]).
    #[inline]
    pub fn relink_alias_to_introduced(
        self,
        home: Rc<Environment>,
        name: Rc<str>,
        found: IntroducedDefinition,
        mention: &ScopeSet,
        mint: impl FnMut() -> Rc<str>,
    ) -> Rc<str> {
        self.env
            .relink_alias_to_introduced(home, name, found, mention, mint)
    }

    // ---------------------------------------------------------------------
    // Stores.
    // ---------------------------------------------------------------------

    /// `set!` of `name`: a store into the location [`get`](Self::get) reads,
    /// by the same walk ([`Environment::set`]): this environment's own
    /// binding, or the library's for an import, an alias's target, a scoped
    /// definition, or the parent's. `Err` with the name when it is bound
    /// nowhere on the chain, having written nothing. The namespace whose
    /// binding it writes borrows `heap` mutably.
    #[inline]
    pub fn set(self, heap: &RefCell<Heap>, name: &str, value: TaggedValue) -> Result<(), String> {
        self.env.set(heap, name, value)
    }

    /// A store into a slot [`local_slot`](Self::local_slot) answered: this
    /// environment's own binding, or the library's for an import
    /// ([`Environment::set_slot_value`]). A namespace borrows `heap`
    /// mutably.
    #[inline(always)]
    pub fn set_slot_value(self, heap: &RefCell<Heap>, slot: u32, value: TaggedValue) {
        self.env.set_slot_value(heap, slot, value);
    }

    // ---------------------------------------------------------------------
    // Within the crate.
    // ---------------------------------------------------------------------

    /// The environment this views, for `Cx`'s methods over the view.
    #[inline(always)]
    pub(crate) fn env(self) -> &'e Environment {
        self.env
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::new_shared_heap;

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    /// A library with one binding, and a namespace that defines `own` and
    /// imports the library's `count` as `imported`.
    fn library_and_namespace(heap: &crate::heap::SharedHeap) -> (Rc<Environment>, Rc<Environment>) {
        let library = Rc::new(Environment::with_heap(heap));
        NamespaceView::of(&library).define(heap, "count", n(1));
        let namespace = Rc::new(Environment::with_heap(heap));
        let view = NamespaceView::of(&namespace);
        view.define(heap, "own", n(2));
        let count = NamespaceView::of(&library).cell_of(heap, "count").unwrap();
        view.import_cell(heap, "imported", count);
        (library, namespace)
    }

    /// The heap's running sums of records and cells (#834).
    fn sums(heap: &crate::heap::SharedHeap) -> (usize, usize) {
        let heap = heap.borrow();
        (heap.binding_records(), heap.cells())
    }

    /// That `env`'s running sums are what its tables hold.
    fn assert_recounted(heap: &crate::heap::SharedHeap, env: &Environment) {
        assert_eq!(env.recount(&heap.borrow()), env.counted());
    }

    #[test]
    fn a_lookup_inserts_nothing() {
        let heap = new_shared_heap();
        let (library, namespace) = library_and_namespace(&heap);
        let view = NamespaceView::of(&namespace);
        let names = namespace.local_names(&heap);
        let charged = namespace.charged_bytes();
        let library_charged = library.charged_bytes();
        let counted = sums(&heap);

        for _ in 0..3 {
            assert_eq!(view.get(&heap, "own"), Some(n(2)));
            assert_eq!(view.get(&heap, "imported"), Some(n(1)));
            assert_eq!(view.get(&heap, "unbound"), None);
            assert!(view.binding_location(&heap, "own").is_some());
            assert_eq!(
                view.binding_location(&heap, "imported"),
                library.binding_location(&heap, "count")
            );
            assert_eq!(view.binding_location(&heap, "unbound"), None);
            let own = view.local_slot("own").unwrap();
            let imported = view.local_slot("imported").unwrap();
            assert_eq!(view.slot_value(&heap, own), n(2));
            assert_eq!(view.slot_value(&heap, imported), n(1));
            assert_eq!(view.local_slot("unbound"), None);
            assert_eq!(view.cell_of(&heap, "unbound"), None);
            assert_eq!(sums(&heap), counted, "a lookup makes no record or cell");
        }

        assert_eq!(namespace.local_names(&heap), names);
        assert_eq!(namespace.charged_bytes(), charged);
        assert_eq!(library.charged_bytes(), library_charged);
        assert_eq!(sums(&heap), counted);
        assert_recounted(&heap, &namespace);
        assert_recounted(&heap, &library);
    }

    #[test]
    fn each_write_is_an_insert_a_store_or_a_re_point() {
        let heap = new_shared_heap();
        let (library, namespace) = library_and_namespace(&heap);
        let view = NamespaceView::of(&namespace);
        let names = |env: &Environment| env.local_names(&heap).len();
        let count = NamespaceView::of(&library).cell_of(&heap, "count").unwrap();
        let record_word = |name: &str| namespace.local_record(name).unwrap().word();
        assert_recounted(&heap, &namespace);

        // Inserts: a fresh `define` and a fresh import add a name and a
        // record; the `define` makes a cell, the import none.
        let before = names(&namespace);
        let (records, cells) = sums(&heap);
        view.define(&heap, "fresh", n(3));
        assert_eq!(names(&namespace), before + 1);
        assert_eq!(sums(&heap), (records + 1, cells + 1));
        view.import_cell(&heap, "fresh-import", count);
        assert_eq!(names(&namespace), before + 2);
        assert_eq!(sums(&heap), (records + 2, cells + 1));
        assert_eq!(
            view.binding_location(&heap, "fresh-import"),
            library.binding_location(&heap, "count")
        );
        assert_recounted(&heap, &namespace);

        // A store: a `define` over the namespace's own binding keeps its
        // location, its record and its cell, and changes its value; so does
        // `set!`.
        let location = view.binding_location(&heap, "own");
        let word = record_word("own");
        let cell = view.cell_of(&heap, "own");
        let counted = sums(&heap);
        let count_names = names(&namespace);
        view.define(&heap, "own", n(4));
        assert_eq!(view.binding_location(&heap, "own"), location);
        assert_eq!(view.get(&heap, "own"), Some(n(4)));
        view.set(&heap, "own", n(5)).unwrap();
        assert_eq!(view.binding_location(&heap, "own"), location);
        assert_eq!(view.get(&heap, "own"), Some(n(5)));
        assert_eq!(
            (record_word("own"), view.cell_of(&heap, "own")),
            (word, cell)
        );
        assert_eq!(names(&namespace), count_names);
        assert_eq!(sums(&heap), counted);
        // A store through an import writes the library's location.
        let slot = view.local_slot("imported").unwrap();
        view.set_slot_value(&heap, slot, n(6));
        assert_eq!(library.get(&heap, "count"), Some(n(6)));
        assert_eq!(sums(&heap), counted);
        assert_recounted(&heap, &namespace);

        // Re-points: a `define` over an import, and an import over a
        // definition, change the name's location and cell and add no name
        // and no record. The first makes a cell of the namespace's own, the
        // second lets one go.
        let imported = view.binding_location(&heap, "imported");
        let word = record_word("imported");
        let (records, cells) = sums(&heap);
        view.define(&heap, "imported", n(7));
        assert_ne!(view.binding_location(&heap, "imported"), imported);
        assert_ne!(view.cell_of(&heap, "imported"), Some(count));
        assert_eq!(
            record_word("imported"),
            word,
            "the record stays in its slot"
        );
        assert_eq!(view.get(&heap, "imported"), Some(n(7)));
        assert_eq!(
            library.get(&heap, "count"),
            Some(n(6)),
            "the library's binding is left alone"
        );
        assert_eq!(names(&namespace), count_names);
        assert_eq!(sums(&heap), (records, cells + 1));
        assert_recounted(&heap, &namespace);

        let own = view.binding_location(&heap, "own");
        let word = record_word("own");
        view.import_cell(&heap, "own", count);
        assert_ne!(view.binding_location(&heap, "own"), own);
        assert_eq!(view.cell_of(&heap, "own"), Some(count));
        assert_eq!(record_word("own"), word, "the record stays in its slot");
        assert_eq!(
            view.binding_location(&heap, "own"),
            library.binding_location(&heap, "count")
        );
        assert_eq!(view.get(&heap, "own"), Some(n(6)));
        assert_eq!(names(&namespace), count_names);
        assert_eq!(sums(&heap), (records, cells));
        assert_recounted(&heap, &namespace);

        // An unbound `set!` writes nothing.
        assert_eq!(view.set(&heap, "unbound", n(8)), Err("unbound".to_string()));
        assert_eq!(names(&namespace), count_names);
        assert_eq!(sums(&heap), (records, cells));
        assert_recounted(&heap, &library);
    }
}
