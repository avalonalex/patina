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

use crate::macro_expander::utils::pattern_to_string_with_names;
use crate::macro_expander::{Pattern, Site};
use patina_core::{Cx, Value};
use patina_runtime::{MatchEnv, PVRef};
use std::collections::HashMap;
use std::rc::Rc;

/// Pattern matcher for PVREF-based macro system
///
/// This implements the pattern matching phase of macro expansion.
/// It takes a compiled Pattern and an input expression, returning
/// a MatchEnv with all pattern variables bound.
///
/// Based on Gauche's pattern matching approach (macro.c:600+).
///
/// ## Value Storage
///
/// The Matcher reads its input through the context it is given, and binds
/// the values it matches in a MatchEnv branded with that context, which the
/// Expander reads back within the same context.
pub struct Matcher<'a, 'gc> {
    /// Number of pattern variables (determines MatchEnv size)
    num_pvars: usize,

    /// Optional mapping from PVREF to variable names (for debug output)
    pvar_names: Option<HashMap<PVRef, Rc<str>>>,

    /// Where the macro was defined and where it is being used. A literal
    /// matches an input that reaches the same binding (R7RS §4.3.2); the
    /// literal resolves at the first site, the input at the second. Absent for
    /// the direct-API paths, which have no environments, so nothing resolves
    /// there and literals compare by spelling. Borrowed, because both are the
    /// same for every rule an expansion tries.
    definition: Option<Site<'a>>,
    use_site: Option<Site<'a>>,

    /// The scope this expansion flipped onto its input. Everything being
    /// matched carries it, so an input identifier carrying nothing else was
    /// written at the use site and stands in the use site's scopes.
    macro_scope: Option<patina_runtime::ScopeId>,

    /// The macro object whose patterns are matched, which holds the literals
    /// they name (#718). Unset, a literal pattern finds none.
    mac: Value<'gc>,
}

impl<'a, 'gc> Matcher<'a, 'gc> {
    /// Create a new matcher
    ///
    /// # Arguments
    /// * `num_pvars` - Total number of pattern variables in the pattern
    pub fn new(num_pvars: usize) -> Self {
        Self {
            num_pvars,
            pvar_names: None,
            definition: None,
            use_site: None,
            macro_scope: None,
            mac: Value::UNSPECIFIED,
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
            definition: None,
            use_site: None,
            macro_scope: None,
            mac: Value::UNSPECIFIED,
        }
    }

    /// Give the matcher the two sites a literal comparison resolves at: where
    /// the macro was defined and where it is used. Each environment is
    /// optional; without one, nothing resolves on that side.
    pub fn with_sites(mut self, definition: Option<Site<'a>>, use_site: Option<Site<'a>>) -> Self {
        self.definition = definition;
        self.use_site = use_site;
        self
    }

    /// Tell the matcher which scope this expansion flipped onto its input.
    pub fn with_macro_scope(mut self, macro_scope: patina_runtime::ScopeId) -> Self {
        self.macro_scope = Some(macro_scope);
        self
    }

    /// Matching the patterns of the macro object `mac`, whose literals they
    /// name by index (#718).
    pub fn for_macro(mut self, mac: Value<'gc>) -> Self {
        self.mac = mac;
        self
    }

    /// Match a pattern against an input expression, read through `cx`
    ///
    /// This is the main entry point for pattern matching.
    /// Returns a MatchEnv with all pattern variables bound on success.
    pub fn match_pattern_tagged(
        &self,
        cx: &Cx<'gc>,
        pattern: &Pattern,
        input: Value<'gc>,
    ) -> Result<MatchEnv<'gc>, MatchError> {
        if patina_runtime::macro_debug::is_enabled() {
            let pattern_str = if let Some(ref names) = self.pvar_names {
                pattern_to_string_with_names(pattern, names)
            } else {
                crate::macro_expander::utils::pattern_to_string(pattern)
            };
            println!("[MACRO]     Pattern: {}", pattern_str);
            println!("[MACRO]     Input: {}", cx.format(input));
        }

        let mut env = MatchEnv::new(self.num_pvars);
        self.match_impl_tagged(pattern, input, &mut env, 0, cx)?;

        if patina_runtime::macro_debug::is_enabled() {
            println!("[MACRO]     Match: SUCCESS");
            println!("[MACRO]     Bindings:");
            debug::print_bindings(&env, self.num_pvars, self.pvar_names.as_ref());
        }

