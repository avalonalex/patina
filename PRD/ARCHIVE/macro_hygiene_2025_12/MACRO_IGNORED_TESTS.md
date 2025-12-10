# Macro System Ignored Tests - PRD

This document tracks the macro-related tests that are currently ignored, their root causes, and the work required to fix them.

## Overview

As of 2025-12-04, there are **0 ignored macro tests** across the codebase.

All previously ignored macro tests have been fixed! See the "Resolved Tests" section below for details.

---

## Resolved Tests

### `test_macro_introduced_temp_variable` (FIXED 2025-12-04)

**Resolution:** The test expectation was wrong, not the implementation!

Verified against chibi-scheme: the correct result is `5`, not `10`. The test was expecting the wrong behavior. In hygienic macros:
- Pattern variables preserve their use-site lexical context
- The `temp` in the body came from the use-site where `temp=5`
- The macro-introduced `temp` (bound to 10) is distinct due to hygiene
- Therefore the body's `temp` refers to the user's `temp` = 5

Patina was already correct; only the test assertion needed updating.

### `test_nested_ellipsis_macro` (FIXED 2025-12-04)

**Resolution:** Two changes were needed:

1. **Compiler fix** (`compiler.rs`): The `verify_ellipsis_nesting` function was too strict - it required variables at each exact ellipsis level. For nested patterns like `((item ...) ...)` where `item` is at level 2, the outer ellipsis at level 1 has no direct variable. The fix allows variables at deeper levels to satisfy the iteration requirement.

2. **Expander fix** (`expander.rs`): The `expand_single_ellipsis` function was using `get_iteration_count` which expected the variable's native level. Changed to use `get_iteration_count_at_level` to get the count at the ellipsis level being expanded, allowing level-2 variables to drive level-1 iteration through their outer Branch structure.

### `test_recursive_macro_hygiene` (FIXED 2025-12-04)

**Resolution:** The original test was fundamentally flawed - it tried to match runtime values against compile-time patterns. The pattern `(countdown 0)` could never match `(countdown (- temp 1))` because `(- temp 1)` is an expression, not the literal `0`. Chibi-scheme also fails on this pattern with `SEXP_MAX_ANALYZE_DEPTH exceeded`.

The test was rewritten to properly test recursive macro hygiene using a `nested-let` macro that generates multiple nested `let` bindings with the same `temp` variable name. With proper hygiene, each expansion creates a distinct binding.

### `test_let_syntax_nested_lexical_scoping` (FIXED 2025-12-04)

**Resolution:** The bug was in the scope-set tie-breaking logic in `crates/patina-core/src/environment.rs`.

When looking up an identifier with scopes, the algorithm finds all bindings whose scope set is a subset of the reference's scopes, then picks the "most specific" (largest scope set). The bug occurred when two bindings had the same scope count - `max_by_key` returns the LAST matching element, which was the parent (outer) binding instead of the child (inner) binding.

The fix changed `get_with_scopes` to use a manual loop that prefers earlier candidates when scope sets have equal size:
```rust
// Find the most specific binding (largest scope set)
// When scope sets have the same size, prefer the earlier candidate (closer binding)
let mut best: Option<(ScopeSet, Value)> = None;
for (scope_set, value) in candidates {
    match &best {
        None => best = Some((scope_set, value)),
        Some((best_scopes, _)) => {
            // Prefer strictly larger scope set, or keep existing on tie
            if scope_set.len() > best_scopes.len() {
                best = Some((scope_set, value));
            }
            // On tie (same length), keep the earlier candidate (child binding)
        }
    }
}
```

Verified against chibi-scheme: `(outer middle)` is the correct result.

### `test_nested_macro_literal_matching_same_symbol` (FIXED 2025-12-04)

**Resolution:** The fix was in `crates/patina-macros/src/macro_expander/matcher.rs`.

The issue was in `values_match_as_literal` which compared Identifier scopes using exact equality (`==`). R7RS 4.3.2 requires `bound-identifier=?` semantics for literal matching: two identifiers match if they have the same binding (or both are unbound with the same name).

In our scope-set system, when a macro generates another macro with a literal, the literal gets definition scopes `{S1, S2}`. When the input comes from the outer macro's template, it has those scopes plus additional expansion scopes `{S1, S2, S3}`. The key insight is: `{S1, S2} ⊆ {S1, S2, S3}` means they come from the same binding context.

The fix changed the Identifier vs Identifier comparison from exact scope equality to subset matching:

```rust
// Before (too strict):
pat_id.scopes == inp_id.scopes

// After (R7RS compliant):
pat_id.scopes.is_subset_of(&inp_id.scopes)
```

This allows literals to match when:
- Same name, AND
- Pattern's scopes are a subset of input's scopes (same binding context)

Shadowing is handled separately by `is_literal_shadowed()` which checks if the input identifier is bound by a local binding AFTER the macro was defined.

Verified against both chibi-scheme and Gauche: `matched-k` is the correct result.

## Testing Strategy

After each fix:
1. Run `cargo test --package patina-macros`
2. Run `cargo test --package patina-tests --test hygiene`
3. Run `cargo test --package patina-tests --test compliance -- macros`
4. Verify chibi r7rs-tests.scm macro section still passes

## References

- R7RS Section 4.3: Macros
- R7RS Section 4.3.2: Pattern language
- Chibi-scheme source: `lib/init-7.scm`
- Racket reference: "Binding as Sets of Scopes" (Flatt 2016)
