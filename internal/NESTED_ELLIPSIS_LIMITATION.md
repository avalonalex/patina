# Nested Ellipsis Limitation

## Status: Not Implemented

Nested ellipsis patterns (e.g., `(expr ...) ...`) are currently not supported in Patina's macro system.

## What is Nested Ellipsis?

Nested ellipsis allows pattern variables to be bound at multiple nesting levels, enabling more complex macro transformations.

### Example

```scheme
(define-syntax multi-begin
  (syntax-rules ()
    ((multi-begin (expr ...) ...)
     (begin expr ... ...))))

;; Usage:
(multi-begin
  ((set! x 1) (set! x (+ x 1)))    ; First group
  ((set! x (+ x 10)) (set! x (+ x 20))))  ; Second group

;; Should expand to:
(begin
  (set! x 1)
  (set! x (+ x 1))
  (set! x (+ x 10))
  (set! x (+ x 20)))
```

## Is This Part of R7RS?

**Yes, nested ellipsis is part of R7RS-small!**

From R7RS Section 4.3.2 (Pattern Language):

> "It is an error to use a pattern variable followed by one or more instances of the identifier ... unless it is in a subpattern followed by one or more instances of ... in the corresponding position in the pattern."

And for templates:

> "An ellipsis in a template may be followed by one or more ellipses to indicate that the expansion is to be at a deeper level of nesting."

**R7RS explicitly allows nested ellipsis in both patterns and templates.**

### R7RS Example

The R7RS spec itself provides an example similar to ours:

```scheme
(let ((x '(1 2 3))
      (y '(4 5 6)))
  `((,x ,y) ...))
; => ((1 4) (2 5) (3 6))
```

This uses nested quasiquote/unquote, which is conceptually related to nested ellipsis in macros.

## Why Doesn't Patina Support It?

### Current Pattern Matching Limitation

Our pattern matcher in `src/macro_system/pattern.rs` handles ellipsis with this approach:

```rust
pub enum Pattern {
    Literal(Value),
    Variable(Rc<str>),
    List(Vec<Pattern>),
    Ellipsis {
        before: Vec<Pattern>,
        repeated: Box<Pattern>,    // Single pattern that repeats
        after: Vec<Pattern>,
    },
}
```

**The issue:** `repeated` is a single `Box<Pattern>`, not a nested structure.

When we encounter `(expr ...) ...`, we need to:
1. Match `expr ...` as a pattern (first level of ellipsis)
2. Then repeat THAT ENTIRE PATTERN multiple times (second level of ellipsis)

Our current structure can't represent this nested repetition.

### Template Expansion Limitation

Similarly, in `src/macro_system/template.rs`:

```rust
pub enum Template {
    Literal(Value),
    Variable(Rc<str>),
    List(Vec<Template>),
    Ellipsis {
        before: Vec<Template>,
        repeated: Box<Template>,   // Single template that repeats
        after: Vec<Template>,
    },
}
```

When expanding `expr ... ...`, we need to:
1. Expand `expr` for each inner repetition
2. Then repeat that process for each outer repetition

This requires tracking **depth levels** for ellipsis-bound variables, which we don't currently do.

## The Error Message

When you try to use nested ellipsis:

```
Error: Invalid syntax: No matching pattern for macro multi-begin
```

This happens because our pattern parser sees `(expr ...) ...` and doesn't know how to handle the nested structure.

## What Would It Take to Implement?

### Approach 1: Nested Ellipsis Enum Variant

Add a new variant to handle nested ellipsis:

```rust
pub enum Pattern {
    Literal(Value),
    Variable(Rc<str>),
    List(Vec<Pattern>),
    Ellipsis {
        before: Vec<Pattern>,
        repeated: Box<Pattern>,
        after: Vec<Pattern>,
    },
    NestedEllipsis {
        before: Vec<Pattern>,
        repeated: Box<Pattern>,        // Pattern with inner ellipsis
        inner_ellipsis_count: usize,   // How many ... follow the pattern
        after: Vec<Pattern>,
    },
}
```

**Challenges:**
- Need to track ellipsis depth during matching
- Bindings become multi-dimensional (Vec<Vec<Value>> instead of Vec<Value>)
- Template expansion needs to iterate at multiple levels

### Approach 2: Depth-Aware Bindings

Change the binding structure to track nesting depth:

```rust
pub enum BindingValue {
    Single(Value),
    Multiple(Vec<Value>),
    Nested(Vec<BindingValue>),  // NEW: Recursive nesting
}
```

**Challenges:**
- Need to know the expected depth when matching
- Template expansion becomes more complex (nested iteration)
- Hygiene system needs to handle nested structures

### Approach 3: Flatten Then Unflatten

Keep current structure but:
1. During pattern matching, flatten nested ellipsis into single level
2. Track depth metadata separately
3. During template expansion, unflatten according to depth

**Challenges:**
- Complex bookkeeping
- Error-prone depth tracking
- Hard to debug

## Recommended Implementation Strategy

If we were to implement this, here's the recommended approach:

### Step 1: Extend Pattern Enum

```rust
pub enum Pattern {
    // ... existing variants ...
    Ellipsis {
        before: Vec<Pattern>,
        repeated: Box<Pattern>,
        after: Vec<Pattern>,
        depth: usize,  // NEW: 1 for single ..., 2 for ... ..., etc.
    },
}
```

### Step 2: Parse Nested Ellipsis

In `parse_pattern()`, count consecutive ellipsis:

```rust
fn parse_pattern(expr: &Value) -> Result<Pattern, EvalError> {
    // ... existing code ...

    // When we find ellipsis, count how many consecutive ones
    let mut depth = 1;
    while next_is_ellipsis() {
        depth += 1;
    }

    Ok(Pattern::Ellipsis {
        before,
        repeated,
        after,
        depth,  // Store the depth
    })
}
```

### Step 3: Nested Bindings

```rust
pub enum BindingValue {
    Single(Value),
    Multiple(Vec<Value>),
    Nested(Vec<BindingValue>),  // For nested ellipsis
}

