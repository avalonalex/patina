//! Library types and structures for R7RS library system
//!
//! This module defines the core types for representing Scheme libraries.
//! The library loading and resolution logic is in the evaluator.

use crate::Trace;
use crate::binding_record::BindingCell;
use crate::environment::Environment;
use crate::heap::Heap;
use crate::tagged_value::TaggedValue;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

/// Represents a loaded Scheme library
///
/// A library encapsulates:
/// - A unique name (e.g., (scheme base) → ["scheme", "base"])
/// - Exported bindings (name → the cell each export is)
/// - Internal environment (for library-private definitions)
/// - Optional source location (for debugging)
///
/// Each export is the cell its binding named when the library was built,
/// resolved one hop to the owning cell (#834): a re-export is the cell of
/// the library that defined it. Build-time cells are not observable,
/// because no library namespace is written after its load (#823): a
/// definition there would re-point the library's record, not the export.
/// A `Library` clone carries untraced copies of the registry's cell words,
/// safe while the registry entry or an importer's record keeps the cell.
///
/// **The raw layer** (#831): the type's name is inside the embedding API's
/// promise, since a promised signature answers one
/// (`ApplyContext::load_scheme_library`), but its fields, methods and impls
/// are not (`docs/README.md`, "Changing the embedding API"). They change
/// without a deprecated twin; its exports are cells since #834.
///
/// Its trace (derived, #686) reports its two root sets (`docs/GC_DESIGN.md`
/// §5.3): each export's cell, and the library's environment. Pinned by the
/// sentinel test `gc_edge_tests::library_exports_and_environment`.
#[derive(Debug, Clone, Trace)]
pub struct Library {
    /// Library name as a list of strings
    /// Example: (scheme base) → vec!["scheme", "base"]
    #[trace(skip, reason = "the library's name")]
    pub name: Vec<String>,

    /// Exported bindings: identifier name → the cell it is. Only these
    /// bindings are visible when the library is imported, and an import is
    /// the cell, not a copy of what it holds (#406).
    pub exports: HashMap<String, BindingCell>,

    /// Library's internal environment
    /// Contains both exported and private bindings
    pub env: Rc<Environment>,

    /// Optional source file path (for error messages and debugging)
    #[trace(skip, reason = "a file path")]
    pub source: Option<PathBuf>,
}

impl Library {
    /// Create a library with an existing environment
    pub fn with_env(name: Vec<String>, env: Rc<Environment>) -> Self {
        Self {
            name,
            exports: HashMap::new(),
            env,
            source: None,
        }
    }

    /// Export `cell` as `name`: the binding a loader read off the library's
    /// environment (`Environment::cell_of`), under the library's name for it
    /// or a renamed one.
    pub fn export_cell(&mut self, name: String, cell: BindingCell) {
        self.exports.insert(name, cell);
    }

    /// Export `value` as `name` with no binding behind it: a free cell,
    /// which every importer shares and which no namespace counts among its
    /// cells. For a library assembled by hand, as the registry's tests do.
    pub fn export_value(&mut self, heap: &RefCell<Heap>, name: String, value: TaggedValue) {
        self.export_free(&mut heap.borrow_mut(), name, value);
    }

    /// [`export_value`](Self::export_value) with the heap in hand, for this
    /// crate's tests.
    pub(crate) fn export_free(&mut self, heap: &mut Heap, name: String, value: TaggedValue) {
        let cell = BindingCell::free(heap, value);
        self.exports.insert(name, cell);
    }

    /// Forget every export, for a caller about to rebuild the table.
    pub fn clear_exports(&mut self) {
        self.exports.clear();
    }

    /// Import `export` into `env` as `name`: the same binding the library
    /// has, its cell, so that what the library assigns afterwards the
    /// importer sees (R7RS §5.2; #406). `false` when the library has no
    /// such export.
    ///
    /// Every import in both backends comes through here, so that "what an
    /// import installs" is decided in one place, through the namespace view
    /// ([`NamespaceView::import_cell`](crate::NamespaceView::import_cell),
    /// an insert or a re-point).
    ///
    /// `heap` is the one the library's and `env`'s values live on, which a
    /// namespace's step borrows mutably to write `env`'s record (#834).
    pub fn import_into(
        &self,
        heap: &RefCell<Heap>,
        env: &Environment,
        name: impl Into<Rc<str>>,
        export: &str,
    ) -> bool {
        let Some(&cell) = self.exports.get(export) else {
            return false;
        };
        crate::NamespaceView::of(env).import_cell(heap, name, cell);
        true
    }

    /// Set the source file path
    pub fn set_source(&mut self, path: PathBuf) {
        self.source = Some(path);
    }

    /// Get library name as a string for display
    /// Example: ["scheme", "base"] → "(scheme base)"
    pub fn name_string(&self) -> String {
        format!("({})", self.name.join(" "))
    }

    /// Check if this library exports a given identifier
    pub fn exports_identifier(&self, name: &str) -> bool {
        self.exports.contains_key(name)
    }

    /// What the export `name` holds now: its cell's value, read under a
    /// shared borrow of `heap`.
    pub fn get_export_tagged(&self, heap: &RefCell<Heap>, name: &str) -> Option<TaggedValue> {
        let cell = *self.exports.get(name)?;
        Some(cell.value(&heap.borrow()))
    }

    /// Get all export names
    pub fn export_names(&self) -> Vec<&str> {
        self.exports.keys().map(|s| s.as_str()).collect()
    }

