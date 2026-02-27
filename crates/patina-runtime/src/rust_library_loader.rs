//! Rust Library Loader
//!
//! Loads libraries implemented in Rust code.
//! This is used for performance-critical standard libraries like (scheme base).

use crate::Environment;
use crate::library::Library;
use crate::library_loader::{LibraryLoader, RustLibraryBuilder};
use crate::library_registry::LibraryError;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

/// Loader for libraries implemented in Rust
///
/// This loader maintains a registry of library builders - functions that
/// populate a library's environment with Rust-implemented primitives.
///
/// Libraries are registered at compile time and loaded on demand.
pub struct RustLibraryLoader {
    /// Map from library name to builder function
    builders: HashMap<Vec<String>, RustLibraryBuilder>,
}

impl RustLibraryLoader {
    /// Create a new Rust library loader
    pub fn new() -> Self {
        Self {
            builders: HashMap::new(),
        }
    }

    /// Register a library builder
    ///
    /// The builder function will be called when the library is loaded.
    /// It should populate the environment and return a list of exported identifiers.
    pub fn register(&mut self, name: Vec<String>, builder: RustLibraryBuilder) {
        self.builders.insert(name, builder);
    }

    /// Create a loader with standard libraries pre-registered
    ///
    /// This includes:
    /// - (scheme base) - Core R7RS library
    /// - Additional standard libraries as they are implemented
    pub fn with_standard_libraries() -> Self {
        // Note: Standard libraries are registered by the backend (tree-walker, VM, etc.)
        // when initializing the evaluator. This keeps the runtime crate backend-agnostic.

        Self::new()
    }
}

impl Default for RustLibraryLoader {
    fn default() -> Self {
        Self::with_standard_libraries()
    }
}

impl LibraryLoader for RustLibraryLoader {
    fn load(&self, name: &[String], _search_paths: &[PathBuf]) -> Result<Library, LibraryError> {
        // Look up the builder for this library
        let builder = self
            .builders
            .get(name)
            .ok_or_else(|| LibraryError::NotFound(name.to_vec()))?;

        // Create a new environment for the library
        let env = Rc::new(Environment::new());

        // Call the builder to populate the environment
        let exports = builder(name.to_vec(), env.clone());

        // Create the library
        let mut lib = Library::with_env(name.to_vec(), env);

        // Populate exports
        for export_name in exports {
            if let Some(value) = lib.env.get(&export_name) {
                lib.export_tagged(export_name, value);
            }
        }

        Ok(lib)
    }

    fn load_with_heap(
        &self,
        name: &[String],
        _search_paths: &[PathBuf],
        heap: crate::heap::SharedHeap,
    ) -> Result<Library, LibraryError> {
        // Look up the builder for this library
        let builder = self
            .builders
            .get(name)
            .ok_or_else(|| LibraryError::NotFound(name.to_vec()))?;

        // Create a new environment that shares the provided heap
        // This ensures TaggedValues are allocated on the global heap
        let env = Rc::new(Environment::with_heap(heap));

        // Call the builder to populate the environment
        let exports = builder(name.to_vec(), env.clone());

        // Create the library
        let mut lib = Library::with_env(name.to_vec(), env);

        // Populate exports
        for export_name in exports {
            if let Some(value) = lib.env.get(&export_name) {
                lib.export_tagged(export_name, value);
            }
        }

        Ok(lib)
    }

    fn can_load(&self, name: &[String]) -> bool {
        self.builders.contains_key(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_core::TaggedValue;

    // Test builder function
    fn build_test_lib(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
        let tv_internal = env.heap().borrow_mut().alloc_string("private".to_string());

        env.define("foo".to_string(), TaggedValue::fixnum(42));
        env.define("bar".to_string(), TaggedValue::TRUE);
        env.define("internal".to_string(), tv_internal);

        // Only export foo and bar
        vec!["foo".to_string(), "bar".to_string()]
    }

    #[test]
    fn test_rust_loader_basic() {
        let mut loader = RustLibraryLoader::new();
        loader.register(vec!["test".to_string()], build_test_lib);

        let lib = loader.load(&["test".to_string()], &[]).unwrap();

        assert_eq!(lib.name, vec!["test"]);
        assert!(lib.exports_identifier("foo"));
        assert!(lib.exports_identifier("bar"));
        assert!(!lib.exports_identifier("internal")); // Not exported

        // Check export values
        assert_eq!(lib.get_export_tagged("foo"), Some(TaggedValue::fixnum(42)));
        assert_eq!(lib.get_export_tagged("bar"), Some(TaggedValue::TRUE));

        // Internal value should be in environment but not exported
        assert!(lib.env.get("internal").is_some());
    }

    #[test]
    fn test_rust_loader_not_found() {
        let loader = RustLibraryLoader::new();
        let result = loader.load(&["nonexistent".to_string()], &[]);
        assert!(matches!(result, Err(LibraryError::NotFound(_))));
    }

    #[test]
    fn test_can_load() {
        let mut loader = RustLibraryLoader::new();
        loader.register(vec!["test".to_string()], build_test_lib);

        assert!(loader.can_load(&["test".to_string()]));
        assert!(!loader.can_load(&["other".to_string()]));
    }

    #[test]
    fn test_multipart_name() {
        let mut loader = RustLibraryLoader::new();
        let name = vec!["scheme".to_string(), "base".to_string()];
        loader.register(name.clone(), build_test_lib);

        assert!(loader.can_load(&name));
        let lib = loader.load(&name, &[]).unwrap();
        assert_eq!(lib.name, name);
    }
}