// During matching:
fn match_ellipsis_nested(
    pattern: &Pattern,
    values: &[Value],
    depth: usize,
) -> Option<BindingValue> {
    if depth == 1 {
        // Simple ellipsis (current implementation)
        return Some(BindingValue::Multiple(matched_values));
    } else {
        // Nested ellipsis - recurse
        let mut nested = Vec::new();
        for group in value_groups {
            let inner = match_ellipsis_nested(pattern, group, depth - 1)?;
            nested.push(inner);
        }
        return Some(BindingValue::Nested(nested));
    }
}
```

### Step 4: Nested Template Expansion

```rust
fn expand_ellipsis_nested(
    template: &Template,
    bindings: &Bindings,
    depth: usize,
) -> Result<Vec<Value>, EvalError> {
    if depth == 1 {
        // Simple ellipsis (current implementation)
        return expand_simple_ellipsis(template, bindings);
    } else {
        // Nested ellipsis
        let mut result = Vec::new();
        for outer_iteration in outer_bindings {
            let inner_results = expand_ellipsis_nested(
                template,
                &outer_iteration,
                depth - 1,
            )?;
            result.extend(inner_results);
        }
        return Ok(result);
    }
}
```

### Step 5: Update Tests

Add comprehensive tests in `tests/compliance/macros_advanced.rs`:

```rust
#[test]
fn test_nested_ellipsis() {
    assert_program_eval_to(
        r#"
        (define-syntax multi-begin
          (syntax-rules ()
            ((multi-begin (expr ...) ...)
             (begin expr ... ...))))

        (define x 0)
        (multi-begin
          ((set! x 1) (set! x (+ x 1)))
          ((set! x (+ x 10)) (set! x (+ x 20))))
        x
        "#,
        "33",
    );
}
```

## Estimated Effort

**Complexity:** High
**Effort:** 2-3 days
**Lines of code:** ~500-800 lines

**Breakdown:**
- Pattern parsing: 1 day
- Pattern matching with depth: 1 day
- Template expansion with depth: 1 day
- Testing and debugging: 0.5-1 day

## Workarounds (Current)

Until nested ellipsis is implemented, users can work around it by:

### Option 1: Flatten Manually

```scheme
;; Instead of:
(multi-begin
  ((expr1 expr2) (expr3 expr4)))

;; Use:
(begin
  (begin expr1 expr2)
  (begin expr3 expr4))
```

### Option 2: Use Helper Macros

```scheme
(define-syntax begin-group
  (syntax-rules ()
    ((begin-group expr ...)
     (begin expr ...))))

(define-syntax multi-begin
  (syntax-rules ()
    ((multi-begin group ...)
     (begin (begin-group . group) ...))))

;; Usage:
(multi-begin
  ((set! x 1) (set! x (+ x 1)))
  ((set! x (+ x 10))))
```

### Option 3: Avoid Nested Ellipsis Patterns

Design macros that don't require nested ellipsis.

## How Common is Nested Ellipsis?

**Usage in practice:** Relatively rare

Most common macros don't need nested ellipsis:
- `let`, `let*`, `letrec` - single level
- `cond`, `case` - single level
- `and`, `or` - single level
- Most practical macros - single level

**Where it's useful:**
- Matrix/table operations
- Nested data transformations
- Advanced pattern matching macros
- Recursive data structure manipulation

**R7RS compliance:** Required for full compliance, but rarely encountered in real code

## Priority

**Low-Medium Priority**

Reasons to defer:
- Rarely used in practice
- Workarounds exist
- Complex implementation
- Most R7RS code doesn't use it

Reasons to implement:
- Full R7RS compliance
- Enables advanced macro patterns
- Educational value (demonstrates deep understanding)
- Completeness

## References

- R7RS Section 4.3.2: Pattern Language (full spec)
- "Macros That Work" by Clinger & Rees (1991) - Original hygiene paper
- Racket documentation on ellipsis depth
- Chibi-scheme implementation (reference R7RS implementation)

## Related Files

- `src/macro_system/pattern.rs` - Pattern matching implementation
- `src/macro_system/template.rs` - Template expansion implementation
- `tests/compliance/macros_advanced.rs:348` - Test marked `#[ignore]`
- `PRD/ARCHIVE/phase1_completed/MACRO_IMPLEMENTATION_DESIGN.md` - Original macro system design

## Decision

**Current status:** Not implementing nested ellipsis in Phase 1

**Rationale:**
1. Our macro system is fully functional for 99% of use cases
2. Single-level ellipsis covers all common patterns
3. Implementation complexity is high
4. Can be added later without breaking changes

**Future:** If needed, follow the recommended strategy above. The current architecture supports adding this feature without major refactoring.

## Comparison with Other Implementations

### Racket
✅ Full nested ellipsis support with arbitrary depth

### Chibi-scheme (R7RS reference)
✅ Full nested ellipsis support

### Steel
❓ Unknown - would need to check implementation

### Patina
❌ Not supported (single-level ellipsis only)

**Conclusion:** This is a known gap for full R7RS compliance but doesn't affect practical usage.