    /// Iterate over exports as (name, what its cell holds now) pairs, each
    /// read under a shared borrow of `heap` taken for that item alone.
    pub fn exports_iter_tagged<'a>(
        &'a self,
        heap: &'a RefCell<Heap>,
    ) -> impl Iterator<Item = (&'a String, TaggedValue)> + 'a {
        self.exports
            .iter()
            .map(move |(name, cell)| (name, cell.value(&heap.borrow())))
    }
}

impl std::fmt::Display for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#<library:{}>", self.name_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::{SharedHeap, new_shared_heap};

    /// An empty library named `name`, its namespace on `heap`.
    fn library(heap: &SharedHeap, name: &[&str]) -> Library {
        Library::with_env(
            name.iter().map(|part| part.to_string()).collect(),
            Rc::new(Environment::with_heap(heap)),
        )
    }

    #[test]
    fn test_library_creation() {
        let heap = new_shared_heap();
        let lib = library(&heap, &["scheme", "base"]);
        assert_eq!(lib.name, vec!["scheme", "base"]);
        assert_eq!(lib.name_string(), "(scheme base)");
        assert!(lib.exports.is_empty());
    }

    #[test]
    fn test_library_exports() {
        let heap = new_shared_heap();
        let mut lib = library(&heap, &["test"]);

        lib.export_value(&heap, "foo".to_string(), TaggedValue::fixnum(42));
        lib.export_value(&heap, "bar".to_string(), TaggedValue::TRUE);

        assert!(lib.exports_identifier("foo"));
        assert!(lib.exports_identifier("bar"));
        assert!(!lib.exports_identifier("baz"));

        let mut names = lib.export_names();
        names.sort();
        assert_eq!(names, vec!["bar", "foo"]);
    }

    #[test]
    fn an_import_is_the_librarys_binding_under_either_of_its_names() {
        let heap = new_shared_heap();
        let mut lib = library(&heap, &["counter"]);
        lib.env.define(&heap, "count", TaggedValue::fixnum(0));
        let cell = lib.env.cell_of(&heap, "count").unwrap();
        lib.export_cell("count".to_string(), cell);
        lib.export_cell("tally".to_string(), cell);

        let importer = Environment::with_heap(&heap);
        assert!(lib.import_into(&heap, &importer, "count", "count"));
        assert!(lib.import_into(&heap, &importer, "c:tally", "tally"));
        assert!(!lib.import_into(&heap, &importer, "nope", "nope"));

        // The library assigns; the importer sees it under both names, because
        // the renamed export is the cell the library's name names.
        lib.env.set(&heap, "count", TaggedValue::fixnum(2)).unwrap();
        assert_eq!(importer.get(&heap, "count"), Some(TaggedValue::fixnum(2)));
        assert_eq!(importer.get(&heap, "c:tally"), Some(TaggedValue::fixnum(2)));
        assert_eq!(importer.get(&heap, "nope"), None);
    }

    #[test]
    fn an_export_value_is_a_free_cell() {
        // A library assembled by hand, as the registry's own tests do: the
        // export is a free cell, which every importer shares and which no
        // namespace counts among its own cells.
        let heap = new_shared_heap();
        let mut lib = library(&heap, &["bare"]);
        let cells = heap.borrow().cells();
        lib.export_value(&heap, "v".to_string(), TaggedValue::fixnum(7));
        assert_eq!(heap.borrow().cells(), cells, "a free cell is no one's");
        let first = Environment::with_heap(&heap);
        let second = Environment::with_heap(&heap);
        assert!(lib.import_into(&heap, &first, "v", "v"));
        assert!(lib.import_into(&heap, &second, "v", "v"));
        assert_eq!(first.get(&heap, "v"), Some(TaggedValue::fixnum(7)));
        assert_eq!(
            first.binding_location(&heap, "v"),
            second.binding_location(&heap, "v")
        );
        first.set(&heap, "v", TaggedValue::fixnum(8)).unwrap();
        assert_eq!(second.get(&heap, "v"), Some(TaggedValue::fixnum(8)));
        assert_eq!(
            lib.get_export_tagged(&heap, "v"),
            Some(TaggedValue::fixnum(8))
        );
        assert_eq!(heap.borrow().cells(), cells);
    }

    #[test]
    fn test_library_display() {
        let heap = new_shared_heap();
        let lib = library(&heap, &["mylib", "utils"]);
        assert_eq!(lib.to_string(), "#<library:(mylib utils)>");
    }
}

/// A library's two root sets, each with a value nothing else reaches (#623):
/// an export and a binding of its environment. Built by a struct literal, so
/// a new field breaks this test as well as `for_each_gc_edge`, and rooted
/// through a `Library` heap object, so the object arm is covered too.
#[cfg(test)]
mod gc_edge_tests {
    use super::*;
    use crate::heap::new_shared_heap;
    use crate::heap::sentinels::Sentinels;
    use crate::heap::trace_sentinels::{collect_from, env_holding};

    #[test]
    fn library_exports_and_environment() {
        let heap = new_shared_heap();
        let mut h = heap.borrow_mut();
        let mut s = Sentinels::new(&mut h);
        let exported = s.pair(&mut h, "Library.exports");
        let private = s.vector(&mut h, "Library.env");
        drop(h);

        // A free cell the environment does not reach, so the export alone
        // keeps its value.
        let exported = BindingCell::free(&mut heap.borrow_mut(), exported);
        let library = Library {
            name: vec!["sentinel".to_string()],
            exports: HashMap::from([("exported".to_string(), exported)]),
            env: env_holding(&heap, private),
            source: None,
        };
        let mut h = heap.borrow_mut();
        let root = h.alloc_library(Rc::new(library));
        collect_from(&mut h, &[root]);
        s.assert_survived(&h);
    }
}
