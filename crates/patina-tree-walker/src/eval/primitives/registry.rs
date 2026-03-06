//! Primitive procedure registry for runtime-extensible primitives
//!
//! This module provides a registry-based system for primitive procedures that enables:
//! - Runtime registration of new primitives (for plugins/extensions)
//! - Automatic arity checking
//! - Introspection and help system support
//! - Centralized primitive management

use super::super::{EvalError, EvalResult, Evaluator};
use patina_core::TaggedValue;
use patina_runtime::Arity;
use std::collections::HashMap;

/// Handler function type for primitives that take `Vec<TaggedValue>` directly
pub type TaggedHandler = fn(&Evaluator, Vec<TaggedValue>, bool) -> Result<EvalResult, EvalError>;

/// A primitive procedure with full metadata
///
/// This structure contains everything needed to call a primitive:
/// - Library namespace (e.g., "scheme.base", "patina.debug")
/// - Name for identification (e.g., "+", "car", "string-append")
/// - Arity specification for automatic checking
/// - Help text for documentation
/// - Handler function that performs the operation
pub struct PrimitiveFn {
    /// The library namespace this primitive belongs to
    /// Examples: "scheme.base", "scheme.char", "patina.debug"
    pub library: &'static str,

    /// The name of the primitive (e.g., "+", "car", "string-append")
    pub name: &'static str,

    /// The arity specification (exact count, at least N, or range)
    pub arity: Arity,

    /// Help text describing what this primitive does
    ///
    /// This will be used by the help system (e.g., `(help '+)`)
    #[allow(dead_code)] // Will be used by future help system
    pub help: &'static str,

    /// The handler function that implements this primitive (takes Vec<TaggedValue>)
    pub tagged_handler: TaggedHandler,
}

impl PrimitiveFn {
    /// Create a new primitive function with a TaggedValue handler
    ///
    /// # Arguments
    /// * `library` - The library namespace (e.g., "scheme.base")
    /// * `name` - The primitive name (e.g., "+")
    /// * `arity` - Arity specification
    /// * `help` - Help text for documentation
    /// * `tagged_handler` - The implementation function (takes Vec<TaggedValue>)
    pub fn new_tagged(
        library: &'static str,
        name: &'static str,
        arity: Arity,
        help: &'static str,
        tagged_handler: TaggedHandler,
    ) -> Self {
        PrimitiveFn {
            library,
            name,
            arity,
            help,
            tagged_handler,
        }
    }

    /// Get the fully qualified name of this primitive
    ///
    /// Returns: "library/name" (e.g., "scheme.base/+")
    pub fn qualified_name(&self) -> String {
        format!("{}/{}", self.library, self.name)
    }

    /// Check if the given argument count matches this primitive's arity
    pub fn check_arity(&self, arg_count: usize) -> Result<(), EvalError> {
        match self.arity {
            Arity::Exact(n) => {
                if arg_count != n {
                    return Err(EvalError::InvalidSyntax(format!(
                        "{} expects exactly {} arguments, got {}",
                        self.name, n, arg_count
                    )));
                }
            }
            Arity::Min(n) => {
                if arg_count < n {
                    return Err(EvalError::InvalidSyntax(format!(
                        "{} expects at least {} arguments, got {}",
                        self.name, n, arg_count
                    )));
                }
            }
            Arity::Range(min, max) => {
                if arg_count < min || arg_count > max {
                    return Err(EvalError::InvalidSyntax(format!(
                        "{} expects {}-{} arguments, got {}",
                        self.name, min, max, arg_count
                    )));
                }
            }
        }
        Ok(())
    }

    /// Call this primitive with TaggedValue arguments
    pub fn call_tagged(
        &self,
        evaluator: &Evaluator,
        args: Vec<TaggedValue>,
        in_tail: bool,
    ) -> Result<EvalResult, EvalError> {
        self.check_arity(args.len())?;
        (self.tagged_handler)(evaluator, args, in_tail)
    }
}

/// Registry for primitive procedures
///
/// This registry manages all primitive procedures available in the interpreter.
/// Primitives are stored with their fully qualified names (library/name) and can be
/// looked up by either qualified or unqualified names.
///
/// Primitives can be registered at initialization time or dynamically at runtime.
///
/// ## Performance note
///
/// Variable lookup yields a `Procedure::Primitive { qualified_name, .. }` where
/// `qualified_name` is from the internal library system (e.g. `"patina.internal.numbers/+"`)
/// which differs from the registry key (e.g. `"scheme.base/+"`). The `name_index` maps
/// each short name to its canonical qualified name for O(1) fallback lookup without
/// scanning all 300+ entries.
pub struct PrimitiveRegistry {
    /// Map from fully qualified name (library/name) to primitive function
    /// Example: "scheme.base/+" -> PrimitiveFn { ... }
    primitives: HashMap<String, PrimitiveFn>,
    /// O(1) index: short name -> canonical qualified name.
    /// First registration for a given name wins (earlier registrations shadow later ones).
    name_index: HashMap<&'static str, String>,
}

