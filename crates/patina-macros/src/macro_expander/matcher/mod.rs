//! PVREF-based pattern matching (Version 2)
//!
//! This module implements pattern matching for the PVREF-based macro system.
//! It takes compiled Pattern structures and matches them against input expressions,
//! building MatchEnv trees that properly represent nested ellipsis bindings.
//!
//! Key improvements over the original matcher:
//! - Uses PVREF encoding for O(1) variable binding
//! - Uses MatchEnv tree structure for nested ellipsis
//! - Implements Gauche's num_following optimization to avoid backtracking
//! - Proper level tracking for nested patterns
//!
//! Inspired by Gauche's pattern matching (macro.c:600+)
//! Reference: https://github.com/shirok/Gauche/blob/master/src/macro.c

mod debug;
mod error;
mod list_match;
mod literal;

pub use error::MatchError;

use crate::macro_expander::pattern::Pattern;
use crate::macro_expander::utils::{pattern_to_string, pattern_to_string_with_names};
use patina_runtime::{LiteralBinding, MatchEnv, PVRef, ScopeSet, Value};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Pattern matcher for PVREF-based macro system
///
/// This implements the pattern matching phase of macro expansion.
/// It takes a compiled Pattern and an input expression, returning
/// a MatchEnv with all pattern variables bound.
///
/// Based on Gauche's pattern matching approach (macro.c:600+).
pub struct Matcher {
    /// Number of pattern variables (determines MatchEnv size)
    num_pvars: usize,

    /// Optional mapping from PVREF to variable names (for debug output)
    pvar_names: Option<HashMap<PVRef, Rc<str>>>,

    /// Names that are shadowed by local bindings at the macro use site.
    /// When a literal identifier (like `=>` in cond) is in this set,
    /// it should NOT match as a literal (R7RS 4.3.2).
    ///
    /// This is compile-time shadowing info from the desugarer's `shadowed_names`.
    shadowed_names: HashSet<Rc<str>>,

    /// Scopes from the use-site (set from the input identifier's scopes)
    /// Used to determine if an identifier's binding matches a literal's binding.
    use_site_scopes: ScopeSet,

    /// Literal identifiers from the macro definition with their binding information.
    /// Used with use_site_scopes to check if literals have the same binding.
    literals: Vec<LiteralBinding>,
}

impl Matcher {
    /// Create a new matcher
    ///
    /// # Arguments
    /// * `num_pvars` - Total number of pattern variables in the pattern
    pub fn new(num_pvars: usize) -> Self {
        Self {
            num_pvars,
            pvar_names: None,
            shadowed_names: HashSet::new(),
            use_site_scopes: ScopeSet::new(),
            literals: Vec::new(),
        }
    }

    /// Create a new matcher with pattern variable names for debug output
    ///
    /// # Arguments
    /// * `num_pvars` - Total number of pattern variables in the pattern
    /// * `pvar_names` - Mapping from PVREF to variable names
    pub fn new_with_names(num_pvars: usize, pvar_names: HashMap<PVRef, Rc<str>>) -> Self {
        Self {
            num_pvars,
            pvar_names: Some(pvar_names),
            shadowed_names: HashSet::new(),
            use_site_scopes: ScopeSet::new(),
            literals: Vec::new(),
        }
    }

    /// Create a new matcher with hygiene support for literal shadowing
    ///
    /// # Arguments
    /// * `num_pvars` - Total number of pattern variables in the pattern
    /// * `pvar_names` - Mapping from PVREF to variable names
    /// * `shadowed_names` - Names shadowed by local bindings at macro use site
    /// * `use_site_scopes` - Scopes from the use-site for binding comparison
    /// * `literals` - Literal identifiers from the macro definition with binding info
    pub fn new_with_hygiene(
        num_pvars: usize,
        pvar_names: HashMap<PVRef, Rc<str>>,
        shadowed_names: HashSet<Rc<str>>,
        use_site_scopes: ScopeSet,
        literals: Vec<LiteralBinding>,
    ) -> Self {
        Self {
            num_pvars,
            pvar_names: Some(pvar_names),
            shadowed_names,
            use_site_scopes,
            literals,
        }
    }

    /// Match a pattern against an input expression
    ///
    /// This is the main entry point for pattern matching.
    /// Returns a MatchEnv with all pattern variables bound on success.
    ///
    /// Inspired by Gauche's match_pattern (macro.c:600+)
    pub fn match_pattern(&self, pattern: &Pattern, input: &Value) -> Result<MatchEnv, MatchError> {
        if patina_runtime::macro_debug::is_enabled() {
            // Use names if available, otherwise fall back to generic representation
            let pattern_str = if let Some(ref names) = self.pvar_names {
                pattern_to_string_with_names(pattern, names)
            } else {
                pattern_to_string(pattern)
            };
            println!("[MACRO]     Pattern: {}", pattern_str);
            println!("[MACRO]     Input: {}", input);
        }

        let mut env = MatchEnv::new(self.num_pvars);
        let result = self.match_impl(pattern, input, &mut env, 0);

        if patina_runtime::macro_debug::is_enabled() {
            match &result {
                Ok(_) => {
                    println!("[MACRO]     Match: SUCCESS");
                    println!("[MACRO]     Bindings:");
                    debug::print_bindings(&env, self.num_pvars, self.pvar_names.as_ref());
                }
                Err(e) => {
                    println!("[MACRO]     Match: FAILED ({})", e);
                }
            }
        }

        result?;
        Ok(env)
    }

