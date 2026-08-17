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

use crate::Environment;
use crate::heap::SharedHeap;
use crate::library::Library;
use crate::library_registry::LibraryError;
use patina_core::TaggedValue;
use patina_core::core_syntax::{ALL_CORE_FORMS, CoreForm};
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

    /// Load a library with a shared heap for TaggedValue compatibility
    ///
    /// When provided, the library environment should use this heap so that
    /// TaggedValues are allocated on the same heap as the global environment.
    /// This prevents heap index mismatches when importing library exports.
    ///
    /// Default implementation ignores the heap and calls load().
    fn load_with_heap(
        &self,
        name: &[String],
        search_paths: &[PathBuf],
        _heap: SharedHeap,
    ) -> Result<Library, LibraryError> {
        self.load(name, search_paths)
    }

    /// Check if this loader can handle the given library name
    ///
    /// Returns true if this loader knows how to load this library.
    /// This allows for fallback chains: try Rust first, then Scheme.
    fn can_load(&self, name: &[String]) -> bool;
}

/// Export specification for library declarations
///
/// Describes how identifiers are exported from a library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportSpec {
    /// Simple export: identifier
    Identifier(String),

    /// Renamed export: (rename internal-name external-name)
    Rename { internal: String, external: String },
}

/// Is `name` the spelling of a syntactic keyword?
///
/// Keywords are ordinary bindings now, so this asks about *spelling* only — a
/// question with exactly one honest use: how lenient to be about a keyword that
/// a library never imported, and therefore has no binding for. Both callers
/// ([`build_library`] below, and each backend's `(only …)`) exist for that case,
/// and stage 2 removes them along with the desugarer's spelling fallback.
///
/// Derived from [`CoreForm`] rather than listed. Two hand-kept `&[&str]`s used
/// to restate the same table a crate away, and had already drifted: they omitted
/// `else` and `=>`, so `(export begin)` was lenient where `(export else)` was
/// not — two spellings of one rule answering differently.
pub fn is_core_syntax(name: &str) -> bool {
    CoreForm::from_name(name).is_some()
}

/// Every name [`is_core_syntax`] accepts.
pub fn core_syntax_names() -> impl Iterator<Item = &'static str> {
    ALL_CORE_FORMS.iter().map(|form| form.name())
}

/// Assemble the library a parsed `define-library` describes, resolving its
/// `export` declarations against the environment its body produced.
///
/// Shared by both backends, which evaluate library bodies independently but
/// must agree on what a valid export is.
pub fn build_library(
    parsed: ParsedLibrary,
    lib_env: Rc<Environment>,
) -> Result<Library, LibraryError> {
    let mut library = Library::with_env(parsed.name, lib_env);
    if let Some(source) = parsed.source {
        library.set_source(source);
    }
    let file = library.source.clone();
    let reject = |name: &str, detail: &str| {
        LibraryError::parse(
            file.as_deref(),
            format!("Exported identifier '{}' {}", name, detail),
        )
    };

    for spec in &parsed.exports {
        match spec {
            ExportSpec::Identifier(name) => match library.env.get(name) {
                Some(value) => library.export_tagged(name.clone(), value),
                // A syntactic keyword this library never imported. It has no
                // marker to carry, but the desugarer still recognizes it by
                // spelling where nothing is bound, so the importer gets working
                // syntax and there is nothing to report. Stage 2 removes that
                // fallback, and with it this arm.
                None if is_core_syntax(name) => {}
                None => return Err(reject(name, "not defined")),
            },
            ExportSpec::Rename { internal, external } => match library.env.get(internal) {
                // Renaming a keyword works like renaming anything else now: the
                // marker goes out under the new name and the importer's
                // desugarer dispatches on the form, not the spelling.
                Some(value) => library.export_tagged(external.clone(), value),
                // Except when the library never imported it, where there is no
                // marker to send and the use site's fallback only knows the
                // original spelling. Refused rather than exported as a name
                // that would not work.
                //
                // The message leads with the rule that actually applies — you
                // cannot export what you did not import — because renaming a
                // keyword on export *does* work, and a message reading "cannot
                // rename core syntax" would send the reader after a limitation
                // that no longer exists. It still avoids opening with
                // "Exported identifier", the prefix `patina-compat`'s
                // classifier keys on for a package's missing export.
                None if is_core_syntax(internal) => {
                    return Err(LibraryError::parse(
                        file.as_deref(),
                        format!(
                            "Cannot export '{}' under a new name: it is a syntactic keyword this \
                             library does not import",
                            internal
                        ),
                    ));
                }
                None => return Err(reject(internal, "not defined")),
            },
        }
    }
    Ok(library)
}

