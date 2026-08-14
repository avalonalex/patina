//! Library Declaration Parser
//!
//! Parses R7RS define-library forms into structured data.
//!
//! Syntax:
//! ```scheme
//! (define-library (library name parts ...)
//!   (export identifier ...)
//!   (import (scheme base) ...)
//!   (begin
//!     definitions and expressions)
//!   (include "file1.scm" "file2.scm"))
//! ```

use crate::ParseError;
use crate::cond_expand::{parse_library_name_tagged, tagged_list_to_vec};
use patina_core::{SharedHeap, TaggedValue};

// Re-export library types from runtime (they moved there to fix dependency issues)
pub use patina_runtime::library_loader::{ExportSpec, ImportSet};

/// Represents a body element in a library definition.
///
/// R7RS §5.6.1: The expressions from all `begin`, `include` and `include-ci`
/// library declarations are expanded in that environment in the order in which
/// they occur in the library.
#[derive(Debug, Clone)]
pub enum BodyElement {
    /// Inline code from `(begin expr1 expr2 ...)`
    ///
    /// Expressions are stored as TaggedValues on a shared heap provided
    /// during library definition parsing.
    Begin(Vec<TaggedValue>),

    /// Files to include: `(include "file1.scm" "file2.scm" ...)`
    /// The bool indicates case-insensitive mode (include-ci)
    Include {
        /// File paths relative to the .sld file
        paths: Vec<String>,
        /// If true, parse with case-insensitive reader (include-ci)
        case_insensitive: bool,
    },

    /// Library declarations to include: `(include-library-declarations "file.scm" ...)`
    ///
    /// R7RS §5.6.1: The contents of the file are spliced directly into the
    /// current library definition. The file should contain library declarations
    /// like export, import, begin, include, etc.
    IncludeLibraryDeclarations {
        /// File paths relative to the .sld file
        paths: Vec<String>,
    },
}

/// Whether unknown `define-library` declarations abort the load.
///
/// Default is lenient (warn to stderr and skip the declaration), so one
/// vendor-specific clause does not make an otherwise-portable library
/// unloadable. Set `PATINA_STRICT_LIBRARY_SYNTAX=1` to restore the error.
fn strict_library_syntax() -> bool {
    std::env::var_os("PATINA_STRICT_LIBRARY_SYNTAX").is_some_and(|v| v != "0")
}

/// A parsed library definition
#[derive(Debug, Clone)]
pub struct LibraryDefinition {
    /// Library name as list of symbols: (scheme base) → ["scheme", "base"]
    pub name: Vec<String>,

    /// Export specifications
    pub exports: Vec<ExportSpec>,

    /// Import sets
    pub imports: Vec<ImportSet>,

    /// Library body elements in declaration order.
    /// Must be resolved by the loader to produce actual body expressions.
    pub body_elements: Vec<BodyElement>,
}

impl LibraryDefinition {
    /// Parse a define-library form from a TaggedValue
    ///
    /// Expects a list: (define-library <name> <declaration>*)
    ///
    /// Note: This method cannot check `(library <name>)` requirements in cond-expand.
    /// Use `from_tagged_with_library_checker` if you need library availability checks.
    pub fn from_tagged(tv: TaggedValue, heap: &SharedHeap) -> Result<Self, ParseError> {
        Self::from_tagged_with_library_checker(tv, heap, &|_| false)
    }