    /// Internal matching implementation
    ///
    /// # Arguments
    /// * `pattern` - The pattern to match
    /// * `input` - The input expression
    /// * `env` - The match environment being built
    /// * `level` - Current ellipsis nesting level
    ///
    /// Based on Gauche's match_rec (macro.c:620+)
    fn match_impl(
        &self,
        pattern: &Pattern,
        input: &Value,
        env: &mut MatchEnv,
        level: usize,
    ) -> Result<(), MatchError> {
        match pattern {
            Pattern::Wildcard => {
                // Wildcard matches anything, binds nothing
                Ok(())
            }

            Pattern::Literal(lit) => {
                // Literal must match exactly, BUT we also need to check hygiene:
                // If the literal is an identifier that's shadowed at the use site,
                // it should NOT match (R7RS 4.3.2).
                //
                // Example: (let ((=> #f)) (cond (#t => 'ok)))
                // Here `=>` is shadowed, so it shouldn't match cond's `=>` literal.
                if literal::is_literal_shadowed(
                    lit,
                    input,
                    &self.shadowed_names,
                    &self.use_site_scopes,
                    &self.literals,
                ) {
                    return Err(MatchError::LiteralMismatch {
                        expected: format!("{:?}", lit),
                        actual: format!("{:?} (shadowed)", input),
                    });
                }

                // Check for match
                // For identifiers (Symbol and Identifier types), compare by name only.
                // This enables recursive macro expansion where literals like `else`
                // may be transformed from Symbol to Identifier with scopes during
                // pattern variable substitution, but should still match.
                if literal::values_match_as_literal(lit, input) {
                    Ok(())
                } else {
                    Err(MatchError::LiteralMismatch {
                        expected: format!("{:?}", lit),
                        actual: format!("{:?}", input),
                    })
                }
            }

            Pattern::Var(pvref) => {
                // Bind pattern variable
                // At level 0, this is a simple leaf binding
                env.insert(*pvref, input.clone());
                Ok(())
            }

            Pattern::List(patterns) => {
                // Match list pattern
                list_match::match_list(patterns, input, env, level, self.num_pvars, |p, i, e, l| {
                    self.match_impl(p, i, e, l)
                })
            }

            Pattern::Vector(patterns) => {
                // Match vector pattern
                list_match::match_vector(patterns, input, env, level, |p, i, e, l| {
                    self.match_impl(p, i, e, l)
                })
            }

            Pattern::DottedList { patterns, tail } => {
                // Match dotted list: (p1 p2 . rest)
                list_match::match_dotted_list(
                    patterns,
                    tail,
                    input,
                    env,
                    level,
                    self.num_pvars,
                    |p, i, e, l| self.match_impl(p, i, e, l),
                )
            }

            Pattern::Ellipsis {
                subpattern: _,
                level: _pattern_level,
                num_following: _,
                vars: _,
            } => {
                // Match ellipsis pattern: (p ...)
                // This is handled inline in match_list
                // If we get here, we're in a context where ellipsis isn't supported yet
                Err(MatchError::TypeMismatch {
                    expected: "ellipsis in list context".to_string(),
                    actual: format!("{}", input),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macro_expander::utils::vec_to_list;

    // Use vec_to_list from utils as make_list alias for test readability
    fn make_list(values: Vec<Value>) -> Value {
        vec_to_list(values)
    }

    #[test]
    fn test_match_wildcard() {
        let matcher = Matcher::new(0);
        let pattern = Pattern::Wildcard;
        let input = Value::Integer(42);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_match_literal_success() {
        let matcher = Matcher::new(0);
        let pattern = Pattern::Literal(Value::Integer(42));
        let input = Value::Integer(42);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_match_literal_failure() {
        let matcher = Matcher::new(0);
        let pattern = Pattern::Literal(Value::Integer(42));
        let input = Value::Integer(43);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MatchError::LiteralMismatch { .. }
        ));
    }

    #[test]
    fn test_match_var() {
        let matcher = Matcher::new(1);
        let pvref = PVRef::new(0, 0);
        let pattern = Pattern::Var(pvref);
        let input = Value::Integer(42);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_ok());

        let env = result.unwrap();
        let bound = env.get(pvref, &[]);
        assert_eq!(
            format!("{:?}", bound),
            format!("{:?}", Some(Value::Integer(42)))
        );
    }

    #[test]
    fn test_match_simple_list() {
        // Pattern: (x y)
        let matcher = Matcher::new(2);
        let x = PVRef::new(0, 0);
        let y = PVRef::new(0, 1);
        let pattern = Pattern::List(vec![Pattern::Var(x), Pattern::Var(y)]);

        let input = make_list(vec![Value::Integer(1), Value::Integer(2)]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_ok());

        let env = result.unwrap();
        assert!(env.get(x, &[]).is_some());
        assert!(env.get(y, &[]).is_some());
    }

    #[test]
    fn test_match_list_too_few_elements() {
        // Pattern: (x y z)
        let matcher = Matcher::new(3);
        let pattern = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
            Pattern::Var(PVRef::new(0, 2)),
        ]);

        let input = make_list(vec![Value::Integer(1), Value::Integer(2)]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MatchError::TooFewElements { .. }
        ));
    }

    #[test]
    fn test_match_ellipsis_simple() {
        use patina_runtime::MatchValue;

        // Pattern: (x ...)
        let matcher = Matcher::new(1);
        let x = PVRef::new(1, 0);
        let pattern = Pattern::List(vec![Pattern::Ellipsis {
            subpattern: Box::new(Pattern::Var(x)),
            level: 1,
            num_following: 0,
            vars: vec![x],
        }]);

        let input = make_list(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_ok());

        let env = result.unwrap();
        let bound_raw = env.get_raw(x);
        assert!(bound_raw.is_some());
        // Should be a Branch with 3 elements
        if let Some(MatchValue::Branch(items)) = bound_raw {
            assert_eq!(items.len(), 3);
        } else {
            panic!("Expected Branch, got {:?}", bound_raw);
        }
    }

    #[test]
    fn test_match_ellipsis_with_following() {
        use patina_runtime::MatchValue;

        // Pattern: (x ... y)
        let matcher = Matcher::new(2);
        let x = PVRef::new(1, 0);
        let y = PVRef::new(0, 1);
        let pattern = Pattern::List(vec![
            Pattern::Ellipsis {
                subpattern: Box::new(Pattern::Var(x)),
                level: 1,
                num_following: 1,
                vars: vec![x],
            },
            Pattern::Var(y),
        ]);

        let input = make_list(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
            Value::Integer(99),
        ]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_ok());

        let env = result.unwrap();

        // x should match first 3 elements
        let x_bound = env.get_raw(x);
        if let Some(MatchValue::Branch(items)) = x_bound {
            assert_eq!(items.len(), 3);
        } else {
            panic!("Expected Branch for x, got {:?}", x_bound);
        }

        // y should match last element (99)
        let y_bound = env.get(y, &[]);
        assert_eq!(
            format!("{:?}", y_bound),
            format!("{:?}", Some(Value::Integer(99)))
        );
    }

