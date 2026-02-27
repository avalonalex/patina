//! Debug output for pattern matching
//!
//! This module provides debug printing utilities for the pattern matcher.

use patina_runtime::{MatchEnv, MatchValue, PVRef};
use std::collections::HashMap;
use std::rc::Rc;

/// Print bindings from match environment
pub fn print_bindings(
    env: &MatchEnv,
    num_pvars: usize,
    pvar_names: Option<&HashMap<PVRef, Rc<str>>>,
) {
    // If we have pvar_names, iterate through them and print in order
    // Otherwise, fall back to printing all bindings at levels 0 and 1
    if let Some(names) = pvar_names {
        // Collect and sort PVREFs by (level, index) for consistent ordering
        let mut sorted_pvrefs: Vec<_> = names.keys().collect();
        sorted_pvrefs.sort_by_key(|pv| (pv.level(), pv.index()));

        for pv in sorted_pvrefs {
            if let Some(value) = env.get_raw(*pv) {
                let name = &names[pv];
                println!(
                    "[MACRO]       {} = {}",
                    name,
                    match_value_to_string(&value, env)
                );
            }
        }
    } else {
        // Fallback: print all bindings at levels 0 and 1
        for level in 0..=1 {
            for i in 0..num_pvars {
                let pv = PVRef::new(level, i as u8);
                if let Some(value) = env.get_raw(pv) {
                    println!(
                        "[MACRO]       var#{}@L{} = {}",
                        i,
                        level,
                        match_value_to_string(&value, env)
                    );
                }
            }
        }
    }
}

/// Convert a match value to a readable string
///
/// Since Branch now stores an index into MatchEnv's branches storage,
/// we need access to the env to resolve branch contents.
pub fn match_value_to_string(mv: &MatchValue, env: &MatchEnv) -> String {
    match mv {
        MatchValue::Leaf(tv) => format!("{:?}", tv),
        MatchValue::Branch(branch_idx) => {
            if let Some(values) = env.branches().get(*branch_idx) {
                let items: Vec<String> = values
                    .iter()
                    .map(|v| match_value_to_string(v, env))
                    .collect();
                format!("[{}]", items.join(", "))
            } else {
                format!("[invalid branch #{}]", branch_idx)
            }
        }
    }
}
