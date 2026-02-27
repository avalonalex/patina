//! List and ellipsis pattern matching
//!
//! This module implements matching for list patterns, including the complex
//! ellipsis handling with Gauche's num_following optimization.

use super::error::MatchError;
use crate::macro_expander::Pattern;
use crate::macro_expander::utils::{
    TaggedListIter, collect_pattern_pvars, list_to_vec_tagged, list_to_vec_with_tail_tagged,
};
use patina_core::{Heap, TaggedValue};
use patina_runtime::{MatchEnv, MatchValue, PVRef};

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

/// Match a list pattern against a TaggedValue list
pub fn match_list_tagged<F>(
    patterns: &[Pattern],
    input: TaggedValue,
    env: &mut MatchEnv,
    level: usize,
    num_pvars: usize,
    heap: &Heap,
    match_impl: F,
) -> Result<(), MatchError>
where
    F: Fn(&Pattern, TaggedValue, &mut MatchEnv, usize, &Heap) -> Result<(), MatchError>,
{
    // Input must be a list (pair or null)
    if !(input.is_pair() || input == TaggedValue::NULL) {
        return Err(MatchError::TypeMismatch {
            expected: "list".to_string(),
            actual: patina_core::format_tagged(input, heap),
        });
    }

    // Check if any pattern is an ellipsis
    let has_ellipsis = patterns.iter().any(|p| p.is_ellipsis());

    if has_ellipsis {
        match_list_with_ellipsis_tagged(patterns, input, env, level, num_pvars, heap, match_impl)
    } else {
        match_list_simple_tagged(patterns, input, env, level, heap, match_impl)
    }
}

/// Match a list pattern without ellipsis using TaggedValue lazy iteration
fn match_list_simple_tagged<F>(
    patterns: &[Pattern],
    input: TaggedValue,
    env: &mut MatchEnv,
    level: usize,
    heap: &Heap,
    match_impl: F,
) -> Result<(), MatchError>
where
    F: Fn(&Pattern, TaggedValue, &mut MatchEnv, usize, &Heap) -> Result<(), MatchError>,
{
    let mut iter = TaggedListIter::new(input, heap);
    let mut pattern_idx = 0;

    for pattern in patterns {
        match iter.next() {
            Some(Ok(elem)) => {
                match_impl(pattern, elem, env, level, heap)?;
                pattern_idx += 1;
            }
            Some(Err(_)) => {
                return Err(MatchError::TypeMismatch {
                    expected: "proper list".to_string(),
                    actual: patina_core::format_tagged(input, heap),
                });
            }
            None => {
                return Err(MatchError::TooFewElements {
                    pattern: "list pattern".to_string(),
                    expected: patterns.len(),
                    actual: pattern_idx,
                });
            }
        }
    }

    // Check for unconsumed input elements
    if iter.next().is_some() {
        let remaining = 1 + iter.count();
        return Err(MatchError::TooManyElements {
            expected: patterns.len(),
            actual: patterns.len() + remaining,
        });
    }

    Ok(())
}