impl PrimitiveRegistry {
    /// Create a new empty primitive registry
    pub fn new() -> Self {
        PrimitiveRegistry {
            primitives: HashMap::new(),
            name_index: HashMap::new(),
        }
    }

    /// Register a primitive procedure
    ///
    /// The primitive is stored using its fully qualified name (library/name).
    /// If a primitive with the same qualified name already exists, it will be replaced.
    /// This enables primitive overriding for testing or customization.
    pub fn register(&mut self, primitive: PrimitiveFn) {
        let qualified_name = primitive.qualified_name();
        // Index the short name for O(1) fallback lookup. First registration wins.
        self.name_index
            .entry(primitive.name)
            .or_insert_with(|| qualified_name.clone());
        self.primitives.insert(qualified_name, primitive);
    }

    /// Register multiple primitives at once
    #[allow(dead_code)] // Part of public API, used during initialization
    pub fn register_all(&mut self, primitives: Vec<PrimitiveFn>) {
        for primitive in primitives {
            self.register(primitive);
        }
    }

    /// Get a primitive by qualified name (library/name)
    ///
    /// Example: `registry.get("scheme.base/+")`
    pub fn get(&self, qualified_name: &str) -> Option<&PrimitiveFn> {
        self.primitives.get(qualified_name)
    }

    /// Get a primitive by unqualified name (O(1) via name_index)
    ///
    /// This searches for any primitive with the given short name, regardless of
    /// which library it's registered under. Useful for internal libraries that
    /// re-export primitives without needing to know the original library.
    ///
    /// Example: `registry.get_by_name("+")` finds "scheme.base/+"
    pub fn get_by_name(&self, name: &str) -> Option<&PrimitiveFn> {
        let qn = self.name_index.get(name)?;
        self.primitives.get(qn.as_str())
    }

    /// Get a primitive by unqualified name from a specific library
    ///
    /// Example: `registry.get_from_library("scheme.base", "+")`
    #[allow(dead_code)] // Part of public API for library introspection
    pub fn get_from_library(&self, library: &str, name: &str) -> Option<&PrimitiveFn> {
        let qualified = format!("{}/{}", library, name);
        self.primitives.get(&qualified)
    }

    /// Get all primitives from a specific library
    ///
    /// Returns an iterator over all primitives whose library matches.
    /// Example: `registry.get_library_primitives("scheme.base")`
    #[allow(dead_code)] // Will be used for library building and introspection
    pub fn get_library_primitives(&self, library: &str) -> impl Iterator<Item = &PrimitiveFn> {
        let prefix = format!("{}/", library);
        self.primitives
            .iter()
            .filter_map(move |(qualified_name, prim_fn)| {
                if qualified_name.starts_with(&prefix) {
                    Some(prim_fn)
                } else {
                    None
                }
            })
    }

    /// Check if a primitive with the given qualified name exists
    #[allow(dead_code)] // Part of public API for introspection
    pub fn contains(&self, qualified_name: &str) -> bool {
        self.primitives.contains_key(qualified_name)
    }

    /// Get the number of registered primitives
    #[allow(dead_code)] // Part of public API for introspection
    pub fn len(&self) -> usize {
        self.primitives.len()
    }

    /// Check if the registry is empty
    #[allow(dead_code)] // Part of public API for introspection
    pub fn is_empty(&self) -> bool {
        self.primitives.is_empty()
    }

    /// List all registered primitive names (sorted)
    ///
    /// This is useful for auto-completion and help systems.
    #[allow(dead_code)] // Will be used for auto-completion
    pub fn list_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.primitives.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// Get all primitives (for iteration)
    #[allow(dead_code)] // Part of public API for introspection
    pub fn primitives(&self) -> impl Iterator<Item = &PrimitiveFn> {
        self.primitives.values()
    }

    /// Apply a primitive procedure with TaggedValue arguments
    ///
    /// This is the preferred entry point for CPS evaluation. If the primitive has
    /// a tagged_handler, it's called directly without conversion. Otherwise, args
    /// are converted to Value and the Value handler is called.
    pub fn apply_tagged(
        &self,
        qualified_name: &str,
        args: Vec<TaggedValue>,
        evaluator: &Evaluator,
        in_tail: bool,
    ) -> Result<EvalResult, EvalError> {
        let primitive = self.lookup_primitive(qualified_name)?;
        primitive.call_tagged(evaluator, args, in_tail)
    }

