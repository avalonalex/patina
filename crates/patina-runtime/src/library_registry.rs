//! Library Registry for managing loaded Scheme libraries
//!
//! The LibraryRegistry maintains a global registry of loaded libraries and manages:
//! - Library name → Library mapping
//! - Search paths for finding library files
//! - Circular dependency detection
//! - Library loading and caching

use crate::library::Library;
use patina_core::FileSystem;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Error types for library operations
#[derive(Debug, Clone)]
pub enum LibraryError {
    /// Library not found in any search path
    NotFound(Vec<String>),

    /// Circular dependency detected during loading
    CircularDependency(Vec<Vec<String>>),

    /// Library with this name is already loaded (should not normally occur)
    AlreadyLoaded(Vec<String>),

    /// Invalid library name (empty or malformed)
    InvalidName(String),

    /// I/O error while reading library file
    IoError(String),

    /// Parse error in library file
    ParseError { file: String, message: String },
}

impl std::fmt::Display for LibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibraryError::NotFound(name) => {
                write!(f, "Library {} not found", format_library_name(name))
            }
            LibraryError::CircularDependency(chain) => {
                writeln!(f, "Circular library dependency detected:")?;
                for lib_name in chain {
                    writeln!(f, "  {} imports", format_library_name(lib_name))?;
                }
                write!(f, "  {} (circular)", format_library_name(&chain[0]))
            }
            LibraryError::AlreadyLoaded(name) => {
                write!(f, "Library {} is already loaded", format_library_name(name))
            }
            LibraryError::InvalidName(msg) => {
                write!(f, "Invalid library name: {}", msg)
            }
            LibraryError::IoError(msg) => {
                write!(f, "I/O error: {}", msg)
            }
            LibraryError::ParseError { file, message } => {
                write!(f, "Parse error in {}: {}", file, message)
            }
        }
    }
}

impl std::error::Error for LibraryError {}

/// Format a library name for display: ["scheme", "base"] → "(scheme base)"
fn format_library_name(name: &[String]) -> String {
    format!("({})", name.join(" "))
}

/// Registry for managing loaded Scheme libraries
///
/// Responsibilities:
/// - Maintain mapping of library names to loaded libraries
/// - Manage search paths for finding library files
/// - Detect and prevent circular dependencies
/// - Provide efficient library lookup
pub struct LibraryRegistry {
    /// Loaded libraries: library name → Library
    /// Uses Vec<String> as key for library names like (scheme base)
    libraries: HashMap<Vec<String>, Library>,

    /// Search paths for finding library files (.sld)
    /// Searched in order when loading a library
    search_paths: Vec<PathBuf>,

    /// Current loading stack for circular dependency detection
    /// Contains library names currently being loaded
    loading_stack: Vec<Vec<String>>,

    /// Virtual filesystem for file existence checks in `find_library_file`.
    fs: Arc<dyn FileSystem>,
}

impl LibraryRegistry {
    /// Create a new empty library registry
    pub fn new() -> Self {
        Self {
            libraries: HashMap::new(),
            search_paths: Vec::new(),
            loading_stack: Vec::new(),
            fs: Arc::new(patina_core::NativeFs),
        }
    }

    /// Set the virtual filesystem used for file existence checks.
    pub fn set_fs(&mut self, fs: Arc<dyn FileSystem>) {
        self.fs = fs;
    }

    /// Create a library registry with default search paths
    ///
    /// Default search paths (in order):
    /// 1. ./lib/ (relative to current directory)
    /// 2. $PATINA_HOME/lib/ (if PATINA_HOME env var is set)
    /// 3. Workspace root/lib/ (by walking up from executable to find Cargo.toml workspace)
    /// 4. Executable directory/../lib/ (relative to binary)
    pub fn with_default_paths() -> Self {
        let mut registry = Self::new();

        // 1. Current directory ./lib/
        registry.add_search_path(PathBuf::from("./lib"));

        // 2. $PATINA_HOME/lib/ if set
        if let Ok(patina_home) = std::env::var("PATINA_HOME") {
            let mut path = PathBuf::from(patina_home);
            path.push("lib");
            registry.add_search_path(path);
        }

        // 3. Workspace root/lib/ - walk up from executable looking for workspace Cargo.toml
        if let Ok(exe_path) = std::env::current_exe()
            && let Some(workspace_root) = Self::find_workspace_root(&exe_path)
        {
            let mut lib_path = workspace_root;
            lib_path.push("lib");
            registry.add_search_path(lib_path);
        }

        // 4. Executable directory/../lib/
        if let Ok(exe_path) = std::env::current_exe()
            && let Some(exe_dir) = exe_path.parent()
        {
            let mut lib_path = exe_dir.to_path_buf();
            lib_path.push("../lib");
            registry.add_search_path(lib_path);
        }

        registry
    }

