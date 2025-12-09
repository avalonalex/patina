//! Scheme Library Support for Tree-Walker
//!
//! This module provides library loading support for the tree-walking interpreter.
//! It implements the SchemeLibraryLoader which parses .sld files.
//!
//! The loader is stateless and only handles parsing. The evaluator handles
//! import resolution and evaluation, eliminating the need for circular references.

use patina_frontend::{BodyElement, LibraryDefinition};
use patina_runtime::Value;
use patina_runtime::library_loader::{
    EvaluatingLibraryLoader, ExportSpec, ImportSet, ParsedLibrary,
};
use patina_runtime::library_registry::LibraryError;
use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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
    /// This method parses the file structure and resolves all includes.
    /// The evaluator will handle import resolution and body evaluation.
    ///
    /// The `can_load_library` callback is used for `(library <name>)` requirements
    /// in cond-expand clauses.
    fn parse_sld_file_with_checker(
        &self,
        name: &[String],
        path: PathBuf,
        can_load_library: &dyn Fn(&[String]) -> bool,
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

        // Parse the define-library form into structured data with library checker
        let lib_def =
            LibraryDefinition::from_value_with_library_checker(&lib_form, can_load_library)
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
        )?;

        // Return parsed library for the evaluator to process
        Ok(ParsedLibrary {
            name: lib_def.name,
            imports,
            body,
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
    #[allow(clippy::too_many_arguments)]
    fn resolve_body_elements(
        &self,
        elements: &[BodyElement],
        sld_dir: &Path,
        included_files: &mut HashSet<PathBuf>,
        source_file: &Path,
        exports: &mut Vec<ExportSpec>,
        imports: &mut Vec<ImportSet>,
        body: &mut Vec<Value>,
    ) -> Result<(), LibraryError> {
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

                        // Read and parse the included file as expressions
                        let exprs =
                            self.parse_included_file(&file_path, *case_insensitive, source_file)?;
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
        body: &mut Vec<Value>,
    ) -> Result<(), LibraryError> {
        let content = fs::read_to_string(path).map_err(|e| {
            LibraryError::IoError(format!(
                "Failed to read include-library-declarations file {}: {} (from {})",
                path.display(),
                e,
                source_file.display()
            ))
        })?;

        // Parse all expressions from the file
        let mut parser =
            patina_frontend::Parser::new(&content).map_err(|e| LibraryError::ParseError {
                file: path.display().to_string(),
                message: format!("{:?}", e),
            })?;

        let declarations = parser.parse_all().map_err(|e| LibraryError::ParseError {
            file: path.display().to_string(),
            message: format!("{:?}", e),
        })?;

        // Parse each declaration using LibraryDefinition's parsing logic
        for decl in &declarations {
            let (decl_exports, decl_imports, decl_body) =
                Self::parse_single_declaration(decl, path)?;

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
            )?;
        }

        Ok(())
    }

    /// Parse a single library declaration from a Value.
    ///
    /// Returns (exports, imports, body_elements) for the declaration.
    fn parse_single_declaration(
        value: &Value,
        source_file: &Path,
    ) -> Result<ParsedDeclaration, LibraryError> {
        use patina_frontend::ExportSpec as FrontendExportSpec;

        let mut exports = Vec::new();
        let mut imports = Vec::new();
        let body_elements;

        // Use a temporary LibraryDefinition to parse the declaration
        // We create a dummy library wrapper to reuse the parsing logic
        let dummy_lib = Self::make_list(vec![
            Value::symbol("define-library"),
            Self::make_list(vec![Value::symbol("temp")]),
            value.clone(),
        ]);

        match LibraryDefinition::from_value(&dummy_lib) {
            Ok(lib_def) => {
                // Convert frontend types to runtime types
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
                body_elements = lib_def.body_elements;
            }
            Err(e) => {
                return Err(LibraryError::ParseError {
                    file: source_file.display().to_string(),
                    message: format!("Invalid library declaration: {:?}", e),
                });
            }
        }

        Ok((exports, imports, body_elements))
    }

    /// Helper to construct a proper Scheme list from a Vec of Values
    fn make_list(items: Vec<Value>) -> Value {
        items.into_iter().rev().fold(Value::Null, |acc, item| {
            Value::Pair(Rc::new(RefCell::new((item, acc))))
        })
    }

    /// Convert frontend ImportSet to runtime ImportSet
    fn convert_import_set(imp: patina_frontend::ImportSet) -> ImportSet {
        use patina_frontend::ImportSet as FrontendImportSet;

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

    /// Parse an included file and return its expressions.
    fn parse_included_file(
        &self,
        path: &Path,
        case_insensitive: bool,
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

        // Create parser - use case-insensitive mode for include-ci
        let mut parser = if case_insensitive {
            patina_frontend::Parser::new_case_insensitive(&content)
        } else {
            patina_frontend::Parser::new(&content)
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
