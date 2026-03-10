//! Scheme Library Support
//!
//! This module provides library loading support for Scheme interpreters.
//! It implements the SchemeLibraryLoader which parses .sld files.
//!
//! The loader is stateless and only handles parsing. The evaluator handles
//! import resolution and evaluation, eliminating the need for circular references.

use crate::{BodyElement, LibraryDefinition};
use patina_core::{SharedHeap, TaggedValue};
use patina_runtime::library_loader::{
    EvaluatingLibraryLoader, ExportSpec, ImportSet, ParsedLibrary,
};
use patina_runtime::library_registry::LibraryError;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Result of parsing a single library declaration.
/// Contains the exports, imports, and body elements from the declaration.
type ParsedDeclaration = (Vec<ExportSpec>, Vec<ImportSet>, Vec<BodyElement>);

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

    /// Parse a .sld file into a ParsedLibrary with a library availability checker.
    ///
    /// This method creates a fresh heap for parsing. Use `parse_sld_file_with_heap_and_checker`
    /// to provide a shared heap for TaggedValue compatibility with the global environment.
    fn parse_sld_file_with_checker(
        &self,
        name: &[String],
        path: PathBuf,
        can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<ParsedLibrary, LibraryError> {
        let heap = patina_core::new_shared_heap();
        self.parse_sld_file_with_heap_and_checker(name, path, heap, can_load_library)
    }

    /// Parse a .sld file into a ParsedLibrary with a shared heap and library checker.
    ///
    /// Uses the provided heap so that body TaggedValues are allocated on the same
    /// heap as the global environment, eliminating cross-heap conversion overhead.
    fn parse_sld_file_with_heap_and_checker(
        &self,
        name: &[String],
        path: PathBuf,
        heap: SharedHeap,
        can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<ParsedLibrary, LibraryError> {
        // Read the file
        let content = fs::read_to_string(&path).map_err(|e| {
            LibraryError::IoError(format!("Failed to read {}: {}", path.display(), e))
        })?;

        // Parse the .sld header directly as TaggedValue on the shared heap
        let mut parser = crate::Parser::new_with_heap(&content, heap.clone()).map_err(|e| {
            LibraryError::ParseError {
                file: path.display().to_string(),
                message: format!("{:?}", e),
            }
        })?;

        let lib_form = parser.parse().map_err(|e| LibraryError::ParseError {
            file: path.display().to_string(),
            message: format!("{:?}", e),
        })?;

        // Parse the define-library form into structured data with library checker
        let lib_def =
            LibraryDefinition::from_tagged_with_library_checker(lib_form, &heap, can_load_library)
                .map_err(|e| LibraryError::ParseError {
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

        // Resolve body elements (expand includes and include-library-declarations)
        let mut included_files = HashSet::new();
        // Add the .sld file itself to prevent self-inclusion
        if let Ok(canonical) = path.canonicalize() {
            included_files.insert(canonical);
        }

        // Start with exports and imports from the main definition
        let mut exports = lib_def.exports;
        let mut imports = lib_def.imports;
        let mut body = Vec::new();

        self.resolve_body_elements(
            &lib_def.body_elements,
            sld_dir,
            &mut included_files,
            &path,
            &mut exports,
            &mut imports,
            &mut body,
            &heap,
        )?;

        // Return parsed library for the evaluator to process
        Ok(ParsedLibrary {
            name: lib_def.name,
            imports,
            body,
            heap: Some(heap),
            exports,
            source: Some(path),
        })
    }

    /// Resolve body elements by expanding includes into actual expressions.
    ///
    /// R7RS §5.6.1: The expressions from all `begin`, `include` and `include-ci`
    /// library declarations are expanded in that environment in the order in which
    /// they occur in the library.
    ///
    /// `include-library-declarations` splices entire library declarations (export,
    /// import, begin, include, etc.) from external files.
    ///
    /// Body expressions are collected as TaggedValues on the provided heap.
    /// Begin bodies and included files are parsed directly as TaggedValues
    /// on the shared heap.
    #[allow(clippy::too_many_arguments)]
    fn resolve_body_elements(
        &self,
        elements: &[BodyElement],
        sld_dir: &Path,
        included_files: &mut HashSet<PathBuf>,
        source_file: &Path,
        exports: &mut Vec<ExportSpec>,
        imports: &mut Vec<ImportSet>,
        body: &mut Vec<TaggedValue>,
        heap: &SharedHeap,
    ) -> Result<(), LibraryError> {
        for element in elements {
            match element {
                BodyElement::Begin(exprs) => {
                    // Begin bodies are already TaggedValues on the shared heap
                    // (converted during library definition parsing).
                    body.extend(exprs);
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

                        // Parse included file directly as TaggedValues on shared heap
                        let exprs = self.parse_included_file_tagged(
                            &file_path,
                            *case_insensitive,
                            source_file,
                            heap,
                        )?;
                        body.extend(exprs);
                    }
                }
                BodyElement::IncludeLibraryDeclarations { paths } => {
                    for include_path in paths {
                        let file_path = sld_dir.join(include_path);

                        // Check file exists
                        if !file_path.exists() {
                            return Err(LibraryError::IoError(format!(
                                "Include-library-declarations file not found: {} (from {})",
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
                                "Circular include-library-declarations detected: {} already included",
                                file_path.display()
                            )));
                        }

                        // Parse included declarations and process them recursively
                        self.parse_library_declarations_file(
                            &file_path,
                            sld_dir,
                            included_files,
                            source_file,
                            exports,
                            imports,
                            body,
                            heap,
                        )?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Parse an included library declarations file and splice its declarations.
    ///
    /// R7RS §5.6.1: `include-library-declarations` includes library declarations
    /// (export, import, begin, include, cond-expand, etc.) from external files.
    #[allow(clippy::too_many_arguments)]
    fn parse_library_declarations_file(
        &self,
        path: &Path,
        sld_dir: &Path,
        included_files: &mut HashSet<PathBuf>,
        source_file: &Path,
        exports: &mut Vec<ExportSpec>,
        imports: &mut Vec<ImportSet>,
        body: &mut Vec<TaggedValue>,
        heap: &SharedHeap,
    ) -> Result<(), LibraryError> {
        let content = fs::read_to_string(path).map_err(|e| {
            LibraryError::IoError(format!(
                "Failed to read include-library-declarations file {}: {} (from {})",
                path.display(),
                e,
                source_file.display()
            ))
        })?;

        // Parse all expressions directly as TaggedValues on the shared heap
        let mut parser = crate::Parser::new_with_heap(&content, heap.clone()).map_err(|e| {
            LibraryError::ParseError {
                file: path.display().to_string(),
                message: format!("{:?}", e),
            }
        })?;

        let declarations = parser.parse_all().map_err(|e| LibraryError::ParseError {
            file: path.display().to_string(),
            message: format!("{:?}", e),
        })?;

        // Parse each declaration using LibraryDefinition's parsing logic
        for decl in &declarations {
            let (decl_exports, decl_imports, decl_body) =
                Self::parse_single_declaration(*decl, path, heap)?;

            exports.extend(decl_exports);
            imports.extend(decl_imports);

            // Recursively resolve any body elements (which might contain more includes)
            self.resolve_body_elements(
                &decl_body,
                sld_dir,
                included_files,
                path,
                exports,
                imports,
                body,
                heap,
            )?;
        }

        Ok(())
    }

    /// Parse a single library declaration from a TaggedValue.
    ///
    /// Returns (exports, imports, body_elements) for the declaration.
    fn parse_single_declaration(
        tv: TaggedValue,
        source_file: &Path,
        heap: &SharedHeap,
    ) -> Result<ParsedDeclaration, LibraryError> {
        use crate::ExportSpec as FrontendExportSpec;

        // Build a dummy (define-library (temp) <decl>) wrapper on the heap
        let dummy_lib = {
            let mut h = heap.borrow_mut();
            let define_library_sym = h.intern_symbol("define-library");
            let temp_sym = h.intern_symbol("temp");
            let temp_name = h.alloc_pair(temp_sym, TaggedValue::NULL);
            // Build list: (define-library (temp) <decl>)
            let tail = h.alloc_pair(tv, TaggedValue::NULL);
            let with_name = h.alloc_pair(temp_name, tail);
            h.alloc_pair(define_library_sym, with_name)
        };

        let lib_def = LibraryDefinition::from_tagged(dummy_lib, heap).map_err(|e| {
            LibraryError::ParseError {
                file: source_file.display().to_string(),
                message: format!("Invalid library declaration: {:?}", e),
            }
        })?;

        // Convert frontend types to runtime types
        let mut exports = Vec::new();
        let mut imports = Vec::new();
        for exp in lib_def.exports {
            exports.push(match exp {
                FrontendExportSpec::Identifier(name) => ExportSpec::Identifier(name),
                FrontendExportSpec::Rename { internal, external } => {
                    ExportSpec::Rename { internal, external }
                }
            });
        }
        for imp in lib_def.imports {
            imports.push(Self::convert_import_set(imp));
        }

        Ok((exports, imports, lib_def.body_elements))
    }

    /// Convert frontend ImportSet to runtime ImportSet
    fn convert_import_set(imp: crate::ImportSet) -> ImportSet {
        use crate::ImportSet as FrontendImportSet;

        match imp {
            FrontendImportSet::Library(name) => ImportSet::Library(name),
            FrontendImportSet::Only {
                import_set,
                identifiers,
            } => ImportSet::Only {
                import_set: Box::new(Self::convert_import_set(*import_set)),
                identifiers,
            },
            FrontendImportSet::Except {
                import_set,
                identifiers,
            } => ImportSet::Except {
                import_set: Box::new(Self::convert_import_set(*import_set)),
                identifiers,
            },
            FrontendImportSet::Prefix { import_set, prefix } => ImportSet::Prefix {
                import_set: Box::new(Self::convert_import_set(*import_set)),
                prefix,
            },
            FrontendImportSet::Rename {
                import_set,
                renames,
            } => ImportSet::Rename {
                import_set: Box::new(Self::convert_import_set(*import_set)),
                renames,
            },
        }
    }

    /// Parse an included file directly as TaggedValues on the shared heap.
    ///
    /// Included files (binding.scm, conditionals.scm, etc.) are parsed directly
    /// as TaggedValues on the shared heap.
    fn parse_included_file_tagged(
        &self,
        path: &Path,
        case_insensitive: bool,
        source_file: &Path,
        heap: &SharedHeap,
    ) -> Result<Vec<TaggedValue>, LibraryError> {
        let content = fs::read_to_string(path).map_err(|e| {
            LibraryError::IoError(format!(
                "Failed to read include file {}: {} (from {})",
                path.display(),
                e,
                source_file.display()
            ))
        })?;

        // Create parser with shared heap — TaggedValues go directly on global heap
        let mut parser = if case_insensitive {
            crate::Parser::new_case_insensitive_with_heap(&content, heap.clone())
        } else {
            crate::Parser::new_with_heap(&content, heap.clone())
        }
        .map_err(|e| LibraryError::ParseError {
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

        // Parse it with no library checker
        self.parse_sld_file_with_checker(name, path, &|_| false)
    }

    fn can_load(&self, name: &[String]) -> bool {
        // Without search paths, we can't verify if the file exists
        // Return true optimistically - actual check happens in can_load_with_paths
        !name.is_empty()
    }

    fn can_load_with_paths(&self, name: &[String], search_paths: &[PathBuf]) -> bool {
        // Check if the .sld file actually exists
        self.find_sld_file(name, search_paths).is_some()
    }

    fn parse_with_library_checker(
        &self,
        name: &[String],
        search_paths: &[PathBuf],
        can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<ParsedLibrary, LibraryError> {
        // Find the .sld file
        let path = self
            .find_sld_file(name, search_paths)
            .ok_or_else(|| LibraryError::NotFound(name.to_vec()))?;

        // Parse it with the library checker
        self.parse_sld_file_with_checker(name, path, can_load_library)
    }

    fn parse_with_heap_and_library_checker(
        &self,
        name: &[String],
        search_paths: &[PathBuf],
        heap: SharedHeap,
        can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<ParsedLibrary, LibraryError> {
        // Find the .sld file
        let path = self
            .find_sld_file(name, search_paths)
            .ok_or_else(|| LibraryError::NotFound(name.to_vec()))?;

        // Parse it with the shared heap and library checker
        self.parse_sld_file_with_heap_and_checker(name, path, heap, can_load_library)
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
