//! Library types and structures for R7RS library system
//!
//! This module defines the core types for representing Scheme libraries.
//! The library loading and resolution logic is in the evaluator.

use crate::environment::Environment;
use crate::tagged_value::TaggedValue;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

/// Represents a loaded Scheme library
///
/// A library encapsulates:
/// - A unique name (e.g., (scheme base) → ["scheme", "base"])
/// - Exported bindings (name → TaggedValue mapping)
/// - Internal environment (for library-private definitions)
/// - Optional source location (for debugging)
///
/// Library exports are stored as `TaggedValue` for memory efficiency.
#[derive(Debug, Clone)]
pub struct Library {
    /// Library name as a list of strings
    /// Example: (scheme base) → vec!["scheme", "base"]
    pub name: Vec<String>,

    /// Exported bindings: identifier name → TaggedValue
    /// Only these bindings are visible when the library is imported
    /// Stored as TaggedValue for memory efficiency
    pub exports: HashMap<String, TaggedValue>,

    /// Library's internal environment
    /// Contains both exported and private bindings
    pub env: Rc<Environment>,

    /// For an export the library renamed — `(export (rename count tally))` —
    /// the name it has inside: `tally` → `count`. An export absent from here
    /// goes by the same name in `env`.
    ///
    /// `exports` records what each export *held* when the library was built,
    /// which cannot say where it lives. An import needs that: it is the
    /// library's binding, not a copy of its value (#406), and for a renamed
    /// export the location is under the other name.
    internal_names: HashMap<String, String>,

    /// Optional source file path (for error messages and debugging)
    pub source: Option<PathBuf>,
}

impl Library {
    /// Create a new empty library with the given name
    pub fn new(name: Vec<String>) -> Self {
        Self {
            name,
            exports: HashMap::new(),
            env: Rc::new(Environment::new()),
            internal_names: HashMap::new(),
            source: None,
        }
    }

    /// Create a library with an existing environment
    pub fn with_env(name: Vec<String>, env: Rc<Environment>) -> Self {
        Self {
            name,
            exports: HashMap::new(),
            env,
            internal_names: HashMap::new(),
            source: None,
        }
    }

    /// Add an exported binding
    pub fn export_tagged(&mut self, name: String, value: TaggedValue) {
        self.internal_names.remove(&name);
        self.exports.insert(name, value);
    }

    /// Forget every export, for a caller about to rebuild the table. `exports`
    /// is public and has been cleared directly, which would leave a renamed
    /// export's internal name behind to misdirect a later plain export of
    /// the same name.
    pub fn clear_exports(&mut self) {
        self.exports.clear();
        self.internal_names.clear();
    }

    /// Add an exported binding that the library knows by another name:
    /// `(export (rename internal external))`.
    pub fn export_renamed(&mut self, internal: String, external: String, value: TaggedValue) {
        self.exports.insert(external.clone(), value);
        self.internal_names.insert(external, internal);
    }

    /// Import `export` into `env` as `name`: the same binding the library
    /// has, so that what the library assigns afterwards the importer sees
    /// (R7RS §5.2; #406). `false` when the library has no such export.
    ///
    /// Every import in both backends comes through here, so that "what an
    /// import installs" is decided in one place. Falls back to the value the
    /// export held when the library was built, for an export that is not a
    /// plain binding in `env` — see `Environment::share_binding` for which
    /// those are.
    pub fn import_into(&self, env: &Environment, name: impl Into<Rc<str>>, export: &str) -> bool {
        let Some(&value) = self.exports.get(export) else {
            return false;
        };
        let name = name.into();
        let internal = self
            .internal_names
            .get(export)
            .map_or(export, String::as_str);
        if !env.share_binding(Rc::clone(&name), &self.env, internal) {
            env.define(name, value);
        }
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

    /// Get an exported value by name
    pub fn get_export_tagged(&self, name: &str) -> Option<TaggedValue> {
        self.exports.get(name).copied()
    }

    /// Get all export names
    pub fn export_names(&self) -> Vec<&str> {
        self.exports.keys().map(|s| s.as_str()).collect()
    }

    /// Iterate over exports as (name, TaggedValue) pairs
    pub fn exports_iter_tagged(&self) -> impl Iterator<Item = (&String, TaggedValue)> + '_ {
        self.exports.iter().map(|(k, tv)| (k, *tv))
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

    #[test]
    fn test_library_creation() {
        let lib = Library::new(vec!["scheme".to_string(), "base".to_string()]);
        assert_eq!(lib.name, vec!["scheme", "base"]);
        assert_eq!(lib.name_string(), "(scheme base)");
        assert!(lib.exports.is_empty());
    }

    #[test]
    fn test_library_exports() {
        let mut lib = Library::new(vec!["test".to_string()]);

        lib.export_tagged("foo".to_string(), TaggedValue::fixnum(42));
        lib.export_tagged("bar".to_string(), TaggedValue::TRUE);

        assert!(lib.exports_identifier("foo"));
        assert!(lib.exports_identifier("bar"));
        assert!(!lib.exports_identifier("baz"));

        let mut names = lib.export_names();
        names.sort();
        assert_eq!(names, vec!["bar", "foo"]);
    }

    #[test]
    fn an_import_is_the_librarys_binding_under_either_of_its_names() {
        let mut lib = Library::new(vec!["counter".to_string()]);
        lib.env.define("count", TaggedValue::fixnum(0));
        lib.export_tagged("count".to_string(), TaggedValue::fixnum(0));
        lib.export_renamed(
            "count".to_string(),
            "tally".to_string(),
            TaggedValue::fixnum(0),
        );

        let importer = Environment::with_heap(lib.env.heap().clone());
        assert!(lib.import_into(&importer, "count", "count"));
        assert!(lib.import_into(&importer, "c:tally", "tally"));
        assert!(!lib.import_into(&importer, "nope", "nope"));

        // The library assigns; the importer sees it under both names, because
        // the renamed export was found under the name the library uses.
        lib.env.set("count", TaggedValue::fixnum(2)).unwrap();
        assert_eq!(importer.get("count"), Some(TaggedValue::fixnum(2)));
        assert_eq!(importer.get("c:tally"), Some(TaggedValue::fixnum(2)));
        assert_eq!(importer.get("nope"), None);
    }

    #[test]
    fn an_export_with_no_binding_behind_it_is_imported_by_value() {
        // A library assembled by hand, as the registry's own tests do: the
        // export table is all there is, so its value is what gets installed.
        let mut lib = Library::new(vec!["bare".to_string()]);
        lib.export_tagged("v".to_string(), TaggedValue::fixnum(7));
        let importer = Environment::with_heap(lib.env.heap().clone());
        assert!(lib.import_into(&importer, "v", "v"));
        assert_eq!(importer.get("v"), Some(TaggedValue::fixnum(7)));
    }

    #[test]
    fn test_library_display() {
        let lib = Library::new(vec!["mylib".to_string(), "utils".to_string()]);
        assert_eq!(lib.to_string(), "#<library:(mylib utils)>");
    }
}
