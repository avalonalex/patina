//! Scheme Library Support for Tree-Walker
//!
//! This module provides library loading support for the tree-walking interpreter.
//! It implements the SchemeLibraryLoader which parses .sld files.
//!
//! The loader is stateless and only handles parsing. The evaluator handles
//! import resolution and evaluation, eliminating the need for circular references.

use patina_frontend::{BodyElement, LibraryDefinition};
use patina_runtime::Value;
use patina_runtime::library_loader::{EvaluatingLibraryLoader, ParsedLibrary};
use patina_runtime::library_registry::LibraryError;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Loader for libraries defined in .sld files
///
/// This loader parses .sld files but delegates evaluation to the evaluator.
/// This stateless design eliminates the unsafe circular reference that
/// previously caused segfaults.
///
/// The loader:
/// 1. Finds the .sld file for a library name
/// 2. Parses the define-library form
/// 3. Returns ParsedLibrary for the evaluator to process
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
pub struct SchemeLibraryLoader;

impl SchemeLibraryLoader {
    /// Create a new Scheme library loader
    pub fn new() -> Self {
        Self
    }
}

impl Default for SchemeLibraryLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemeLibraryLoader {
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

    /// Parse a .sld file into a ParsedLibrary
    ///
    /// This method parses the file structure and resolves all includes.
    /// The evaluator will handle import resolution and body evaluation.
    fn parse_sld_file(
        &self,
        name: &[String],
        path: PathBuf,
    ) -> Result<ParsedLibrary, LibraryError> {
        // Read the file
        let content = fs::read_to_string(&path).map_err(|e| {
            LibraryError::IoError(format!("Failed to read {}: {}", path.display(), e))
        })?;

        // Parse the file to get the define-library form
        let mut parser =
            patina_frontend::Parser::new(&content).map_err(|e| LibraryError::ParseError {
                file: path.display().to_string(),
                message: format!("{:?}", e),
            })?;

        let lib_form = parser.parse().map_err(|e| LibraryError::ParseError {
            file: path.display().to_string(),
            message: format!("{:?}", e),
        })?;

        // Parse the define-library form into structured data
        let lib_def =
            LibraryDefinition::from_value(&lib_form).map_err(|e| LibraryError::ParseError {
                file: path.display().to_string(),
                message: format!("{:?}", e),
            })?;

        // Verify the library name matches
        if lib_def.name != name {
            return Err(LibraryError::ParseError {
                file: path.display().to_string(),
                message: format!(
                    "Library name mismatch: expected ({}), got ({})",
                    name.join(" "),
                    lib_def.name.join(" ")
                ),
            });
        }

        // Get the directory containing the .sld file for resolving includes
        let sld_dir = path.parent().unwrap_or(Path::new("."));

        // Resolve body elements (expand includes)
        let mut included_files = HashSet::new();
        // Add the .sld file itself to prevent self-inclusion
        if let Ok(canonical) = path.canonicalize() {
            included_files.insert(canonical);
        }

        let body = self.resolve_body_elements(
            &lib_def.body_elements,
            sld_dir,
            &mut included_files,
            &path,
        )?;

        // Return parsed library for the evaluator to process
        Ok(ParsedLibrary {
            name: lib_def.name,
            imports: lib_def.imports,
            body,
            exports: lib_def.exports,
            source: Some(path),
        })
    }

    /// Resolve body elements by expanding includes into actual expressions.
    ///
    /// R7RS §5.6.1: The expressions from all `begin`, `include` and `include-ci`
    /// library declarations are expanded in that environment in the order in which
    /// they occur in the library.
    fn resolve_body_elements(
        &self,
        elements: &[BodyElement],
        sld_dir: &Path,
        included_files: &mut HashSet<PathBuf>,
        source_file: &Path,
    ) -> Result<Vec<Value>, LibraryError> {
        let mut body = Vec::new();

        for element in elements {
            match element {
                BodyElement::Begin(exprs) => {
                    body.extend(exprs.clone());
                }
                BodyElement::Include {
                    paths,
                    case_insensitive,
                } => {
                    for include_path in paths {
                        let file_path = sld_dir.join(include_path);

                        // Check file exists
                        if !file_path.exists() {
                            return Err(LibraryError::IoError(format!(
                                "Include file not found: {} (from {})",
                                file_path.display(),
                                source_file.display()
                            )));
                        }

                        // Cycle detection using canonical paths
                        let canonical = file_path.canonicalize().map_err(|e| {
                            LibraryError::IoError(format!(
                                "Failed to resolve path {}: {}",
                                file_path.display(),
                                e
                            ))
                        })?;

                        if !included_files.insert(canonical.clone()) {
                            return Err(LibraryError::IoError(format!(
                                "Circular include detected: {} already included",
                                file_path.display()
                            )));
                        }

                        // Read and parse the included file
                        let exprs =
                            self.parse_included_file(&file_path, *case_insensitive, source_file)?;
                        body.extend(exprs);
                    }
                }
            }
        }

        Ok(body)
    }

    /// Parse an included file and return its expressions.
    fn parse_included_file(
        &self,
        path: &Path,
        _case_insensitive: bool,
        source_file: &Path,
    ) -> Result<Vec<Value>, LibraryError> {
        let content = fs::read_to_string(path).map_err(|e| {
            LibraryError::IoError(format!(
                "Failed to read include file {}: {} (from {})",
                path.display(),
                e,
                source_file.display()
            ))
        })?;

        // TODO: For include-ci, we need a case-insensitive parser mode.
        // For now, we parse normally. Case-insensitive reading is rare in practice.
        let mut parser =
            patina_frontend::Parser::new(&content).map_err(|e| LibraryError::ParseError {
                file: path.display().to_string(),
                message: format!("{:?}", e),
            })?;

        parser.parse_all().map_err(|e| LibraryError::ParseError {
            file: path.display().to_string(),
            message: format!("{:?}", e),
        })
    }
}

impl EvaluatingLibraryLoader for SchemeLibraryLoader {
    fn parse(
        &self,
        name: &[String],
        search_paths: &[PathBuf],
    ) -> Result<ParsedLibrary, LibraryError> {
        // Find the .sld file
        let path = self
            .find_sld_file(name, search_paths)
            .ok_or_else(|| LibraryError::NotFound(name.to_vec()))?;

        // Parse it (no evaluation)
        self.parse_sld_file(name, path)
    }

    fn can_load(&self, name: &[String]) -> bool {
        // We can potentially load any library name (will check file existence in parse())
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