/// Import set (R7RS 5.6.1)
///
/// Describes how to import identifiers from other libraries,
/// with optional filtering and renaming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportSet {
    /// Direct library import: (scheme base)
    Library(Vec<String>),

    /// Only specific identifiers: (only <import-set> id1 id2 ...)
    Only {
        import_set: Box<ImportSet>,
        identifiers: Vec<String>,
    },

    /// Exclude specific identifiers: (except <import-set> id1 id2 ...)
    Except {
        import_set: Box<ImportSet>,
        identifiers: Vec<String>,
    },

    /// Add prefix to all imports: (prefix <import-set> prefix)
    Prefix {
        import_set: Box<ImportSet>,
        prefix: String,
    },

    /// Rename imports: (rename <import-set> (old1 new1) (old2 new2) ...)
    Rename {
        import_set: Box<ImportSet>,
        renames: Vec<(String, String)>,
    },
}

/// Parsed library ready for evaluation
///
/// This struct holds the parsed components of a .sld library file
/// before evaluation. It allows separating parsing from evaluation,
/// which eliminates the need for circular references between loaders
/// and evaluators.
/// # Garbage collection
///
/// `body` holds unevaluated forms as `TaggedValue`s in a plain Rust struct,
/// which no GC root provider can see. This value therefore **defers
/// collection for as long as it exists**: it carries a `GcDeferGuard`,
/// installed by [`ParsedLibrary::new`] and released when it drops.
///
/// That is deliberately a property of the data rather than of the loaders.
/// Three separate code paths evaluate a `ParsedLibrary` (the VM backend, the
/// VM state, and the tree-walker), each a near-copy of the others; when the
/// guard was placed at the call sites instead, one path was missed and
/// bootstrap failed with a use-after-free the moment collection was enabled.
/// Attaching it here makes a fourth path safe by construction — and is also
/// correct for a `ParsedLibrary` held longer than one loading call, which a
/// call-site guard would get wrong.
pub struct ParsedLibrary {
    /// Library name
    pub name: Vec<String>,

    /// Import specifications (will need to be resolved)
    pub imports: Vec<ImportSet>,

    /// Library body expressions (need evaluation) as TaggedValues
    pub body: Vec<TaggedValue>,

    /// The heap containing the body TaggedValues
    pub heap: Option<SharedHeap>,

    /// Export specifications
    pub exports: Vec<ExportSpec>,

    /// Source file path (for error reporting)
    pub source: Option<PathBuf>,

    /// Defers collection while the unevaluated `body` forms are unrooted.
    /// Private so the only way to build a `ParsedLibrary` is [`Self::new`],
    /// which cannot forget it.
    _gc_defer: Option<patina_core::GcDeferGuard>,
}

impl ParsedLibrary {
    /// Build a parsed library, installing the GC defer guard described above.
    pub fn new(
        name: Vec<String>,
        imports: Vec<ImportSet>,
        body: Vec<TaggedValue>,
        heap: Option<SharedHeap>,
        exports: Vec<ExportSpec>,
        source: Option<PathBuf>,
    ) -> Self {
        let _gc_defer = heap.as_ref().map(patina_core::GcDeferGuard::new);
        Self {
            name,
            imports,
            body,
            heap,
            exports,
            source,
            _gc_defer,
        }
    }
}

impl std::fmt::Debug for ParsedLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParsedLibrary")
            .field("name", &self.name)
            .field("imports", &self.imports)
            .field("body", &self.body)
            .field("exports", &self.exports)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

/// Trait for loaders that need evaluation support
///
/// Some library loaders (like SchemeLibraryLoader) parse library files
/// but need the evaluator's help to:
/// - Resolve imports (which may require loading other libraries)
/// - Evaluate library body expressions
/// - Collect exports from the evaluated environment
///
/// This trait separates parsing from evaluation, eliminating the need
/// for unsafe circular references.
pub trait EvaluatingLibraryLoader {
    /// Parse a library definition without evaluating it
    ///
    /// This method:
    /// - Finds the library source (e.g., .sld file)
    /// - Parses it into structured form
    /// - Returns ParsedLibrary for the evaluator to process
    ///
    /// The evaluator will then handle imports and evaluation.
    fn parse(
        &self,
        name: &[String],
        search_paths: &[PathBuf],
    ) -> Result<ParsedLibrary, LibraryError>;