    // === Error Condition Tests ===

    #[test]
    fn test_error_too_many_elements() {
        // Pattern: (x y) without ellipsis
        // Input: (1 2 3) - too many elements
        let matcher = Matcher::new(2);
        let pattern = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);

        let input = make_list(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3), // extra!
        ]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::TooManyElements { .. }),
            "Expected TooManyElements error"
        );
    }

    #[test]
    fn test_error_type_mismatch_list_vs_vector() {
        // Pattern: (x y) - expects list
        // Input: #(1 2) - vector
        let matcher = Matcher::new(2);
        let pattern = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);

        let input = Value::Vector(Rc::new(std::cell::RefCell::new(vec![
            Value::Integer(1),
            Value::Integer(2),
        ])));

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::TypeMismatch { .. }),
            "Expected TypeMismatch error"
        );
    }

    #[test]
    fn test_error_type_mismatch_vector_vs_list() {
        // Pattern: #(x y) - expects vector
        // Input: (1 2) - list
        let matcher = Matcher::new(2);
        let pattern = Pattern::Vector(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);

        let input = make_list(vec![Value::Integer(1), Value::Integer(2)]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::TypeMismatch { .. }),
            "Expected TypeMismatch error"
        );
    }

    #[test]
    fn test_error_vector_size_mismatch() {
        // Pattern: #(x y z) - expects 3 elements
        // Input: #(1 2) - only 2 elements
        let matcher = Matcher::new(3);
        let pattern = Pattern::Vector(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
            Pattern::Var(PVRef::new(0, 2)),
        ]);

        let input = Value::Vector(Rc::new(std::cell::RefCell::new(vec![
            Value::Integer(1),
            Value::Integer(2),
        ])));

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::VectorSizeMismatch { .. }),
            "Expected VectorSizeMismatch error"
        );
    }

    #[test]
    fn test_error_type_mismatch_list_vs_atom() {
        // Pattern: (x y) - expects list
        // Input: 42 - atom
        let matcher = Matcher::new(2);
        let pattern = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);

        let input = Value::Integer(42);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::TypeMismatch { .. }),
            "Expected TypeMismatch error"
        );
    }

    #[test]
    fn test_error_dotted_list_too_few() {
        // Pattern: (x y . rest) - expects at least 2 elements
        // Input: (1) - only 1 element
        let matcher = Matcher::new(3);
        let pattern = Pattern::DottedList {
            patterns: vec![
                Pattern::Var(PVRef::new(0, 0)),
                Pattern::Var(PVRef::new(0, 1)),
            ],
            tail: Box::new(Pattern::Var(PVRef::new(0, 2))),
        };

        let input = make_list(vec![Value::Integer(1)]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::TooFewElements { .. }),
            "Expected TooFewElements error"
        );
    }
}