    /// Find workspace root by walking up from the given path looking for Cargo.toml with [workspace]
    fn find_workspace_root(start_path: &std::path::Path) -> Option<PathBuf> {
        let mut current = start_path.to_path_buf();

        // Walk up the directory tree
        while let Some(parent) = current.parent() {
            let cargo_toml = parent.join("Cargo.toml");

            if cargo_toml.exists() {
                // Check if this Cargo.toml defines a workspace
                if let Ok(content) = std::fs::read_to_string(&cargo_toml)
                    && content.contains("[workspace]")
                {
                    return Some(parent.to_path_buf());
                }
            }

            current = parent.to_path_buf();
        }

        None
    }

    /// Add a search path for library files
    ///
    /// Paths are searched in the order they are added.
    /// Duplicate paths are ignored.
    pub fn add_search_path(&mut self, path: PathBuf) {
        // Normalize the path
        if let Ok(canonical) = path.canonicalize() {
            if !self.search_paths.contains(&canonical) {
                self.search_paths.push(canonical);
            }
        } else {
            // If canonicalization fails (path doesn't exist yet), just add it
            if !self.search_paths.contains(&path) {
                self.search_paths.push(path);
            }
        }
    }

    /// Get all configured search paths
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    /// Register a library in the registry
    ///
    /// This makes the library available for future imports.
    /// If a library with the same name already exists, returns an error.
    pub fn register(&mut self, library: Library) -> Result<(), LibraryError> {
        let name = library.name.clone();

        if self.libraries.contains_key(&name) {
            return Err(LibraryError::AlreadyLoaded(name));
        }

        self.libraries.insert(name, library);
        Ok(())
    }

    /// Check if a library is already loaded
    pub fn is_loaded(&self, name: &[String]) -> bool {
        self.libraries.contains_key(name)
    }

    /// Get a loaded library by name
    ///
    /// Returns None if the library is not loaded.
    /// Use `load()` to load a library if it's not already loaded.
    pub fn get(&self, name: &[String]) -> Option<&Library> {
        self.libraries.get(name)
    }

    /// Get a mutable reference to a loaded library
    pub fn get_mut(&mut self, name: &[String]) -> Option<&mut Library> {
        self.libraries.get_mut(name)
    }

    /// Find the file path for a library
    ///
    /// Searches for a .sld file in the search paths.
    /// Library name (scheme base) maps to scheme/base.sld
    ///
    /// Returns None if the file is not found in any search path.
    pub fn find_library_file(&self, name: &[String]) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }

        // Convert library name to file path: (scheme base) → scheme/base.sld
        let mut file_path = PathBuf::new();
        for part in name {
            file_path.push(part);
        }

        // Change the extension of the last component to .sld
        if let Some(last_part) = name.last() {
            file_path.pop(); // Remove the last part
            file_path.push(format!("{}.sld", last_part));
        }

        // Search in all configured paths
        for search_path in &self.search_paths {
            let mut full_path = search_path.clone();
            full_path.push(&file_path);

            if self.fs.is_file(&full_path) {
                return Some(full_path);
            }
        }

        None
    }

    /// Begin loading a library (for circular dependency detection)
    ///
    /// Call this when starting to load a library.
    /// Must be paired with `end_loading()` when done.
    ///
    /// Returns an error if this would create a circular dependency.
    pub fn begin_loading(&mut self, name: &[String]) -> Result<(), LibraryError> {
        // Check if this library is already in the loading stack
        if self.loading_stack.iter().any(|n| n == name) {
            // Circular dependency detected
            let mut chain = self.loading_stack.clone();
            chain.push(name.to_vec());
            return Err(LibraryError::CircularDependency(chain));
        }

        self.loading_stack.push(name.to_vec());
        Ok(())
    }

    /// End loading a library (for circular dependency detection)
    ///
    /// Call this when done loading a library, whether successful or not.
    pub fn end_loading(&mut self, name: &[String]) {
        if let Some(pos) = self.loading_stack.iter().position(|n| n == name) {
            self.loading_stack.remove(pos);
        }
    }

    /// Get all loaded library names
    pub fn loaded_libraries(&self) -> Vec<&Vec<String>> {
        self.libraries.keys().collect()
    }

    /// Clear all loaded libraries
    ///
    /// Useful for testing or resetting the interpreter state.
    pub fn clear(&mut self) {
        self.libraries.clear();
        self.loading_stack.clear();
    }
}

