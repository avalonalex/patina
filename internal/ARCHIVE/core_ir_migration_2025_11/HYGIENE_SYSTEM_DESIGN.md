# Hygiene System Design

## Overview

Patina's macro hygiene system is designed to be **pluggable**, allowing different hygiene algorithms to be swapped without changing the macro expansion code. This future-proofs the implementation for R7RS compliance improvements.

## Architecture

### Trait-Based Abstraction

The `HygieneSystem` trait defines the interface all hygiene implementations must provide:

```rust
pub trait HygieneSystem {
    /// Apply hygiene to expanded macro output
    fn apply_hygiene(
        &self,
        expr: &Value,
        pattern_vars: &HashSet<Rc<str>>,
        expansion_env: &Rc<Environment>,
    ) -> Value;

    /// Check if identifier is hygienically generated
    fn is_generated(&self, name: &str) -> bool;

    /// Generate a fresh unique identifier
    fn fresh_identifier(&self, base: &Rc<str>) -> Rc<str>;

    /// Get system name (for debugging)
    fn name(&self) -> &str;

    /// Clone into a trait object
    fn clone_box(&self) -> Box<dyn HygieneSystem>;
}
```

### Current Implementation: GensymHygiene

**Location:** `crates/patina-macros/src/hygiene_trait.rs`

The default implementation uses a gensym-based approach:

- **Format:** `##name#counter`
- **Method:** Post-expansion renaming with global counter
- **Pros:** Simple, fast, easy to understand
- **Cons:** Limited for complex nested macros, no phase tracking

## Swapping Hygiene Implementations

### Option 1: Implement New HygieneSystem

Create a new struct that implements the `HygieneSystem` trait:

```rust
// Example: Marks-and-ribs hygiene system
pub struct MarksAndRibsHygiene {
    mark_counter: AtomicUsize,
    // ... other state
}

impl HygieneSystem for MarksAndRibsHygiene {
    fn apply_hygiene(
        &self,
        expr: &Value,
        pattern_vars: &HashSet<Rc<str>>,
        expansion_env: &Rc<Environment>,
    ) -> Value {
        // Implement marks-and-ribs algorithm here
        // See internal/MACRO_HYGIENE_APPROACHES.md for details
        todo!()
    }

    fn is_generated(&self, name: &str) -> bool {
        // Detect marked identifiers
        // Format might be: name@mark1@mark2
        name.contains('@')
    }

    fn fresh_identifier(&self, base: &Rc<str>) -> Rc<str> {
        // Generate identifier with fresh mark
        let mark = self.mark_counter.fetch_add(1, Ordering::Relaxed);
        Rc::from(format!("{}@{}", base, mark))
    }

    fn name(&self) -> &str {
        "marks-and-ribs"
    }

    fn clone_box(&self) -> Box<dyn HygieneSystem> {
        Box::new(self.clone())
    }
}
```

### Option 2: Configure at Runtime

Use `HygieneConfig` to select the hygiene system:

```rust
use patina_macros::{HygieneConfig, GensymHygiene, MarksAndRibsHygiene};

// Default (gensym)
let config = HygieneConfig::default();

// Or specify custom system
let config = HygieneConfig::with_system(
    Box::new(MarksAndRibsHygiene::new())
);

// Use in macro expander
let expander = MacroExpander::with_hygiene(config);
```

### Option 3: Environment Variable / Feature Flag

For gradual rollout or testing:

```rust
pub fn create_hygiene_system() -> Box<dyn HygieneSystem> {
    match std::env::var("PATINA_HYGIENE") {
        Ok(val) if val == "marks-and-ribs" => {
            Box::new(MarksAndRibsHygiene::new())
        }
        Ok(val) if val == "scope-sets" => {
            Box::new(ScopeSetsHygiene::new())
        }
        _ => {
            Box::new(GensymHygiene::new())  // Default
        }
    }
}
```

## Future Implementations

### Marks-and-Ribs (Recommended Next Step)