    /// Check if this loader can handle the given library name
    fn can_load(&self, name: &[String]) -> bool;

    /// Check if this loader can handle the given library name with search paths.
    ///
    /// This allows for accurate checking of file-based libraries (e.g., .sld files).
    /// The default implementation just calls `can_load`.
    fn can_load_with_paths(&self, name: &[String], _search_paths: &[PathBuf]) -> bool {
        self.can_load(name)
    }

    /// Parse a library with a library availability checker.
    ///
    /// This is used for `(library <name>)` requirements in cond-expand.
    /// The default implementation ignores the checker and calls `parse()`.
    ///
    /// Implementations should override this to support library checks in cond-expand.
    fn parse_with_library_checker(
        &self,
        name: &[String],
        search_paths: &[PathBuf],
        _can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<ParsedLibrary, LibraryError> {
        // Default: ignore the checker
        self.parse(name, search_paths)
    }

    /// Parse a library with a shared heap and library availability checker.
    ///
    /// When provided, the parser uses the shared heap so that TaggedValues
    /// in the body are allocated on the same heap as the global environment.
    /// This eliminates the need for cross-heap conversion when evaluating.
    ///
    /// Default implementation ignores the heap and calls `parse_with_library_checker()`.
    fn parse_with_heap_and_library_checker(
        &self,
        name: &[String],
        search_paths: &[PathBuf],
        _heap: SharedHeap,
        can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<ParsedLibrary, LibraryError> {
        self.parse_with_library_checker(name, search_paths, can_load_library)
    }
}

/// Registry for library loaders
///
/// Manages multiple loaders with a priority order:
/// 1. Built-in Rust libraries (fastest, highest priority)
/// 2. .sld files (standard R7RS libraries)
/// 3. Custom loaders (extensions)
///
/// Supports two types of loaders:
/// - Simple loaders (LibraryLoader): Can load libraries completely on their own
/// - Evaluating loaders (EvaluatingLibraryLoader): Parse libraries but need evaluation support
pub struct LibraryLoaderRegistry {
    /// Loaders that can load libraries completely (e.g., RustLibraryLoader)
    simple_loaders: Vec<Box<dyn LibraryLoader>>,

    /// Loaders that parse but need evaluation (e.g., SchemeLibraryLoader)
    evaluating_loaders: Vec<Box<dyn EvaluatingLibraryLoader>>,
}

impl LibraryLoaderRegistry {
    /// Create a new loader registry
    pub fn new() -> Self {
        Self {
            simple_loaders: Vec::new(),
            evaluating_loaders: Vec::new(),
        }
    }

    /// Add a simple loader to the registry
    ///
    /// Simple loaders can load libraries completely without external help.
    /// Add highest-priority loaders first.
    pub fn add_loader(&mut self, loader: Box<dyn LibraryLoader>) {
        self.simple_loaders.push(loader);
    }

    /// Add an evaluating loader to the registry
    ///
    /// Evaluating loaders parse libraries but need the evaluator's help.
    /// Add highest-priority loaders first.
    pub fn add_evaluating_loader(&mut self, loader: Box<dyn EvaluatingLibraryLoader>) {
        self.evaluating_loaders.push(loader);
    }

    /// Try to load a library using simple loaders
    ///
    /// Returns Ok(Some(lib)) if a loader successfully loaded the library.
    /// Returns Ok(None) if no loader can handle this library.
    /// Returns Err if a loader tried but failed.
    pub fn try_simple_load(
        &self,
        name: &[String],
        search_paths: &[PathBuf],
    ) -> Result<Option<Library>, LibraryError> {
        for loader in &self.simple_loaders {
            if loader.can_load(name) {
                return loader.load(name, search_paths).map(Some);
            }
        }
        Ok(None)
    }

    /// Try to load a library using simple loaders with a shared heap
    ///
    /// Like try_simple_load, but passes a heap to loaders for TaggedValue compatibility.
    /// This ensures that library environments use the same heap as the global environment,
    /// preventing heap index mismatches when importing exports.
    pub fn try_simple_load_with_heap(
        &self,
        name: &[String],
        search_paths: &[PathBuf],
        heap: SharedHeap,
    ) -> Result<Option<Library>, LibraryError> {
        for loader in &self.simple_loaders {
            if loader.can_load(name) {
                return loader.load_with_heap(name, search_paths, heap).map(Some);
            }
        }
        Ok(None)
    }

    /// Try to parse a library using evaluating loaders
    ///
    /// Returns Ok(Some(parsed)) if a loader can parse this library.
    /// Returns Ok(None) if no loader can handle this library.
    /// Returns Err if a loader tried but failed.
    pub fn try_parse(
        &self,
        name: &[String],
        search_paths: &[PathBuf],
    ) -> Result<Option<ParsedLibrary>, LibraryError> {
        self.try_parse_with_library_checker(name, search_paths, &|_| false)
    }

    /// Try to parse a library with a library availability checker.
    ///
    /// The checker is used for `(library <name>)` requirements in cond-expand.
    ///
    /// Returns Ok(Some(parsed)) if a loader can parse this library.
    /// Returns Ok(None) if no loader can handle this library.
    /// Returns Err if a loader tried but failed.
    pub fn try_parse_with_library_checker(
        &self,
        name: &[String],
        search_paths: &[PathBuf],
        can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<Option<ParsedLibrary>, LibraryError> {
        for loader in &self.evaluating_loaders {
            if loader.can_load(name) {
                return loader
                    .parse_with_library_checker(name, search_paths, can_load_library)
                    .map(Some);
            }
        }
        Ok(None)
    }

    /// Load a library using the first matching loader (legacy method)
    ///
    /// This method only tries simple loaders and is kept for backward compatibility.
    /// New code should use try_simple_load() and try_parse() separately.
    ///
    /// Tries each loader in order until one succeeds.
    /// Returns NotFound if no loader can handle the library.
    pub fn load(&self, name: &[String], search_paths: &[PathBuf]) -> Result<Library, LibraryError> {
        for loader in &self.simple_loaders {
            if loader.can_load(name) {
                return loader.load(name, search_paths);
            }
        }

        Err(LibraryError::NotFound(name.to_vec()))
    }

    /// Check if any loader can potentially load the given library.
    ///
    /// This is used for `(library <name>)` requirements in cond-expand.
    /// Returns true if any simple or evaluating loader claims to handle this library.
    ///
    /// Note: This method doesn't have access to search paths, so it may return
    /// true for libraries that can't actually be loaded. Use `can_load_with_paths`
    /// for accurate checking.
    pub fn can_load(&self, name: &[String]) -> bool {
        // Check simple loaders first (Rust libraries know their own names)
        for loader in &self.simple_loaders {
            if loader.can_load(name) {
                return true;
            }
        }

        // For evaluating loaders without search paths, we can't accurately check
        // Return false since we can't verify file existence
        false
    }

    /// Try to parse a library with a shared heap and library availability checker.
    ///
    /// Like try_parse_with_library_checker, but passes a heap so that body
    /// TaggedValues are allocated on the global heap.
    pub fn try_parse_with_heap_and_library_checker(
        &self,
        name: &[String],
        search_paths: &[PathBuf],
        heap: SharedHeap,
        can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<Option<ParsedLibrary>, LibraryError> {
        for loader in &self.evaluating_loaders {
            if loader.can_load(name) {
                return loader
                    .parse_with_heap_and_library_checker(name, search_paths, heap, can_load_library)
                    .map(Some);
            }
        }
        Ok(None)
    }

    /// Check if any loader can load the given library with search paths.
    ///
    /// This is used for `(library <name>)` requirements in cond-expand.
    /// More accurate than `can_load` as it can check file existence.
    pub fn can_load_with_paths(&self, name: &[String], search_paths: &[PathBuf]) -> bool {
        // Check simple loaders first (they don't need search paths)
        for loader in &self.simple_loaders {
            if loader.can_load(name) {
                return true;
            }
        }

        // Check evaluating loaders with search paths
        for loader in &self.evaluating_loaders {
            if loader.can_load_with_paths(name, search_paths) {
                return true;
            }
        }

        false
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
        assert!(
            registry
                .try_simple_load(&["foo".to_string()], &[])
                .unwrap()
                .is_some()
        );

        // Should find bar with second loader
        assert!(
            registry
                .try_simple_load(&["bar".to_string()], &[])
                .unwrap()
                .is_some()
        );

        // Should not find baz
        assert!(
            registry
                .try_simple_load(&["baz".to_string()], &[])
                .unwrap()
                .is_none()
        );
    }
}
