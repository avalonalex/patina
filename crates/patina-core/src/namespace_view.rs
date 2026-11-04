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
//! hosts' `define` and `lookup`, and the desugarer's alias installs. What
//! stays on [`Environment`]'s methods: the tree-walker's reads and writes by
//! name, which run from whatever frame the step holds; the desugarer's and
//! the expander's queries; and the loaders' export snapshots, which 4b.6
//! rewrites with cells. The view, not the type system, holds the discipline:
//! namespaces and frames are one type (4b's start decision 4 on #782).
//!
//! Its operations come in four groups, which 4b.6 implements over binding
//! records. The first three are what a threaded build keeps apart, C6's
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

use crate::environment::{BindingLocation, Environment, IntroducedDefinition};
use crate::heap::Heap;
use crate::procedure::{Arity, Procedure};
use crate::scope::ScopeSet;
use crate::tagged_value::TaggedValue;
use std::cell::RefCell;
use std::rc::Rc;

/// A top-level environment's own bindings, as the one API the sites in the
/// [module's](self) list read and write them through. See the module for the
/// four groups its operations fall in.
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
    /// name bound nowhere on the chain, which inserts nothing.
    #[inline]
    pub fn get(self, name: &str) -> Option<TaggedValue> {
        self.env.get(name)
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
    /// ([`Environment::slot_value`]).
    #[inline(always)]
    pub fn slot_value(self, slot: u32) -> TaggedValue {
        self.env.slot_value(slot)
    }

    /// The location [`get`](Self::get) reads for `name`, by the same walk, or
    /// `None` where it finds nothing ([`Environment::binding_location`]).
    #[inline]
    pub fn binding_location(self, name: &str) -> Option<BindingLocation> {
        self.env.binding_location(name)
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
    #[inline]
    pub fn define(self, name: impl Into<Rc<str>>, value: TaggedValue) {
        self.env.define(name, value);
    }

    /// Import `source_name` from `source` as `name` here, as the same
    /// location ([`Environment::share_binding`]):
    /// - an **insert** when the name has no plain slot here;
    /// - a **re-point** when it has one, a definition or another import:
    ///   the slot stays and now means `source`'s binding, resolved to the
    ///   environment that owns it.
    ///
    /// `false`, having done nothing, when `source` has no plain binding of
    /// that name; the caller then installs the value it has, by
    /// [`define`](Self::define), as `Library::import_into` does. A name of
    /// this environment's own imported under a second name is copied, by
    /// `define`'s rules.
    #[inline]
    pub fn share_binding(
        self,
        name: impl Into<Rc<str>>,
        source: &Rc<Environment>,
        source_name: &str,
    ) -> bool {
        self.env.share_binding(name, source, source_name)
    }

    /// [`share_binding`](Self::share_binding) from a staging environment,
    /// which owns nothing ([`Environment::copy_binding`]): a staged import
    /// arrives as the same import (an insert or a re-point, as
    /// `share_binding`'s), and a staged value as a definition of it (an
    /// insert, a store or a re-point, as [`define`](Self::define)'s).
    #[inline]
    pub fn copy_binding(
        self,
        name: impl Into<Rc<str>>,
        staging: &Rc<Environment>,
        staged_name: &str,
    ) -> bool {
        self.env.copy_binding(name, staging, staged_name)
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
        let tv = heap.borrow_mut().alloc_procedure(proc);
        self.define(name, tv);
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
    /// same location. `None` when `name` is not an import here.
    #[inline]
    pub fn import_alias(self, name: &str, mint: impl FnMut() -> Rc<str>) -> Option<Rc<str>> {
        self.env.import_alias(name, mint)
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
    /// nowhere on the chain, having written nothing.
    #[inline]
    pub fn set(self, name: &str, value: TaggedValue) -> Result<(), String> {
        self.env.set(name, value)
    }

    /// A store into a slot [`local_slot`](Self::local_slot) answered: this
    /// environment's own binding, or the library's for an import
    /// ([`Environment::set_slot_value`]).
    #[inline(always)]
    pub fn set_slot_value(self, slot: u32, value: TaggedValue) {
        self.env.set_slot_value(slot, value);
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
        NamespaceView::of(&library).define("count", n(1));
        let namespace = Rc::new(Environment::with_heap(heap));
        let view = NamespaceView::of(&namespace);
        view.define("own", n(2));
        assert!(view.share_binding("imported", &library, "count"));
        (library, namespace)
    }

    #[test]
    fn a_lookup_inserts_nothing() {
        let heap = new_shared_heap();
        let (library, namespace) = library_and_namespace(&heap);
        let view = NamespaceView::of(&namespace);
        let names = namespace.local_names();
        let charged = namespace.charged_bytes();
        let library_charged = library.charged_bytes();

        for _ in 0..3 {
            assert_eq!(view.get("own"), Some(n(2)));
            assert_eq!(view.get("imported"), Some(n(1)));
            assert_eq!(view.get("unbound"), None);
            assert!(view.binding_location("own").is_some());
            assert_eq!(
                view.binding_location("imported"),
                library.binding_location("count")
            );
            assert_eq!(view.binding_location("unbound"), None);
            let own = view.local_slot("own").unwrap();
            let imported = view.local_slot("imported").unwrap();
            assert_eq!(view.slot_value(own), n(2));
            assert_eq!(view.slot_value(imported), n(1));
            assert_eq!(view.local_slot("unbound"), None);
        }

        assert_eq!(namespace.local_names(), names);
        assert_eq!(namespace.charged_bytes(), charged);
        assert_eq!(library.charged_bytes(), library_charged);
    }

    #[test]
    fn each_write_is_an_insert_a_store_or_a_re_point() {
        let heap = new_shared_heap();
        let (library, namespace) = library_and_namespace(&heap);
        let view = NamespaceView::of(&namespace);
        let names = |env: &Environment| env.local_names().len();

        // Inserts: a fresh `define` and a fresh import add a name.
        let before = names(&namespace);
        view.define("fresh", n(3));
        assert_eq!(names(&namespace), before + 1);
        assert!(view.share_binding("fresh-import", &library, "count"));
        assert_eq!(names(&namespace), before + 2);
        assert_eq!(
            view.binding_location("fresh-import"),
            library.binding_location("count")
        );

        // A store: a `define` over the namespace's own binding keeps its
        // location and changes its value; so does `set!`.
        let location = view.binding_location("own");
        let count = names(&namespace);
        view.define("own", n(4));
        assert_eq!(view.binding_location("own"), location);
        assert_eq!(view.get("own"), Some(n(4)));
        view.set("own", n(5)).unwrap();
        assert_eq!(view.binding_location("own"), location);
        assert_eq!(view.get("own"), Some(n(5)));
        assert_eq!(names(&namespace), count);
        // A store through an import writes the library's location.
        let slot = view.local_slot("imported").unwrap();
        view.set_slot_value(slot, n(6));
        assert_eq!(library.get("count"), Some(n(6)));

        // Re-points: a `define` over an import, and an import over a
        // definition, change the name's location and add no name.
        let imported = view.binding_location("imported");
        view.define("imported", n(7));
        assert_ne!(view.binding_location("imported"), imported);
        assert_eq!(view.get("imported"), Some(n(7)));
        assert_eq!(
            library.get("count"),
            Some(n(6)),
            "the library's binding is left alone"
        );
        assert_eq!(names(&namespace), count);

        let own = view.binding_location("own");
        assert!(view.share_binding("own", &library, "count"));
        assert_ne!(view.binding_location("own"), own);
        assert_eq!(
            view.binding_location("own"),
            library.binding_location("count")
        );
        assert_eq!(view.get("own"), Some(n(6)));
        assert_eq!(names(&namespace), count);

        // An unbound `set!` writes nothing.
        assert_eq!(view.set("unbound", n(8)), Err("unbound".to_string()));
        assert_eq!(names(&namespace), count);
    }
}