        Ok(env)
    }

    /// Internal matching implementation
    ///
    /// # Arguments
    /// * `pattern` - The pattern to match
    /// * `input` - The input expression
    /// * `env` - The match environment being built
    /// * `level` - Current ellipsis nesting level
    /// * `cx` - The context the input is read through
    fn match_impl_tagged(
        &self,
        pattern: &Pattern,
        input: Value<'gc>,
        env: &mut MatchEnv<'gc>,
        level: usize,
        cx: &Cx<'gc>,
    ) -> Result<(), MatchError> {
        // As deep as the pattern: grown as it goes (#617).
        patina_core::walk::ensure_sufficient_stack_if(pattern.is_nested(), || {
            self.match_node_tagged(pattern, input, env, level, cx)
        })
    }

    /// [`Self::match_impl_tagged`] for one node of the pattern.
    fn match_node_tagged(
        &self,
        pattern: &Pattern,
        input: Value<'gc>,
        env: &mut MatchEnv<'gc>,
        level: usize,
        cx: &Cx<'gc>,
    ) -> Result<(), MatchError> {
        match pattern {
            Pattern::Wildcard => {
                // Wildcard matches anything, binds nothing
                Ok(())
            }

            Pattern::Literal(id) => {
                let lit = cx
                    .macro_literal(self.mac, *id)
                    .expect("a pattern's literal is its macro object's");
                let matched = literal::matches_literal(
                    lit,
                    input,
                    cx,
                    self.definition,
                    self.use_site,
                    self.macro_scope,
                )
                .map_err(MatchError::AmbiguousLiteral)?;
                if matched {
                    Ok(())
                } else {
                    Err(MatchError::LiteralMismatch {
                        expected: format!("{:?}", lit),
                        actual: error::described(input, cx),
                    })
                }
            }

            Pattern::Var(pvref) => {
                // Bind the pattern variable
                env.insert(*pvref, input);
                Ok(())
            }

            Pattern::List(patterns) => list_match::match_list_tagged(
                patterns,
                input,
                env,
                level,
                self.num_pvars,
                cx,
                |p, i, e, l, cx| self.match_impl_tagged(p, i, e, l, cx),
            ),

            Pattern::Vector(patterns) => list_match::match_vector_tagged(
                patterns,
                input,
                env,
                level,
                self.num_pvars,
                cx,
                |p, i, e, l, cx| self.match_impl_tagged(p, i, e, l, cx),
            ),

            Pattern::DottedList { patterns, tail } => list_match::match_dotted_list_tagged(
                patterns,
                tail,
                input,
                env,
                level,
                self.num_pvars,
                cx,
                |p, i, e, l, cx| self.match_impl_tagged(p, i, e, l, cx),
            ),

            Pattern::Ellipsis { .. } => {
                // Ellipsis is handled inline in match_list
                Err(MatchError::TypeMismatch {
                    expected: "ellipsis in list context".to_string(),
                    actual: format!("{:?}", input),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_core::{GcDriver, Instance, new_shared_heap};

    /// Run `f` with a context over a fresh heap.
    fn with_cx<R>(f: impl for<'gc> FnOnce(&mut Cx<'gc>) -> R) -> R {
        let heap = new_shared_heap();
        let instance = Instance::default();
        let mut driver = GcDriver::acquire(&heap).expect("the carrier's driver is free");
        driver.mutate(&instance, f)
    }

    /// A matcher for tests, with no sites and no macro object
    fn make_test_matcher<'gc>(num_pvars: usize) -> Matcher<'static, 'gc> {
        Matcher::new(num_pvars)
    }

    /// [`make_test_matcher`]'s, for patterns whose one literal is `42`.
    fn make_literal_matcher<'gc>(cx: &mut Cx<'gc>) -> Matcher<'static, 'gc> {
        let mac = crate::macro_expander::test_macro_object(cx, vec![Value::fixnum(42)]);
        make_test_matcher(0).for_macro(mac)
    }

    #[test]
    fn test_match_wildcard() {
        with_cx(|cx| {
            let matcher = make_test_matcher(0);
            let pattern = Pattern::Wildcard;
            let input = Value::fixnum(42);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_match_literal_success() {
        with_cx(|cx| {
            let matcher = make_literal_matcher(cx);
            let pattern = Pattern::Literal(patina_core::LiteralId::new(0));
            let input = Value::fixnum(42);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_match_literal_failure() {
        with_cx(|cx| {
            let matcher = make_literal_matcher(cx);
            let pattern = Pattern::Literal(patina_core::LiteralId::new(0));
            let input = Value::fixnum(43);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_err());
            assert!(matches!(
                result.unwrap_err(),
                MatchError::LiteralMismatch { .. }
            ));
        });
    }

    #[test]
    fn test_match_var() {
        with_cx(|cx| {
            let matcher = make_test_matcher(1);
            let pvref = PVRef::new(0, 0);
            let pattern = Pattern::Var(pvref);
            let input = Value::fixnum(42);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_ok());

            let env = result.unwrap();
            let bound = env.get(pvref, &[]);
            assert!(bound.is_some());
            let tv = bound.unwrap();
            assert!(tv.is_fixnum());
            assert_eq!(tv.as_fixnum(), Some(42));
        });
    }

    #[test]
    fn test_match_simple_list() {
        with_cx(|cx| {
            // Pattern: (x y)
            let matcher = make_test_matcher(2);
            let x = PVRef::new(0, 0);
            let y = PVRef::new(0, 1);
            let pattern = Pattern::List(vec![Pattern::Var(x), Pattern::Var(y)]);

            let input = cx.list_from_iter(vec![Value::fixnum(1), Value::fixnum(2)]);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_ok());

            let env = result.unwrap();
            assert!(env.get(x, &[]).is_some());
            assert!(env.get(y, &[]).is_some());
        });
    }

    #[test]
    fn test_match_list_too_few_elements() {
        with_cx(|cx| {
            // Pattern: (x y z)
            let matcher = make_test_matcher(3);
            let pattern = Pattern::List(vec![
                Pattern::Var(PVRef::new(0, 0)),
                Pattern::Var(PVRef::new(0, 1)),
                Pattern::Var(PVRef::new(0, 2)),
            ]);

            let input = cx.list_from_iter(vec![Value::fixnum(1), Value::fixnum(2)]);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_err());
            assert!(matches!(
                result.unwrap_err(),
                MatchError::TooFewElements { .. }
            ));
        });
    }

    #[test]
    fn test_match_ellipsis_simple() {
        with_cx(|cx| {
            use patina_runtime::MatchValue;

            // Pattern: (x ...)
            let matcher = make_test_matcher(1);
            let x = PVRef::new(1, 0);
            let pattern = Pattern::List(vec![Pattern::Ellipsis {
                subpattern: Box::new(Pattern::Var(x)),
                level: 1,
                num_following: 0,
                vars: vec![x],
            }]);

            let input =
                cx.list_from_iter(vec![Value::fixnum(1), Value::fixnum(2), Value::fixnum(3)]);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_ok());

            let env = result.unwrap();
            let bound_raw = env.get_raw(x);
            assert!(bound_raw.is_some());
            // Should be a Branch with 3 elements
            if let Some(MatchValue::Branch(branch_idx)) = bound_raw {
                let items = env.branches().get(branch_idx).expect("branch should exist");
                assert_eq!(items.len(), 3);
            } else {
                panic!("Expected Branch, got {:?}", bound_raw);
            }
        });
    }

    #[test]
    fn test_match_ellipsis_with_following() {
        with_cx(|cx| {
            use patina_runtime::MatchValue;

            // Pattern: (x ... y)
            let matcher = make_test_matcher(2);
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

            let input = cx.list_from_iter(vec![
                Value::fixnum(1),
                Value::fixnum(2),
                Value::fixnum(3),
                Value::fixnum(99),
            ]);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_ok());

            let env = result.unwrap();

            // x should match first 3 elements
            let x_bound = env.get_raw(x);
            if let Some(MatchValue::Branch(branch_idx)) = x_bound {
                let items = env.branches().get(branch_idx).expect("branch should exist");
                assert_eq!(items.len(), 3);
            } else {
                panic!("Expected Branch for x, got {:?}", x_bound);
            }

            // y should match last element (99)
            let y_bound = env.get(y, &[]);
            assert!(y_bound.is_some());
            let tv = y_bound.unwrap();
            assert!(tv.is_fixnum());
            assert_eq!(tv.as_fixnum(), Some(99));
        });
    }

    // === Error Condition Tests ===

    #[test]
    fn test_error_too_many_elements() {
        with_cx(|cx| {
            // Pattern: (x y) without ellipsis
            // Input: (1 2 3) - too many elements
            let matcher = make_test_matcher(2);
            let pattern = Pattern::List(vec![
                Pattern::Var(PVRef::new(0, 0)),
                Pattern::Var(PVRef::new(0, 1)),
            ]);

            let input =
                cx.list_from_iter(vec![Value::fixnum(1), Value::fixnum(2), Value::fixnum(3)]);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_err());
            assert!(
                matches!(result.unwrap_err(), MatchError::TooManyElements { .. }),
                "Expected TooManyElements error"
            );
        });
    }

    #[test]
    fn test_error_type_mismatch_list_vs_vector() {
        with_cx(|cx| {
            // Pattern: (x y) - expects list
            // Input: #(1 2) - vector
            let matcher = make_test_matcher(2);
            let pattern = Pattern::List(vec![
                Pattern::Var(PVRef::new(0, 0)),
                Pattern::Var(PVRef::new(0, 1)),
            ]);

            let input = cx.alloc_vector(vec![Value::fixnum(1), Value::fixnum(2)]);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_err());
            assert!(
                matches!(result.unwrap_err(), MatchError::TypeMismatch { .. }),
                "Expected TypeMismatch error"
            );
        });
    }

    #[test]
    fn test_error_type_mismatch_vector_vs_list() {
        with_cx(|cx| {
            // Pattern: #(x y) - expects vector
            // Input: (1 2) - list
            let matcher = make_test_matcher(2);
            let pattern = Pattern::Vector(vec![
                Pattern::Var(PVRef::new(0, 0)),
                Pattern::Var(PVRef::new(0, 1)),
            ]);

            let input = cx.list_from_iter(vec![Value::fixnum(1), Value::fixnum(2)]);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_err());
            assert!(
                matches!(result.unwrap_err(), MatchError::TypeMismatch { .. }),
                "Expected TypeMismatch error"
            );
        });
    }

    #[test]
    fn test_error_vector_size_mismatch() {
        with_cx(|cx| {
            // Pattern: #(x y z) - expects 3 elements
            // Input: #(1 2) - only 2 elements
            let matcher = make_test_matcher(3);
            let pattern = Pattern::Vector(vec![
                Pattern::Var(PVRef::new(0, 0)),
                Pattern::Var(PVRef::new(0, 1)),
                Pattern::Var(PVRef::new(0, 2)),
            ]);

            let input = cx.alloc_vector(vec![Value::fixnum(1), Value::fixnum(2)]);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_err());
            assert!(
                matches!(result.unwrap_err(), MatchError::VectorSizeMismatch { .. }),
                "Expected VectorSizeMismatch error"
            );
        });
    }

    #[test]
    fn test_error_type_mismatch_list_vs_atom() {
        with_cx(|cx| {
            // Pattern: (x y) - expects list
            // Input: 42 - atom
            let matcher = make_test_matcher(2);
            let pattern = Pattern::List(vec![
                Pattern::Var(PVRef::new(0, 0)),
                Pattern::Var(PVRef::new(0, 1)),
            ]);

            let input = Value::fixnum(42);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_err());
            assert!(
                matches!(result.unwrap_err(), MatchError::TypeMismatch { .. }),
                "Expected TypeMismatch error"
            );
        });
    }

    #[test]
    fn test_error_dotted_list_too_few() {
        with_cx(|cx| {
            // Pattern: (x y . rest) - expects at least 2 elements
            // Input: (1) - only 1 element
            let matcher = make_test_matcher(3);
            let pattern = Pattern::DottedList {
                patterns: vec![
                    Pattern::Var(PVRef::new(0, 0)),
                    Pattern::Var(PVRef::new(0, 1)),
                ],
                tail: Box::new(Pattern::Var(PVRef::new(0, 2))),
            };

            let input = cx.list_from_iter(vec![Value::fixnum(1)]);

            let result = matcher.match_pattern_tagged(cx, &pattern, input);
            assert!(result.is_err());
            assert!(
                matches!(result.unwrap_err(), MatchError::TooFewElements { .. }),
                "Expected TooFewElements error"
            );
        });
    }
}