    /// Internal helper to look up a primitive by qualified or unqualified name.
    ///
    /// Fast path: qualified name matches the registry key directly.
    /// Fallback: extract the short name and use the O(1) name_index.
    fn lookup_primitive(&self, qualified_name: &str) -> Result<&PrimitiveFn, EvalError> {
        // Fast path: qualified name matches (e.g. "scheme.base/+")
        if let Some(prim) = self.get(qualified_name) {
            return Ok(prim);
        }

        // Fallback: extract "name" from "library/name" and use the O(1) name_index.
        // split_once is used to correctly handle "/" as a primitive name (stored as "scheme.base//").
        let name = match qualified_name.split_once('/') {
            Some((_, n)) => n,
            None => qualified_name,
        };
        self.get_by_name(name)
            .ok_or_else(|| EvalError::UndefinedVariable(qualified_name.to_string()))
    }
}

impl Default for PrimitiveRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper primitive for testing - uses TaggedValue
    fn test_add_tagged(
        _eval: &Evaluator,
        args: Vec<TaggedValue>,
        _tail: bool,
    ) -> Result<EvalResult, EvalError> {
        let sum: i64 = args
            .into_iter()
            .map(|tv| {
                tv.as_fixnum()
                    .ok_or_else(|| EvalError::TypeError("Expected integer".to_string()))
            })
            .collect::<Result<Vec<i64>, EvalError>>()?
            .into_iter()
            .sum();

        Ok(EvalResult::Tagged(TaggedValue::fixnum(sum)))
    }

    #[test]
    fn test_primitive_fn_creation() {
        let prim = PrimitiveFn::new_tagged(
            "scheme.base",
            "+",
            Arity::Min(0),
            "Add numbers",
            test_add_tagged,
        );

        assert_eq!(prim.library, "scheme.base");
        assert_eq!(prim.name, "+");
        assert_eq!(prim.help, "Add numbers");
        assert_eq!(prim.qualified_name(), "scheme.base/+");
    }

    #[test]
    fn test_arity_checking() {
        let prim = PrimitiveFn::new_tagged(
            "scheme.base",
            "+",
            Arity::Exact(2),
            "Add two numbers",
            test_add_tagged,
        );

        assert!(prim.check_arity(2).is_ok());
        assert!(prim.check_arity(1).is_err());
        assert!(prim.check_arity(3).is_err());
    }

    #[test]
    fn test_registry_registration() {
        let mut registry = PrimitiveRegistry::new();

        let prim = PrimitiveFn::new_tagged(
            "scheme.base",
            "+",
            Arity::Min(0),
            "Add numbers",
            test_add_tagged,
        );
        registry.register(prim);

        assert_eq!(registry.len(), 1);
        assert!(registry.contains("scheme.base/+"));
        assert!(!registry.contains("scheme.base/-"));
    }

    #[test]
    fn test_registry_lookup() {
        let mut registry = PrimitiveRegistry::new();

        let prim = PrimitiveFn::new_tagged(
            "scheme.base",
            "+",
            Arity::Min(0),
            "Add numbers",
            test_add_tagged,
        );
        registry.register(prim);

        // Test qualified lookup
        let found = registry.get("scheme.base/+");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "+");

        let not_found = registry.get("scheme.base/-");
        assert!(not_found.is_none());

        // Test library-specific lookup
        let found_lib = registry.get_from_library("scheme.base", "+");
        assert!(found_lib.is_some());
        assert_eq!(found_lib.unwrap().name, "+");
    }

    #[test]
    fn test_registry_list_names() {
        let mut registry = PrimitiveRegistry::new();

        registry.register(PrimitiveFn::new_tagged(
            "scheme.base",
            "+",
            Arity::Min(0),
            "Add",
            test_add_tagged,
        ));
        registry.register(PrimitiveFn::new_tagged(
            "scheme.base",
            "car",
            Arity::Exact(1),
            "Car",
            test_add_tagged,
        ));
        registry.register(PrimitiveFn::new_tagged(
            "scheme.base",
            "*",
            Arity::Min(0),
            "Multiply",
            test_add_tagged,
        ));

        let names = registry.list_names();
        // Sorted alphabetically by qualified name
        assert_eq!(
            names,
            vec!["scheme.base/*", "scheme.base/+", "scheme.base/car"]
        );
    }

    #[test]
    fn test_get_library_primitives() {
        let mut registry = PrimitiveRegistry::new();

        registry.register(PrimitiveFn::new_tagged(
            "scheme.base",
            "+",
            Arity::Min(0),
            "Add",
            test_add_tagged,
        ));
        registry.register(PrimitiveFn::new_tagged(
            "scheme.base",
            "-",
            Arity::Min(1),
            "Subtract",
            test_add_tagged,
        ));
        registry.register(PrimitiveFn::new_tagged(
            "scheme.char",
            "char-upcase",
            Arity::Exact(1),
            "Upcase",
            test_add_tagged,
        ));

        let base_prims: Vec<_> = registry.get_library_primitives("scheme.base").collect();
        assert_eq!(base_prims.len(), 2);

        let char_prims: Vec<_> = registry.get_library_primitives("scheme.char").collect();
        assert_eq!(char_prims.len(), 1);
    }
}