**Reference:** `internal/MACRO_HYGIENE_APPROACHES.md` (Chez Scheme section)

**Key Concepts:**
- **Marks:** Track expansion phases (each expansion adds a mark)
- **Ribs:** Track renamings within a phase
- **Identifier:** `(name, marks)` where marks is a list of expansion phases

**Benefits for Patina:**
- Handles nested macros correctly
- Phase-aware (distinguishes macro-time vs runtime)
- Well-tested algorithm (used in Chez, Racket core)
- ~500 lines of code in Chez implementation

**Implementation Steps:**
1. Create `MarksAndRibsHygiene` struct
2. Implement identifier marking during expansion
3. Update name resolution to check marks
4. Add rib tracking for renamings
5. Integrate with module system for cross-module hygiene

### Scope Sets (Future)

**Reference:** `internal/MACRO_HYGIENE_APPROACHES.md` (Racket section)

**Key Concepts:**
- Treat hygiene as a scoping problem
- Each identifier carries a set of scopes
- Expansion adds/removes scopes

**Benefits:**
- Most modern approach
- Excellent module integration
- Used in production Racket

**Challenges:**
- More complex (~3000 lines in Racket)
- Requires rethinking some core concepts

## Impact on Existing Code

### Zero Impact on Macro Expansion Logic

The trait abstraction means that **macro expansion code doesn't change** when you swap hygiene systems:

```rust
// This code works with ANY HygieneSystem implementation
pub fn expand_macro(
    compiled_macro: &CompiledMacro,
    args: &Value,
    expansion_env: &Rc<Environment>,
    hygiene: &dyn HygieneSystem,  // <-- Abstraction layer
) -> Result<Value, MacroError> {
    // ... pattern matching ...

    // Apply hygiene (implementation detail hidden)
    let hygienic = hygiene.apply_hygiene(
        &expanded,
        &pattern_vars,
        expansion_env
    );

    Ok(hygienic)
}
```

### Minimal Changes Required

To swap hygiene implementations:

1. **Create new HygieneSystem impl** - One new file
2. **Update MacroExpander constructor** - One line change
3. **No changes to:**
   - Pattern matching logic
   - Template expansion
   - Macro definition handling
   - Special forms
   - Primitives

## Testing Strategy

### Existing Tests Continue to Work

All current macro tests in `crates/patina-macros/tests/` and `crates/patina-tests/tests/compliance/` work with any `HygieneSystem` implementation.

### Add Implementation-Specific Tests

Each hygiene system should have its own test suite:

```rust
#[cfg(test)]
mod marks_and_ribs_tests {
    use super::*;

    #[test]
    fn test_marks_basic() {
        let hygiene = MarksAndRibsHygiene::new();
        // Test mark application
    }

    #[test]
    fn test_nested_expansion_phases() {
        let hygiene = MarksAndRibsHygiene::new();
        // Test phase tracking
    }
}
```

### Comparative Testing

Test the same macro expansions with different hygiene systems:

```rust
#[test]
fn test_hygiene_systems_equivalent() {
    let expr = /* ... */;
    let pattern_vars = /* ... */;
    let env = /* ... */;

    let gensym = GensymHygiene::new();
    let marks = MarksAndRibsHygiene::new();

    let result1 = gensym.apply_hygiene(&expr, &pattern_vars, &env);
    let result2 = marks.apply_hygiene(&expr, &pattern_vars, &env);

    // Both should produce hygienic results
    // (may differ in form, but semantically equivalent)
}
```

## Migration Guide

### Migrating to Marks-and-Ribs

**Step 1: Implement the trait**

Create `crates/patina-macros/src/marks_and_ribs.rs`:

```rust
pub struct MarksAndRibsHygiene {
    mark_counter: AtomicUsize,
}

impl HygieneSystem for MarksAndRibsHygiene {
    // Implement all trait methods
}
```

**Step 2: Feature flag (optional)**

In `Cargo.toml`:

```toml
[features]
default = ["gensym-hygiene"]
gensym-hygiene = []
marks-and-ribs = []
```

In code:

