//! List and ellipsis pattern matching
//!
//! This module implements matching for list patterns, including the complex
//! ellipsis handling with Gauche's num_following optimization.

use super::error::MatchError;
use crate::macro_expander::pattern::Pattern;
use crate::macro_expander::utils::{collect_pattern_pvars, list_to_vec};
use patina_runtime::{MatchEnv, MatchValue, PVRef, Value};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

/// Match a list pattern against a list value
///
/// Handles both proper lists and lists with ellipsis patterns.
///
/// # Ellipsis Pattern Matching Algorithm
///
/// This implements Gauche's `num_following` optimization (macro.c:138-145) to avoid
/// backtracking. The key insight is:
///
/// 1. **Pre-computation**: During pattern compilation, each ellipsis pattern stores
///    `num_following` - the count of non-ellipsis patterns that follow it
///
/// 2. **Greedy consumption**: The ellipsis consumes `remaining_input - num_following`
///    elements, leaving exactly enough for the trailing patterns
///
/// 3. **Branch collection**: For nested ellipsis support, we collect ALL pattern
///    variables from the subpattern tree (not just direct children), creating a
///    "branch" for each variable that stores values from each iteration
///
/// # Example
///
/// Pattern: `(a b ... c)` with `num_following = 1`
/// Input: `(1 2 3 4 5)`
///
/// - `a` matches `1` (non-ellipsis, consumes 1)
/// - `b ...` consumes `4 - 1 = 3` elements: `(2 3 4)` → `b` bound to branch `[2, 3, 4]`
/// - `c` matches `5` (trailing element)
///
/// # MatchEnv Branches
///
/// A "branch" in `MatchEnv` represents multiple values bound to a pattern variable
/// during ellipsis iteration. For pattern `(x ...)` matching `(1 2 3)`:
/// - `x` gets branch `[Leaf(1), Leaf(2), Leaf(3)]`
///
/// For nested ellipsis `((x ...) ...)` matching `((1 2) (3))`:
/// - `x` gets branch `[Branch([1, 2]), Branch([3])]`
pub fn match_list<F>(
    patterns: &[Pattern],
    input: &Value,
    env: &mut MatchEnv,
    level: usize,
    num_pvars: usize,
    match_impl: F,
) -> Result<(), MatchError>
where
    F: Fn(&Pattern, &Value, &mut MatchEnv, usize) -> Result<(), MatchError>,
{
    // Input must be a list
    if !matches!(input, Value::Pair(_) | Value::Null) {
        return Err(MatchError::TypeMismatch {
            expected: "list".to_string(),
            actual: format!("{}", input),
        });
    }

    // Convert input to Vec for easier processing
    let input_list = value_to_vec(input)?;

    // Check if we have enough elements (accounting for ellipsis patterns)
    let min_required = count_min_required(patterns);
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
                let all_vars = collect_all_pvars(subpattern);

                // Initialize branches for ALL variables
                let mut branches: std::collections::HashMap<PVRef, Vec<MatchValue>> = all_vars
                    .iter()
                    .copied()
                    .map(|pvref| (pvref, Vec::new()))
                    .collect();

                // Match subpattern against each consumed element
                for i in 0..to_consume {
                    let elem = &input_list[input_idx + i];

                    // Create a temporary environment for this iteration
                    let mut temp_env = MatchEnv::new(num_pvars);
                    match_impl(subpattern, elem, &mut temp_env, level + 1)?;

                    // Extract matched values for ALL variables (not just direct ones)
                    // This is the key fix for nested ellipsis
                    for &pvref in &all_vars {
                        if let Some(match_value) = temp_env.get_raw(pvref) {
                            branches.entry(pvref).or_default().push(match_value.clone());
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
            match_impl(pattern, &input_list[input_idx], env, level)?;
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

/// Match a vector pattern against a vector value
pub fn match_vector<F>(
    patterns: &[Pattern],
    input: &Value,
    env: &mut MatchEnv,
    level: usize,
    match_impl: F,
) -> Result<(), MatchError>
where
    F: Fn(&Pattern, &Value, &mut MatchEnv, usize) -> Result<(), MatchError>,
{
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
        match_impl(pattern, input_elem, env, level)?;
    }

    Ok(())
}

/// Match a dotted list pattern: (p1 p2 . rest)
pub fn match_dotted_list<F>(
    patterns: &[Pattern],
    tail: &Pattern,
    input: &Value,
    env: &mut MatchEnv,
    level: usize,
    num_pvars: usize,
    match_impl: F,
) -> Result<(), MatchError>
where
    F: Fn(&Pattern, &Value, &mut MatchEnv, usize) -> Result<(), MatchError>,
{
    // Convert the input to a vector for easier processing
    // For dotted lists, we'll handle the tail separately
    let mut input_list = Vec::new();
    let mut current = input.clone();

    // Collect elements into a vec, keeping track of the tail
    while let Value::Pair(p) = current {
        let borrowed = p.borrow();
        input_list.push(borrowed.0.clone());
        current = borrowed.1.clone();
        drop(borrowed);
    }

    // current now holds the tail (could be Value::Null for proper lists,
    // or some other value for improper lists)

    // Match patterns against input elements
    let mut input_idx = 0;

    for pattern in patterns {
        if pattern.is_ellipsis() {
            // Handle ellipsis pattern
            if let Pattern::Ellipsis {
                subpattern,
                num_following,
                ..
            } = pattern
            {
                // Calculate how many elements to consume
                // Leave num_following elements for patterns after this
                let remaining_input = input_list.len() - input_idx;
                let to_consume = remaining_input.saturating_sub(*num_following);

                // Collect ALL variables from subpattern
                let all_vars = collect_all_pvars(subpattern);

                // Initialize branches for ALL variables
                let mut branches: std::collections::HashMap<PVRef, Vec<MatchValue>> =
                    all_vars.iter().map(|pvref| (*pvref, Vec::new())).collect();

                // Match subpattern against each consumed element
                for i in 0..to_consume {
                    let elem = &input_list[input_idx + i];

                    // Create a temporary environment for this iteration
                    let mut temp_env = MatchEnv::new(num_pvars);
                    match_impl(subpattern, elem, &mut temp_env, level + 1)?;

                    // Extract matched values for ALL variables
                    for pvref in &all_vars {
                        if let Some(match_value) = temp_env.get_raw(*pvref) {
                            branches
                                .entry(*pvref)
                                .or_default()
                                .push(match_value.clone());
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
                    pattern: "dotted list".to_string(),
                    expected: input_idx + 1,
                    actual: input_list.len(),
                });
            }
            match_impl(pattern, &input_list[input_idx], env, level)?;
            input_idx += 1;
        }
    }

    // For dotted patterns, the tail captures remaining elements
    // Reconstruct the remaining list for the tail pattern
    let mut remaining = current; // This is the final cdr (could be Null or improper tail)

    // Build up the remaining list from unconsumed elements
    for i in (input_idx..input_list.len()).rev() {
        remaining = Value::Pair(Rc::new(RefCell::new((input_list[i].clone(), remaining))));
    }

    // Match the tail against the remaining elements
    match_impl(tail, &remaining, env, level)?;

    Ok(())
}

/// Recursively collect all pattern variables from a pattern
///
/// This is essential for nested ellipsis support. We need to extract
/// bindings for ALL variables in the subpattern tree, not just the
/// direct children of the ellipsis.
///
/// Inspired by Gauche's enter_subpattern (macro.c:766+)
fn collect_all_pvars(pattern: &Pattern) -> HashSet<PVRef> {
    collect_pattern_pvars(pattern)
}

/// Convert a Scheme list Value to a Vec for easier processing
fn value_to_vec(value: &Value) -> Result<Vec<Value>, MatchError> {
    list_to_vec(value).map_err(|_| MatchError::TypeMismatch {
        expected: "proper list".to_string(),
        actual: format!("{}", value),
    })
}

/// Count minimum required elements in a pattern list
///
/// This accounts for ellipsis patterns which can match zero elements.
fn count_min_required(patterns: &[Pattern]) -> usize {
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
