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
            source: None,
        }
    }

    /// Create a library with an existing environment
    pub fn with_env(name: Vec<String>, env: Rc<Environment>) -> Self {
        Self {
            name,
            exports: HashMap::new(),
            env,
            source: None,
        }
    }

    /// Add an exported binding
    pub fn export_tagged(&mut self, name: String, value: TaggedValue) {
        self.exports.insert(name, value);
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
    fn test_library_display() {
        let lib = Library::new(vec!["mylib".to_string(), "utils".to_string()]);
        assert_eq!(lib.to_string(), "#<library:(mylib utils)>");
    }
}
