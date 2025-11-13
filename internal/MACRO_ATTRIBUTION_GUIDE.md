# Macro System Attribution Guide

This guide documents how to properly attribute the Gauche Scheme inspiration in code comments when implementing the new PVREF-based macro system.

## File Headers

Every new file in the macro system should include this header:

```rust
//! [Module description]
//!
//! This implementation is inspired by Gauche Scheme's macro system,
//! particularly the PVREF (Pattern Variable Reference) encoding and
//! tree-based match storage for handling nested ellipsis patterns.
//!
//! Original design by Shiro Kawai in Gauche's macro.c.
//! Reference: https://github.com/shirok/Gauche
//!
//! Key concepts borrowed from Gauche:
//! - PVREF encoding: (level, index) for pattern variables
//! - Tree-based MatchValue storage for nested ellipsis
//! - numFollowingItems optimization to avoid backtracking
//! - Two-phase compilation (compile once, expand many times)
```

## Inline Attribution

When implementing specific algorithms or data structures from Gauche, add inline comments:

### PVREF Definition

```rust
/// Pattern Variable Reference - compact encoding of pattern variable location
///
/// Inspired by Gauche's PVREF encoding (macro.c:297-300, macroP.h:133-139).
/// Uses (level, index) tupling where:
/// - level: ellipsis nesting depth (0 = not in ellipsis)
/// - index: unique identifier for this variable within pattern
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PVRef {
    level: u8,
    index: u8,
}
```

### Tree-Based Storage

```rust
/// Match value storage - tree structure for nested ellipsis
///
/// Based on Gauche's MatchVar structure (macro.c:709-726).
/// Tree structure naturally represents ellipsis nesting levels.
#[derive(Clone, Debug)]
pub enum MatchValue {
    Leaf(Value),              // Level 0 - single value
    Branch(Vec<MatchValue>),  // Level > 0 - nested matches
}
```

### numFollowingItems Optimization

```rust
/// Number of items after this ellipsis (excluding final CDR)
///
/// This optimization comes from Gauche's ScmSyntaxPattern.numFollowingItems
/// (macro.c:138-145). It allows the matcher to know exactly how many items
/// the ellipsis should consume without backtracking.
///
/// Example: From (x ... y z), x ... has num_following = 2
/// Example: From (x ... . y), x ... has num_following = 0
pub num_following: usize,
```

### Pattern Matching Algorithm

```rust
fn match_list_pattern(
    &mut self,
    patterns: &[Pattern],
    items: &[Value],
    env: &mut MatchEnv,
) -> Result<(), MatchError> {
    // Pattern matching algorithm based on Gauche's match_synrule (macro.c:763-900)

    // ...

    match &patterns[pat_idx] {
        Pattern::Ellipsis { num_following, .. } => {
            // Calculate items to match using Gauche's numFollowingItems optimization
            // This avoids backtracking by knowing exactly how many items
            // the ellipsis should consume (macro.c:138-145)
            let available = items.len() - form_idx;
            let limit = available - num_following;
            // ...
        }
    }
}
```

### Tree Navigation

```rust
/// Get value at given PVREF location by navigating the match tree
///
/// Based on Gauche's get_pvref_value (macro.c:730-750).
/// Uses indices array to navigate through tree levels.
pub fn get(&self, pvref: PVRef, indices: &[usize]) -> Option<Value> {
    // Navigate tree using current indices at each level
    let mut val = &self.vars[pvref.index()];

    for i in 1..=pvref.level() {
        match val {
            MatchValue::Branch(items) => {
                if indices[i] >= items.len() {
                    return None;  // Exhausted
                }
                val = &items[indices[i]];
            }
            MatchValue::Leaf(_) => return None,
        }
    }

    match val {
        MatchValue::Leaf(expr) => Some(expr.clone()),
        MatchValue::Branch(_) => None,
    }
}
```

### Template Expansion

```rust
fn expand_ellipsis(
    &mut self,
    template: &Template,
    env: &MatchEnv,
    level: usize,
) -> Result<Vec<Value>, ExpandError> {
    // Template expansion algorithm based on Gauche's expand_synrule (macro.c:901+)
    // Uses tree navigation with indices to handle nested ellipsis

    let mut result = Vec::new();
    self.indices[level + 1] = 0;

    loop {
        match self.expand_rec(template, env, level + 1) {
            Ok(expr) => result.push(expr),
            Err(ExpandError::Exhausted) => break,
            Err(e) => return Err(e),
        }
        self.indices[level + 1] += 1;
    }

    Ok(result)
}
```

