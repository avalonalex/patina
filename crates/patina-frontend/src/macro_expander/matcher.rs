//! PVREF-based pattern matching (Version 2)
//!
//! This module implements pattern matching for the PVREF-based macro system.
//! It takes compiled Pattern2 structures and matches them against input expressions,
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

use crate::macro_expander::pattern::Pattern;
use patina_runtime::{MatchEnv, MatchValue, PVRef, Value};

/// Error type for pattern matching failures
#[derive(Debug, Clone, PartialEq)]
pub enum MatchError {
    /// Pattern requires more elements than input provides
    TooFewElements {
        pattern: String,
        expected: usize,
        actual: usize,
    },

    /// Input has more elements than pattern can match (no ellipsis to consume them)
    TooManyElements { expected: usize, actual: usize },

    /// Literal value doesn't match
    LiteralMismatch { expected: String, actual: String },

    /// Type mismatch (e.g., list pattern vs vector input)
    TypeMismatch { expected: String, actual: String },

    /// Vector pattern size doesn't match input vector size
    VectorSizeMismatch { expected: usize, actual: usize },

    /// Ellipsis pattern with inconsistent repetition counts
    InconsistentRepetition {
        var1: String,
        count1: usize,
        var2: String,
        count2: usize,
    },
}

impl std::fmt::Display for MatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchError::TooFewElements {
                pattern,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Pattern {} requires at least {} elements, got {}",
                    pattern, expected, actual
                )
            }
            MatchError::TooManyElements { expected, actual } => {
                write!(
                    f,
                    "Pattern expects {} elements, got {} (no ellipsis to consume extra elements)",
                    expected, actual
                )
            }
            MatchError::LiteralMismatch { expected, actual } => {
                write!(f, "Literal mismatch: expected {}, got {}", expected, actual)
            }
            MatchError::TypeMismatch { expected, actual } => {
                write!(f, "Type mismatch: expected {}, got {}", expected, actual)
            }
            MatchError::VectorSizeMismatch { expected, actual } => {
                write!(
                    f,
                    "Vector size mismatch: expected {}, got {}",
                    expected, actual
                )
            }
            MatchError::InconsistentRepetition {
                var1,
                count1,
                var2,
                count2,
            } => {
                write!(
                    f,
                    "Inconsistent repetition: {} matched {} times, {} matched {} times",
                    var1, count1, var2, count2
                )
            }
        }
    }
}

impl std::error::Error for MatchError {}

/// Pattern matcher for PVREF-based macro system
///
/// This implements the pattern matching phase of macro expansion.
/// It takes a compiled Pattern2 and an input expression, returning
/// a MatchEnv with all pattern variables bound.
///
/// Based on Gauche's pattern matching approach (macro.c:600+).
pub struct Matcher {
    /// Number of pattern variables (determines MatchEnv size)
    num_pvars: usize,
}

impl Matcher {
    /// Create a new matcher
    ///
    /// # Arguments
    /// * `num_pvars` - Total number of pattern variables in the pattern
    pub fn new(num_pvars: usize) -> Self {
        Self { num_pvars }
    }

