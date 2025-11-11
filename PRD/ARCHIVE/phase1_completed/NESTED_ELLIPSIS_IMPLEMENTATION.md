# Nested Ellipsis Implementation Plan for Patina

**Status:** Design Complete - Ready for Implementation
**Created:** 2025-11-10
**Target:** Full R7RS-small compliance for nested ellipsis patterns
**Estimated Effort:** 3-5 days
**Priority:** Medium (Required for full R7RS compliance, rarely used in practice)

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Technical Background](#technical-background)
3. [R7RS Requirements](#r7rs-requirements)
4. [Current Implementation Analysis](#current-implementation-analysis)
5. [Regex-Inspired Approach](#regex-inspired-approach) **NEW**
6. [Proposed Design](#proposed-design)
7. [Implementation Roadmap](#implementation-roadmap)
8. [Testing Strategy](#testing-strategy)
9. [Migration Path](#migration-path)
10. [Reference Examples](#reference-examples)
11. [Risk Analysis](#risk-analysis)

---

## Executive Summary

Patina's macro system currently supports single-level ellipsis patterns (e.g., `(x ...)`), but lacks support for **nested ellipsis** patterns required by R7RS-small. Nested ellipsis allows pattern variables to be bound at multiple nesting levels, enabling complex macro transformations like matrix operations and nested data structure manipulation.

**What's Missing:**
- Pattern matching for nested ellipsis: `((x y) ...)` where the inner list contains ellipsed variables
- Multi-dimensional bindings: Variables that repeat at multiple levels
- Template expansion with nested iteration

**Impact:**
- Currently prevents implementing certain advanced macros as specified in R7RS
- Not a blocker for most practical Scheme programs (rarely used)
- Required for 100% R7RS-small compliance

**Key Quote from R7RS (expr.tex:1744-1747):**
> "Pattern variables that occur in subpatterns followed by one or more instances of the identifier ellipsis are allowed only in subtemplates that are followed by as many instances of ellipsis."

---

## Technical Background

### What is Nested Ellipsis?

Nested ellipsis allows patterns and templates where ellipsed variables appear within other ellipsed contexts.

#### Single-Level Ellipsis (Currently Working)

```scheme
;; Pattern: (when test body ...)
;; Template: (if test (begin body ...))

(when #t (print "a") (print "b"))
;; Expands to:
(if #t (begin (print "a") (print "b")))

;; Bindings:
;; test -> #t (Single)
;; body -> [(print "a"), (print "b")] (Multiple)
```

#### Nested Ellipsis (Not Implemented)

```scheme
;; Pattern: (let-values (((vars vals) ...) ...) body)
;; Template: (let ((vars vals) ... ...) body)

(let-values
  (((a b) (values 1 2))
   ((c d) (values 3 4)))
  (+ a b c d))

;; Bindings (conceptual 2D structure):
;; vars -> [[a, b], [c, d]] (Nested)
;; vals -> [[1, 2], [3, 4]] (Nested)
;; body -> (+ a b c d) (Single)

;; Should expand to:
(let ((a 1) (b 2) (c 3) (d 4))
  (+ a b c d))
```

### Why It's Complex

1. **Multi-dimensional data structures**: Bindings become nested vectors instead of flat vectors
2. **Depth tracking**: Must track how many levels of ellipsis each variable appears in
3. **Synchronized iteration**: Multiple variables at the same depth must have matching lengths at each level
4. **Template expansion logic**: Must iterate correctly through nested structure

---

## R7RS Requirements

### From R7RS Specification (expr.tex)

**Line 1744-1747:**
> "Pattern variables that occur in subpatterns followed by one or more instances of the identifier ellipsis are allowed only in subtemplates that are followed by as many instances of ellipsis."

**Line 1747-1751:**
> "They are replaced in the output by all of the elements they match in the input, distributed as indicated. It is an error if the output cannot be built up as specified."

### Ellipsis Escape (Already Supported)

**Line 1765-1772:**
> "A template of the form `(ellipsis template)` is identical to template, except that ellipses within the template have no special meaning."

Example from R7RS (expr.tex:1776-1786):
```scheme
(define-syntax be-like-begin
  (syntax-rules ()
    ((be-like-begin name)
     (define-syntax name
       (syntax-rules ()
         ((name expr (... ...))
          (begin expr (... ...))))))))

(be-like-begin sequence)
(sequence 1 2 3 4) ;=> 4
```

The `(... ...)` produces a literal `...` in the output macro definition.

### R7RS Compliance Level

- **Required:** Yes, for full R7RS-small compliance
- **Common Usage:** Rare in practical code (< 5% of macros)
- **Priority:** Medium (not blocking most applications)

---

## Current Implementation Analysis

### Existing Data Structures

**File:** `src/macro_system/mod.rs`

```rust
#[derive(Debug, Clone)]
pub enum Pattern {
    Wildcard,
    Literal(Value),
    Variable(Rc<str>),
    List(Vec<Pattern>),
    Vector(Vec<Pattern>),
    Ellipsis {
        before: Vec<Pattern>,
        repeated: Box<Pattern>,  // Single pattern
        after: Vec<Pattern>,
    },
}

#[derive(Debug, Clone)]
pub enum BindingValue {
    Single(Value),           // x -> 42
    Multiple(Vec<Value>),    // x -> [1, 2, 3]
    // Missing: Nested(Vec<BindingValue>) for nested ellipsis
}
```

### Current Limitation

**File:** `src/macro_system/pattern.rs:186-190`

```rust
for (name, binding) in temp_bindings {
    match binding {
        BindingValue::Single(val) => {
            repeated_bindings.entry(name).or_default().push(val);
        }
        BindingValue::Multiple(_) => {
            // Nested ellipsis - not supported in basic implementation
            return false;  // ❌ Fails here
        }
    }
}
```

**This is where nested ellipsis is explicitly rejected.**

### What Works

✅ **Single-level ellipsis:**
- `(x ...)` - matches zero or more x
- `(before ... x ... after)` - ellipsis with before/after elements
- `((x y) ...)` - matches list of pairs (but x and y are NOT ellipsed)

✅ **Ellipsis escape:**
- `(... template)` - produces literal `...`
- `(... ...)` - produces the symbol `...`

### What Doesn't Work

❌ **Nested ellipsis patterns:**
- `((x ...) ...)` - x appears under two ellipses
- `(((name val) ...) ...)` - let-values style binding
- Any pattern where variables have depth > 1

### Error Behavior

```scheme
(define-syntax nested
  (syntax-rules ()
    ((nested ((x y) ...) ...)
     (list x ... ...))))

;; Currently returns:
;; Error: No matching pattern for macro nested
;; (pattern matching returns None due to line 190 in pattern.rs)
```

---

## Regex-Inspired Approach

### Key Insight: Ellipsis Matching is Regex Matching

The fundamental observation is that **ellipsis pattern matching is structurally identical to regex quantifier matching**. This insight provides a proven algorithmic framework for implementing nested ellipsis correctly.

### Comparison Table: Ellipsis vs Regex

| Scheme Pattern | Regex Equivalent | Semantics |
|----------------|------------------|-----------|
| `x` | `x` | Match single element |
| `(x ...)` | `x*` | Match zero or more x |
| `((x y) ...)` | `(xy)*` | Match zero or more pairs |
| `((x ...) ...)` | `(x*)*` | **Nested quantifier** |
| `(a ... b ...)` | `a*b*` | Two consecutive quantifiers |
| `(before ... x ... after)` | `before*x*after` | Quantifier with context |

### The Core Problem: Ambiguous Matching

Just like regex `a*b*` matching against "aaabbb", Scheme patterns with multiple ellipses face ambiguity:

**Regex Example:**
```regex
Pattern: a*b*
Input: "aaabbb"
Possible matches:
  1. a=""   b="aaabbb"  (greedy b)
  2. a="a"  b="aabbb"
  3. a="aa" b="abbb"
  4. a="aaa" b="bbb"     (greedy a)
```

**Scheme Example:**
```scheme
Pattern: (a ... b ...)
Input: (1 2 3 4 5)
Possible matches:
  1. a=()      b=(1 2 3 4 5)
  2. a=(1)     b=(2 3 4 5)
  3. a=(1 2)   b=(3 4 5)
  4. a=(1 2 3) b=(4 5)
  5. a=(1 2 3 4) b=(5)
  6. a=(1 2 3 4 5) b=()
```

### R7RS Resolution: Greedy Left-to-Right

**From R7RS spec (expr.tex:1706-1712):**

> $P$ is of the form `($P_1$ ... $P_k$ $P_e$ ellipsis $P_{m+1}$ ... $P_n$)` where $E$ is a proper list of $n$ elements, the first $k$ of which match $P_1$ through $P_k$, respectively, **whose next $m-k$ elements each match $P_e$**, whose remaining $n-m$ elements match $P_{m+1}$ through $P_n$.

**Key: The spec requires greedy matching for the ellipsed pattern, consuming as much as possible before moving to the next pattern.**

This is equivalent to **greedy regex matching**: `a*` will match as many 'a's as possible before trying to match the rest of the pattern.

### How Chibi-Scheme Implements It

Chibi implements `syntax-rules` entirely in Scheme (`lib/init-7.scm:850-1091`). Key observations:

#### 1. No Backtracking Needed for Simple Cases

For patterns like `(a ... b ...)`, chibi doesn't backtrack. Instead:

```scheme
;; From chibi init-7.scm:905-931
(cond
  ((not (null? (cdr (cdr p))))  ; Has elements after ellipsis
   (let ((len (length* (cdr (cdr p)))))
     ;; Calculate how many to reserve for 'after' elements
     (and (>= _len len)
          ;; Match first (len - after_count) elements with repeated pattern
          ;; Match remaining elements with after patterns
          ...))))
```

**Algorithm:**
1. Count how many elements are in the "after" section (`b ...` in our example)
2. Reserve that many elements from the end of input
3. Match remaining elements greedily with the ellipsed pattern
4. No backtracking needed!

#### 2. Nested Ellipsis Uses Recursive Matching

For `((x ...) ...)`, chibi uses nested loops (lines 936-968):

```scheme
;; Outer loop: match each sublist
(let lp ((w v) (ls-vars '()))
  (if (null? w)
      ;; All sublists matched, bind variables
      (let ((vars (map reverse ls-vars)))  ; Transpose!
        ...)
      ;; Match next sublist
      (and (pair? w)
           (match-pattern (car w))  ; Recursive call
           (lp (cdr w) (accumulate-bindings)))))
```

**No backtracking** because:
- Each sublist is matched independently
- Inner ellipsis is greedy within its sublist
- Outer ellipsis just iterates over sublists

### Regex Engine Techniques (What We DON'T Need)

Research into regex engines revealed several techniques:

#### 1. Backtracking (Not Needed)

**Traditional NFA regex engines** use backtracking for patterns like `(a+)+b`:
- Try matching as many outer groups as possible
- If overall match fails, backtrack and try fewer
- Can lead to **catastrophic backtracking** (exponential time)

**Why Scheme doesn't need this:**
- R7RS patterns are **deterministic**: `(a ... b ...)` always means "greedy match a, then match remaining as b"
- No ambiguity resolution needed
- Fixed-length "after" section provides clear boundary

#### 2. Thompson NFA (Overkill)

**Linear-time regex matching** using NFA simulation:
- Track all possible states simultaneously
- O(mn) time complexity guarantee
- Prevents catastrophic backtracking

**Why Scheme doesn't need this:**
- Pattern matching is simpler than full regex
- No alternation (no `|` operator)
- No lookahead/lookbehind
- Greedy left-to-right is sufficient

#### 3. Bounded Backtracking (Rust `regex` crate approach)

Rust's `regex` crate uses bounded backtracking:
- Explicitly limits repeated work
- Guarantees O(mn) time even with backtracking
- Handles captures efficiently

**What we CAN borrow:**
- The idea of **iterative matching** (not full recursion)
- Clear separation of "before", "repeated", "after" sections
- Length calculation to avoid trial-and-error

### The Correct Algorithm: Greedy Sectioning

Based on R7RS spec and chibi implementation:

```rust
fn match_ellipsis_pattern(
    before: &[Pattern],
    repeated: &Pattern,
    after: &[Pattern],
    exprs: &[Value],
    literals: &[Rc<str>],
    bindings: &mut Bindings,
) -> bool {
    let n = exprs.len();
    let k = before.len();
    let after_count = after.len();

    // Check minimum length
    if n < k + after_count {
        return false;  // Not enough elements
    }

    // Step 1: Match 'before' section (first k elements)
    for i in 0..k {
        if !match_pattern(&before[i], &exprs[i], literals, bindings) {
            return false;
        }
    }

    // Step 2: Match 'after' section (last after_count elements)
    let after_start = n - after_count;
    for i in 0..after_count {
        if !match_pattern(&after[i], &exprs[after_start + i], literals, bindings) {
            return false;
        }
    }

    // Step 3: Match 'repeated' section (middle elements) - GREEDY
    let middle_start = k;
    let middle_end = after_start;
    let middle_exprs = &exprs[middle_start..middle_end];

    // Check if repeated pattern contains nested ellipsis
    if pattern_contains_ellipsis(repeated) {
        // Nested case: ((x ...) ...)
        match_nested_ellipsis(repeated, middle_exprs, literals, bindings)
    } else {
        // Simple case: (x ...)
        match_simple_ellipsis(repeated, middle_exprs, literals, bindings)
    }
}
```

**Key points:**
1. **No backtracking**: Calculate boundaries upfront
2. **Greedy by construction**: Middle section gets all remaining elements
3. **Recursive for nested**: But each level is deterministic
4. **O(n) time**: Single pass through input

### Complexity Analysis

#### Time Complexity

**Single-level ellipsis:** `(x ...)`
- Calculate sections: O(1)
- Match before/after: O(k + after_count) = O(n)
- Match repeated: O(m) where m = middle elements
- **Total: O(n)** where n = input length

**Nested ellipsis:** `((x ...) ...)`
- Outer loop: O(m) iterations where m = number of sublists
- Inner matching: O(n_i) for each sublist of length n_i
- **Total: O(m × avg(n_i))** = O(total elements)
- Still **linear** in total input size!

**Triple nested:** `(((x ...) ...) ...)`
- Three nested loops, but still linear in total elements
- **Total: O(total elements)**

**No exponential blowup** because:
- No backtracking
- No trying multiple alternatives
- Each element matched exactly once

#### Space Complexity

**Single-level:** O(n) for storing bindings
**Nested:** O(n) for nested structure (no duplication)
**Recursion depth:** O(depth of nesting) - typically < 5

### Comparison with Regex Catastrophic Backtracking

**Regex problem:** `(a+)+b` matching "aaaaaaaaa" (no 'b')
- Tries every possible partition: (a)(aaaa...), (aa)(aaa...), etc.
- **Exponential** time: 2^n possibilities

**Why Scheme doesn't have this:**
1. **Fixed right boundary**: "after" section provides clear stopping point
2. **No alternation**: Not trying multiple patterns, just one
3. **Greedy deterministic**: No ambiguity in how to match

**Example that WOULD be catastrophic if we backtracked:**
```scheme
Pattern: ((x ...) ... y ...)
Input: ((1 2 3) (4 5) (6 7 8) (9))

If we backtracked:
  - Try x=((1 2 3) (4 5) (6 7 8) (9)), y=()  [fail if no y match]
  - Try x=((1 2 3) (4 5) (6 7 8)), y=((9))
  - Try x=((1 2 3) (4 5)), y=((6 7 8) (9))
  - ... 2^4 = 16 possibilities

But with greedy sectioning:
  - Calculate: y needs 0 elements (only pattern variable, matches anything)
  - Wait, this is ambiguous!
```

**R7RS Resolution:** Patterns like `(a ... b ...)` where both are pattern variables are **greedy left-to-right**:
- `a` gets as many as possible
- `b` gets the rest
- No ambiguity!

### Key Takeaways for Implementation

1. **Don't implement backtracking**: R7RS semantics are deterministic
2. **Use sectioning algorithm**: Calculate before/after boundaries upfront
3. **Nested = recursive**: Each level uses same algorithm
4. **Borrow from regex**: Iterative approach, length calculation
5. **Avoid regex complexity**: No NFA/DFA, no state machines needed

### What Chibi Teaches Us

Analyzing chibi's implementation (lines 850-1091 in `init-7.scm`):

1. **Implemented entirely in Scheme**: Pattern matching is meta-circular
2. **Uses explicit recursion**: Not hidden in engine
3. **Clear separation**: Pattern parsing, matching, template expansion are distinct phases
4. **No optimization**: Straightforward algorithm, relies on Scheme compiler
5. **Correctness first**: Performance is good enough

**For Patina:**
- We CAN implement this in Rust without exotic techniques
- Focus on **correctness** (match R7RS spec exactly)
- **Linear time** is achievable with simple algorithm
- **No regex engine needed** (too much complexity)

---

## Proposed Design

### Approach: Depth-Aware Bindings with Recursive Structure

We'll extend `BindingValue` to support nested structures and track ellipsis depth during matching and expansion.

### Data Structure Changes

#### 1. Extended BindingValue

**File:** `src/macro_system/mod.rs`

```rust
#[derive(Debug, Clone)]
pub enum BindingValue {
    /// Single value: x -> 42
    Single(Value),

    /// Multiple values (depth 1): x -> [1, 2, 3]
    Multiple(Vec<Value>),

    /// Nested values (depth 2+): x -> [[1, 2], [3, 4], [5, 6]]
    /// Each inner BindingValue can be Single, Multiple, or Nested recursively
    Nested(Vec<BindingValue>),
}
```

**Why recursive?**
- Supports arbitrary nesting depth (depth 2, 3, 4, ...)
- Natural representation of multi-dimensional data
- Matches R7RS semantics directly

**Example bindings:**

```rust
// Pattern: (x ...)
// Input: (1 2 3)
// Binding: x -> Multiple([1, 2, 3])

// Pattern: ((x y) ...)
// Input: ((1 2) (3 4) (5 6))
// Binding:
//   x -> Multiple([1, 3, 5])
//   y -> Multiple([2, 4, 6])

// Pattern: ((x ...) ...)
// Input: ((1 2 3) (4 5) (6 7 8 9))
// Binding:
//   x -> Nested([
//     Multiple([1, 2, 3]),
//     Multiple([4, 5]),
//     Multiple([6, 7, 8, 9])
//   ])
```

#### 2. Ellipsis Depth Tracking

We need to track the depth of ellipsis patterns during parsing, matching, and expansion.

**Option A: Add depth field to Pattern/Template**

```rust
pub enum Pattern {
    // ... existing variants ...
    Ellipsis {
        before: Vec<Pattern>,
        repeated: Box<Pattern>,
        after: Vec<Pattern>,
        depth: usize,  // NEW: 1 for "...", 2 for "... ...", etc.
    },
}
```

**Option B: Count consecutive ellipses during parsing**

```rust
// In parse_list_pattern:
// When we find "...", check if next element is also "..."
let mut ellipsis_count = 1;
while items.get(pos + ellipsis_count) == Some(&Value::Symbol("...".into())) {
    ellipsis_count += 1;
}
```

**Recommendation: Option B (simpler, no data structure change)**

The consecutive ellipses `... ...` can be detected during parsing and handled specially without changing the `Pattern` enum.

#### 3. Pattern Matching Algorithm

**File:** `src/macro_system/pattern.rs`

Current implementation at line 126-202 handles single-level ellipsis. We need to extend it:

```rust
fn match_ellipsis_pattern(
    before: &[Pattern],
    repeated: &Pattern,
    after: &[Pattern],
    exprs: &[Value],
    literals: &[Rc<str>],
    bindings: &mut Bindings,
) -> bool {
    // ... existing before/after matching ...

    // Match repeated pattern (middle section)
    let middle_start = before.len();
    let middle_end = exprs.len() - after.len();
    let middle_exprs = &exprs[middle_start..middle_end];

    // Check if repeated pattern contains variables with ellipsis
    // If so, we're dealing with nested ellipsis
    let contains_nested_ellipsis = pattern_contains_ellipsis(repeated);

    if contains_nested_ellipsis {
        // NEW: Nested ellipsis handling
        match_nested_ellipsis(repeated, middle_exprs, literals, bindings)
    } else {
        // Existing: Single-level ellipsis
        match_simple_ellipsis(repeated, middle_exprs, literals, bindings)
    }
}

// NEW function
fn match_nested_ellipsis(
    pattern: &Pattern,
    exprs: &[Value],
    literals: &[Rc<str>],
    bindings: &mut Bindings,
) -> bool {
    // For each expression, match against pattern
    // Collect bindings into nested structure

    let mut nested_bindings: HashMap<Rc<str>, Vec<BindingValue>> = HashMap::new();

    for expr in exprs {
        let mut temp_bindings = Bindings::new();

        if !match_pattern_impl(pattern, expr, literals, &mut temp_bindings) {
            return false;
        }

        // Accumulate each variable's binding
        for (name, binding) in temp_bindings {
            nested_bindings
                .entry(name)
                .or_default()
                .push(binding);
        }
    }

    // Convert to Nested bindings
    for (name, binding_vec) in nested_bindings {
        bindings.insert(name, BindingValue::Nested(binding_vec));
    }

    true
}

// Helper: Check if pattern contains ellipsis
fn pattern_contains_ellipsis(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Ellipsis { .. } => true,
        Pattern::List(patterns) | Pattern::Vector(patterns) => {
            patterns.iter().any(pattern_contains_ellipsis)
        }
        _ => false,
    }
}
```

**Key Algorithm Points:**

1. Detect nested ellipsis by checking if repeated pattern contains ellipsis
2. For each outer iteration, collect bindings as `BindingValue` (can be Single or Multiple)
3. Wrap collected bindings in `BindingValue::Nested`
4. Preserve the structure: `[[a, b], [c, d]]` not `[a, b, c, d]`

#### 4. Template Expansion Algorithm

**File:** `src/macro_system/template.rs`

Current implementation at line 88-161 handles single-level ellipsis expansion. We need to extend it:

```rust
// In expand_template_impl for Template::Ellipsis case:

// Find pattern variables in repeated template
let vars = find_pattern_vars(repeated);

if vars.is_empty() {
    return Err(EvalError::InvalidSyntax(
        "Ellipsis template contains no pattern variables".to_string()
    ));
}

// Check the type of first variable's binding to determine depth
let first_binding = bindings.get(&vars[0])
    .ok_or_else(|| EvalError::InvalidSyntax(format!(
        "Unbound pattern variable: {}", vars[0]
    )))?;

match first_binding {
    BindingValue::Single(_) => {
        return Err(EvalError::InvalidSyntax(format!(
            "Pattern variable {} used with ellipsis but not bound with ellipsis",
            vars[0]
        )));
    }

    BindingValue::Multiple(values) => {
        // Existing single-level expansion
        expand_simple_ellipsis(repeated, &vars, values.len(), bindings, result, depth)
    }

    BindingValue::Nested(nested_values) => {
        // NEW: Nested ellipsis expansion
        expand_nested_ellipsis(repeated, &vars, nested_values, bindings, result, depth)
    }
}

// NEW function
fn expand_nested_ellipsis(
    repeated: &Template,
    vars: &[Rc<str>],
    nested_values: &[BindingValue],
    bindings: &Bindings,
    result: &mut Vec<Value>,
    depth: usize,
) -> Result<(), EvalError> {
    // Verify all variables at this depth have same length
    for var in vars {
        match bindings.get(var) {
            Some(BindingValue::Nested(v)) if v.len() == nested_values.len() => {}
            _ => {
                return Err(EvalError::InvalidSyntax(
                    "Pattern variables in nested ellipsis have different lengths".to_string()
                ));
            }
        }
    }

    // For each outer iteration
    for i in 0..nested_values.len() {
        // Create bindings for this iteration
        // Inner bindings unwrap one level of nesting
        let mut iter_bindings = bindings.clone();

        for var in vars {
            if let Some(BindingValue::Nested(v)) = bindings.get(var) {
                // Unwrap one level: Nested([Multiple([1,2]), Multiple([3,4])])
                // becomes Multiple([1,2]) for first iteration
                iter_bindings.insert(var.clone(), v[i].clone());
            }
        }

        // Recursively expand with unwrapped bindings
        result.push(expand_template_impl(repeated, &iter_bindings, depth + 1)?);
    }

    Ok(())
}
```

**Key Algorithm Points:**

1. Detect nested ellipsis by checking binding type (`BindingValue::Nested`)
2. For each outer iteration, unwrap one level of nesting
3. Inner template expansion sees the unwrapped bindings (e.g., `Multiple` instead of `Nested`)
4. Recursive expansion naturally handles deeper nesting (depth 3+)

---

## Implementation Roadmap

### Phase 1: Data Structure Extension (Day 1, 3-4 hours)

**Goal:** Add `Nested` variant to `BindingValue` without breaking existing code.

**Tasks:**
- [ ] Add `Nested(Vec<BindingValue>)` variant to `BindingValue` enum
- [ ] Update `Display` implementation for new variant
- [ ] Update all `match` statements on `BindingValue` to handle new variant
- [ ] Ensure existing tests still pass (should be backward compatible)

**Files to modify:**
- `src/macro_system/mod.rs` (add variant)
- `src/macro_system/pattern.rs` (update matches)
- `src/macro_system/template.rs` (update matches)

**Success criteria:**
- ✅ Code compiles
- ✅ All existing tests pass
- ✅ No functional changes yet (new variant not used)

### Phase 2: Pattern Matching Enhancement (Day 1-2, 6-8 hours)

**Goal:** Implement nested ellipsis pattern matching.

**Tasks:**
- [ ] Add `pattern_contains_ellipsis()` helper function
- [ ] Add `match_nested_ellipsis()` function
- [ ] Modify `match_ellipsis_pattern()` to detect and handle nested case
- [ ] Remove the `return false` at line 190 in pattern.rs
- [ ] Write unit tests for nested pattern matching

**Files to modify:**
- `src/macro_system/pattern.rs` (lines 126-202)

**Test cases:**
```rust
#[test]
fn test_nested_ellipsis_simple() {
    // Pattern: ((x y) ...)
    let pattern = Pattern::Ellipsis {
        before: vec![],
        repeated: Box::new(Pattern::List(vec![
            Pattern::Variable("x".into()),
            Pattern::Variable("y".into()),
        ])),
        after: vec![],
    };

    // Input: ((1 2) (3 4) (5 6))
    let input = make_list(vec![
        make_list(vec![Value::Integer(1), Value::Integer(2)]),
        make_list(vec![Value::Integer(3), Value::Integer(4)]),
        make_list(vec![Value::Integer(5), Value::Integer(6)]),
    ]);

    let bindings = match_pattern(&pattern, &input, &[]).unwrap();

    // x should be bound to Multiple([1, 3, 5])
    // y should be bound to Multiple([2, 4, 6])
    assert!(matches!(
        bindings.get(&Rc::from("x")),
        Some(BindingValue::Multiple(v)) if v.len() == 3
    ));
}

#[test]
fn test_nested_ellipsis_double() {
    // Pattern: ((x ...) ...)
    let inner_ellipsis = Pattern::Ellipsis {
        before: vec![],
        repeated: Box::new(Pattern::Variable("x".into())),
        after: vec![],
    };

    let pattern = Pattern::Ellipsis {
        before: vec![],
        repeated: Box::new(inner_ellipsis),
        after: vec![],
    };

    // Input: ((1 2 3) (4 5) (6 7 8 9))
    let input = make_list(vec![
        make_list(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)]),
        make_list(vec![Value::Integer(4), Value::Integer(5)]),
        make_list(vec![Value::Integer(6), Value::Integer(7), Value::Integer(8), Value::Integer(9)]),
    ]);

    let bindings = match_pattern(&pattern, &input, &[]).unwrap();

    // x should be bound to Nested([
    //   Multiple([1, 2, 3]),
    //   Multiple([4, 5]),
    //   Multiple([6, 7, 8, 9])
    // ])
    assert!(matches!(
        bindings.get(&Rc::from("x")),
        Some(BindingValue::Nested(v)) if v.len() == 3
    ));
}
```

**Success criteria:**
- ✅ All new unit tests pass
- ✅ Existing pattern matching tests still pass
- ✅ Can match patterns with nested ellipsis
- ✅ Bindings have correct nested structure

### Phase 3: Template Expansion Enhancement (Day 2-3, 6-8 hours)

**Goal:** Implement nested ellipsis template expansion.

**Tasks:**
- [ ] Add `expand_nested_ellipsis()` helper function
- [ ] Modify ellipsis expansion in `expand_template_impl()` to detect and handle nested bindings
- [ ] Implement iterative unwrapping of nested structure
- [ ] Write unit tests for nested template expansion

**Files to modify:**
- `src/macro_system/template.rs` (lines 88-161)

**Test cases:**
```rust
#[test]
fn test_expand_nested_ellipsis_simple() {
    // Template: (list x ... ...)
    // This is a template with double ellipsis
    // But for this test, we'll use structured template

    let template = Template::Ellipsis {
        before: vec![Template::Variable("let".into())],
        repeated: Box::new(Template::Ellipsis {
            before: vec![],
            repeated: Box::new(Template::List(vec![
                Template::Variable("var".into()),
                Template::Variable("val".into()),
            ])),
            after: vec![],
        }),
        after: vec![Template::Variable("body".into())],
    };

    let mut bindings = Bindings::new();

    // Nested binding structure:
    // var -> Nested([Multiple([a, b]), Multiple([c, d])])
    // val -> Nested([Multiple([1, 2]), Multiple([3, 4])])
    bindings.insert("var".into(), BindingValue::Nested(vec![
        BindingValue::Multiple(vec![
            Value::Symbol("a".into()),
            Value::Symbol("b".into()),
        ]),
        BindingValue::Multiple(vec![
            Value::Symbol("c".into()),
            Value::Symbol("d".into()),
        ]),
    ]));

    bindings.insert("val".into(), BindingValue::Nested(vec![
        BindingValue::Multiple(vec![Value::Integer(1), Value::Integer(2)]),
        BindingValue::Multiple(vec![Value::Integer(3), Value::Integer(4)]),
    ]));

    bindings.insert("body".into(), BindingValue::Single(Value::Symbol("result".into())));

    let result = expand_template(&template, &bindings).unwrap();

    // Should expand to:
    // (let (a 1) (b 2) (c 3) (d 4) result)
    let expected = make_list(vec![
        Value::Symbol("let".into()),
        make_list(vec![Value::Symbol("a".into()), Value::Integer(1)]),
        make_list(vec![Value::Symbol("b".into()), Value::Integer(2)]),
        make_list(vec![Value::Symbol("c".into()), Value::Integer(3)]),
        make_list(vec![Value::Symbol("d".into()), Value::Integer(4)]),
        Value::Symbol("result".into()),
    ]);

    assert_eq!(format!("{}", result), format!("{}", expected));
}
```

**Success criteria:**
- ✅ All new unit tests pass
- ✅ Existing template expansion tests still pass
- ✅ Can expand templates with nested ellipsis
- ✅ Output structure is correct (flat when appropriate)

### Phase 4: Integration Testing (Day 3-4, 4-6 hours)

**Goal:** Test end-to-end macro expansion with nested ellipsis.

**Tasks:**
- [ ] Create integration test file: `tests/compliance/macros_nested_ellipsis.rs`
- [ ] Test basic nested ellipsis macro (matrix transpose example)
- [ ] Test R7RS `be-like-begin` example
- [ ] Test practical use case (let-values style macro)
- [ ] Test error cases (mismatched depths, length mismatches)

**Test cases:**
```rust
#[test]
fn test_macro_be_like_begin() {
    // From R7RS spec
    assert_program_eval_to(
        r#"
        (define-syntax be-like-begin
          (syntax-rules ()
            ((be-like-begin name)
             (define-syntax name
               (syntax-rules ()
                 ((name expr (... ...))
                  (begin expr (... ...))))))))

        (be-like-begin sequence)
        (sequence 1 2 3 4)
        "#,
        "4",
    );
}

#[test]
fn test_macro_matrix_transpose() {
    // Practical example: transpose a matrix represented as nested lists
    assert_program_eval_to(
        r#"
        (define-syntax transpose
          (syntax-rules ()
            ((transpose ((row ...) ...))
             (list (list row ...) ...))))

        (transpose ((1 2 3) (4 5 6) (7 8 9)))
        "#,
        "((1 4 7) (2 5 8) (3 6 9))",
    );
}

#[test]
fn test_macro_flatten() {
    assert_program_eval_to(
        r#"
        (define-syntax flatten
          (syntax-rules ()
            ((flatten ((x ...) ...))
             (list x ... ...))))

        (flatten ((1 2) (3 4) (5 6)))
        "#,
        "(1 2 3 4 5 6)",
    );
}
```

**Error case tests:**
```rust
#[test]
fn test_nested_ellipsis_depth_mismatch() {
    // Pattern has depth 2, template has depth 1 - should error
    let result = eval_program(r#"
        (define-syntax bad-macro
          (syntax-rules ()
            ((bad-macro ((x ...) ...))
             (list x ...))))

        (bad-macro ((1 2) (3 4)))
    "#);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("ellipsis"));
}
```

**Success criteria:**
- ✅ All integration tests pass
- ✅ R7RS example macros work correctly
- ✅ Error messages are clear and helpful
- ✅ No regressions in existing macro tests

### Phase 5: Documentation and Cleanup (Day 4-5, 2-3 hours)

**Goal:** Document the feature and clean up code.

**Tasks:**
- [ ] Add documentation comments to new functions
- [ ] Update `NESTED_ELLIPSIS_LIMITATION.md` to mark as implemented
- [ ] Update `FEATURE_STATUS.md` to reflect new capability
- [ ] Add examples to `tests/fixtures/examples/macros/03_nested_ellipsis.scm`
- [ ] Run `cargo clippy` and address warnings
- [ ] Run `cargo fmt`

**Files to update:**
- `internal/NESTED_ELLIPSIS_LIMITATION.md`
- `docs/FEATURE_STATUS.md`
- `tests/fixtures/examples/macros/03_nested_ellipsis.scm` (new file)

**Success criteria:**
- ✅ All documentation updated
- ✅ No clippy warnings
- ✅ Code is well-formatted
- ✅ Examples demonstrate the feature

---

## Testing Strategy

### Unit Tests

**Pattern Matching (`src/macro_system/pattern.rs`):**
- [ ] Simple nested pattern: `((x y) ...)`
- [ ] Double ellipsis: `((x ...) ...)`
- [ ] Triple ellipsis: `(((x ...) ...) ...)`
- [ ] Nested with before/after: `(keyword ((x y) ...) trailer)`
- [ ] Empty nested: `(() ...)`
- [ ] Mixed depth variables in same pattern

**Template Expansion (`src/macro_system/template.rs`):**
- [ ] Simple nested expansion
- [ ] Double ellipsis expansion
- [ ] Triple ellipsis expansion
- [ ] Nested with literals
- [ ] Zero-length nested expansion
- [ ] Length mismatch errors

### Integration Tests

**End-to-End Macros (`tests/compliance/macros_nested_ellipsis.rs`):**
- [ ] R7RS `be-like-begin` example
- [ ] Matrix transpose macro
- [ ] Flatten nested lists macro
- [ ] let-values style macro
- [ ] Recursive macro with nested ellipsis

### Scheme Test Files

**File:** `tests/fixtures/examples/macros/03_nested_ellipsis.scm`

```scheme
(import (scheme base) (scheme write))

(display "=== Nested Ellipsis Test Suite ===\n\n")

;; Test 1: be-like-begin (from R7RS spec)
(display "Test 1: be-like-begin\n")
(define-syntax be-like-begin
  (syntax-rules ()
    ((be-like-begin name)
     (define-syntax name
       (syntax-rules ()
         ((name expr (... ...))
          (begin expr (... ...))))))))

(be-like-begin sequence)
(define result1 (sequence 1 2 3 4))
(display "  Expected: 4\n")
(display "  Got: ")
(display result1)
(display "\n")
(if (= result1 4)
    (display "  PASS\n")
    (display "  FAIL\n"))

;; Test 2: Matrix transpose
(display "\nTest 2: Matrix transpose\n")
(define-syntax transpose
  (syntax-rules ()
    ((transpose ((col ...) ...))
     (list (list col ...) ...))))

(define matrix '((1 2 3) (4 5 6)))
(define result2 (transpose ((1 2 3) (4 5 6))))
(display "  Expected: ((1 4) (2 5) (3 6))\n")
(display "  Got: ")
(display result2)
(display "\n")

;; Test 3: Flatten nested lists
(display "\nTest 3: Flatten\n")
(define-syntax flatten
  (syntax-rules ()
    ((flatten ((x ...) ...))
     (list x ... ...))))

(define result3 (flatten ((1 2) (3 4) (5 6))))
(display "  Expected: (1 2 3 4 5 6)\n")
(display "  Got: ")
(display result3)
(display "\n")

(display "\n=== Nested ellipsis tests complete ===\n")
```

### Regression Testing

All existing tests must continue to pass:
- `cargo test --test compliance` (283 tests)
- Especially macro tests in `tests/compliance/macros.rs`

---

## Migration Path

### Special Forms to Bootstrap Macros

Once nested ellipsis is implemented, we can move some special forms to bootstrap macros:

**Current Hard-Coded Special Forms:**
- `and` (150 LOC in special_forms.rs)
- `or` (140 LOC in special_forms.rs)
- Some `let` variants could be simplified

**Bootstrap Macro Implementations:**

```scheme
;; lib/bootstrap.scm

;; and macro (currently 150 LOC in Rust)
(define-syntax and
  (syntax-rules ()
    ((and) #t)
    ((and test) test)
    ((and test1 test2 ...)
     (if test1 (and test2 ...) #f))))

;; or macro (currently 140 LOC in Rust)
(define-syntax or
  (syntax-rules ()
    ((or) #f)
    ((or test) test)
    ((or test1 test2 ...)
     (let ((temp test1))
       (if temp temp (or test2 ...))))))
```

**Code Reduction:**
- Remove ~300 LOC from `special_forms.rs`
- Simplify evaluator logic
- Move complexity to standard Scheme (more maintainable)
- Prepare for future VM/JIT work (simpler interpreter core)

**Migration Strategy:**
1. Implement nested ellipsis
2. Add bootstrap macros to `lib/bootstrap.scm`
3. Test both implementations in parallel
4. Remove Rust special form implementations
5. Update evaluator to load bootstrap library

---

## Reference Examples

### R7RS Specification Example

**From expr.tex:1776-1786:**

```scheme
(define-syntax be-like-begin
  (syntax-rules ()
    ((be-like-begin name)
     (define-syntax name
       (syntax-rules ()
         ((name expr (... ...))
          (begin expr (... ...))))))))

(be-like-begin sequence)
(sequence 1 2 3 4)  ;=> 4
```

**How it works:**
1. `be-like-begin` is a macro that creates another macro
2. The inner macro uses `(... ...)` to produce literal `...`
3. Result: `sequence` becomes a `begin`-like macro
4. Requires ellipsis escape feature (already implemented)

### Practical Examples

#### Example 1: Matrix Transpose

```scheme
;; Transpose a 2D matrix
(define-syntax transpose
  (syntax-rules ()
    ((transpose ((col ...) ...))
     (list (list col ...) ...))))

(transpose ((1 2 3)
            (4 5 6)
            (7 8 9)))
;=> ((1 4 7) (2 5 8) (3 6 9))
```

**Pattern matching:**
- Input: `((1 2 3) (4 5 6) (7 8 9))`
- Pattern: `((col ...) ...)`
- Outer ellipsis matches 3 sublists
- Inner ellipsis matches elements in each sublist
- Binding: `col -> Nested([Multiple([1,2,3]), Multiple([4,5,6]), Multiple([7,8,9])])`

**Template expansion:**
- Template: `(list (list col ...) ...)`
- Inner `col ...` expands each row: `(list 1 2 3)`, `(list 4 5 6)`, etc.
- Outer `...` distributes across iterations
- Result: `(list (list 1 4 7) (list 2 5 8) (list 3 6 9))`

#### Example 2: Flatten Nested Lists

```scheme
(define-syntax flatten
  (syntax-rules ()
    ((flatten ((x ...) ...))
     (list x ... ...))))

(flatten ((1 2) (3 4) (5 6)))
;=> (1 2 3 4 5 6)
```

**Pattern matching:**
- Input: `((1 2) (3 4) (5 6))`
- Binding: `x -> Nested([Multiple([1,2]), Multiple([3,4]), Multiple([5,6])])`

**Template expansion:**
- Template: `(list x ... ...)`
- `x ...` expands first level: `1 2`, then `3 4`, then `5 6`
- `... ...` (second ellipsis) iterates outer level
- Result: `(list 1 2 3 4 5 6)` (flattened)

#### Example 3: let-values (Simplified)

```scheme
;; Simplified let-values implementation
(define-syntax my-let-values
  (syntax-rules ()
    ((my-let-values (((var ...) expr)) body)
     (call-with-values (lambda () expr)
       (lambda (var ...) body)))))

(my-let-values (((a b) (values 1 2)))
  (+ a b))
;=> 3
```

**This doesn't actually require nested ellipsis!**
The `(var ...)` is inside a single binding form, not nested.

**True nested ellipsis version:**

```scheme
;; Multiple bindings version
(define-syntax my-let-values*
  (syntax-rules ()
    ((my-let-values* (((vars ...) exprs) ...) body)
     (let-values (((vars ...) exprs) ...) body))))

;; This requires nested ellipsis because:
;; - vars appears with one ellipsis inside the binding
;; - The whole binding appears with another ellipsis
```

---

## Risk Analysis

### Technical Risks

#### Risk 1: Complexity of Nested Structures

**Risk:** Multi-dimensional `BindingValue` structures become hard to debug.

**Mitigation:**
- Add comprehensive `Display` implementation showing structure
- Add debug helpers to pretty-print bindings
- Write extensive unit tests at each level
- Use property-based testing (quickcheck) to generate random nested patterns

#### Risk 2: Performance Degradation

**Risk:** Recursive nested expansion is slower than single-level.

**Mitigation:**
- Profile before and after implementation
- Most macros don't use nested ellipsis (< 5% impact)
- Can optimize later with caching if needed
- Consider shallow vs deep nesting performance

**Measurement:**
```bash
# Before:
cargo bench --bench macro_expansion

# After:
cargo bench --bench macro_expansion
# Compare single-level ellipsis performance
```

#### Risk 3: Edge Cases

**Risk:** Unexpected interactions with other features (hygiene, ellipsis escape).

**Mitigation:**
- Test combinations explicitly
- Review R7RS spec carefully for edge cases
- Compare behavior with chibi-scheme on same inputs
- Add fuzzing tests for pattern/template parsing

#### Risk 4: Breaking Changes

**Risk:** Extending `BindingValue` might break existing code.

**Mitigation:**
- Make changes backward compatible
- Ensure all existing tests pass
- Add `Nested` variant last in enum (doesn't change discriminant values)
- Run full test suite after each phase

### Implementation Risks

#### Risk 5: Scope Creep

**Risk:** Implementation takes longer than 5 days.

**Mitigation:**
- Strict phase boundaries with clear success criteria
- Ship Phase 1-3 first, defer optimization
- Mark tests as `#[ignore]` if blocked
- Time-box each phase (stop if > 8 hours)

#### Risk 6: Incomplete Testing

**Risk:** Tests don't cover all edge cases.

**Mitigation:**
- Review R7RS spec test cases
- Check chibi-scheme test suite for examples
- Use mutation testing to verify test quality
- Add tests for error cases (not just happy path)

### User Impact Risks

#### Risk 7: Confusing Error Messages

**Risk:** Nested ellipsis errors are hard to understand.

**Mitigation:**
- Write clear error messages with examples
- Show the pattern and template in error output
- Indicate which variable has depth mismatch
- Provide suggestions for fixing

**Example error message:**
```
Error: Nested ellipsis depth mismatch
  Pattern variable 'x' appears under 2 ellipses: ((x ...) ...)
  But template uses 'x' with only 1 ellipsis: (list x ...)

  Expected: (list x ... ...) or similar
  Got: (list x ...)

  Hint: Each ellipsis in the pattern requires a matching ellipsis in the template
```

---

## Effort Estimation

### Detailed Breakdown

| Phase | Tasks | Time | LOC | Difficulty |
|-------|-------|------|-----|------------|
| **Phase 1:** Data Structure | Add `Nested` variant, update matches | 3-4 hours | ~50 | Low |
| **Phase 2:** Pattern Matching | New algorithm, helper functions | 6-8 hours | ~200 | High |
| **Phase 3:** Template Expansion | Nested expansion logic | 6-8 hours | ~150 | High |
| **Phase 4:** Integration Tests | End-to-end testing | 4-6 hours | ~300 | Medium |
| **Phase 5:** Documentation | Docs, examples, cleanup | 2-3 hours | ~100 | Low |
| **Total** | | **21-29 hours** | **~800** | **3-5 days** |

### Lines of Code Impact

**Files Modified:**
- `src/macro_system/mod.rs`: +10 lines (add variant)
- `src/macro_system/pattern.rs`: +150 lines (nested matching)
- `src/macro_system/template.rs`: +120 lines (nested expansion)
- `tests/compliance/macros_nested_ellipsis.rs`: +250 lines (new file)
- `tests/fixtures/examples/macros/03_nested_ellipsis.scm`: +80 lines (new file)

**Total:** ~610 new lines, 190 modified lines = ~800 LOC

**Code Reduction Opportunity:**
- After migration to bootstrap macros: -300 LOC from `special_forms.rs`
- Net effect: +500 LOC for feature, -300 LOC from migration = +200 LOC total

---

## Success Criteria

### Functional Requirements

✅ **Pattern Matching:**
- [ ] Can match patterns with depth-2 nested ellipsis
- [ ] Can match patterns with depth-3+ nested ellipsis
- [ ] Correctly creates `Nested` bindings
- [ ] Handles zero-length nested matches
- [ ] Validates depth consistency

✅ **Template Expansion:**
- [ ] Can expand templates with depth-2 nested ellipsis
- [ ] Can expand templates with depth-3+ nested ellipsis
- [ ] Correctly unwraps `Nested` bindings
- [ ] Validates variable depths match pattern
- [ ] Produces correct flat output

✅ **Integration:**
- [ ] R7RS `be-like-begin` example works
- [ ] Matrix transpose macro works
- [ ] Flatten macro works
- [ ] Error messages are clear and helpful

### Quality Requirements

✅ **Testing:**
- [ ] 100% unit test coverage for new code
- [ ] All integration tests pass
- [ ] No regressions in existing tests (283 compliance tests)
- [ ] Scheme test file demonstrates feature

✅ **Performance:**
- [ ] No measurable performance impact on single-level ellipsis
- [ ] Nested ellipsis expansion completes in < 100ms for typical cases
- [ ] No stack overflows for reasonable nesting depth (< 10 levels)

✅ **Code Quality:**
- [ ] No clippy warnings
- [ ] Code is formatted with `cargo fmt`
- [ ] All public functions have documentation comments
- [ ] Clear error messages with examples

### Compliance Requirements

✅ **R7RS-small Compliance:**
- [ ] Implements section 4.3.2 (Pattern Language) fully
- [ ] Handles nested ellipsis as specified in expr.tex:1744-1747
- [ ] Supports arbitrary nesting depth (not limited to depth 2)
- [ ] Compatible with chibi-scheme behavior on test cases

---

## Future Enhancements (Out of Scope)

These are not part of the initial implementation but could be added later:

### 1. Performance Optimizations

**Caching:**
- Cache expanded macros to avoid re-expansion
- Memoize pattern matching results
- Use copy-on-write for bindings

**Complexity:** Medium
**Benefit:** 2-5x speedup for repeated macro uses
**Priority:** Low (optimize only if profiling shows need)

### 2. Better Error Messages

**Enhanced diagnostics:**
- Show visual diff of expected vs actual structure
- Highlight problematic variables in patterns
- Suggest common fixes for depth mismatches

**Complexity:** Low
**Benefit:** Improved developer experience
**Priority:** Medium (add in Phase 5 if time allows)

### 3. Macro Debugging Tools

**REPL commands:**
- `(expand-macro 'macro-name expr)` - Show expansion steps
- `(trace-macro 'macro-name)` - Enable expansion tracing
- `(macro-bindings 'macro-name expr)` - Show pattern bindings

**Complexity:** Medium
**Benefit:** Much easier macro development
**Priority:** Medium (good addition for Phase 2)

### 4. Syntax Objects (R6RS)

**Full syntax object system:**
- Richer hygiene with syntax objects
- Better source location tracking
- `syntax-case` support

**Complexity:** Very High
**Benefit:** More powerful macros (R6RS level)
**Priority:** Low (R7RS-small doesn't require this)

---

## References

### R7RS Specification

**Primary source:**
- `spec/r7rs-small-spec/expr.tex` lines 1443-1850 (Macro section)
- Lines 1744-1747: Nested ellipsis specification
- Lines 1776-1786: `be-like-begin` example

**Key quotes:**
> "Pattern variables that occur in subpatterns followed by one or more instances of the identifier ellipsis are allowed only in subtemplates that are followed by as many instances of ellipsis."

### Implementation References

**Chibi-scheme (R7RS reference implementation):**
- Location: `~/Project/reference/chibi-scheme`
- Files: `eval.c` (macro expansion), `tests/r7rs-tests.scm` (compliance tests)
- Note: Uses C implementation, different approach than Patina

**Steel-scheme (Rust reference):**
- Patina's macro system is based on Steel's design
- Steel uses native Rust pattern matching
- Location: `~/Project/reference/steel` (if available)

### Internal Documentation

**Patina docs:**
- `internal/NESTED_ELLIPSIS_LIMITATION.md` - Current limitations (this file)
- `PRD/ARCHIVE/phase1_completed/MACRO_IMPLEMENTATION_DESIGN.md` - Macro system design
- `docs/FEATURE_STATUS.md` - Feature tracking
- `internal/ARCHIVE/macro_research/` - Research notes

### Academic Papers

**Hygienic macros:**
- "Macros That Work" (Clinger & Rees, 1991)
- "Syntactic Abstraction in Scheme" (Dybvig et al., 1993)
- R7RS Bibliography (spec/r7rs-small-spec/r7rs.bib)

---

## Appendix: Algorithm Pseudocode

### Pattern Matching Algorithm (Depth-Aware)

```
function match_ellipsis_pattern(pattern, exprs):
    # Detect if this is nested ellipsis
    if pattern contains Ellipsis:
        return match_nested_ellipsis(pattern, exprs)
    else:
        return match_simple_ellipsis(pattern, exprs)

function match_nested_ellipsis(pattern, exprs):
    nested_bindings = {}

    # For each expression in the input
    for expr in exprs:
        temp_bindings = {}

        # Match pattern against this expression
        if not match_pattern(pattern, expr, temp_bindings):
            return FAIL

        # Accumulate bindings (creating nested structure)
        for (var, binding) in temp_bindings:
            nested_bindings[var].append(binding)

    # Convert to BindingValue::Nested
    for (var, binding_list) in nested_bindings:
        bindings[var] = Nested(binding_list)

    return SUCCESS

function match_simple_ellipsis(pattern, exprs):
    repeated_bindings = {}

    for expr in exprs:
        temp_bindings = {}

        if not match_pattern(pattern, expr, temp_bindings):
            return FAIL

        for (var, binding) in temp_bindings:
            # Only single values, no nesting
            repeated_bindings[var].append(binding.value)

    for (var, value_list) in repeated_bindings:
        bindings[var] = Multiple(value_list)

    return SUCCESS
```

### Template Expansion Algorithm (Depth-Aware)

```
function expand_ellipsis(template, bindings):
    vars = find_pattern_vars(template)
    first_binding = bindings[vars[0]]

    match first_binding:
        case Single(_):
            ERROR: "Variable used with ellipsis but not bound with ellipsis"

        case Multiple(values):
            # Single-level expansion (existing code)
            return expand_simple_ellipsis(template, bindings, values.length)

        case Nested(nested_values):
            # Multi-level expansion (new code)
            return expand_nested_ellipsis(template, bindings, nested_values)

function expand_nested_ellipsis(template, bindings, nested_values):
    result = []

    # Validate all variables have same length at this depth
    for var in template_vars:
        if bindings[var].length != nested_values.length:
            ERROR: "Depth mismatch"

    # For each outer iteration
    for i in 0..nested_values.length:
        # Create bindings for this iteration
        # Unwrap one level of nesting
        iter_bindings = {}
        for var in template_vars:
            iter_bindings[var] = nested_values[i]  # Unwrap Nested -> Multiple or Single

        # Recursively expand with unwrapped bindings
        expanded = expand_template(template, iter_bindings)
        result.append(expanded)

    return result
```

---

## Document Status

- **Version:** 2.0
- **Created:** 2025-11-10
- **Last Updated:** 2025-11-10
- **Status:** Enhanced with Regex-Inspired Approach, Ready for Implementation
- **Changes in v2.0:**
  - Added comprehensive regex-inspired approach section
  - Analyzed chibi-scheme implementation strategy
  - Detailed R7RS greedy matching semantics
  - Provided complexity analysis proving linear time
  - Clarified why backtracking is NOT needed
- **Reviewers:** (To be filled after review)
- **Approved:** (To be filled after approval)

---

## Next Steps

1. **Review this document** for accuracy and completeness
2. **Estimate effort** with team (if applicable)
3. **Schedule implementation** in sprint/milestone
4. **Begin Phase 1** when ready
5. **Update this document** as implementation progresses

**Point of Contact:** Implementation team / maintainer

---

*End of Document*