/// Match a list pattern containing ellipsis using TaggedValue
fn match_list_with_ellipsis_tagged<F>(
    patterns: &[Pattern],
    input: TaggedValue,
    env: &mut MatchEnv,
    level: usize,
    num_pvars: usize,
    heap: &Heap,
    match_impl: F,
) -> Result<(), MatchError>
where
    F: Fn(&Pattern, TaggedValue, &mut MatchEnv, usize, &Heap) -> Result<(), MatchError>,
{
    // Convert input to Vec for random access
    let input_list = list_to_vec_tagged(input, heap).map_err(|_| MatchError::TypeMismatch {
        expected: "proper list".to_string(),
        actual: patina_core::format_tagged(input, heap),
    })?;

    // Check if we have enough elements
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

    for pattern in patterns {
        if pattern.is_ellipsis() {
            if let Pattern::Ellipsis {
                subpattern,
                num_following,
                ..
            } = pattern
            {
                // Calculate how many elements to consume
                let remaining_input = input_list.len() - input_idx;
                let to_consume = remaining_input.saturating_sub(*num_following);

                // Collect ALL variables from subpattern
                let all_vars = collect_pattern_pvars(subpattern);

                // Initialize branches for ALL variables
                let mut branches: std::collections::HashMap<PVRef, Vec<MatchValue>> = all_vars
                    .iter()
                    .copied()
                    .map(|pvref| (pvref, Vec::new()))
                    .collect();

                // Match subpattern against each consumed element
                for i in 0..to_consume {
                    let elem = input_list[input_idx + i];

                    // Create a temporary environment for this iteration
                    let mut temp_env = MatchEnv::new(num_pvars);
                    match_impl(subpattern, elem, &mut temp_env, level + 1, heap)?;

                    // Extract matched values for ALL variables
                    for &pvref in &all_vars {
                        if let Some(match_value) = temp_env.get_raw(pvref) {
                            let copied = env.copy_match_value_from(match_value, &temp_env);
                            branches.entry(pvref).or_default().push(copied);
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
            match_impl(pattern, input_list[input_idx], env, level, heap)?;
            input_idx += 1;
        }
    }

    Ok(())
}

/// Match a vector pattern against a TaggedValue vector
pub fn match_vector_tagged<F>(
    patterns: &[Pattern],
    input: TaggedValue,
    env: &mut MatchEnv,
    level: usize,
    heap: &Heap,
    match_impl: F,
) -> Result<(), MatchError>
where
    F: Fn(&Pattern, TaggedValue, &mut MatchEnv, usize, &Heap) -> Result<(), MatchError>,
{
    // Handle native vectors (primary path)
    if input.is_vector() {
        let input_len = heap.vector_len(input);

        // Require exact size match (no ellipsis in vectors yet)
        if patterns.len() != input_len {
            return Err(MatchError::VectorSizeMismatch {
                expected: patterns.len(),
                actual: input_len,
            });
        }

        // Match each pattern against native vector elements (already TaggedValue)
        for (i, pattern) in patterns.iter().enumerate() {
            let elem = heap.vector_ref(input, i);
            match_impl(pattern, elem, env, level, heap)?;
        }

        return Ok(());
    }

    // Not a vector
    Err(MatchError::TypeMismatch {
        expected: "vector".to_string(),
        actual: patina_core::format_tagged(input, heap),
    })
}

/// Match a dotted list pattern against TaggedValue: (p1 p2 . rest)
///
/// This function tracks the original input list position to properly capture
/// the tail portion without needing to reconstruct the list.
#[allow(clippy::too_many_arguments)]
pub fn match_dotted_list_tagged<F>(
    patterns: &[Pattern],
    tail: &Pattern,
    input: TaggedValue,
    env: &mut MatchEnv,
    level: usize,
    num_pvars: usize,
    heap: &Heap,
    match_impl: F,
) -> Result<(), MatchError>
where
    F: Fn(&Pattern, TaggedValue, &mut MatchEnv, usize, &Heap) -> Result<(), MatchError>,
{
    // Instead of deconstructing to a Vec, traverse the list directly
    // This preserves the original list structure for tail capture
    let mut current = input;

    // Match each fixed pattern
    for (pattern_idx, pattern) in patterns.iter().enumerate() {
        if pattern.is_ellipsis() {
            // For ellipsis in dotted lists, we need the Vec-based approach
            // Fall back to the original algorithm for this case
            return match_dotted_list_with_ellipsis_tagged(
                patterns, tail, input, env, level, num_pvars, heap, match_impl,
            );
        }

        // Check if we have an element to match
        if !current.is_pair() {
            return Err(MatchError::TooFewElements {
                pattern: "dotted list".to_string(),
                expected: pattern_idx + 1,
                actual: pattern_idx,
            });
        }

        let car = heap.car(current);
        let cdr = heap.cdr(current);

        match_impl(pattern, car, env, level, heap)?;
        current = cdr;
    }

    // Now `current` is the tail portion - match it against the tail pattern
    match_impl(tail, current, env, level, heap)?;

    Ok(())
}

/// Match a dotted list with ellipsis patterns
///
/// This handles the more complex case where ellipsis patterns appear in the list.
#[allow(clippy::too_many_arguments)]
fn match_dotted_list_with_ellipsis_tagged<F>(
    patterns: &[Pattern],
    tail: &Pattern,
    input: TaggedValue,
    env: &mut MatchEnv,
    level: usize,
    num_pvars: usize,
    heap: &Heap,
    match_impl: F,
) -> Result<(), MatchError>
where
    F: Fn(&Pattern, TaggedValue, &mut MatchEnv, usize, &Heap) -> Result<(), MatchError>,
{
    // Convert the input to a vector with tail
    let (input_list, tail_value) = list_to_vec_with_tail_tagged(input, heap);

    // Match patterns against input elements
    let mut input_idx = 0;

    for pattern in patterns {
        if pattern.is_ellipsis() {
            if let Pattern::Ellipsis {
                subpattern,
                num_following,
                ..
            } = pattern
            {
                // Calculate how many elements to consume
                let remaining_input = input_list.len() - input_idx;
                let to_consume = remaining_input.saturating_sub(*num_following);

                // Collect ALL variables from subpattern
                let all_vars = collect_pattern_pvars(subpattern);

                // Initialize branches for ALL variables
                let mut branches: std::collections::HashMap<PVRef, Vec<MatchValue>> =
                    all_vars.iter().map(|pvref| (*pvref, Vec::new())).collect();

                // Match subpattern against each consumed element
                for i in 0..to_consume {
                    let elem = input_list[input_idx + i];

                    // Create a temporary environment for this iteration
                    let mut temp_env = MatchEnv::new(num_pvars);
                    match_impl(subpattern, elem, &mut temp_env, level + 1, heap)?;

                    // Extract matched values for ALL variables
                    for pvref in &all_vars {
                        if let Some(match_value) = temp_env.get_raw(*pvref) {
                            let copied = env.copy_match_value_from(match_value, &temp_env);
                            branches.entry(*pvref).or_default().push(copied);
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
            match_impl(pattern, input_list[input_idx], env, level, heap)?;
            input_idx += 1;
        }
    }

    // For dotted patterns with ellipsis, the tail captures remaining elements
    // Skip to the correct position in the original list
    let mut remaining = input;
    for _ in 0..input_idx {
        if remaining.is_pair() {
            remaining = heap.cdr(remaining);
        } else {
            remaining = tail_value;
            break;
        }
    }

    // Match the tail against the remaining elements
    match_impl(tail, remaining, env, level, heap)?;

    Ok(())
}
