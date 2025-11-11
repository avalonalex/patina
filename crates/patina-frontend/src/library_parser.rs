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
//!     definitions and expressions))
//! ```

use crate::ParseError;
use patina_runtime::Value;

/// A parsed library definition
#[derive(Debug, Clone)]
pub struct LibraryDefinition {
    /// Library name as list of symbols: (scheme base) → ["scheme", "base"]
    pub name: Vec<String>,

    /// Export specifications
    pub exports: Vec<ExportSpec>,

    /// Import sets
    pub imports: Vec<ImportSet>,

    /// Library body (code to evaluate)
    pub body: Vec<Value>,
}

/// Export specification
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportSpec {
    /// Simple export: identifier
    Identifier(String),

    /// Renamed export: (rename internal-name external-name)
    Rename { internal: String, external: String },
}

/// Import set (R7RS 5.6.1)
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

impl LibraryDefinition {
    /// Parse a define-library form from a Value
    ///
    /// Expects a list: (define-library <name> <declaration>*)
    pub fn from_value(value: &Value) -> Result<Self, ParseError> {
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
        let mut body = Vec::new();

        for decl in &list[2..] {
            Self::parse_declaration(decl, &mut exports, &mut imports, &mut body)?;
        }

        Ok(LibraryDefinition {
            name,
            exports,
            imports,
            body,
        })
    }

    /// Parse a library name: (scheme base) → ["scheme", "base"]
    fn parse_library_name(value: &Value) -> Result<Vec<String>, ParseError> {
        let list = Self::expect_list(value)?;

        if list.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "Library name cannot be empty".to_string(),
            ));
        }

        let mut name = Vec::new();
        for part in list {
            if let Value::Symbol(s) = part {
                name.push(s.to_string());
            } else {
                return Err(ParseError::InvalidSyntax(format!(
                    "Library name parts must be symbols, got: {}",
                    part
                )));
            }
        }

        Ok(name)
    }

    /// Parse a library declaration
    fn parse_declaration(
        value: &Value,
        exports: &mut Vec<ExportSpec>,
        imports: &mut Vec<ImportSet>,
        body: &mut Vec<Value>,
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
                    body.extend_from_slice(&list[1..]);
                    Ok(())
                }
                "include" | "include-ci" | "include-library-declarations" | "cond-expand" => {
                    // TODO: Implement these
                    Err(ParseError::InvalidSyntax(format!(
                        "{} not yet implemented",
                        keyword
                    )))
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
    fn parse_import_set(value: &Value) -> Result<ImportSet, ParseError> {
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
        let mut current = value;

        loop {
            match current {
                Value::Null => return Ok(items),
                Value::Pair(pair) => {
                    items.push(pair.0.clone());
                    current = &pair.1;
                }
                _ => {
                    return Err(ParseError::InvalidSyntax(format!(
                        "Expected proper list, got improper list ending with: {}",
                        current
                    )))
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
    use std::rc::Rc;

    fn symbol(s: &str) -> Value {
        Value::Symbol(Rc::from(s))
    }

    fn list(items: Vec<Value>) -> Value {
        items
            .into_iter()
            .rev()
            .fold(Value::Null, |acc, item| Value::Pair(Rc::new((item, acc))))
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
}