    /// Match a pattern against an input expression
    ///
    /// This is the main entry point for pattern matching.
    /// Returns a MatchEnv with all pattern variables bound on success.
    ///
    /// Inspired by Gauche's match_pattern (macro.c:600+)
    pub fn match_pattern(&self, pattern: &Pattern, input: &Value) -> Result<MatchEnv, MatchError> {
        let mut env = MatchEnv::new(self.num_pvars);
        self.match_impl(pattern, input, &mut env, 0)?;
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
                // Literal must match exactly
                if format!("{:?}", lit) == format!("{:?}", input) {
                    Ok(())
                } else {
                    Err(MatchError::LiteralMismatch {
                        expected: format!("{}", lit),
                        actual: format!("{}", input),
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
                self.match_list(patterns, input, env, level)
            }

            Pattern::Vector(patterns) => {
                // Match vector pattern
                self.match_vector(patterns, input, env, level)
            }

            Pattern::DottedList { patterns, tail } => {
                // Match dotted list: (p1 p2 . rest)
                self.match_dotted_list(patterns, tail, input, env, level)
            }

            Pattern::Ellipsis {
                subpattern,
                level: _pattern_level,
                num_following,
                vars,
            } => {
                // Match ellipsis pattern: (p ...)
                // This is the most complex case
                self.match_ellipsis(subpattern, *num_following, vars, input, env, level)
            }
        }
    }

    /// Match a list pattern against a list value
    ///
    /// Handles both proper lists and lists with ellipsis patterns.
    fn match_list(
        &self,
        patterns: &[Pattern],
        input: &Value,
        env: &mut MatchEnv,
        level: usize,
    ) -> Result<(), MatchError> {
        // Input must be a list
        if !matches!(input, Value::Pair(_) | Value::Null) {
            return Err(MatchError::TypeMismatch {
                expected: "list".to_string(),
                actual: format!("{}", input),
            });
        }

        // Convert input to Vec for easier processing
        let input_list = self.value_to_vec(input)?;

        // Check if we have enough elements (accounting for ellipsis patterns)
        let min_required = self.count_min_required(patterns);
        if input_list.len() < min_required {
            return Err(MatchError::TooFewElements {
                pattern: "list pattern".to_string(),
                expected: min_required,
                actual: input_list.len(),
            });
        }

        // Match patterns against input
        let mut input_idx = 0;
        let mut has_ellipsis = false;

        for pattern in patterns {
            if pattern.is_ellipsis() {
                has_ellipsis = true;
                // Handle ellipsis pattern
                if let Pattern::Ellipsis {
                    subpattern,
                    num_following,
                    ..
                } = pattern
                {
                    // Calculate how many elements to consume
                    // Use Gauche's optimization: leave num_following elements for patterns after this
                    let remaining_input = input_list.len() - input_idx;
                    let to_consume = remaining_input.saturating_sub(*num_following);

                    // Collect ALL variables from subpattern (recursively)
                    // This is critical for nested ellipsis support
                    let all_vars = self.collect_all_pvars(subpattern);

                    // Initialize branches for ALL variables
                    let mut branches: std::collections::HashMap<PVRef, Vec<MatchValue>> =
                        all_vars.iter().map(|pvref| (*pvref, Vec::new())).collect();

                    // Match subpattern against each consumed element
                    for i in 0..to_consume {
                        let elem = &input_list[input_idx + i];

                        // Create a temporary environment for this iteration
                        let mut temp_env = MatchEnv::new(self.num_pvars);
                        self.match_impl(subpattern, elem, &mut temp_env, level + 1)?;

                        // Extract matched values for ALL variables (not just direct ones)
                        // This is the key fix for nested ellipsis
                        for pvref in &all_vars {
                            if let Some(match_value) = temp_env.get_raw(*pvref) {
                                branches.get_mut(pvref).unwrap().push(match_value.clone());
                            }
                        }
                    }

                    // Install branches into environment
                    for (pvref, values) in branches {
                        env.insert_branch(pvref, values);
                    }

                    input_idx += to_consume;
                }
            } else {
                // Regular pattern
                if input_idx >= input_list.len() {
                    return Err(MatchError::TooFewElements {
                        pattern: format!("{}", pattern),
                        expected: input_idx + 1,
                        actual: input_list.len(),
                    });
                }
                self.match_impl(pattern, &input_list[input_idx], env, level)?;
                input_idx += 1;
            }
        }

        // Check for unconsumed input elements
        // If the pattern has no ellipsis, all input elements must be consumed.
        // This matches Gauche's behavior (macro.c:901): return SCM_NULLP(form);
        if !has_ellipsis && input_idx < input_list.len() {
            return Err(MatchError::TooManyElements {
                expected: input_idx,
                actual: input_list.len(),
            });
        }

        Ok(())
    }

    /// Recursively collect all pattern variables from a pattern
    ///
    /// This is essential for nested ellipsis support. We need to extract
    /// bindings for ALL variables in the subpattern tree, not just the
    /// direct children of the ellipsis.
    ///
    /// Inspired by Gauche's enter_subpattern (macro.c:766+)
    fn collect_all_pvars(&self, pattern: &Pattern) -> Vec<PVRef> {
        let mut result = Vec::new();
        Self::collect_pvars_impl(pattern, &mut result);
        result
    }

    /// Internal implementation of collect_all_pvars
    fn collect_pvars_impl(pattern: &Pattern, result: &mut Vec<PVRef>) {
        match pattern {
            Pattern::Var(pvref) => {
                if !result.contains(pvref) {
                    result.push(*pvref);
                }
            }
            Pattern::List(patterns) => {
                for p in patterns {
                    Self::collect_pvars_impl(p, result);
                }
            }
            Pattern::Vector(patterns) => {
                for p in patterns {
                    Self::collect_pvars_impl(p, result);
                }
            }
            Pattern::DottedList { patterns, tail } => {
                for p in patterns {
                    Self::collect_pvars_impl(p, result);
                }
                Self::collect_pvars_impl(tail, result);
            }
            Pattern::Ellipsis {
                subpattern, vars, ..
            } => {
                // Include the direct vars from this ellipsis
                for pvref in vars {
                    if !result.contains(pvref) {
                        result.push(*pvref);
                    }
                }
                // Recursively collect from subpattern
                Self::collect_pvars_impl(subpattern, result);
            }
            Pattern::Wildcard | Pattern::Literal(_) => {
                // No variables
            }
        }
    }

    /// Match a vector pattern against a vector value
    fn match_vector(
        &self,
        patterns: &[Pattern],
        input: &Value,
        env: &mut MatchEnv,
        level: usize,
    ) -> Result<(), MatchError> {
        // Input must be a vector
        let input_vec = match input {
            Value::Vector(v) => v.borrow().clone(),
            _ => {
                return Err(MatchError::TypeMismatch {
                    expected: "vector".to_string(),
                    actual: format!("{}", input),
                });
            }
        };

        // For now, require exact size match (no ellipsis in vectors yet)
        if patterns.len() != input_vec.len() {
            return Err(MatchError::VectorSizeMismatch {
                expected: patterns.len(),
                actual: input_vec.len(),
            });
        }

        // Match each pattern
        for (pattern, input_elem) in patterns.iter().zip(input_vec.iter()) {
            self.match_impl(pattern, input_elem, env, level)?;
        }

        Ok(())
    }

    /// Match a dotted list pattern: (p1 p2 . rest)
    fn match_dotted_list(
        &self,
        patterns: &[Pattern],
        tail: &Pattern,
        input: &Value,
        env: &mut MatchEnv,
        level: usize,
    ) -> Result<(), MatchError> {
        // Input must be a list (possibly improper)
        let mut current = input.clone();
        let mut matched_count = 0;

        // Match the fixed patterns
        for pattern in patterns {
            match &current {
                Value::Pair(p) => {
                    let (car, cdr) = &**p;
                    self.match_impl(pattern, car, env, level)?;
                    current = cdr.clone();
                    matched_count += 1;
                }
                _ => {
                    return Err(MatchError::TooFewElements {
                        pattern: "dotted list".to_string(),
                        expected: patterns.len(),
                        actual: matched_count,
                    });
                }
            }
        }

        // Match the tail against the rest
        self.match_impl(tail, &current, env, level)?;

        Ok(())
    }

    /// Match an ellipsis pattern
    ///
    /// This implements Gauche's clever num_following optimization (macro.c:138-145)
    /// to avoid backtracking.
    fn match_ellipsis(
        &self,
        _subpattern: &Pattern,
        _num_following: usize,
        _vars: &[PVRef],
        input: &Value,
        _env: &mut MatchEnv,
        _level: usize,
    ) -> Result<(), MatchError> {
        // This is handled inline in match_list
        // If we get here, we're in a context where ellipsis isn't supported yet
        Err(MatchError::TypeMismatch {
            expected: "ellipsis in list context".to_string(),
            actual: format!("{}", input),
        })
    }

    /// Convert a Scheme list Value to a Vec for easier processing
    fn value_to_vec(&self, value: &Value) -> Result<Vec<Value>, MatchError> {
        let mut result = Vec::new();
        let mut current = value.clone();

        loop {
            match current {
                Value::Null => break,
                Value::Pair(p) => {
                    let (car, cdr) = &*p;
                    result.push(car.clone());
                    current = cdr.clone();
                }
                _ => {
                    // Improper list - not supported here
                    return Err(MatchError::TypeMismatch {
                        expected: "proper list".to_string(),
                        actual: format!("{}", value),
                    });
                }
            }
        }

        Ok(result)
    }

    /// Count minimum required elements in a pattern list
    ///
    /// This accounts for ellipsis patterns which can match zero elements.
    fn count_min_required(&self, patterns: &[Pattern]) -> usize {
        let mut count = 0;
        for pattern in patterns {
            if pattern.is_ellipsis() {
                // Ellipsis can match zero, but need to account for num_following
                if let Pattern::Ellipsis { num_following, .. } = pattern {
                    // The items after this ellipsis are required
                    count += num_following;
                    break; // No more patterns after this (they're accounted for in num_following)
                }
            } else {
                count += 1;
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    // Helper to create a list from values
    fn make_list(values: Vec<Value>) -> Value {
        let mut result = Value::Null;
        for value in values.into_iter().rev() {
            result = Value::Pair(Rc::new((value, result)));
        }
        result
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
}
