//! Scheme Library Support for Tree-Walker
//!
//! This module provides library loading support for the tree-walking interpreter.
//! It implements the SchemeLibraryLoader which parses and evaluates .sld files.

use crate::eval::Evaluator;
use patina_frontend::{ExportSpec, ImportSet, LibraryDefinition};
use patina_runtime::library::Library;
use patina_runtime::library_loader::LibraryLoader;
use patina_runtime::library_registry::LibraryError;
use patina_runtime::{Environment, Value};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

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
    /// This method:
    /// 1. Parses the define-library form
    /// 2. Resolves imports (recursively loads imported libraries)
    /// 3. Evaluates the library body (definitions only)
    /// 4. Collects and validates exports
    fn load_sld_file(&self, name: &[String], path: PathBuf) -> Result<Library, LibraryError> {
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

        // Create a fresh environment for this library
        let lib_env = Rc::new(Environment::new());

        // Step 1: Resolve imports
        self.resolve_imports(&lib_def.imports, &lib_env)?;

        // Step 2: Evaluate library body (definitions only)
        self.evaluate_body(&lib_def.body, &lib_env)?;

        // Step 3: Collect exports and create library
        let mut library = Library::with_env(name.to_vec(), lib_env.clone());
        library.set_source(path);
        self.collect_exports(&lib_def.exports, &lib_env, &mut library)?;

        Ok(library)
    }

    /// Resolve imports: load imported libraries and import their exports
    fn resolve_imports(
        &self,
        imports: &[ImportSet],
        lib_env: &Rc<Environment>,
    ) -> Result<(), LibraryError> {
        let evaluator = unsafe { &*self.evaluator };

        for import_set in imports {
            self.process_import_set(import_set, lib_env, evaluator)?;
        }

        Ok(())
    }

    /// Process a single import set
    #[allow(clippy::only_used_in_recursion)]
    fn process_import_set(
        &self,
        import_set: &ImportSet,
        lib_env: &Rc<Environment>,
        evaluator: &Evaluator,
    ) -> Result<(), LibraryError> {
        match import_set {
            ImportSet::Library(lib_name) => {
                // Direct library import: import all exports
                let imported_lib = evaluator.load_library(lib_name)?;

                // Import all exports into this library's environment
                for (name, value) in &imported_lib.exports {
                    lib_env.define(name.clone(), value.clone());
                }
                Ok(())
            }

            ImportSet::Only {
                import_set,
                identifiers,
            } => {
                // Import only specific identifiers
                // First process the inner import set to get the bindings
                let temp_env = Rc::new(Environment::new());
                self.process_import_set(import_set, &temp_env, evaluator)?;

                // Then import only the specified identifiers
                for id in identifiers {
                    if let Some(value) = temp_env.get(id) {
                        lib_env.define(id.clone(), value);
                    } else {
                        return Err(LibraryError::ParseError {
                            file: String::new(),
                            message: format!("Identifier '{}' not found in import set", id),
                        });
                    }
                }
                Ok(())
            }

            ImportSet::Except {
                import_set,
                identifiers,
            } => {
                // Import all except specific identifiers
                let temp_env = Rc::new(Environment::new());
                self.process_import_set(import_set, &temp_env, evaluator)?;

                let exclude: HashSet<_> = identifiers.iter().collect();

                // Import all bindings except the excluded ones
                for (name, value) in temp_env.bindings() {
                    if !exclude.contains(&name) {
                        lib_env.define(name, value);
                    }
                }

                Ok(())
            }

            ImportSet::Prefix { import_set, prefix } => {
                // Import with prefix: foo → prefix:foo
                let temp_env = Rc::new(Environment::new());
                self.process_import_set(import_set, &temp_env, evaluator)?;

                // Import all bindings with the prefix added
                for (name, value) in temp_env.bindings() {
                    let prefixed_name = format!("{}{}", prefix, name);
                    lib_env.define(prefixed_name, value);
                }

                Ok(())
            }

            ImportSet::Rename {
                import_set,
                renames,
            } => {
                // Import with renames: old-name → new-name
                let temp_env = Rc::new(Environment::new());
                self.process_import_set(import_set, &temp_env, evaluator)?;

                // Apply renames
                for (old_name, new_name) in renames {
                    if let Some(value) = temp_env.get(old_name) {
                        lib_env.define(new_name.clone(), value);
                    } else {
                        return Err(LibraryError::ParseError {
                            file: String::new(),
                            message: format!("Identifier '{}' not found for rename", old_name),
                        });
                    }
                }
                Ok(())
            }
        }
    }

    /// Evaluate library body (definitions only, no executable code)
    fn evaluate_body(&self, body: &[Value], lib_env: &Rc<Environment>) -> Result<(), LibraryError> {
        let evaluator = unsafe { &*self.evaluator };

        for expr in body {
            // Evaluate each expression in the library environment
            // Libraries should only contain definitions, but we don't enforce that here
            // The evaluator will handle it
            evaluator
                .eval_in_env(expr, lib_env)
                .map_err(|e| LibraryError::ParseError {
                    file: String::new(),
                    message: format!("Error evaluating library body: {:?}", e),
                })?;
        }

        Ok(())
    }

    /// Collect exports from the library environment
    fn collect_exports(
        &self,
        export_specs: &[ExportSpec],
        lib_env: &Rc<Environment>,
        library: &mut Library,
    ) -> Result<(), LibraryError> {
        for spec in export_specs {
            match spec {
                ExportSpec::Identifier(name) => {
                    // Export with same name
                    if let Some(value) = lib_env.get(name) {
                        library.export(name.clone(), value);
                    } else {
                        return Err(LibraryError::ParseError {
                            file: library
                                .source
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                            message: format!("Exported identifier '{}' not defined", name),
                        });
                    }
                }

                ExportSpec::Rename { internal, external } => {
                    // Export with different name
                    if let Some(value) = lib_env.get(internal) {
                        library.export(external.clone(), value);
                    } else {
                        return Err(LibraryError::ParseError {
                            file: library
                                .source
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                            message: format!("Exported identifier '{}' not defined", internal),
                        });
                    }
                }
            }
        }

        Ok(())
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