## When Attribution is NOT Required

You don't need to attribute Gauche for:

1. **Standard R7RS features** - these are from the spec, not Gauche-specific
2. **Common Rust patterns** - standard Rust idioms like `Result`, `Option`, etc.
3. **Integration code** - glue code connecting macro system to rest of Patina
4. **Tests** - though tests can reference Gauche examples for documentation

## Attribution Best Practices

### DO

✓ Credit specific algorithms and data structures from Gauche
✓ Include line number references to Gauche source when helpful
✓ Explain WHY the Gauche approach is better/faster/clearer
✓ Link to Gauche GitHub repo in module documentation

### DON'T

✗ Copy-paste Gauche code directly (it's C, we're writing Rust)
✗ Copy Gauche comments verbatim (paraphrase and adapt)
✗ Over-attribute every single line (focus on key concepts)
✗ Forget that we're adapting ideas, not translating code

## Example: Full Module with Attribution

```rust
//! Pattern compilation for PVREF-based macro system
//!
//! This module compiles Scheme syntax-rules patterns into an efficient
//! intermediate representation using PVREF (Pattern Variable Reference) encoding.
//!
//! Inspired by Gauche Scheme's macro.c implementation by Shiro Kawai.
//! Key concepts:
//! - Two-phase design: compile pattern once, match many times
//! - PVREF encoding for O(1) variable lookup
//! - Precomputed metadata (num_following, max_level) for optimization
//!
//! Reference: https://github.com/shirok/Gauche/blob/master/src/macro.c

use patina_runtime::{Value, Environment};
use std::collections::HashMap;
use std::rc::Rc;

/// Pattern Variable Reference - compact encoding of pattern variable location
///
/// Inspired by Gauche's PVREF (macro.c:297-300, macroP.h:133-139).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PVRef {
    /// Ellipsis nesting depth (0 = not in ellipsis, 1 = in one ..., etc.)
    level: u8,

    /// Unique index for this variable within the pattern
    index: u8,
}

impl PVRef {
    pub fn new(level: u8, index: u8) -> Self {
        Self { level, index }
    }

    pub fn level(&self) -> usize {
        self.level as usize
    }

    pub fn index(&self) -> usize {
        self.index as usize
    }
}

/// Pattern compiler - converts S-expressions to compiled patterns
///
/// Based on Gauche's compile_rules (macro.c:604-683).
pub struct PatternCompiler {
    // Context for current rule
    pvars: HashMap<Symbol, PVRef>,
    pvar_count: usize,
    max_level: usize,
}

impl PatternCompiler {
    /// Compile a list pattern, detecting ellipsis patterns
    ///
    /// Algorithm based on Gauche's compile_rule1 (macro.c:400+).
    fn compile_list_pattern(
        &mut self,
        items: &[Value],
        level: usize,
    ) -> Result<Pattern, MacroError> {
        let mut patterns = Vec::new();
        let mut i = 0;

        while i < items.len() {
            if i + 1 < items.len() && self.is_ellipsis(&items[i + 1]) {
                // Count trailing items - Gauche's numFollowingItems optimization
                // (macro.c:138-145)
                let num_following = items.len() - i - 2;

                let subpattern = self.compile_pattern(&items[i], level + 1)?;

                patterns.push(Pattern::Ellipsis {
                    subpattern: Box::new(subpattern),
                    level: (level + 1) as u8,
                    num_following,
                });

                i += 2;
            } else {
                patterns.push(self.compile_pattern(&items[i], level)?);
                i += 1;
            }
        }

        Ok(Pattern::List(patterns))
    }
}
```

## Gauche License Reference

When appropriate (e.g., in top-level documentation), include Gauche's license notice:

```
Inspired by Gauche Scheme's macro system
Copyright (c) 2000-2025 Shiro Kawai <shiro@acm.org>

Gauche is distributed under a 3-clause BSD license.
See: https://github.com/shirok/Gauche/blob/master/COPYING
```

## Summary

The goal is to:

1. **Give proper credit** to Shiro Kawai and Gauche for the brilliant design
2. **Document our learning** so future maintainers understand the provenance
3. **Show respect** for the open-source community we're learning from
4. **Be transparent** about what's original vs adapted

When in doubt, err on the side of more attribution rather than less. It's both ethical and educational!