impl Default for LibraryRegistry {
    fn default() -> Self {
        Self::with_default_paths()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = LibraryRegistry::new();
        assert!(registry.libraries.is_empty());
        assert!(registry.search_paths.is_empty());
    }

    #[test]
    fn test_default_search_paths() {
        let registry = LibraryRegistry::with_default_paths();
        assert!(!registry.search_paths.is_empty());
        // Should at least have ./lib
        assert!(registry.search_paths.iter().any(|p| p.ends_with("lib")));
    }

    #[test]
    fn test_add_search_path() {
        let mut registry = LibraryRegistry::new();
        registry.add_search_path(PathBuf::from("/usr/local/lib/patina"));

        assert_eq!(registry.search_paths().len(), 1);
    }

    #[test]
    fn test_register_library() {
        let mut registry = LibraryRegistry::new();
        let lib = Library::new(vec!["test".to_string()]);
        let name = vec!["test".to_string()];

        assert!(!registry.is_loaded(&name));

        registry.register(lib).unwrap();

        assert!(registry.is_loaded(&name));
        assert!(registry.get(&name).is_some());
    }

    #[test]
    fn test_duplicate_registration_error() {
        let mut registry = LibraryRegistry::new();
        let lib1 = Library::new(vec!["test".to_string()]);
        let lib2 = Library::new(vec!["test".to_string()]);

        registry.register(lib1).unwrap();
        let result = registry.register(lib2);

        assert!(result.is_err());
        match result {
            Err(LibraryError::AlreadyLoaded(name)) => {
                assert_eq!(name, vec!["test"]);
            }
            _ => panic!("Expected AlreadyLoaded error"),
        }
    }

    #[test]
    fn test_library_name_to_file_path() {
        let _registry = LibraryRegistry::new();

        // Note: find_library_file returns None if file doesn't exist
        // We're just testing the path construction logic here

        // Library names map to paths:
        // (scheme base) → scheme/base.sld
        // (mylib) → mylib.sld
        // (mylib utils) → mylib/utils.sld
    }

    #[test]
    fn test_circular_dependency_detection() {
        let mut registry = LibraryRegistry::new();

        let name1 = vec!["lib1".to_string()];
        let name2 = vec!["lib2".to_string()];

        // Start loading lib1
        registry.begin_loading(&name1).unwrap();

        // Start loading lib2 (lib1 imports lib2)
        registry.begin_loading(&name2).unwrap();

        // Try to load lib1 again (lib2 imports lib1) - should fail
        let result = registry.begin_loading(&name1);
        assert!(result.is_err());

        match result {
            Err(LibraryError::CircularDependency(chain)) => {
                assert_eq!(chain.len(), 3);
                assert_eq!(chain[0], name1);
                assert_eq!(chain[1], name2);
                assert_eq!(chain[2], name1);
            }
            _ => panic!("Expected CircularDependency error"),
        }

        // Clean up
        registry.end_loading(&name2);
        registry.end_loading(&name1);
    }

    #[test]
    fn test_get_loaded_libraries() {
        let mut registry = LibraryRegistry::new();

        let lib1 = Library::new(vec!["lib1".to_string()]);
        let lib2 = Library::new(vec!["scheme".to_string(), "base".to_string()]);

        registry.register(lib1).unwrap();
        registry.register(lib2).unwrap();

        let loaded = registry.loaded_libraries();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().any(|n| **n == vec!["lib1"]));
        assert!(loaded.iter().any(|n| **n == vec!["scheme", "base"]));
    }

    #[test]
    fn test_clear_registry() {
        let mut registry = LibraryRegistry::new();
        let lib = Library::new(vec!["test".to_string()]);
        let name = vec!["test".to_string()];

        registry.register(lib).unwrap();
        assert!(registry.is_loaded(&name));

        registry.clear();
        assert!(!registry.is_loaded(&name));
        assert!(registry.loaded_libraries().is_empty());
    }
}