```rust
#[cfg(feature = "marks-and-ribs")]
pub fn default_hygiene() -> Box<dyn HygieneSystem> {
    Box::new(MarksAndRibsHygiene::new())
}

#[cfg(not(feature = "marks-and-ribs"))]
pub fn default_hygiene() -> Box<dyn HygieneSystem> {
    Box::new(GensymHygiene::new())
}
```

**Step 3: Test incrementally**

```bash
# Test with gensym (current)
cargo test

# Test with marks-and-ribs
cargo test --features marks-and-ribs

# Compare results
diff <(cargo test 2>&1) <(cargo test --features marks-and-ribs 2>&1)
```

**Step 4: Make it default**

Once all tests pass:

```toml
[features]
default = ["marks-and-ribs"]  # Changed from gensym-hygiene
```

## Performance Considerations

### Trait Object Overhead

Using `Box<dyn HygieneSystem>` has minimal overhead:
- Virtual dispatch: ~2-3 CPU cycles per call
- Heap allocation: Once per `HygieneConfig` creation
- **Impact:** Negligible (<1% of macro expansion time)

### Hygiene Algorithm Complexity

| Algorithm | Time Complexity | Space Complexity | Notes |
|-----------|----------------|------------------|-------|
| Gensym (current) | O(n) | O(n) | n = AST nodes |
| Marks-and-ribs | O(n × m) | O(n × m) | m = expansion depth |
| Scope sets | O(n × s) | O(n × s) | s = scope count |

For typical code, all are fast enough. Marks-and-ribs may be 2-3× slower than gensym, but still <1ms for most macros.

### Optimization Strategy

1. **Start simple:** Use gensym for basic compliance
2. **Profile first:** Measure macro expansion time
3. **Optimize if needed:** Consider caching, interning, etc.
4. **Feature flag:** Allow users to choose speed vs correctness

## References

- **Trait definition:** `crates/patina-macros/src/hygiene_trait.rs`
- **Current implementation:** `crates/patina-macros/src/macro_expander/hygiene.rs`
- **Research on algorithms:** `internal/MACRO_HYGIENE_APPROACHES.md`
- **R7RS compliance analysis:** `docs/HYGIENE_COMPLIANCE_ANALYSIS.md`
- **Implementation roadmap:** Chez Scheme section in `MACRO_HYGIENE_APPROACHES.md`

## FAQ

### Q: Why use a trait instead of just swapping the module?

**A:** Traits provide:
- Type safety: Enforce that all implementations provide required methods
- Runtime selection: Choose hygiene system based on config/environment
- Testing: Compare multiple implementations side-by-side
- Future flexibility: Support per-macro hygiene systems

### Q: When should I implement a new hygiene system?

**A:** Consider a new system when:
- R7RS macro tests fail due to hygiene issues
- Nested macros have unexpected variable capture
- Module system needs phase-aware hygiene
- Contributing to Scheme language research

### Q: Will changing hygiene break existing code?

**A:** No, **if** the new system is correct:
- Hygiene is an implementation detail
- User code shouldn't depend on gensym naming
- All R7RS-compliant code should work with any correct hygiene system
- Tests verify this invariant

### Q: How do I debug hygiene issues?

**A:** Use the macro debugging tools:

```bash
# Enable macro tracing
MACRO_DEBUG=1 cargo run

# In code
use patina_macros::MacroTracer;
MacroTracer::enable_all();
```

See `docs/MACRO_DEBUGGING.md` for comprehensive guide.

### Q: What's the recommended timeline for marks-and-ribs?

**A:** See `internal/MACRO_HYGIENE_APPROACHES.md` for 7-phase implementation plan (~7 weeks). Can be done incrementally:

1. **Phase 1-2 (2 weeks):** Core data structures and basic marking
2. **Phase 3-4 (2 weeks):** Integration with expansion
3. **Phase 5-6 (2 weeks):** Testing and refinement
4. **Phase 7 (1 week):** Documentation and migration

Can ship earlier behind a feature flag for testing.
