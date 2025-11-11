//! Library Loading Infrastructure
//!
//! This module provides the infrastructure for loading different types of libraries:
//! - Pure Rust libraries: Primitives implemented in Rust and registered programmatically
//! - Pure Scheme libraries: Defined entirely in .sld files
//! - Mixed libraries: Rust primitives extended with Scheme code
//!
//! Design Philosophy:
//! - Keep performance-critical primitives in Rust (arithmetic, list operations, etc.)
//! - Use Scheme for derived functions and syntactic sugar
//! - Support both for maximum flexibility

use crate::library::Library;
use crate::library_registry::LibraryError;
use crate::Environment;
use std::path::PathBuf;
use std::rc::Rc;

/// Trait for different library loading strategies
///
/// This allows us to support multiple library types:
/// - RustLibraryLoader: Loads libraries implemented as Rust code
/// - SchemeLibraryLoader: Parses and evaluates .sld files
/// - MixedLibraryLoader: Combines both approaches
pub trait LibraryLoader {
    /// Load a library with the given name
    ///
    /// Returns the loaded Library or an error if loading fails.
    /// The loader is responsible for:
    /// - Finding the library source (file, built-in, etc.)
    /// - Creating the library environment
    /// - Populating exports
    /// - Handling dependencies (imports)
    fn load(&self, name: &[String], search_paths: &[PathBuf]) -> Result<Library, LibraryError>;

    /// Check if this loader can handle the given library name
    ///
    /// Returns true if this loader knows how to load this library.
    /// This allows for fallback chains: try Rust first, then Scheme.
    fn can_load(&self, name: &[String]) -> bool;
}

/// Registry for library loaders
///
/// Manages multiple loaders with a priority order:
/// 1. Built-in Rust libraries (fastest, highest priority)
/// 2. .sld files (standard R7RS libraries)
/// 3. Custom loaders (extensions)
pub struct LibraryLoaderRegistry {
    loaders: Vec<Box<dyn LibraryLoader>>,
}

impl LibraryLoaderRegistry {
    /// Create a new loader registry
    pub fn new() -> Self {
        Self {
            loaders: Vec::new(),
        }
    }

    /// Add a loader to the registry
    ///
    /// Loaders are tried in the order they are added.
    /// Add highest-priority loaders first.
    pub fn add_loader(&mut self, loader: Box<dyn LibraryLoader>) {
        self.loaders.push(loader);
    }

    /// Load a library using the first matching loader
    ///
    /// Tries each loader in order until one succeeds.
    /// Returns NotFound if no loader can handle the library.
    pub fn load(&self, name: &[String], search_paths: &[PathBuf]) -> Result<Library, LibraryError> {
        for loader in &self.loaders {
            if loader.can_load(name) {
                match loader.load(name, search_paths) {
                    Ok(lib) => return Ok(lib),
                    Err(e) => return Err(e),
                }
            }
        }

        Err(LibraryError::NotFound(name.to_vec()))
    }
}

impl Default for LibraryLoaderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for a Rust library builder function
///
/// A builder takes a library name and environment, populates the environment
/// with primitives and definitions, and returns the list of exported identifiers.
///
/// Example:
/// ```rust,ignore
/// fn build_scheme_base(name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
///     // Register primitives
///     env.define("+", Value::Primitive(...));
///     env.define("-", Value::Primitive(...));
///
///     // Return exports
///     vec!["+".to_string(), "-".to_string(), ...]
/// }
/// ```
pub type RustLibraryBuilder = fn(Vec<String>, Rc<Environment>) -> Vec<String>;

#[cfg(test)]
mod tests {
    use super::*;

    // Mock loader for testing
    struct MockLoader {
        can_handle: Vec<Vec<String>>,
    }

    impl LibraryLoader for MockLoader {
        fn load(
            &self,
            name: &[String],
            _search_paths: &[PathBuf],
        ) -> Result<Library, LibraryError> {
            if self.can_load(name) {
                Ok(Library::new(name.to_vec()))
            } else {
                Err(LibraryError::NotFound(name.to_vec()))
            }
        }

        fn can_load(&self, name: &[String]) -> bool {
            self.can_handle.iter().any(|n| n == name)
        }
    }

    #[test]
    fn test_registry_empty() {
        let registry = LibraryLoaderRegistry::new();
        let result = registry.load(&["test".to_string()], &[]);
        assert!(matches!(result, Err(LibraryError::NotFound(_))));
    }

    #[test]
    fn test_registry_finds_loader() {
        let mut registry = LibraryLoaderRegistry::new();
        let loader = MockLoader {
            can_handle: vec![vec!["test".to_string()]],
        };
        registry.add_loader(Box::new(loader));

        let result = registry.load(&["test".to_string()], &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_registry_priority_order() {
        let mut registry = LibraryLoaderRegistry::new();

        // First loader handles "foo"
        let loader1 = MockLoader {
            can_handle: vec![vec!["foo".to_string()]],
        };

        // Second loader handles "bar"
        let loader2 = MockLoader {
            can_handle: vec![vec!["bar".to_string()]],
        };

        registry.add_loader(Box::new(loader1));
        registry.add_loader(Box::new(loader2));

        // Should find foo with first loader
        assert!(registry.load(&["foo".to_string()], &[]).is_ok());

        // Should find bar with second loader
        assert!(registry.load(&["bar".to_string()], &[]).is_ok());

        // Should not find baz
        assert!(registry.load(&["baz".to_string()], &[]).is_err());
    }
}