    /// Parse a define-library form with a library availability checker.
    ///
    /// The `can_load_library` callback is used for `(library <name>)` requirements
    /// in cond-expand clauses. It should return true if the named library can be loaded.
    ///
    /// The `heap` is used to store begin body expressions as TaggedValues.
    ///
    /// Expects a list: (define-library <name> <declaration>*)
    pub fn from_tagged_with_library_checker(
        tv: TaggedValue,
        heap: &SharedHeap,
        can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<Self, ParseError> {
        let list = tagged_list_to_vec(tv, heap)?;

        if list.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "Empty define-library form".to_string(),
            ));
        }

        // First element must be the symbol 'define-library
        Self::expect_symbol_tagged(list[0], heap, "define-library")?;

        if list.len() < 2 {
            return Err(ParseError::InvalidSyntax(
                "define-library requires a library name".to_string(),
            ));
        }

        // Second element is the library name
        let name = parse_library_name_tagged(list[1], heap)?;

        // Rest are declarations
        let mut exports = Vec::new();
        let mut imports = Vec::new();
        let mut body_elements = Vec::new();

        for &decl in &list[2..] {
            Self::parse_declaration_tagged(
                decl,
                &mut exports,
                &mut imports,
                &mut body_elements,
                can_load_library,
                heap,
            )?;
        }

        Ok(LibraryDefinition {
            name,
            exports,
            imports,
            body_elements,
        })
    }

    /// Parse a library declaration from a TaggedValue
    fn parse_declaration_tagged(
        tv: TaggedValue,
        exports: &mut Vec<ExportSpec>,
        imports: &mut Vec<ImportSet>,
        body_elements: &mut Vec<BodyElement>,
        can_load_library: &dyn Fn(&[String]) -> bool,
        heap: &SharedHeap,
    ) -> Result<(), ParseError> {
        let list = tagged_list_to_vec(tv, heap)?;

        if list.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "Empty library declaration".to_string(),
            ));
        }

        // First element determines the type
        let keyword = {
            let h = heap.borrow();
            h.get_symbol_name(list[0]).map(|s| s.to_string())
        };

        if let Some(keyword) = keyword {
            match keyword.as_str() {
                "export" => {
                    for &spec in &list[1..] {
                        exports.push(Self::parse_export_spec_tagged(spec, heap)?);
                    }
                    Ok(())
                }
                "import" => {
                    for &set in &list[1..] {
                        imports.push(Self::parse_import_set_tagged(set, heap)?);
                    }
                    Ok(())
                }
                "begin" => {
                    // Begin body exprs are already TaggedValues on the heap
                    body_elements.push(BodyElement::Begin(list[1..].to_vec()));
                    Ok(())
                }
                "include" => {
                    let paths = Self::parse_include_paths_tagged(&list[1..], heap)?;
                    body_elements.push(BodyElement::Include {
                        paths,
                        case_insensitive: false,
                    });
                    Ok(())
                }
                "include-ci" => {
                    let paths = Self::parse_include_paths_tagged(&list[1..], heap)?;
                    body_elements.push(BodyElement::Include {
                        paths,
                        case_insensitive: true,
                    });
                    Ok(())
                }
                "cond-expand" => Self::parse_cond_expand_tagged(
                    &list[1..],
                    exports,
                    imports,
                    body_elements,
                    can_load_library,
                    heap,
                ),
                "include-library-declarations" => {
                    let paths = Self::parse_include_paths_tagged(&list[1..], heap)?;
                    body_elements.push(BodyElement::IncludeLibraryDeclarations { paths });
                    Ok(())
                }
                _ => {
                    // Portable .sld files occasionally carry vendor-specific
                    // declarations. Skipping one keeps the rest of the library
                    // loadable; aborting the whole load is opt-in.
                    if strict_library_syntax() {
                        Err(ParseError::InvalidSyntax(format!(
                            "Unknown library declaration: {}",
                            keyword
                        )))
                    } else {
                        eprintln!(
                            "warning: ignoring unknown library declaration `{}` in define-library",
                            keyword
                        );
                        Ok(())
                    }
                }
            }
        } else {
            Err(ParseError::InvalidSyntax(
                "Library declaration must start with a symbol".to_string(),
            ))
        }
    }

    /// Parse include file paths from a slice of TaggedValues
    fn parse_include_paths_tagged(
        values: &[TaggedValue],
        heap: &SharedHeap,
    ) -> Result<Vec<String>, ParseError> {
        if values.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "include requires at least one filename".to_string(),
            ));
        }

        let h = heap.borrow();
        let mut paths = Vec::new();
        for &tv in values {
            if let Some(s) = h.get_string_contents(tv) {
                paths.push(s);
            } else {
                return Err(ParseError::InvalidSyntax(format!(
                    "include filename must be a string, got: {}",
                    h.type_name(tv)
                )));
            }
        }
        Ok(paths)
    }

    /// Parse cond-expand declaration in library context (tagged version)
    fn parse_cond_expand_tagged(
        clauses: &[TaggedValue],
        exports: &mut Vec<ExportSpec>,
        imports: &mut Vec<ImportSet>,
        body_elements: &mut Vec<BodyElement>,
        can_load_library: &dyn Fn(&[String]) -> bool,
        heap: &SharedHeap,
    ) -> Result<(), ParseError> {
        use crate::cond_expand::evaluate_feature_requirement_tagged;
        use patina_runtime::default_features;

        if clauses.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "cond-expand requires at least one clause".to_string(),
            ));
        }

        let features = default_features();

        for &clause in clauses {
            let clause_list = tagged_list_to_vec(clause, heap)?;

            if clause_list.is_empty() {
                return Err(ParseError::InvalidSyntax(
                    "Empty cond-expand clause".to_string(),
                ));
            }

            // Check for else clause
            let is_else = {
                let h = heap.borrow();
                h.get_symbol_name(clause_list[0]) == Some("else")
            };

            let matches = if is_else {
                true
            } else {
                evaluate_feature_requirement_tagged(
                    clause_list[0],
                    heap,
                    &features,
                    can_load_library,
                )?
            };

            if matches {
                for &decl in &clause_list[1..] {
                    Self::parse_declaration_tagged(
                        decl,
                        exports,
                        imports,
                        body_elements,
                        can_load_library,
                        heap,
                    )?;
                }
                return Ok(());
            }
        }

        Ok(())
    }

    /// Parse an export spec from a TaggedValue
    fn parse_export_spec_tagged(
        tv: TaggedValue,
        heap: &SharedHeap,
    ) -> Result<ExportSpec, ParseError> {
        // Check if it's a simple symbol
        {
            let h = heap.borrow();
            if let Some(name) = h.get_symbol_name(tv) {
                return Ok(ExportSpec::Identifier(name.to_string()));
            }
            if tv.is_pair() {
                // fall through to list parsing below
            } else {
                return Err(ParseError::InvalidSyntax(format!(
                    "Invalid export spec: {}",
                    h.type_name(tv)
                )));
            }
        }

        // (rename internal external)
        let list = tagged_list_to_vec(tv, heap)?;

        if list.len() != 3 {
            return Err(ParseError::InvalidSyntax(
                "rename export requires exactly 3 elements".to_string(),
            ));
        }

        Self::expect_symbol_tagged(list[0], heap, "rename")?;

        let h = heap.borrow();
        let internal = h
            .get_symbol_name(list[1])
            .ok_or_else(|| {
                ParseError::InvalidSyntax("rename internal name must be a symbol".to_string())
            })?
            .to_string();

        let external = h
            .get_symbol_name(list[2])
            .ok_or_else(|| {
                ParseError::InvalidSyntax("rename external name must be a symbol".to_string())
            })?
            .to_string();

        Ok(ExportSpec::Rename { internal, external })
    }

    /// Parse an import set from a TaggedValue
    pub fn parse_import_set_tagged(
        tv: TaggedValue,
        heap: &SharedHeap,
    ) -> Result<ImportSet, ParseError> {
        {
            let h = heap.borrow();
            if !tv.is_pair() {
                return Err(ParseError::InvalidSyntax(format!(
                    "Invalid import set: {}",
                    h.type_name(tv)
                )));
            }
        }

        let list = tagged_list_to_vec(tv, heap)?;

        if list.is_empty() {
            return Err(ParseError::InvalidSyntax("Empty import set".to_string()));
        }

        // Check if first element is a modifier keyword
        let first_sym = {
            let h = heap.borrow();
            h.get_symbol_name(list[0]).map(|s| s.to_string())
        };

        if let Some(ref first) = first_sym {
            match first.as_str() {
                "only" => return Self::parse_only_import_tagged(&list, heap),
                "except" => return Self::parse_except_import_tagged(&list, heap),
                "prefix" => return Self::parse_prefix_import_tagged(&list, heap),
                "rename" => return Self::parse_rename_import_tagged(&list, heap),
                _ => {}
            }
        }

        // It's a library name
        parse_library_name_tagged(tv, heap).map(ImportSet::Library)
    }

    /// Parse (only <import-set> id1 id2 ...)
    fn parse_only_import_tagged(
        list: &[TaggedValue],
        heap: &SharedHeap,
    ) -> Result<ImportSet, ParseError> {
        if list.len() < 3 {
            return Err(ParseError::InvalidSyntax(
                "only requires at least one identifier".to_string(),
            ));
        }

        let import_set = Box::new(Self::parse_import_set_tagged(list[1], heap)?);
        let h = heap.borrow();
        let mut identifiers = Vec::new();

        for &id_tv in &list[2..] {
            if let Some(name) = h.get_symbol_name(id_tv) {
                identifiers.push(name.to_string());
            } else {
                return Err(ParseError::InvalidSyntax(
                    "only identifiers must be symbols".to_string(),
                ));
            }
        }

        Ok(ImportSet::Only {
            import_set,
            identifiers,
        })
    }

    /// Parse (except <import-set> id1 id2 ...)
    fn parse_except_import_tagged(
        list: &[TaggedValue],
        heap: &SharedHeap,
    ) -> Result<ImportSet, ParseError> {
        if list.len() < 3 {
            return Err(ParseError::InvalidSyntax(
                "except requires at least one identifier".to_string(),
            ));
        }

        let import_set = Box::new(Self::parse_import_set_tagged(list[1], heap)?);
        let h = heap.borrow();
        let mut identifiers = Vec::new();

        for &id_tv in &list[2..] {
            if let Some(name) = h.get_symbol_name(id_tv) {
                identifiers.push(name.to_string());
            } else {
                return Err(ParseError::InvalidSyntax(
                    "except identifiers must be symbols".to_string(),
                ));
            }
        }

        Ok(ImportSet::Except {
            import_set,
            identifiers,
        })
    }

    /// Parse (prefix <import-set> prefix-id)
    fn parse_prefix_import_tagged(
        list: &[TaggedValue],
        heap: &SharedHeap,
    ) -> Result<ImportSet, ParseError> {
        if list.len() != 3 {
            return Err(ParseError::InvalidSyntax(
                "prefix requires exactly 2 arguments".to_string(),
            ));
        }

        let import_set = Box::new(Self::parse_import_set_tagged(list[1], heap)?);
        let prefix = {
            let h = heap.borrow();
            h.get_symbol_name(list[2])
                .ok_or_else(|| ParseError::InvalidSyntax("prefix must be a symbol".to_string()))?
                .to_string()
        };

        Ok(ImportSet::Prefix { import_set, prefix })
    }

    /// Parse (rename <import-set> (old1 new1) (old2 new2) ...)
    fn parse_rename_import_tagged(
        list: &[TaggedValue],
        heap: &SharedHeap,
    ) -> Result<ImportSet, ParseError> {
        if list.len() < 3 {
            return Err(ParseError::InvalidSyntax(
                "rename requires at least one rename pair".to_string(),
            ));
        }

        let import_set = Box::new(Self::parse_import_set_tagged(list[1], heap)?);
        let mut renames = Vec::new();

        for &pair_tv in &list[2..] {
            let pair = tagged_list_to_vec(pair_tv, heap)?;

            if pair.len() != 2 {
                return Err(ParseError::InvalidSyntax(
                    "rename pair must have exactly 2 elements".to_string(),
                ));
            }

            let h = heap.borrow();
            let old = h
                .get_symbol_name(pair[0])
                .ok_or_else(|| {
                    ParseError::InvalidSyntax("rename old name must be a symbol".to_string())
                })?
                .to_string();

            let new = h
                .get_symbol_name(pair[1])
                .ok_or_else(|| {
                    ParseError::InvalidSyntax("rename new name must be a symbol".to_string())
                })?
                .to_string();

            renames.push((old, new));
        }

        Ok(ImportSet::Rename {
            import_set,
            renames,
        })
    }

    /// Helper: expect a specific symbol in a TaggedValue
    fn expect_symbol_tagged(
        tv: TaggedValue,
        heap: &SharedHeap,
        expected: &str,
    ) -> Result<(), ParseError> {
        let h = heap.borrow();
        if let Some(name) = h.get_symbol_name(tv) {
            if name == expected {
                Ok(())
            } else {
                Err(ParseError::InvalidSyntax(format!(
                    "Expected '{}', got '{}'",
                    expected, name
                )))
            }
        } else {
            Err(ParseError::InvalidSyntax(format!(
                "Expected symbol '{}', got: {}",
                expected,
                h.type_name(tv)
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_heap() -> SharedHeap {
        patina_core::new_shared_heap()
    }

    fn sym(s: &str, heap: &SharedHeap) -> TaggedValue {
        heap.borrow_mut().intern_symbol(s)
    }

    fn fixnum(n: i64) -> TaggedValue {
        TaggedValue::fixnum(n)
    }

    fn str_val(s: &str, heap: &SharedHeap) -> TaggedValue {
        heap.borrow_mut().alloc_string(s.to_string())
    }

    fn list(items: Vec<TaggedValue>, heap: &SharedHeap) -> TaggedValue {
        heap.borrow_mut().list_from_iter(items)
    }

    #[test]
    fn test_parse_simple_library() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(
                    vec![sym("export", &heap), sym("foo", &heap), sym("bar", &heap)],
                    &heap,
                ),
                list(
                    vec![
                        sym("import", &heap),
                        list(vec![sym("scheme", &heap), sym("base", &heap)], &heap),
                    ],
                    &heap,
                ),
                list(vec![sym("begin", &heap)], &heap),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();

        assert_eq!(parsed.name, vec!["test", "lib"]);
        assert_eq!(parsed.exports.len(), 2);
        assert_eq!(parsed.imports.len(), 1);
    }

    #[test]
    fn test_parse_export_rename() {
        let heap = test_heap();
        let export_spec = list(
            vec![
                sym("rename", &heap),
                sym("internal-name", &heap),
                sym("external-name", &heap),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::parse_export_spec_tagged(export_spec, &heap).unwrap();

        assert_eq!(
            parsed,
            ExportSpec::Rename {
                internal: "internal-name".to_string(),
                external: "external-name".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_import_only() {
        let heap = test_heap();
        let import_set = list(
            vec![
                sym("only", &heap),
                list(vec![sym("scheme", &heap), sym("base", &heap)], &heap),
                sym("car", &heap),
                sym("cdr", &heap),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::parse_import_set_tagged(import_set, &heap).unwrap();

        match parsed {
            ImportSet::Only { identifiers, .. } => {
                assert_eq!(identifiers, vec!["car", "cdr"]);
            }
            _ => panic!("Expected Only variant"),
        }
    }

    #[test]
    fn test_parse_import_prefix() {
        let heap = test_heap();
        let import_set = list(
            vec![
                sym("prefix", &heap),
                list(vec![sym("scheme", &heap), sym("base", &heap)], &heap),
                sym("scheme:", &heap),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::parse_import_set_tagged(import_set, &heap).unwrap();

        match parsed {
            ImportSet::Prefix { prefix, .. } => {
                assert_eq!(prefix, "scheme:");
            }
            _ => panic!("Expected Prefix variant"),
        }
    }

    #[test]
    fn test_unknown_declaration_skipped_leniently() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                // A vendor-specific declaration Patina does not know.
                list(vec![sym("declare", &heap), sym("pure", &heap)], &heap),
                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
                list(vec![sym("begin", &heap), sym("foo-def", &heap)], &heap),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap)
            .expect("unknown declaration must not abort the load");

        // The unknown clause is dropped; everything around it survives.
        assert_eq!(parsed.name, vec!["test", "lib"]);
        assert_eq!(parsed.exports.len(), 1);
        assert_eq!(parsed.body_elements.len(), 1);
    }

    // =========================================================================
    // Integer Library Names (R7RS §5.6)
    // =========================================================================

    #[test]
    fn test_parse_library_name_with_integer() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("srfi", &heap), fixnum(1)], &heap),
                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.name, vec!["srfi", "1"]);
    }

    #[test]
    fn test_parse_library_name_integer_in_middle() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(
                    vec![sym("srfi", &heap), fixnum(1), sym("lists", &heap)],
                    &heap,
                ),
                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.name, vec!["srfi", "1", "lists"]);
    }

    #[test]
    fn test_parse_library_name_multiple_integers() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(
                    vec![sym("lib", &heap), fixnum(1), fixnum(2), fixnum(3)],
                    &heap,
                ),
                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.name, vec!["lib", "1", "2", "3"]);
    }

    #[test]
    fn test_parse_library_name_zero() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("lib", &heap), fixnum(0)], &heap),
                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.name, vec!["lib", "0"]);
    }

    #[test]
    fn test_parse_library_name_large_integer() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("srfi", &heap), fixnum(125)], &heap),
                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.name, vec!["srfi", "125"]);
    }

    #[test]
    fn test_parse_library_name_negative_integer_rejected() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), fixnum(-1)], &heap),
                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
            ],
            &heap,
        );

        let result = LibraryDefinition::from_tagged(lib_def, &heap);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("non-negative"),
            "Error should mention non-negative: {}",
            err
        );
    }

    #[test]
    fn test_parse_import_set_with_integer() {
        let heap = test_heap();
        let import_set = list(vec![sym("srfi", &heap), fixnum(1)], &heap);

        let parsed = LibraryDefinition::parse_import_set_tagged(import_set, &heap).unwrap();

        match parsed {
            ImportSet::Library(name) => {
                assert_eq!(name, vec!["srfi", "1"]);
            }
            _ => panic!("Expected Library variant"),
        }
    }

    #[test]
    fn test_parse_import_only_with_integer_library() {
        let heap = test_heap();
        let import_set = list(
            vec![
                sym("only", &heap),
                list(vec![sym("srfi", &heap), fixnum(1)], &heap),
                sym("xcons", &heap),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::parse_import_set_tagged(import_set, &heap).unwrap();

        match parsed {
            ImportSet::Only {
                import_set,
                identifiers,
            } => {
                match *import_set {
                    ImportSet::Library(name) => {
                        assert_eq!(name, vec!["srfi", "1"]);
                    }
                    _ => panic!("Expected Library variant in Only"),
                }
                assert_eq!(identifiers, vec!["xcons"]);
            }
            _ => panic!("Expected Only variant"),
        }
    }

    // =========================================================================
    // Include Declaration (R7RS §5.6.1)
    // =========================================================================

    #[test]
    fn test_parse_include_single_file() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
                list(
                    vec![sym("include", &heap), str_val("impl.scm", &heap)],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.name, vec!["test", "lib"]);
        assert_eq!(parsed.body_elements.len(), 1);

        match &parsed.body_elements[0] {
            BodyElement::Include {
                paths,
                case_insensitive,
            } => {
                assert_eq!(paths, &vec!["impl.scm".to_string()]);
                assert!(!case_insensitive);
            }
            _ => panic!("Expected Include variant"),
        }
    }

    #[test]
    fn test_parse_include_multiple_files() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(
                    vec![
                        sym("include", &heap),
                        str_val("a.scm", &heap),
                        str_val("b.scm", &heap),
                        str_val("c.scm", &heap),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();

        match &parsed.body_elements[0] {
            BodyElement::Include { paths, .. } => {
                assert_eq!(
                    paths,
                    &vec![
                        "a.scm".to_string(),
                        "b.scm".to_string(),
                        "c.scm".to_string()
                    ]
                );
            }
            _ => panic!("Expected Include variant"),
        }
    }

    #[test]
    fn test_parse_include_with_subdirectory() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("srfi", &heap), fixnum(1)], &heap),
                list(
                    vec![sym("include", &heap), str_val("1/predicates.scm", &heap)],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();

        match &parsed.body_elements[0] {
            BodyElement::Include { paths, .. } => {
                assert_eq!(paths, &vec!["1/predicates.scm".to_string()]);
            }
            _ => panic!("Expected Include variant"),
        }
    }

    #[test]
    fn test_parse_include_ci() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(
                    vec![sym("include-ci", &heap), str_val("legacy.scm", &heap)],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();

        match &parsed.body_elements[0] {
            BodyElement::Include {
                paths,
                case_insensitive,
            } => {
                assert_eq!(paths, &vec!["legacy.scm".to_string()]);
                assert!(case_insensitive);
            }
            _ => panic!("Expected Include variant"),
        }
    }

    #[test]
    fn test_parse_include_empty_rejected() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(vec![sym("include", &heap)], &heap),
            ],
            &heap,
        );

        let result = LibraryDefinition::from_tagged(lib_def, &heap);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("at least one filename")
        );
    }

    #[test]
    fn test_parse_include_non_string_rejected() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(vec![sym("include", &heap), sym("foo", &heap)], &heap),
            ],
            &heap,
        );

        let result = LibraryDefinition::from_tagged(lib_def, &heap);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be a string"));
    }

    #[test]
    fn test_parse_mixed_begin_and_include() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
                list(vec![sym("begin", &heap), sym("expr1", &heap)], &heap),
                list(
                    vec![sym("include", &heap), str_val("impl.scm", &heap)],
                    &heap,
                ),
                list(vec![sym("begin", &heap), sym("expr2", &heap)], &heap),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.body_elements.len(), 3);

        assert!(matches!(&parsed.body_elements[0], BodyElement::Begin(_)));
        assert!(matches!(
            &parsed.body_elements[1],
            BodyElement::Include { .. }
        ));
        assert!(matches!(&parsed.body_elements[2], BodyElement::Begin(_)));
    }

    #[test]
    fn test_parse_body_elements_order_preserved() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("order", &heap)], &heap),
                list(vec![sym("begin", &heap), sym("first", &heap)], &heap),
                list(
                    vec![sym("include", &heap), str_val("second.scm", &heap)],
                    &heap,
                ),
                list(vec![sym("begin", &heap), sym("third", &heap)], &heap),
                list(
                    vec![
                        sym("include", &heap),
                        str_val("fourth.scm", &heap),
                        str_val("fifth.scm", &heap),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.body_elements.len(), 4);

        // Verify order is preserved
        let h = heap.borrow();
        match &parsed.body_elements[0] {
            BodyElement::Begin(exprs) => {
                assert_eq!(h.get_symbol_name(exprs[0]), Some("first"));
            }
            _ => panic!("Expected Begin"),
        }

        match &parsed.body_elements[1] {
            BodyElement::Include { paths, .. } => {
                assert_eq!(paths[0], "second.scm");
            }
            _ => panic!("Expected Include"),
        }

        match &parsed.body_elements[2] {
            BodyElement::Begin(exprs) => {
                assert_eq!(h.get_symbol_name(exprs[0]), Some("third"));
            }
            _ => panic!("Expected Begin"),
        }

        match &parsed.body_elements[3] {
            BodyElement::Include { paths, .. } => {
                assert_eq!(paths.len(), 2);
                assert_eq!(paths[0], "fourth.scm");
                assert_eq!(paths[1], "fifth.scm");
            }
            _ => panic!("Expected Include"),
        }
    }

    // =========================================================================
    // cond-expand Declaration (R7RS §5.6.1)
    // =========================================================================

    #[test]
    fn test_cond_expand_simple_feature() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(
                    vec![
                        sym("cond-expand", &heap),
                        list(
                            vec![
                                sym("r7rs", &heap),
                                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
                            ],
                            &heap,
                        ),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.exports.len(), 1);
        assert!(matches!(&parsed.exports[0], ExportSpec::Identifier(s) if s == "foo"));
    }

    #[test]
    fn test_cond_expand_patina_feature() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(
                    vec![
                        sym("cond-expand", &heap),
                        list(
                            vec![
                                sym("patina", &heap),
                                list(vec![sym("export", &heap), sym("bar", &heap)], &heap),
                            ],
                            &heap,
                        ),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.exports.len(), 1);
        assert!(matches!(&parsed.exports[0], ExportSpec::Identifier(s) if s == "bar"));
    }

    #[test]
    fn test_cond_expand_else_clause() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(
                    vec![
                        sym("cond-expand", &heap),
                        list(
                            vec![
                                sym("nonexistent", &heap),
                                list(vec![sym("export", &heap), sym("bad", &heap)], &heap),
                            ],
                            &heap,
                        ),
                        list(
                            vec![
                                sym("else", &heap),
                                list(vec![sym("export", &heap), sym("good", &heap)], &heap),
                            ],
                            &heap,
                        ),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.exports.len(), 1);
        assert!(matches!(&parsed.exports[0], ExportSpec::Identifier(s) if s == "good"));
    }

    #[test]
    fn test_cond_expand_no_match() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(
                    vec![
                        sym("cond-expand", &heap),
                        list(
                            vec![
                                sym("nonexistent", &heap),
                                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
                            ],
                            &heap,
                        ),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.exports.len(), 0);
    }

    #[test]
    fn test_cond_expand_multiple_declarations() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(
                    vec![
                        sym("cond-expand", &heap),
                        list(
                            vec![
                                sym("r7rs", &heap),
                                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
                                list(vec![sym("export", &heap), sym("bar", &heap)], &heap),
                            ],
                            &heap,
                        ),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.exports.len(), 2);
    }

    #[test]
    fn test_cond_expand_and_requirement() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(
                    vec![
                        sym("cond-expand", &heap),
                        list(
                            vec![
                                list(
                                    vec![
                                        sym("and", &heap),
                                        sym("r7rs", &heap),
                                        sym("patina", &heap),
                                    ],
                                    &heap,
                                ),
                                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
                            ],
                            &heap,
                        ),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.exports.len(), 1);
    }

    #[test]
    fn test_cond_expand_or_requirement() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(
                    vec![
                        sym("cond-expand", &heap),
                        list(
                            vec![
                                list(
                                    vec![
                                        sym("or", &heap),
                                        sym("nonexistent", &heap),
                                        sym("r7rs", &heap),
                                    ],
                                    &heap,
                                ),
                                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
                            ],
                            &heap,
                        ),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.exports.len(), 1);
    }

    #[test]
    fn test_cond_expand_not_requirement() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(
                    vec![
                        sym("cond-expand", &heap),
                        list(
                            vec![
                                list(vec![sym("not", &heap), sym("nonexistent", &heap)], &heap),
                                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
                            ],
                            &heap,
                        ),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.exports.len(), 1);
    }

    #[test]
    fn test_cond_expand_with_begin() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(vec![sym("export", &heap), sym("x", &heap)], &heap),
                list(
                    vec![
                        sym("cond-expand", &heap),
                        list(
                            vec![
                                sym("r7rs", &heap),
                                list(
                                    vec![
                                        sym("begin", &heap),
                                        list(
                                            vec![sym("define", &heap), sym("x", &heap), fixnum(1)],
                                            &heap,
                                        ),
                                    ],
                                    &heap,
                                ),
                            ],
                            &heap,
                        ),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.body_elements.len(), 1);
        assert!(matches!(&parsed.body_elements[0], BodyElement::Begin(_)));
    }

    #[test]
    fn test_cond_expand_with_include() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(vec![sym("export", &heap), sym("foo", &heap)], &heap),
                list(
                    vec![
                        sym("cond-expand", &heap),
                        list(
                            vec![
                                sym("r7rs", &heap),
                                list(
                                    vec![sym("include", &heap), str_val("impl.scm", &heap)],
                                    &heap,
                                ),
                            ],
                            &heap,
                        ),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.body_elements.len(), 1);
        match &parsed.body_elements[0] {
            BodyElement::Include { paths, .. } => {
                assert_eq!(paths[0], "impl.scm");
            }
            _ => panic!("Expected Include"),
        }
    }

    #[test]
    fn test_cond_expand_first_match_wins() {
        let heap = test_heap();
        let lib_def = list(
            vec![
                sym("define-library", &heap),
                list(vec![sym("test", &heap), sym("lib", &heap)], &heap),
                list(
                    vec![
                        sym("cond-expand", &heap),
                        list(
                            vec![
                                sym("r7rs", &heap),
                                list(vec![sym("export", &heap), sym("first", &heap)], &heap),
                            ],
                            &heap,
                        ),
                        list(
                            vec![
                                sym("patina", &heap),
                                list(vec![sym("export", &heap), sym("second", &heap)], &heap),
                            ],
                            &heap,
                        ),
                    ],
                    &heap,
                ),
            ],
            &heap,
        );

        let parsed = LibraryDefinition::from_tagged(lib_def, &heap).unwrap();
        assert_eq!(parsed.exports.len(), 1);
        assert!(matches!(&parsed.exports[0], ExportSpec::Identifier(s) if s == "first"));
    }
}
