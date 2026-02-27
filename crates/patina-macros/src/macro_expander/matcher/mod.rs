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

use crate::macro_expander::Pattern;
use crate::macro_expander::utils::pattern_to_string_with_names;
use patina_core::{Heap, SharedHeap, TaggedValue};
use patina_runtime::{LiteralBinding, MatchEnv, PVRef, ScopeSet};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Pattern matcher for PVREF-based macro system
///
/// This implements the pattern matching phase of macro expansion.
/// It takes a compiled Pattern and an input expression, returning
/// a MatchEnv with all pattern variables bound.
///
/// Based on Gauche's pattern matching approach (macro.c:600+).
///
/// ## TaggedValue Storage
///
/// The Matcher stores matched values as TaggedValue in MatchEnv for memory efficiency.
/// When a SharedHeap is provided, values are converted to TaggedValue during storage.
/// The Expander retrieves TaggedValues and converts back to Value for template expansion.
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

    /// Shared heap for TaggedValue storage in MatchEnv
    shared_heap: Option<SharedHeap>,
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
            shared_heap: None,
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
            shared_heap: None,
        }
    }

    /// Create a new matcher with hygiene support and SharedHeap
    ///
    /// When SharedHeap is provided, the matcher can access heap-allocated
    /// values (pairs, identifiers, etc.) during pattern matching.
    ///
    /// # Arguments
    /// * `num_pvars` - Total number of pattern variables in the pattern
    /// * `pvar_names` - Mapping from PVREF to variable names
    /// * `shadowed_names` - Names shadowed by local bindings at macro use site
    /// * `use_site_scopes` - Scopes from the use-site for binding comparison
    /// * `literals` - Literal identifiers from the macro definition with binding info
    /// * `shared_heap` - Shared heap for TaggedValue storage
    pub fn new_with_heap(
        num_pvars: usize,
        pvar_names: HashMap<PVRef, Rc<str>>,
        shadowed_names: HashSet<Rc<str>>,
        use_site_scopes: ScopeSet,
        literals: Vec<LiteralBinding>,
        shared_heap: SharedHeap,
    ) -> Self {
        Self {
            num_pvars,
            pvar_names: Some(pvar_names),
            shadowed_names,
            use_site_scopes,
            literals,
            shared_heap: Some(shared_heap),
        }
    }

    /// Get reference to the shared heap (if available)
    pub fn shared_heap(&self) -> Option<&SharedHeap> {
        self.shared_heap.as_ref()
    }

    /// Match a pattern against a TaggedValue input expression
    ///
    /// This is the main entry point for pattern matching.
    /// It operates directly on TaggedValue without converting to Value first.
    /// Returns a MatchEnv with all pattern variables bound on success.
    ///
    /// Requires a SharedHeap to be configured in the Matcher.
    pub fn match_pattern_tagged(
        &self,
        pattern: &Pattern,
        input: TaggedValue,
    ) -> Result<MatchEnv, MatchError> {
        let shared_heap = self.shared_heap.as_ref().ok_or_else(|| {
            MatchError::InternalError("match_pattern_tagged requires a SharedHeap".to_string())
        })?;

        if patina_runtime::macro_debug::is_enabled() {
            let pattern_str = if let Some(ref names) = self.pvar_names {
                pattern_to_string_with_names(pattern, names)
            } else {
                crate::macro_expander::utils::pattern_to_string(pattern)
            };
            println!("[MACRO]     Pattern: {}", pattern_str);
            println!(
                "[MACRO]     Input: {}",
                patina_core::format_tagged(input, &shared_heap.borrow())
            );
        }

        let mut env = MatchEnv::new(self.num_pvars);
        {
            let heap = shared_heap.borrow();
            self.match_impl_tagged(pattern, input, &mut env, 0, &heap)?;
        }

        if patina_runtime::macro_debug::is_enabled() {
            println!("[MACRO]     Match: SUCCESS");
            println!("[MACRO]     Bindings:");
            debug::print_bindings(&env, self.num_pvars, self.pvar_names.as_ref());
        }

        Ok(env)
    }

    /// Internal matching implementation for TaggedValue
    ///
    /// # Arguments
    /// * `pattern` - The pattern to match
    /// * `input` - The TaggedValue input expression
    /// * `env` - The match environment being built
    /// * `level` - Current ellipsis nesting level
    /// * `heap` - The heap for TaggedValue operations
    fn match_impl_tagged(
        &self,
        pattern: &Pattern,
        input: TaggedValue,
        env: &mut MatchEnv,
        level: usize,
        heap: &Heap,
    ) -> Result<(), MatchError> {
        match pattern {
            Pattern::Wildcard => {
                // Wildcard matches anything, binds nothing
                Ok(())
            }

            Pattern::Literal(lit) => {
                // Check for shadowing first
                if literal::is_literal_shadowed_tagged(
                    *lit,
                    input,
                    heap,
                    &self.shadowed_names,
                    &self.use_site_scopes,
                    &self.literals,
                ) {
                    return Err(MatchError::LiteralMismatch {
                        expected: format!("{:?}", lit),
                        actual: format!("{} (shadowed)", patina_core::format_tagged(input, heap)),
                    });
                }

                // Check for match using TaggedValue literal comparison
                if literal::tagged_matches_literal(input, *lit, heap) {
                    Ok(())
                } else {
                    Err(MatchError::LiteralMismatch {
                        expected: format!("{:?}", lit),
                        actual: patina_core::format_tagged(input, heap),
                    })
                }
            }

            Pattern::Var(pvref) => {
                // Bind pattern variable with TaggedValue directly
                env.insert(*pvref, input);
                Ok(())
            }

            Pattern::List(patterns) => {
                // Match list pattern using TaggedValue
                list_match::match_list_tagged(
                    patterns,
                    input,
                    env,
                    level,
                    self.num_pvars,
                    heap,
                    |p, i, e, l, h| self.match_impl_tagged(p, i, e, l, h),
                )
            }

            Pattern::Vector(patterns) => {
                // Match vector pattern using TaggedValue
                list_match::match_vector_tagged(
                    patterns,
                    input,
                    env,
                    level,
                    heap,
                    |p, i, e, l, h| self.match_impl_tagged(p, i, e, l, h),
                )
            }

            Pattern::DottedList { patterns, tail } => {
                // Match dotted list pattern using TaggedValue
                list_match::match_dotted_list_tagged(
                    patterns,
                    tail,
                    input,
                    env,
                    level,
                    self.num_pvars,
                    heap,
                    |p, i, e, l, h| self.match_impl_tagged(p, i, e, l, h),
                )
            }

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
    use patina_core::Heap;
    use std::cell::RefCell;

    /// Helper to create a SharedHeap and Matcher for tests
    fn make_test_matcher(num_pvars: usize) -> (Matcher, SharedHeap) {
        let heap = Rc::new(RefCell::new(Heap::new()));
        let matcher = Matcher {
            num_pvars,
            pvar_names: None,
            shadowed_names: HashSet::new(),
            use_site_scopes: ScopeSet::new(),
            literals: Vec::new(),
            shared_heap: Some(heap.clone()),
        };
        (matcher, heap)
    }

    /// Helper to build a proper list from a Vec<TaggedValue> on the heap
    fn make_list_tagged(values: Vec<TaggedValue>, heap: &SharedHeap) -> TaggedValue {
        let mut result = TaggedValue::NULL;
        let mut h = heap.borrow_mut();
        for v in values.into_iter().rev() {
            result = h.alloc_pair(v, result);
        }
        result
    }

    #[test]
    fn test_match_wildcard() {
        let (matcher, _heap) = make_test_matcher(0);
        let pattern = Pattern::Wildcard;
        let input = TaggedValue::fixnum(42);

        let result = matcher.match_pattern_tagged(&pattern, input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_match_literal_success() {
        let (matcher, _heap) = make_test_matcher(0);
        let pattern = Pattern::Literal(TaggedValue::fixnum(42));
        let input = TaggedValue::fixnum(42);

        let result = matcher.match_pattern_tagged(&pattern, input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_match_literal_failure() {
        let (matcher, _heap) = make_test_matcher(0);
        let pattern = Pattern::Literal(TaggedValue::fixnum(42));
        let input = TaggedValue::fixnum(43);

        let result = matcher.match_pattern_tagged(&pattern, input);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MatchError::LiteralMismatch { .. }
        ));
    }

    #[test]
    fn test_match_var() {
        let (matcher, _heap) = make_test_matcher(1);
        let pvref = PVRef::new(0, 0);
        let pattern = Pattern::Var(pvref);
        let input = TaggedValue::fixnum(42);

        let result = matcher.match_pattern_tagged(&pattern, input);
        assert!(result.is_ok());

        let env = result.unwrap();
        let bound = env.get(pvref, &[]);
        assert!(bound.is_some());
        let tv = bound.unwrap();
        assert!(tv.is_fixnum());
        assert_eq!(tv.as_fixnum(), Some(42));
    }

    #[test]
    fn test_match_simple_list() {
        // Pattern: (x y)
        let (matcher, heap) = make_test_matcher(2);
        let x = PVRef::new(0, 0);
        let y = PVRef::new(0, 1);
        let pattern = Pattern::List(vec![Pattern::Var(x), Pattern::Var(y)]);

        let input = make_list_tagged(vec![TaggedValue::fixnum(1), TaggedValue::fixnum(2)], &heap);

        let result = matcher.match_pattern_tagged(&pattern, input);
        assert!(result.is_ok());

        let env = result.unwrap();
        assert!(env.get(x, &[]).is_some());
        assert!(env.get(y, &[]).is_some());
    }

    #[test]
    fn test_match_list_too_few_elements() {
        // Pattern: (x y z)
        let (matcher, heap) = make_test_matcher(3);
        let pattern = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
            Pattern::Var(PVRef::new(0, 2)),
        ]);

        let input = make_list_tagged(vec![TaggedValue::fixnum(1), TaggedValue::fixnum(2)], &heap);

        let result = matcher.match_pattern_tagged(&pattern, input);
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
        let (matcher, heap) = make_test_matcher(1);
        let x = PVRef::new(1, 0);
        let pattern = Pattern::List(vec![Pattern::Ellipsis {
            subpattern: Box::new(Pattern::Var(x)),
            level: 1,
            num_following: 0,
            vars: vec![x],
        }]);

        let input = make_list_tagged(
            vec![
                TaggedValue::fixnum(1),
                TaggedValue::fixnum(2),
                TaggedValue::fixnum(3),
            ],
            &heap,
        );

        let result = matcher.match_pattern_tagged(&pattern, input);
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
    }

    #[test]
    fn test_match_ellipsis_with_following() {
        use patina_runtime::MatchValue;

        // Pattern: (x ... y)
        let (matcher, heap) = make_test_matcher(2);
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

        let input = make_list_tagged(
            vec![
                TaggedValue::fixnum(1),
                TaggedValue::fixnum(2),
                TaggedValue::fixnum(3),
                TaggedValue::fixnum(99),
            ],
            &heap,
        );

        let result = matcher.match_pattern_tagged(&pattern, input);
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
    }

    // === Error Condition Tests ===

    #[test]
    fn test_error_too_many_elements() {
        // Pattern: (x y) without ellipsis
        // Input: (1 2 3) - too many elements
        let (matcher, heap) = make_test_matcher(2);
        let pattern = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);

        let input = make_list_tagged(
            vec![
                TaggedValue::fixnum(1),
                TaggedValue::fixnum(2),
                TaggedValue::fixnum(3),
            ],
            &heap,
        );

        let result = matcher.match_pattern_tagged(&pattern, input);
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
        let (matcher, heap) = make_test_matcher(2);
        let pattern = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);

        let input = heap
            .borrow_mut()
            .alloc_vector(vec![TaggedValue::fixnum(1), TaggedValue::fixnum(2)]);

        let result = matcher.match_pattern_tagged(&pattern, input);
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
        let (matcher, heap) = make_test_matcher(2);
        let pattern = Pattern::Vector(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);

        let input = make_list_tagged(vec![TaggedValue::fixnum(1), TaggedValue::fixnum(2)], &heap);

        let result = matcher.match_pattern_tagged(&pattern, input);
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
        let (matcher, heap) = make_test_matcher(3);
        let pattern = Pattern::Vector(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
            Pattern::Var(PVRef::new(0, 2)),
        ]);

        let input = heap
            .borrow_mut()
            .alloc_vector(vec![TaggedValue::fixnum(1), TaggedValue::fixnum(2)]);

        let result = matcher.match_pattern_tagged(&pattern, input);
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
        let (matcher, _heap) = make_test_matcher(2);
        let pattern = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);

        let input = TaggedValue::fixnum(42);

        let result = matcher.match_pattern_tagged(&pattern, input);
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
        let (matcher, heap) = make_test_matcher(3);
        let pattern = Pattern::DottedList {
            patterns: vec![
                Pattern::Var(PVRef::new(0, 0)),
                Pattern::Var(PVRef::new(0, 1)),
            ],
            tail: Box::new(Pattern::Var(PVRef::new(0, 2))),
        };

        let input = make_list_tagged(vec![TaggedValue::fixnum(1)], &heap);

        let result = matcher.match_pattern_tagged(&pattern, input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::TooFewElements { .. }),
            "Expected TooFewElements error"
        );
    }
}
