//! Scheme Library Support for Tree-Walker
//!
//! This module provides library loading support for the tree-walking interpreter.
//! It implements the SchemeLibraryLoader which parses and evaluates .sld files.

use crate::eval::Evaluator;
use patina_runtime::library::Library;
use patina_runtime::library_loader::LibraryLoader;
use patina_runtime::library_registry::LibraryError;
use std::fs;
use std::path::PathBuf;

/// Loader for libraries defined in .sld files
///
/// This loader:
/// 1. Finds the .sld file for a library name
/// 2. Parses the define-library form
/// 3. Processes imports
/// 4. Evaluates library body
/// 5. Collects exports
///
/// Example .sld file:
/// ```scheme
/// (define-library (mylib utils)
///   (import (scheme base))
///   (export double triple)
///   (begin
///     (define (double x) (* x 2))
///     (define (triple x) (* x 3))))
/// ```
pub struct SchemeLibraryLoader {
    /// Reference to evaluator for evaluating library bodies
    #[allow(dead_code)] // Will be used when we implement full .sld parsing
    evaluator: *const Evaluator,
}

impl SchemeLibraryLoader {
    /// Create a new Scheme library loader
    ///
    /// SAFETY: The evaluator pointer must remain valid for the lifetime
    /// of this loader. This is guaranteed in practice because the loader
    /// is owned by the evaluator.
    pub fn new(evaluator: &Evaluator) -> Self {
        Self {
            evaluator: evaluator as *const Evaluator,
        }
    }

    /// Find the .sld file for a library
    fn find_sld_file(&self, name: &[String], search_paths: &[PathBuf]) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }

        // Convert library name to file path: (scheme base) → scheme/base.sld
        let mut file_path = PathBuf::new();
        for part in &name[..name.len() - 1] {
            file_path.push(part);
        }
        file_path.push(format!("{}.sld", name.last().unwrap()));

        // Search in all configured paths
        for search_path in search_paths {
            let mut full_path = search_path.clone();
            full_path.push(&file_path);

            if full_path.exists() && full_path.is_file() {
                return Some(full_path);
            }
        }

        None
    }

    /// Parse and load a .sld file
    ///
    /// For now, this is a simplified implementation that just creates
    /// an empty library. Full implementation will parse define-library forms.
    fn load_sld_file(&self, name: &[String], path: PathBuf) -> Result<Library, LibraryError> {
        // Read the file
        let content = fs::read_to_string(&path).map_err(|e| {
            LibraryError::IoError(format!("Failed to read {}: {}", path.display(), e))
        })?;

        // TODO: Parse the define-library form
        // For now, just create an empty library
        let mut lib = Library::new(name.to_vec());
        lib.set_source(path);

        // Parse the file (simplified for now)
        let _parser =
            patina_frontend::Parser::new(&content).map_err(|e| LibraryError::ParseError {
                file: lib.source.as_ref().unwrap().display().to_string(),
                message: format!("{:?}", e),
            })?;

        // TODO: Process define-library form:
        // 1. Parse imports
        // 2. Parse exports
        // 3. Parse body
        // 4. Evaluate body in library environment
        // 5. Collect exports

        Ok(lib)
    }
}

impl LibraryLoader for SchemeLibraryLoader {
    fn load(&self, name: &[String], search_paths: &[PathBuf]) -> Result<Library, LibraryError> {
        // Find the .sld file
        let path = self
            .find_sld_file(name, search_paths)
            .ok_or_else(|| LibraryError::NotFound(name.to_vec()))?;

        // Load and parse it
        self.load_sld_file(name, path)
    }

    fn can_load(&self, name: &[String]) -> bool {
        // For now, we can't easily check without the search paths
        // In practice, this will be called with search_paths available
        // So we'll just return true and let load() handle the actual check
        !name.is_empty()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_find_sld_file_conversion() {
        // We can't easily test find_sld_file without an evaluator,
        // but we can document the expected path conversions:
        //
        // (scheme base) → scheme/base.sld
        // (mylib) → mylib.sld
        // (mylib utils) → mylib/utils.sld
        // (srfi 1) → srfi/1.sld
    }
}
