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
use patina_runtime::Value;

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
    Begin(Vec<Value>),

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
    /// Parse a define-library form from a Value
    ///
    /// Expects a list: (define-library <name> <declaration>*)
    ///
    /// Note: This method cannot check `(library <name>)` requirements in cond-expand.
    /// Use `from_value_with_library_checker` if you need library availability checks.
    pub fn from_value(value: &Value) -> Result<Self, ParseError> {
        // Default: no library checker available
        Self::from_value_with_library_checker(value, &|_| false)
    }

    /// Parse a define-library form with a library availability checker.
    ///
    /// The `can_load_library` callback is used for `(library <name>)` requirements
    /// in cond-expand clauses. It should return true if the named library can be loaded.
    ///
    /// Expects a list: (define-library <name> <declaration>*)
    pub fn from_value_with_library_checker(
        value: &Value,
        can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<Self, ParseError> {
        // Must be a list starting with 'define-library
        let list = Self::expect_list(value)?;

        if list.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "Empty define-library form".to_string(),
            ));
        }

        // First element must be the symbol 'define-library
        Self::expect_symbol(&list[0], "define-library")?;

        if list.len() < 2 {
            return Err(ParseError::InvalidSyntax(
                "define-library requires a library name".to_string(),
            ));
        }

        // Second element is the library name
        let name = Self::parse_library_name(&list[1])?;

        // Rest are declarations
        let mut exports = Vec::new();
        let mut imports = Vec::new();
        let mut body_elements = Vec::new();

        for decl in &list[2..] {
            Self::parse_declaration(
                decl,
                &mut exports,
                &mut imports,
                &mut body_elements,
                can_load_library,
            )?;
        }

        Ok(LibraryDefinition {
            name,
            exports,
            imports,
            body_elements,
        })
    }

    /// Parse a library name: (scheme base) → ["scheme", "base"]
    ///
    /// R7RS §5.6: Library names are lists of identifiers and exact non-negative integers.
    /// Examples: (scheme base), (srfi 1), (srfi 1 lists)
    fn parse_library_name(value: &Value) -> Result<Vec<String>, ParseError> {
        let list = Self::expect_list(value)?;

        if list.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "Library name cannot be empty".to_string(),
            ));
        }

        let mut name = Vec::new();
        for part in list {
            match &part {
                Value::Symbol(s) => name.push(s.to_string()),
                Value::Integer(n) if *n >= 0 => name.push(n.to_string()),
                _ => {
                    return Err(ParseError::InvalidSyntax(format!(
                        "Library name parts must be identifiers or non-negative integers, got: {}",
                        part
                    )));
                }
            }
        }

        Ok(name)
    }

    /// Parse a library declaration
    fn parse_declaration(
        value: &Value,
        exports: &mut Vec<ExportSpec>,
        imports: &mut Vec<ImportSet>,
        body_elements: &mut Vec<BodyElement>,
        can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<(), ParseError> {
        let list = Self::expect_list(value)?;

        if list.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "Empty library declaration".to_string(),
            ));
        }

        // First element determines the type
        if let Value::Symbol(keyword) = &list[0] {
            match keyword.as_ref() {
                "export" => {
                    // (export spec1 spec2 ...)
                    for spec_value in &list[1..] {
                        exports.push(Self::parse_export_spec(spec_value)?);
                    }
                    Ok(())
                }
                "import" => {
                    // (import set1 set2 ...)
                    for set_value in &list[1..] {
                        imports.push(Self::parse_import_set(set_value)?);
                    }
                    Ok(())
                }
                "begin" => {
                    // (begin expr1 expr2 ...)
                    body_elements.push(BodyElement::Begin(list[1..].to_vec()));
                    Ok(())
                }
                "include" => {
                    // (include "file1.scm" "file2.scm" ...)
                    let paths = Self::parse_include_paths(&list[1..])?;
                    body_elements.push(BodyElement::Include {
                        paths,
                        case_insensitive: false,
                    });
                    Ok(())
                }
                "include-ci" => {
                    // (include-ci "file1.scm" "file2.scm" ...)
                    let paths = Self::parse_include_paths(&list[1..])?;
                    body_elements.push(BodyElement::Include {
                        paths,
                        case_insensitive: true,
                    });
                    Ok(())
                }
                "cond-expand" => {
                    // (cond-expand clause1 clause2 ...)
                    // Each clause is (<feature-requirement> <declaration>*)
                    // or (else <declaration>*)
                    Self::parse_cond_expand(
                        &list[1..],
                        exports,
                        imports,
                        body_elements,
                        can_load_library,
                    )
                }
                "include-library-declarations" => {
                    // (include-library-declarations "file1.scm" "file2.scm" ...)
                    // The file contents are spliced as library declarations
                    let paths = Self::parse_include_paths(&list[1..])?;
                    body_elements.push(BodyElement::IncludeLibraryDeclarations { paths });
                    Ok(())
                }
                _ => Err(ParseError::InvalidSyntax(format!(
                    "Unknown library declaration: {}",
                    keyword
                ))),
            }
        } else {
            Err(ParseError::InvalidSyntax(
                "Library declaration must start with a symbol".to_string(),
            ))
        }
    }

    /// Parse include file paths from a list of string values
    fn parse_include_paths(values: &[Value]) -> Result<Vec<String>, ParseError> {
        if values.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "include requires at least one filename".to_string(),
            ));
        }

        let mut paths = Vec::new();
        for value in values {
            match value {
                Value::String(s) => {
                    // Convert Vec<char> back to String for file path
                    let path: String = s.borrow().iter().collect();
                    paths.push(path);
                }
                _ => {
                    return Err(ParseError::InvalidSyntax(format!(
                        "include filename must be a string, got: {}",
                        value
                    )));
                }
            }
        }
        Ok(paths)
    }

    /// Parse cond-expand declaration in library context
    ///
    /// R7RS §5.6.1: The cond-expand library declaration provides for
    /// conditionally including library declarations.
    ///
    /// Syntax: (cond-expand <clause>+)
    /// where <clause> is (<feature-requirement> <declaration>*)
    ///                or (else <declaration>*)
    fn parse_cond_expand(
        clauses: &[Value],
        exports: &mut Vec<ExportSpec>,
        imports: &mut Vec<ImportSet>,
        body_elements: &mut Vec<BodyElement>,
        can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<(), ParseError> {
        use crate::cond_expand::evaluate_feature_requirement;
        use patina_runtime::default_features;

        if clauses.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "cond-expand requires at least one clause".to_string(),
            ));
        }

        let features = default_features();

        for clause in clauses {
            let clause_list = Self::expect_list(clause)?;

            if clause_list.is_empty() {
                return Err(ParseError::InvalidSyntax(
                    "Empty cond-expand clause".to_string(),
                ));
            }

            // Check for else clause
            let is_else = matches!(&clause_list[0], Value::Symbol(s) if s.as_ref() == "else");

            let matches = if is_else {
                true
            } else {
                // Evaluate the feature requirement
                evaluate_feature_requirement(&clause_list[0], &features, can_load_library)?
            };

            if matches {
                // Splice in the matching declarations
                for decl in &clause_list[1..] {
                    Self::parse_declaration(
                        decl,
                        exports,
                        imports,
                        body_elements,
                        can_load_library,
                    )?;
                }
                return Ok(()); // Stop after first match
            }
        }

        // No clause matched - R7RS says this is an error if no else clause
        // But many implementations just continue silently. We'll do the same.
        Ok(())
    }

    /// Parse an export spec
    fn parse_export_spec(value: &Value) -> Result<ExportSpec, ParseError> {
        match value {
            Value::Symbol(id) => Ok(ExportSpec::Identifier(id.to_string())),
            Value::Pair(_) => {
                // (rename internal external)
                let list = Self::expect_list(value)?;

                if list.len() != 3 {
                    return Err(ParseError::InvalidSyntax(
                        "rename export requires exactly 3 elements".to_string(),
                    ));
                }

                Self::expect_symbol(&list[0], "rename")?;

                let internal = if let Value::Symbol(s) = &list[1] {
                    s.to_string()
                } else {
                    return Err(ParseError::InvalidSyntax(
                        "rename internal name must be a symbol".to_string(),
                    ));
                };

                let external = if let Value::Symbol(s) = &list[2] {
                    s.to_string()
                } else {
                    return Err(ParseError::InvalidSyntax(
                        "rename external name must be a symbol".to_string(),
                    ));
                };

                Ok(ExportSpec::Rename { internal, external })
            }
            _ => Err(ParseError::InvalidSyntax(format!(
                "Invalid export spec: {}",
                value
            ))),
        }
    }

    /// Parse an import set
    pub fn parse_import_set(value: &Value) -> Result<ImportSet, ParseError> {
        match value {
            Value::Pair(_) => {
                let list = Self::expect_list(value)?;

                if list.is_empty() {
                    return Err(ParseError::InvalidSyntax("Empty import set".to_string()));
                }

                // Check if it's a library name or an import modifier
                if let Value::Symbol(first) = &list[0] {
                    match first.as_ref() {
                        "only" => Self::parse_only_import(&list),
                        "except" => Self::parse_except_import(&list),
                        "prefix" => Self::parse_prefix_import(&list),
                        "rename" => Self::parse_rename_import(&list),
                        _ => {
                            // It's a library name
                            Self::parse_library_name(value).map(ImportSet::Library)
                        }
                    }
                } else {
                    // Not a symbol, try to parse as library name
                    Self::parse_library_name(value).map(ImportSet::Library)
                }
            }
            _ => Err(ParseError::InvalidSyntax(format!(
                "Invalid import set: {}",
                value
            ))),
        }
    }

    /// Parse (only <import-set> id1 id2 ...)
    fn parse_only_import(list: &[Value]) -> Result<ImportSet, ParseError> {
        if list.len() < 3 {
            return Err(ParseError::InvalidSyntax(
                "only requires at least one identifier".to_string(),
            ));
        }

        let import_set = Box::new(Self::parse_import_set(&list[1])?);
        let mut identifiers = Vec::new();

        for id_value in &list[2..] {
            if let Value::Symbol(id) = id_value {
                identifiers.push(id.to_string());
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
    fn parse_except_import(list: &[Value]) -> Result<ImportSet, ParseError> {
        if list.len() < 3 {
            return Err(ParseError::InvalidSyntax(
                "except requires at least one identifier".to_string(),
            ));
        }

        let import_set = Box::new(Self::parse_import_set(&list[1])?);
        let mut identifiers = Vec::new();

        for id_value in &list[2..] {
            if let Value::Symbol(id) = id_value {
                identifiers.push(id.to_string());
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
    fn parse_prefix_import(list: &[Value]) -> Result<ImportSet, ParseError> {
        if list.len() != 3 {
            return Err(ParseError::InvalidSyntax(
                "prefix requires exactly 2 arguments".to_string(),
            ));
        }

        let import_set = Box::new(Self::parse_import_set(&list[1])?);
        let prefix = if let Value::Symbol(p) = &list[2] {
            p.to_string()
        } else {
            return Err(ParseError::InvalidSyntax(
                "prefix must be a symbol".to_string(),
            ));
        };

        Ok(ImportSet::Prefix { import_set, prefix })
    }

    /// Parse (rename <import-set> (old1 new1) (old2 new2) ...)
    fn parse_rename_import(list: &[Value]) -> Result<ImportSet, ParseError> {
        if list.len() < 3 {
            return Err(ParseError::InvalidSyntax(
                "rename requires at least one rename pair".to_string(),
            ));
        }

        let import_set = Box::new(Self::parse_import_set(&list[1])?);
        let mut renames = Vec::new();

        for pair_value in &list[2..] {
            let pair = Self::expect_list(pair_value)?;

            if pair.len() != 2 {
                return Err(ParseError::InvalidSyntax(
                    "rename pair must have exactly 2 elements".to_string(),
                ));
            }

            let old = if let Value::Symbol(s) = &pair[0] {
                s.to_string()
            } else {
                return Err(ParseError::InvalidSyntax(
                    "rename old name must be a symbol".to_string(),
                ));
            };

            let new = if let Value::Symbol(s) = &pair[1] {
                s.to_string()
            } else {
                return Err(ParseError::InvalidSyntax(
                    "rename new name must be a symbol".to_string(),
                ));
            };

            renames.push((old, new));
        }

        Ok(ImportSet::Rename {
            import_set,
            renames,
        })
    }

    /// Helper: expect a list and return its elements
    fn expect_list(value: &Value) -> Result<Vec<Value>, ParseError> {
        let mut items = Vec::new();
        let mut current = value.clone();

        loop {
            match current {
                Value::Null => return Ok(items),
                Value::Pair(pair) => {
                    let borrowed = pair.borrow();
                    items.push(borrowed.0.clone());
                    current = borrowed.1.clone();
                }
                _ => {
                    return Err(ParseError::InvalidSyntax(format!(
                        "Expected proper list, got improper list ending with: {}",
                        current
                    )));
                }
            }
        }
    }

    /// Helper: expect a specific symbol
    fn expect_symbol(value: &Value, expected: &str) -> Result<(), ParseError> {
        if let Value::Symbol(s) = value {
            if s.as_ref() == expected {
                Ok(())
            } else {
                Err(ParseError::InvalidSyntax(format!(
                    "Expected '{}', got '{}'",
                    expected, s
                )))
            }
        } else {
            Err(ParseError::InvalidSyntax(format!(
                "Expected symbol '{}', got: {}",
                expected, value
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn symbol(s: &str) -> Value {
        Value::symbol(s)
    }

    fn list(items: Vec<Value>) -> Value {
        items.into_iter().rev().fold(Value::Null, |acc, item| {
            Value::Pair(Rc::new(RefCell::new((item, acc))))
        })
    }

    #[test]
    fn test_parse_simple_library() {
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![symbol("export"), symbol("foo"), symbol("bar")]),
            list(vec![
                symbol("import"),
                list(vec![symbol("scheme"), symbol("base")]),
            ]),
            list(vec![symbol("begin")]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();

        assert_eq!(parsed.name, vec!["test", "lib"]);
        assert_eq!(parsed.exports.len(), 2);
        assert_eq!(parsed.imports.len(), 1);
    }

    #[test]
    fn test_parse_export_rename() {
        let export_spec = list(vec![
            symbol("rename"),
            symbol("internal-name"),
            symbol("external-name"),
        ]);

        let parsed = LibraryDefinition::parse_export_spec(&export_spec).unwrap();

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
        let import_set = list(vec![
            symbol("only"),
            list(vec![symbol("scheme"), symbol("base")]),
            symbol("car"),
            symbol("cdr"),
        ]);

        let parsed = LibraryDefinition::parse_import_set(&import_set).unwrap();

        match parsed {
            ImportSet::Only { identifiers, .. } => {
                assert_eq!(identifiers, vec!["car", "cdr"]);
            }
            _ => panic!("Expected Only variant"),
        }
    }

    #[test]
    fn test_parse_import_prefix() {
        let import_set = list(vec![
            symbol("prefix"),
            list(vec![symbol("scheme"), symbol("base")]),
            symbol("scheme:"),
        ]);

        let parsed = LibraryDefinition::parse_import_set(&import_set).unwrap();

        match parsed {
            ImportSet::Prefix { prefix, .. } => {
                assert_eq!(prefix, "scheme:");
            }
            _ => panic!("Expected Prefix variant"),
        }
    }

    // =========================================================================
    // Integer Library Names (R7RS §5.6)
    // =========================================================================

    fn integer(n: i64) -> Value {
        Value::Integer(n)
    }

    #[test]
    fn test_parse_library_name_with_integer() {
        // (srfi 1) - standard SRFI naming
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("srfi"), integer(1)]),
            list(vec![symbol("export"), symbol("foo")]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.name, vec!["srfi", "1"]);
    }

    #[test]
    fn test_parse_library_name_integer_in_middle() {
        // (srfi 1 lists) - integer in the middle
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("srfi"), integer(1), symbol("lists")]),
            list(vec![symbol("export"), symbol("foo")]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.name, vec!["srfi", "1", "lists"]);
    }

    #[test]
    fn test_parse_library_name_multiple_integers() {
        // (lib 1 2 3) - multiple integers
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("lib"), integer(1), integer(2), integer(3)]),
            list(vec![symbol("export"), symbol("foo")]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.name, vec!["lib", "1", "2", "3"]);
    }

    #[test]
    fn test_parse_library_name_zero() {
        // (lib 0) - zero is valid non-negative integer
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("lib"), integer(0)]),
            list(vec![symbol("export"), symbol("foo")]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.name, vec!["lib", "0"]);
    }

    #[test]
    fn test_parse_library_name_large_integer() {
        // (srfi 125) - common SRFI numbers
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("srfi"), integer(125)]),
            list(vec![symbol("export"), symbol("foo")]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.name, vec!["srfi", "125"]);
    }

    #[test]
    fn test_parse_library_name_negative_integer_rejected() {
        // (test -1) - negative integers should be rejected per R7RS
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), integer(-1)]),
            list(vec![symbol("export"), symbol("foo")]),
        ]);

        let result = LibraryDefinition::from_value(&lib_def);
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
        // (import (srfi 1)) - integer in import
        let import_set = list(vec![symbol("srfi"), integer(1)]);

        let parsed = LibraryDefinition::parse_import_set(&import_set).unwrap();

        match parsed {
            ImportSet::Library(name) => {
                assert_eq!(name, vec!["srfi", "1"]);
            }
            _ => panic!("Expected Library variant"),
        }
    }

    #[test]
    fn test_parse_import_only_with_integer_library() {
        // (only (srfi 1) xcons) - integer library with only modifier
        let import_set = list(vec![
            symbol("only"),
            list(vec![symbol("srfi"), integer(1)]),
            symbol("xcons"),
        ]);

        let parsed = LibraryDefinition::parse_import_set(&import_set).unwrap();

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

    fn string(s: &str) -> Value {
        Value::String(Rc::new(RefCell::new(s.chars().collect())))
    }

    #[test]
    fn test_parse_include_single_file() {
        // (include "impl.scm")
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![symbol("export"), symbol("foo")]),
            list(vec![symbol("include"), string("impl.scm")]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
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
        // (include "a.scm" "b.scm" "c.scm")
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![
                symbol("include"),
                string("a.scm"),
                string("b.scm"),
                string("c.scm"),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();

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
        // (include "1/predicates.scm") - SRFI-1 pattern
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("srfi"), integer(1)]),
            list(vec![symbol("include"), string("1/predicates.scm")]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();

        match &parsed.body_elements[0] {
            BodyElement::Include { paths, .. } => {
                assert_eq!(paths, &vec!["1/predicates.scm".to_string()]);
            }
            _ => panic!("Expected Include variant"),
        }
    }

    #[test]
    fn test_parse_include_ci() {
        // (include-ci "legacy.scm")
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![symbol("include-ci"), string("legacy.scm")]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();

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
        // (include) - no files specified
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![symbol("include")]),
        ]);

        let result = LibraryDefinition::from_value(&lib_def);
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
        // (include foo) - symbol instead of string
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![symbol("include"), symbol("foo")]),
        ]);

        let result = LibraryDefinition::from_value(&lib_def);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be a string"));
    }

    #[test]
    fn test_parse_mixed_begin_and_include() {
        // Library with both begin and include declarations
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![symbol("export"), symbol("foo")]),
            list(vec![symbol("begin"), symbol("expr1")]),
            list(vec![symbol("include"), string("impl.scm")]),
            list(vec![symbol("begin"), symbol("expr2")]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.body_elements.len(), 3);

        // First: begin
        assert!(matches!(&parsed.body_elements[0], BodyElement::Begin(_)));
        // Second: include
        assert!(matches!(
            &parsed.body_elements[1],
            BodyElement::Include { .. }
        ));
        // Third: begin
        assert!(matches!(&parsed.body_elements[2], BodyElement::Begin(_)));
    }

    #[test]
    fn test_parse_body_elements_order_preserved() {
        // R7RS: expressions from begin/include are processed in order
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("order")]),
            list(vec![symbol("begin"), symbol("first")]),
            list(vec![symbol("include"), string("second.scm")]),
            list(vec![symbol("begin"), symbol("third")]),
            list(vec![
                symbol("include"),
                string("fourth.scm"),
                string("fifth.scm"),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.body_elements.len(), 4);

        // Verify order is preserved
        match &parsed.body_elements[0] {
            BodyElement::Begin(exprs) => {
                assert!(matches!(&exprs[0], Value::Symbol(s) if s.as_ref() == "first"));
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
                assert!(matches!(&exprs[0], Value::Symbol(s) if s.as_ref() == "third"));
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
        // (cond-expand (r7rs (export foo)))
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![
                symbol("cond-expand"),
                list(vec![
                    symbol("r7rs"),
                    list(vec![symbol("export"), symbol("foo")]),
                ]),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.exports.len(), 1);
        assert!(matches!(&parsed.exports[0], ExportSpec::Identifier(s) if s == "foo"));
    }

    #[test]
    fn test_cond_expand_patina_feature() {
        // (cond-expand (patina (export bar)))
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![
                symbol("cond-expand"),
                list(vec![
                    symbol("patina"),
                    list(vec![symbol("export"), symbol("bar")]),
                ]),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.exports.len(), 1);
        assert!(matches!(&parsed.exports[0], ExportSpec::Identifier(s) if s == "bar"));
    }

    #[test]
    fn test_cond_expand_else_clause() {
        // (cond-expand (nonexistent (export bad)) (else (export good)))
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![
                symbol("cond-expand"),
                list(vec![
                    symbol("nonexistent"),
                    list(vec![symbol("export"), symbol("bad")]),
                ]),
                list(vec![
                    symbol("else"),
                    list(vec![symbol("export"), symbol("good")]),
                ]),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.exports.len(), 1);
        assert!(matches!(&parsed.exports[0], ExportSpec::Identifier(s) if s == "good"));
    }

    #[test]
    fn test_cond_expand_no_match() {
        // (cond-expand (nonexistent (export foo))) - no match, no else
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![
                symbol("cond-expand"),
                list(vec![
                    symbol("nonexistent"),
                    list(vec![symbol("export"), symbol("foo")]),
                ]),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.exports.len(), 0); // No exports since clause didn't match
    }

    #[test]
    fn test_cond_expand_multiple_declarations() {
        // (cond-expand (r7rs (export foo) (export bar)))
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![
                symbol("cond-expand"),
                list(vec![
                    symbol("r7rs"),
                    list(vec![symbol("export"), symbol("foo")]),
                    list(vec![symbol("export"), symbol("bar")]),
                ]),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.exports.len(), 2);
    }

    #[test]
    fn test_cond_expand_and_requirement() {
        // (cond-expand ((and r7rs patina) (export foo)))
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![
                symbol("cond-expand"),
                list(vec![
                    list(vec![symbol("and"), symbol("r7rs"), symbol("patina")]),
                    list(vec![symbol("export"), symbol("foo")]),
                ]),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.exports.len(), 1);
    }

    #[test]
    fn test_cond_expand_or_requirement() {
        // (cond-expand ((or nonexistent r7rs) (export foo)))
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![
                symbol("cond-expand"),
                list(vec![
                    list(vec![symbol("or"), symbol("nonexistent"), symbol("r7rs")]),
                    list(vec![symbol("export"), symbol("foo")]),
                ]),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.exports.len(), 1);
    }

    #[test]
    fn test_cond_expand_not_requirement() {
        // (cond-expand ((not nonexistent) (export foo)))
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![
                symbol("cond-expand"),
                list(vec![
                    list(vec![symbol("not"), symbol("nonexistent")]),
                    list(vec![symbol("export"), symbol("foo")]),
                ]),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.exports.len(), 1);
    }

    #[test]
    fn test_cond_expand_with_begin() {
        // (cond-expand (r7rs (begin (define x 1))))
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![symbol("export"), symbol("x")]),
            list(vec![
                symbol("cond-expand"),
                list(vec![
                    symbol("r7rs"),
                    list(vec![
                        symbol("begin"),
                        list(vec![symbol("define"), symbol("x"), integer(1)]),
                    ]),
                ]),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.body_elements.len(), 1);
        assert!(matches!(&parsed.body_elements[0], BodyElement::Begin(_)));
    }

    #[test]
    fn test_cond_expand_with_include() {
        // (cond-expand (r7rs (include "impl.scm")))
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![symbol("export"), symbol("foo")]),
            list(vec![
                symbol("cond-expand"),
                list(vec![
                    symbol("r7rs"),
                    list(vec![symbol("include"), string("impl.scm")]),
                ]),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
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
        // (cond-expand (r7rs (export first)) (patina (export second)))
        // Both match, but first should win
        let lib_def = list(vec![
            symbol("define-library"),
            list(vec![symbol("test"), symbol("lib")]),
            list(vec![
                symbol("cond-expand"),
                list(vec![
                    symbol("r7rs"),
                    list(vec![symbol("export"), symbol("first")]),
                ]),
                list(vec![
                    symbol("patina"),
                    list(vec![symbol("export"), symbol("second")]),
                ]),
            ]),
        ]);

        let parsed = LibraryDefinition::from_value(&lib_def).unwrap();
        assert_eq!(parsed.exports.len(), 1);
        assert!(matches!(&parsed.exports[0], ExportSpec::Identifier(s) if s == "first"));
    }
}
