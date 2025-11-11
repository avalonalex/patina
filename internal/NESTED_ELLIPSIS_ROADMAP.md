# Roadmap: Migrating to Gauche-Style Nested Ellipsis

**Date:** 2025-11-10
**Status:** Future Enhancement (Not Required for R7RS)
**Prerequisites:** Current implementation complete
**Related:** `ARCHIVE/macro_research/MACRO_ARCHITECTURE_DECISIONS.md`, `NESTED_ELLIPSIS_LIMITATION.md`

## Executive Summary

This document outlines a **step-by-step migration path** from our current "nested ellipsis in after" approach to Gauche's index-based tree walking approach, which would enable **true nested ellipsis** patterns like `((x ...) ...)`.

**Important:** This is **not required** for R7RS compliance. This is only needed for:
- SRFI-46: Basic Syntax-rules Extensions
- SRFI-149: Extended Syntax-rules
- Advanced macro metaprogramming

**Estimated effort:** 2-3 weeks for experienced Rust developer

---

## Current vs Future Capabilities

### What Works Now (Our Approach)

```scheme
;; Multiple ellipses at same level ✅
(define-syntax test
  (syntax-rules ()
    ((_ ((x y) ...) z ...)
     (list x ... z ...))))

(test ((1 2) (3 4)) 5 6 7)
;; → (1 3 5 6 7)
```

**Bindings:**
```rust
{
    "x": Multiple([1, 3]),
    "y": Multiple([2, 4]),
    "z": Multiple([5, 6, 7])
}
```

### What Doesn't Work (True Nested Ellipsis)

```scheme
;; Nested ellipsis ❌
(define-syntax matrix
  (syntax-rules ()
    ((_ ((x ...) ...))
     (quote ((x ...) ...)))))

(matrix ((1 2 3) (4 5) (6 7 8 9)))
;; Should produce: ((1 2 3) (4 5) (6 7 8 9))
;; But we can't represent x as 2D structure
```

**Needed bindings:**
```rust
// Current: Can't represent this!
{
    "x": Nested([
        Multiple([1, 2, 3]),     // First row
        Multiple([4, 5]),        // Second row
        Multiple([6, 7, 8, 9])   // Third row
    ])
}
```

---

## Migration Strategy Overview

### Phase 0: Preparation (1-2 days)

- [ ] Comprehensive test suite for nested patterns
- [ ] Benchmark current implementation
- [ ] Document all edge cases

### Phase 1: Multi-Dimensional Bindings (3-4 days)

- [ ] Change `BindingValue` to support nesting
- [ ] Update pattern matching to create nested bindings
- [ ] Maintain backward compatibility

### Phase 2: Depth Tracking (2-3 days)

- [ ] Add ellipsis depth to pattern/template structures
- [ ] Track depth during parsing
- [ ] Propagate depth through matching

### Phase 3: Index-Based Access (4-5 days)

- [ ] Implement tree walking with indices
- [ ] Refactor template expansion to use indices
- [ ] Handle exhaustion at different depths

### Phase 4: Integration & Testing (3-4 days)

- [ ] Comprehensive testing
- [ ] Performance validation
- [ ] Documentation updates

**Total estimated time:** 12-18 days of focused work

---

## Phase 1: Multi-Dimensional Bindings

### Current Structure

```rust
// src/macro_system/mod.rs
pub enum BindingValue {
    Single(Value),         // x → 42
    Multiple(Vec<Value>),  // x → [1, 2, 3]
}

pub type Bindings = HashMap<Rc<str>, BindingValue>;
```

### Target Structure

```rust
// src/macro_system/mod.rs
pub enum BindingValue {
    Single(Value),                    // Depth 0: x → 42
    Multiple(Vec<BindingValue>),      // Depth 1+: recursive!
}

pub type Bindings = HashMap<Rc<str>, BindingValue>;
```

### Migration Steps

#### Step 1.1: Add Recursive Multiple (1 day)

**File:** `src/macro_system/mod.rs`

```rust
#[derive(Debug, Clone)]
pub enum BindingValue {
    /// Single value (depth 0)
    Single(Value),

    /// Multiple values (depth 1+)
    /// Can contain Single OR Multiple (for nested ellipsis)
    Multiple(Vec<BindingValue>),
}

impl BindingValue {
    /// Get the ellipsis depth of this binding
    pub fn depth(&self) -> usize {
        match self {
            BindingValue::Single(_) => 0,
            BindingValue::Multiple(values) => {
                if values.is_empty() {
                    1
                } else {
                    1 + values[0].depth()
                }
            }
        }
    }

    /// Flatten to a vector of Values (for backward compatibility)
    pub fn flatten(&self) -> Vec<Value> {
        match self {
            BindingValue::Single(v) => vec![v.clone()],
            BindingValue::Multiple(values) => {
                values.iter()
                    .flat_map(|bv| bv.flatten())
                    .collect()
            }
        }
    }

    /// Get value at specific index path
    /// indices = [i, j, k] means: self[i][j][k]
    pub fn get_at(&self, indices: &[usize]) -> Option<&Value> {
        if indices.is_empty() {
            match self {
                BindingValue::Single(v) => Some(v),
                _ => None,
            }
        } else {
            match self {
                BindingValue::Multiple(values) => {
                    values.get(indices[0])?
                        .get_at(&indices[1..])
                }
                _ => None,
            }
        }
    }
}
```

**Tests to add:**
```rust
#[test]
fn test_binding_depth() {
    let single = BindingValue::Single(Value::Integer(42));
    assert_eq!(single.depth(), 0);

    let simple = BindingValue::Multiple(vec![
        BindingValue::Single(Value::Integer(1)),
        BindingValue::Single(Value::Integer(2)),
    ]);
    assert_eq!(simple.depth(), 1);

    let nested = BindingValue::Multiple(vec![
        BindingValue::Multiple(vec![
            BindingValue::Single(Value::Integer(1)),
            BindingValue::Single(Value::Integer(2)),
        ]),
        BindingValue::Multiple(vec![
            BindingValue::Single(Value::Integer(3)),
        ]),
    ]);
    assert_eq!(nested.depth(), 2);
}

#[test]
fn test_get_at() {
    let nested = BindingValue::Multiple(vec![
        BindingValue::Multiple(vec![
            BindingValue::Single(Value::Integer(1)),
            BindingValue::Single(Value::Integer(2)),
        ]),
        BindingValue::Multiple(vec![
            BindingValue::Single(Value::Integer(3)),
        ]),
    ]);

    assert_eq!(nested.get_at(&[0, 0]), Some(&Value::Integer(1)));
    assert_eq!(nested.get_at(&[0, 1]), Some(&Value::Integer(2)));
    assert_eq!(nested.get_at(&[1, 0]), Some(&Value::Integer(3)));
    assert_eq!(nested.get_at(&[1, 1]), None);
}
```

#### Step 1.2: Update Pattern Matching (2 days)

**File:** `src/macro_system/pattern.rs`

Current code that needs updating:
```rust
// Line 187-190
super::BindingValue::Multiple(_) => {
    // Nested ellipsis - not supported in basic implementation
    return false;
}
```

New code:
```rust
super::BindingValue::Multiple(inner_values) => {
    // Nested ellipsis - accumulate as 2D structure
    repeated_bindings
        .entry(name)
        .or_insert_with(Vec::new)
        .push(super::BindingValue::Multiple(inner_values));
}
```

Full function update:
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

    // Collect bindings for repeated section
    let mut repeated_bindings: HashMap<Rc<str>, Vec<BindingValue>> = HashMap::new();

    for expr in middle_exprs {
        let mut temp_bindings = Bindings::new();
        if !match_pattern_impl(repeated, expr, literals, &mut temp_bindings) {
            return false;
        }

        // Accumulate bindings - now supports nesting!
        for (name, binding) in temp_bindings {
            repeated_bindings
                .entry(name)
                .or_default()
                .push(binding);  // Push BindingValue (Single or Multiple)
        }
    }

    // Insert accumulated bindings as Multiple
    for (name, values) in repeated_bindings {
        bindings.insert(name, super::BindingValue::Multiple(values));
    }

    true
}
```

**Key change:** Instead of extracting `Value` from `Single` and rejecting `Multiple`, we now **preserve the BindingValue structure**.

#### Step 1.3: Backward Compatibility Layer (1 day)

Add helper to maintain compatibility with existing code:

```rust
impl BindingValue {
    /// Convert old-style flat Multiple to new-style
    /// Used during migration period
    pub fn from_values(values: Vec<Value>) -> Self {
        BindingValue::Multiple(
            values.into_iter()
                .map(BindingValue::Single)
                .collect()
        )
    }

    /// Extract values at depth 1 (for current use cases)
    pub fn as_flat_multiple(&self) -> Option<Vec<Value>> {
        match self {
            BindingValue::Multiple(values) => {
                let flattened: Option<Vec<_>> = values.iter()
                    .map(|bv| match bv {
                        BindingValue::Single(v) => Some(v.clone()),
                        _ => None,
                    })
                    .collect();
                flattened
            }
            _ => None,
        }
    }
}
```

**Migration strategy:**
1. Add new enum variant `Multiple(Vec<BindingValue>)`
2. Update all construction sites to use `from_values()`
3. Update all access sites to use `as_flat_multiple()`
4. Once working, remove compatibility helpers

---

## Phase 2: Depth Tracking

### Current Pattern/Template Structure

```rust
pub enum Pattern {
    Ellipsis {
        before: Vec<Pattern>,
        repeated: Box<Pattern>,
        after: Vec<Pattern>,
    },
    // ...
}
```

### Target Structure

```rust
pub enum Pattern {
    Ellipsis {
        before: Vec<Pattern>,
        repeated: Box<Pattern>,
        after: Vec<Pattern>,
        depth: usize,  // NEW: ellipsis nesting depth
    },
    // ...
}
```

### Migration Steps

#### Step 2.1: Add Depth Field (1 day)

**File:** `src/macro_system/mod.rs`

```rust
#[derive(Debug, Clone)]
pub enum Pattern {
    // ... existing variants ...

    Ellipsis {
        /// Patterns before ellipsis
        before: Vec<Pattern>,
        /// Pattern to repeat (zero or more times)
        repeated: Box<Pattern>,
        /// Patterns after ellipsis
        after: Vec<Pattern>,
        /// Ellipsis nesting depth (0 = outermost)
        depth: usize,
    },
}

#[derive(Debug, Clone)]
pub enum Template {
    // ... existing variants ...

    Ellipsis {
        before: Vec<Template>,
        repeated: Box<Template>,
        after: Vec<Template>,
        depth: usize,  // NEW
    },

    EllipsisEscape(Box<Template>),
}
```

**Impact:** All pattern/template construction sites need updating.

#### Step 2.2: Calculate Depth During Parsing (2 days)

**File:** `src/macro_system/mod.rs`

```rust
fn parse_list_pattern(items: &[Value]) -> Result<Pattern, crate::EvalError> {
    parse_list_pattern_with_depth(items, 0)
}

fn parse_list_pattern_with_depth(
    items: &[Value],
    current_depth: usize
) -> Result<Pattern, crate::EvalError> {
    let ellipsis_pos = items.iter()
        .position(|item| matches!(item, Value::Symbol(s) if s.as_ref() == "..."));

    match ellipsis_pos {
        None => {
            let patterns: Result<Vec<_>, _> = items.iter()
                .map(|item| parse_pattern_with_depth(item, current_depth))
                .collect();
            Ok(Pattern::List(patterns?))
        }
        Some(pos) => {
            if pos == 0 {
                return Err(EvalError::InvalidSyntax(
                    "Ellipsis cannot appear at start of pattern".to_string(),
                ));
            }

            let before: Result<Vec<_>, _> = items[..pos - 1].iter()
                .map(|item| parse_pattern_with_depth(item, current_depth))
                .collect();

            // Parse repeated pattern with INCREASED depth
            let repeated = Box::new(
                parse_pattern_with_depth(&items[pos - 1], current_depth + 1)?
            );

            // Parse after with ORIGINAL depth (not nested)
            let after_items = &items[pos + 1..];
            let after = if after_items.is_empty() {
                vec![]
            } else {
                match parse_list_pattern_with_depth(after_items, current_depth)? {
                    Pattern::List(patterns) => patterns,
                    Pattern::Ellipsis { before: b, repeated: r, after: a, depth: d } => {
                        let mut result = b;
                        result.push(Pattern::Ellipsis {
                            before: vec![],
                            repeated: r,
                            after: a,
                            depth: d,
                        });
                        result
                    }
                    other => vec![other],
                }
            };

            Ok(Pattern::Ellipsis {
                before: before?,
                repeated,
                after,
                depth: current_depth,  // Depth of THIS ellipsis
            })
        }
    }
}

fn parse_pattern_with_depth(
    expr: &Value,
    depth: usize
) -> Result<Pattern, crate::EvalError> {
    match expr {
        Value::Symbol(s) if s.as_ref() == "_" => Ok(Pattern::Wildcard),
        Value::Symbol(s) => Ok(Pattern::Variable(s.clone())),
        Value::Pair(_) => {
            let items = collect_list_items(expr)?;
            parse_list_pattern_with_depth(&items, depth)
        }
        // ... other cases ...
    }
}
```

**Key insight:** Depth increases when entering `repeated` section, stays same for `before`/`after`.

**Example:**
```scheme
Pattern: ((x ...) ...)

Parsing trace:
1. Outer list: depth=0
2. Find ellipsis at position 1
3. Parse repeated ((x ...)) with depth=1
   - Inner list: depth=1
   - Find ellipsis at position 1
   - Parse repeated (x) with depth=2
4. Result:
   Pattern::Ellipsis {
       depth: 0,
       repeated: Pattern::List([
           Pattern::Ellipsis {
               depth: 1,
               repeated: Pattern::Variable("x"),
           }
       ])
   }
```

---

## Phase 3: Index-Based Template Expansion

This is the core of Gauche's approach: using an **indices array** to track position at each ellipsis depth.

### Current Approach

```rust
// src/macro_system/template.rs
for i in 0..repeat_count {
    let mut iter_bindings = bindings.clone();
    for var in &vars {
        if let Some(BindingValue::Multiple(values)) = bindings.get(var) {
            iter_bindings.insert(var, BindingValue::Single(values[i]));
        }
    }
    result.push(expand_template_impl(repeated, &iter_bindings, depth)?);
}
```

**Problem:** This only handles 1D iteration.

### Target Approach (Gauche-style)

```rust
// Expansion context carries indices array
struct ExpansionContext<'a> {
    bindings: &'a Bindings,
    indices: Vec<usize>,  // indices[depth] = current position at that depth
}

fn expand_template_with_context(
    template: &Template,
    ctx: &mut ExpansionContext,
) -> Result<Value, EvalError> {
    match template {
        Template::Variable(name) => {
            // Look up variable using indices array
            get_binding_at_indices(ctx.bindings, name, &ctx.indices)
        }

        Template::Ellipsis { before, repeated, after, depth } => {
            let mut result = Vec::new();

            // Expand before
            for t in before {
                result.push(expand_template_with_context(t, ctx)?);
            }

            // Find pattern variables at this depth
            let vars = find_pattern_vars_in_bindings(repeated, ctx.bindings);

            // Get iteration count from first variable
            let repeat_count = get_iteration_count(
                ctx.bindings,
                &vars[0],
                *depth,
                &ctx.indices
            )?;

            // Iterate at this depth
            for i in 0..repeat_count {
                ctx.indices[*depth] = i;  // Set index at this depth
                result.push(expand_template_with_context(repeated, ctx)?);
            }

            // Expand after
            for t in after {
                let expanded = expand_template_with_context(t, ctx)?;
                if matches!(t, Template::Ellipsis { .. }) {
                    if let Ok(items) = list_to_vec(&expanded) {
                        result.extend(items);
                    } else {
                        result.push(expanded);
                    }
                } else {
                    result.push(expanded);
                }
            }

            Ok(list_from_vec(result))
        }

        // ... other cases ...
    }
}
```

### Step 3.1: Implement Index-Based Binding Lookup (2 days)

```rust
/// Get binding value at specific index path
///
/// # Arguments
/// - `bindings`: All pattern variable bindings
/// - `name`: Variable name to look up
/// - `indices`: Index at each ellipsis depth [i0, i1, i2, ...]
///
/// # Example
/// ```
/// // bindings["x"] = Multiple([Multiple([1, 2]), Multiple([3, 4, 5])])
/// // indices = [1, 2] means: x[1][2] = 5
/// get_binding_at_indices(bindings, "x", &[1, 2]) // → Value::Integer(5)
/// ```
fn get_binding_at_indices(
    bindings: &Bindings,
    name: &Rc<str>,
    indices: &[usize],
) -> Result<Value, EvalError> {
    let binding = bindings.get(name)
        .ok_or_else(|| EvalError::InvalidSyntax(
            format!("Unbound pattern variable: {}", name)
        ))?;

    get_binding_value_at(binding, indices)
}

fn get_binding_value_at(
    binding: &BindingValue,
    indices: &[usize],
) -> Result<Value, EvalError> {
    if indices.is_empty() {
        // Base case: no more indices, expecting Single
        match binding {
            BindingValue::Single(v) => Ok(v.clone()),
            BindingValue::Multiple(_) => Err(EvalError::InvalidSyntax(
                "Expected single value but found multiple".to_string()
            )),
        }
    } else {
        // Recursive case: peel off one index level
        match binding {
            BindingValue::Multiple(values) => {
                let idx = indices[0];
                if idx >= values.len() {
                    // Exhausted at this level
                    Err(EvalError::InvalidSyntax(
                        format!("Index {} out of bounds (length {})", idx, values.len())
                    ))
                } else {
                    get_binding_value_at(&values[idx], &indices[1..])
                }
            }
            BindingValue::Single(_) => Err(EvalError::InvalidSyntax(
                "Expected multiple values but found single".to_string()
            )),
        }
    }
}
```

### Step 3.2: Get Iteration Count at Depth (1 day)

```rust
/// Get the number of iterations needed at a specific depth
///
/// # Example
/// ```
/// // bindings["x"] = Multiple([Multiple([1, 2]), Multiple([3, 4, 5])])
/// //
/// // At depth 0, indices=[]:
/// //   x has 2 outer groups → count = 2
/// //
/// // At depth 1, indices=[0]:
/// //   x[0] has 2 elements → count = 2
/// //
/// // At depth 1, indices=[1]:
/// //   x[1] has 3 elements → count = 3
/// ```
fn get_iteration_count(
    bindings: &Bindings,
    var_name: &Rc<str>,
    depth: usize,
    indices: &[usize],
) -> Result<usize, EvalError> {
    let binding = bindings.get(var_name)
        .ok_or_else(|| EvalError::InvalidSyntax(
            format!("Unbound pattern variable: {}", var_name)
        ))?;

    get_count_at_depth(binding, depth, indices)
}

fn get_count_at_depth(
    binding: &BindingValue,
    target_depth: usize,
    indices: &[usize],
) -> Result<usize, EvalError> {
    if indices.len() == target_depth {
        // We're at the target depth, get count here
        match binding {
            BindingValue::Multiple(values) => Ok(values.len()),
            BindingValue::Single(_) => Err(EvalError::InvalidSyntax(
                "Expected multiple values at ellipsis depth".to_string()
            )),
        }
    } else if indices.len() < target_depth {
        // Need to go deeper
        match binding {
            BindingValue::Multiple(values) => {
                let idx = indices[indices.len()];
                get_count_at_depth(&values[idx], target_depth, indices)
            }
            BindingValue::Single(_) => Err(EvalError::InvalidSyntax(
                "Insufficient depth in bindings".to_string()
            )),
        }
    } else {
        Err(EvalError::InvalidSyntax(
            "Indices exceed target depth".to_string()
        ))
    }
}
```

### Step 3.3: Refactor Template Expansion (2 days)

```rust
pub fn expand_template(template: &Template, bindings: &Bindings) -> Result<Value, EvalError> {
    // Determine maximum ellipsis depth
    let max_depth = find_max_depth(template);

    // Initialize expansion context
    let mut ctx = ExpansionContext {
        bindings,
        indices: vec![0; max_depth + 1],  // One index per depth level
    };

    expand_template_with_context(template, &mut ctx)
}

fn find_max_depth(template: &Template) -> usize {
    match template {
        Template::Ellipsis { repeated, depth, .. } => {
            (*depth).max(find_max_depth(repeated))
        }
        Template::List(templates) | Template::Vector(templates) => {
            templates.iter()
                .map(find_max_depth)
                .max()
                .unwrap_or(0)
        }
        _ => 0,
    }
}

fn expand_template_with_context(
    template: &Template,
    ctx: &mut ExpansionContext,
) -> Result<Value, EvalError> {
    match template {
        Template::Literal(val) => Ok(val.clone()),

        Template::Variable(name) => {
            // KEY: Use indices to look up variable
            get_binding_at_indices(ctx.bindings, name, &ctx.indices)
        }

        Template::List(templates) => {
            let mut expanded = Vec::new();
            for t in templates {
                expanded.push(expand_template_with_context(t, ctx)?);
            }
            Ok(list_from_vec(expanded))
        }

        Template::Vector(templates) => {
            let mut expanded = Vec::new();
            for t in templates {
                expanded.push(expand_template_with_context(t, ctx)?);
            }
            Ok(Value::Vector(Rc::new(RefCell::new(expanded))))
        }

        Template::Ellipsis { before, repeated, after, depth } => {
            let mut result = Vec::new();

            // Expand 'before' templates
            for t in before {
                result.push(expand_template_with_context(t, ctx)?);
            }

            // Find pattern variables in repeated template
            let vars = find_pattern_vars_in_bindings(repeated, ctx.bindings);

            if vars.is_empty() {
                return Err(EvalError::InvalidSyntax(
                    "Ellipsis template contains no pattern variables".to_string(),
                ));
            }

            // Get iteration count at this depth
            let repeat_count = get_iteration_count(
                ctx.bindings,
                &vars[0],
                *depth,
                &ctx.indices[..*depth]  // Indices up to current depth
            )?;

            // Verify all variables have same count at this depth
            for var in &vars[1..] {
                let count = get_iteration_count(
                    ctx.bindings,
                    var,
                    *depth,
                    &ctx.indices[..*depth]
                )?;
                if count != repeat_count {
                    return Err(EvalError::InvalidSyntax(
                        "Pattern variables in ellipsis have different lengths".to_string(),
                    ));
                }
            }

            // Iterate at this depth
            for i in 0..repeat_count {
                ctx.indices[*depth] = i;  // Set index at this depth
                result.push(expand_template_with_context(repeated, ctx)?);
            }

            // Expand 'after' templates
            for t in after {
                let expanded = expand_template_with_context(t, ctx)?;
                if matches!(t, Template::Ellipsis { .. }) {
                    if let Ok(items) = list_to_vec(&expanded) {
                        result.extend(items);
                    } else {
                        result.push(expanded);
                    }
                } else {
                    result.push(expanded);
                }
            }

            Ok(list_from_vec(result))
        }

        Template::EllipsisEscape(inner) => {
            let ellipsis = Value::Symbol("...".into());
            let expanded = expand_template_with_context(inner, ctx)?;
            Ok(list_from_vec(vec![ellipsis, expanded]))
        }
    }
}
```

---

## Phase 4: Testing & Integration

### Step 4.1: Test Suite for Nested Ellipsis (2 days)

**File:** `tests/fixtures/examples/macros/05_nested_ellipsis.scm`

```scheme
;;; Nested ellipsis test cases
;;; These patterns require true 2D binding support

;; Test 1: Simple 2D matrix
(define-syntax matrix
  (syntax-rules ()
    ((_ ((x ...) ...))
     (quote ((x ...) ...)))))

(display "Test 1 - Simple matrix: ")
(display (matrix ((1 2 3) (4 5) (6 7 8 9))))
(newline)
;; Expected: ((1 2 3) (4 5) (6 7 8 9))

;; Test 2: Transpose pattern (requires 2D access)
(define-syntax list-of-lists
  (syntax-rules ()
    ((_ ((a b) ...))
     (quote ((a ...) (b ...))))))

(display "Test 2 - Separate lists: ")
(display (list-of-lists ((1 10) (2 20) (3 30))))
(newline)
;; Expected: ((1 2 3) (10 20 30))

;; Test 3: Nested let (from SRFI-46)
(define-syntax let**
  (syntax-rules ()
    ((_ ((var val) ...) body ...)
     (let ((var val) ...) body ...))))

(display "Test 3 - Nested let: ")
(display (let** ((x 1) (y 2)) (+ x y)))
(newline)
;; Expected: 3

;; Test 4: True 2D iteration
(define-syntax iterate-2d
  (syntax-rules ()
    ((_ ((row ...) ...))
     (quote (row ... ...)))))

(display "Test 4 - Flatten matrix: ")
(display (iterate-2d ((1 2) (3 4 5) (6))))
(newline)
;; Expected: (1 2 3 4 5 6)
```

### Step 4.2: Unit Tests for Components (1 day)

```rust
// tests/macro_system/nested_ellipsis.rs

#[test]
fn test_2d_bindings() {
    use patina::macro_system::{BindingValue, Value};

    // Create 2D structure: [[1, 2], [3, 4, 5]]
    let binding = BindingValue::Multiple(vec![
        BindingValue::Multiple(vec![
            BindingValue::Single(Value::Integer(1)),
            BindingValue::Single(Value::Integer(2)),
        ]),
        BindingValue::Multiple(vec![
            BindingValue::Single(Value::Integer(3)),
            BindingValue::Single(Value::Integer(4)),
            BindingValue::Single(Value::Integer(5)),
        ]),
    ]);

    assert_eq!(binding.depth(), 2);
    assert_eq!(binding.get_at(&[0, 0]), Some(&Value::Integer(1)));
    assert_eq!(binding.get_at(&[0, 1]), Some(&Value::Integer(2)));
    assert_eq!(binding.get_at(&[1, 0]), Some(&Value::Integer(3)));
    assert_eq!(binding.get_at(&[1, 2]), Some(&Value::Integer(5)));
    assert_eq!(binding.get_at(&[1, 3]), None);
}

#[test]
fn test_pattern_depth_calculation() {
    use patina::macro_system::parse_pattern;

    // ((x ...) ...)
    let pattern_str = "((x ...) ...)";
    let parsed = parse_pattern(&parse_value(pattern_str)).unwrap();

    // Verify depth is calculated correctly
    if let Pattern::Ellipsis { depth, repeated, .. } = parsed {
        assert_eq!(depth, 0);  // Outer ellipsis at depth 0

        if let Pattern::List(inner) = &**repeated {
            if let Pattern::Ellipsis { depth: inner_depth, .. } = &inner[0] {
                assert_eq!(*inner_depth, 1);  // Inner ellipsis at depth 1
            } else {
                panic!("Expected inner ellipsis");
            }
        } else {
            panic!("Expected list");
        }
    } else {
        panic!("Expected ellipsis pattern");
    }
}

#[test]
fn test_nested_ellipsis_expansion() {
    use patina::Interpreter;

    let mut interp = Interpreter::new();

    // Define matrix macro
    interp.eval_str(r#"
        (define-syntax matrix
          (syntax-rules ()
            ((_ ((x ...) ...))
             (quote ((x ...) ...)))))
    "#).unwrap();

    // Test expansion
    let result = interp.eval_str("(matrix ((1 2 3) (4 5) (6 7 8 9)))").unwrap();

    // Expected: ((1 2 3) (4 5) (6 7 8 9))
    let expected = "((1 2 3) (4 5) (6 7 8 9))";
    assert_eq!(format!("{}", result), expected);
}
```

### Step 4.3: Performance Benchmarks (1 day)

```rust
// benches/macro_expansion.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use patina::Interpreter;

fn bench_simple_ellipsis(c: &mut Criterion) {
    c.bench_function("simple ellipsis x ...", |b| {
        let mut interp = Interpreter::new();
        interp.eval_str(r#"
            (define-syntax test
              (syntax-rules ()
                ((_ x ...)
                 (quote (x ...)))))
        "#).unwrap();

        b.iter(|| {
            interp.eval_str(black_box("(test 1 2 3 4 5 6 7 8 9 10)"))
        });
    });
}

fn bench_nested_ellipsis(c: &mut Criterion) {
    c.bench_function("nested ellipsis ((x ...) ...)", |b| {
        let mut interp = Interpreter::new();
        interp.eval_str(r#"
            (define-syntax matrix
              (syntax-rules ()
                ((_ ((x ...) ...))
                 (quote ((x ...) ...)))))
        "#).unwrap();

        b.iter(|| {
            interp.eval_str(black_box(
                "(matrix ((1 2 3) (4 5 6) (7 8 9) (10 11 12)))"
            ))
        });
    });
}

fn bench_current_multiple_ellipsis(c: &mut Criterion) {
    c.bench_function("multiple ellipsis x ... y ...", |b| {
        let mut interp = Interpreter::new();
        interp.eval_str(r#"
            (define-syntax test
              (syntax-rules ()
                ((_ ((x y) ...) z ...)
                 (quote (x ... z ...)))))
        "#).unwrap();

        b.iter(|| {
            interp.eval_str(black_box("(test ((1 2) (3 4) (5 6)) 7 8 9)"))
        });
    });
}

criterion_group!(
    benches,
    bench_simple_ellipsis,
    bench_nested_ellipsis,
    bench_current_multiple_ellipsis
);
criterion_main!(benches);
```

**Expected results:**
- Simple ellipsis: ~same performance (baseline)
- Nested ellipsis: 2-3x slower (acceptable for rarely-used feature)
- Current multiple: ~same performance (shouldn't regress)

---

## Compatibility Strategy

### Maintain Backward Compatibility

**Goal:** All existing code continues to work without changes.

**Strategy:**

1. **Add, don't replace:**
   ```rust
   // Old code path still exists
   pub fn expand_template_simple(
       template: &Template,
       bindings: &Bindings
   ) -> Result<Value, EvalError> {
       // Current implementation for non-nested cases
   }

   // New code path for nested
   pub fn expand_template(
       template: &Template,
       bindings: &Bindings
   ) -> Result<Value, EvalError> {
       if has_nested_ellipsis(template) {
           expand_template_with_context(template, bindings)
       } else {
           expand_template_simple(template, bindings)
       }
   }
   ```

2. **Feature flag during development:**
   ```toml
   # Cargo.toml
   [features]
   nested-ellipsis = []
   ```

   ```rust
   #[cfg(feature = "nested-ellipsis")]
   pub fn expand_template(...) { /* new implementation */ }

   #[cfg(not(feature = "nested-ellipsis"))]
   pub fn expand_template(...) { /* old implementation */ }
   ```

3. **Gradual rollout:**
   - Week 1-2: Implement behind feature flag, tests only
   - Week 3: Enable by default, old path still available
   - Week 4: Remove old path after validation

---

## Migration Checklist

### Before Starting

- [ ] Full test suite passes (285/285 tests)
- [ ] Benchmark baseline established
- [ ] Documentation of current behavior complete
- [ ] Create `nested-ellipsis` feature branch

### Phase 1: Multi-Dimensional Bindings

- [ ] Update `BindingValue` enum
- [ ] Add `depth()`, `flatten()`, `get_at()` methods
- [ ] Update pattern matching to preserve structure
- [ ] Add compatibility helpers
- [ ] Unit tests for binding operations
- [ ] All existing tests still pass

### Phase 2: Depth Tracking

- [ ] Add `depth` field to `Pattern::Ellipsis`
- [ ] Add `depth` field to `Template::Ellipsis`
- [ ] Implement `parse_pattern_with_depth()`
- [ ] Implement `parse_template_with_depth()`
- [ ] Unit tests for depth calculation
- [ ] All existing tests still pass

### Phase 3: Index-Based Access

- [ ] Implement `ExpansionContext`
- [ ] Implement `get_binding_at_indices()`
- [ ] Implement `get_iteration_count()`
- [ ] Refactor `expand_template_with_context()`
- [ ] Add `find_max_depth()`
- [ ] Unit tests for index-based lookup
- [ ] All existing tests still pass

### Phase 4: Testing & Integration

- [ ] Create nested ellipsis test suite
- [ ] Verify against Gauche/Chibi behavior
- [ ] Performance benchmarks acceptable
- [ ] Update documentation
- [ ] Code review
- [ ] Merge to main

---

## Example: Complete Flow

Let's trace through a complete example with the new system:

### Input Macro

```scheme
(define-syntax matrix
  (syntax-rules ()
    ((_ ((x ...) ...))
     (quote ((x ...) ...)))))

(matrix ((1 2 3) (4 5) (6 7 8 9)))
```

### Step 1: Parse Pattern

**Pattern:** `((x ...) ...)`

```rust
// Outer: parse_list_pattern_with_depth([((x ...) ...)], depth=0)
Pattern::Ellipsis {
    depth: 0,
    before: [],
    repeated: Pattern::List([
        // Inner: parse_list_pattern_with_depth([(x ...)], depth=1)
        Pattern::Ellipsis {
            depth: 1,
            before: [],
            repeated: Pattern::Variable("x"),  // depth=2
            after: []
        }
    ]),
    after: []
}
```

### Step 2: Match Against Input

**Input:** `((1 2 3) (4 5) (6 7 8 9))`

```rust
// Outer ellipsis matching (depth 0)
middle_exprs = [(1 2 3), (4 5), (6 7 8 9)]

// For each expr, match against Pattern::List([Ellipsis{...}])
for expr in middle_exprs:
    // expr = (1 2 3)
    // Match against Pattern::Ellipsis { depth: 1, repeated: Variable("x") }
    // Inner ellipsis matching (depth 1)
    inner_middle = [1, 2, 3]
    temp_bindings["x"] = Multiple([Single(1), Single(2), Single(3)])

    // Accumulate
    repeated_bindings["x"].push(Multiple([Single(1), Single(2), Single(3)]))

// Final bindings
bindings["x"] = Multiple([
    Multiple([Single(1), Single(2), Single(3)]),      // First row
    Multiple([Single(4), Single(5)]),                  // Second row
    Multiple([Single(6), Single(7), Single(8), Single(9)])  // Third row
])
```

**Structure:**
```
x: depth=2
└─ Multiple (depth 1)
   ├─ Multiple (depth 0): [1, 2, 3]
   ├─ Multiple (depth 0): [4, 5]
   └─ Multiple (depth 0): [6, 7, 8, 9]
```

### Step 3: Expand Template

**Template:** `(quote ((x ...) ...))`

```rust
// Initialize context
ctx.indices = [0, 0, 0]  // Max depth is 2
ctx.bindings = { "x": <2D structure above> }

// Outer ellipsis (depth 0)
repeat_count = get_iteration_count("x", depth=0, indices=[])
             = bindings["x"].len() = 3

for i in 0..3:
    ctx.indices[0] = i
    // Inner ellipsis (depth 1)
    repeat_count_inner = get_iteration_count("x", depth=1, indices=[i])
                       = bindings["x"][i].len()
                       // i=0: 3, i=1: 2, i=2: 4

    for j in 0..repeat_count_inner:
        ctx.indices[1] = j
        // Variable lookup
        value = get_binding_at_indices("x", [i, j])
              = bindings["x"][i][j]
        // i=0, j=0: 1
        // i=0, j=1: 2
        // i=0, j=2: 3
        // i=1, j=0: 4
        // i=1, j=1: 5
        // ...

// Result: ((1 2 3) (4 5) (6 7 8 9))
```

---

## Risk Assessment

### High Risk Areas

1. **Breaking Changes in BindingValue**
   - **Risk:** All code using bindings may break
   - **Mitigation:** Compatibility layer with `from_values()` / `as_flat_multiple()`
   - **Fallback:** Feature flag to enable/disable

2. **Performance Regression**
   - **Risk:** Index-based access slower than current approach
   - **Mitigation:** Benchmark at each phase, optimize hot paths
   - **Fallback:** Fast path for non-nested cases

3. **Correctness Issues**
   - **Risk:** Subtle bugs in depth tracking or index arithmetic
   - **Mitigation:** Comprehensive test suite, compare to Gauche/Chibi
   - **Fallback:** Revert to previous approach if bugs found

### Medium Risk Areas

1. **API Stability**
   - **Risk:** Public API changes break user code
   - **Mitigation:** Maintain old API, add new functions
   - **Fallback:** Deprecation warnings before removal

2. **Complexity Increase**
   - **Risk:** Codebase becomes harder to maintain
   - **Mitigation:** Excellent documentation, clear structure
   - **Fallback:** Keep old simple path for reference

### Low Risk Areas

1. **Feature Adoption**
   - **Risk:** Users don't need nested ellipsis
   - **Mitigation:** Not required for R7RS, optional feature
   - **Impact:** Low - feature is purely additive

---

## Success Criteria

### Functional Requirements

- [ ] All existing tests pass (285/285)
- [ ] New nested ellipsis tests pass (10+ cases)
- [ ] Output matches Gauche for equivalent macros
- [ ] SRFI-46 test suite passes
- [ ] SRFI-149 basic tests pass

### Performance Requirements

- [ ] Simple ellipsis: <5% slowdown
- [ ] Multiple ellipsis: <10% slowdown
- [ ] Nested ellipsis: <3x overhead (acceptable for rare feature)
- [ ] Compilation time: <20% increase

### Quality Requirements

- [ ] Code coverage >85%
- [ ] All clippy warnings resolved
- [ ] Documentation complete
- [ ] Examples in guide
- [ ] Migration guide written

---

## Timeline Estimate

### Conservative Estimate (Solo Developer)

| Phase | Duration | Effort | Calendar Time |
|-------|----------|--------|---------------|
| Phase 0: Preparation | 2 days | 12 hours | 1 week |
| Phase 1: Multi-Dimensional Bindings | 4 days | 24 hours | 1 week |
| Phase 2: Depth Tracking | 3 days | 18 hours | 1 week |
| Phase 3: Index-Based Access | 5 days | 30 hours | 1.5 weeks |
| Phase 4: Testing & Integration | 4 days | 24 hours | 1 week |
| **Total** | **18 days** | **108 hours** | **5.5 weeks** |

### Optimistic Estimate (Experienced Developer)

| Phase | Duration | Effort | Calendar Time |
|-------|----------|--------|---------------|
| Phase 0: Preparation | 1 day | 6 hours | 3 days |
| Phase 1: Multi-Dimensional Bindings | 2 days | 12 hours | 1 week |
| Phase 2: Depth Tracking | 2 days | 12 hours | 1 week |
| Phase 3: Index-Based Access | 3 days | 18 hours | 1 week |
| Phase 4: Testing & Integration | 2 days | 12 hours | 1 week |
| **Total** | **10 days** | **60 hours** | **4 weeks** |

**Recommendation:** Budget 3-4 weeks of focused development time.

---

## Alternatives to Full Migration

If full Gauche-style migration proves too complex, consider these alternatives:

### Alternative 1: Hybrid Approach

Keep current system for `x ... y ...`, add **limited** nested support:

```rust
pub enum BindingValue {
    Single(Value),
    Multiple(Vec<Value>),         // Current: 1D arrays
    Nested2D(Vec<Vec<Value>>),    // New: 2D arrays only
}
```

**Pros:**
- Simpler than full recursive approach
- Covers 99% of real-world use cases
- Minimal code changes

**Cons:**
- Limited to 2D (no `(((x ...) ...) ...)`)
- Still requires depth tracking

### Alternative 2: Macro-Level Rewriting

Transform nested ellipsis macros into multiple simpler macros:

```scheme
;; User writes:
(define-syntax matrix
  (syntax-rules ()
    ((_ ((x ...) ...))
     (quote ((x ...) ...)))))

;; System rewrites to:
(define-syntax matrix-row
  (syntax-rules ()
    ((_ (x ...))
     (quote (x ...)))))

(define-syntax matrix
  (syntax-rules ()
    ((_ (row ...))
     (quote ((matrix-row row) ...)))))
```

**Pros:**
- No changes to core system
- Works with current implementation

**Cons:**
- Non-standard transformation
- May break macro hygiene
- Hard to implement correctly

### Alternative 3: Document Limitation

Simply document that nested ellipsis is not supported:

```markdown
## Known Limitations

Patina supports all R7RS-small macro patterns, including:
- ✅ `x ...` - Simple ellipsis
- ✅ `x ... y ...` - Multiple ellipses at same level
- ❌ `((x ...) ...)` - Nested ellipsis (SRFI-149)

Nested ellipsis is not required for R7RS compliance and is rarely used in practice.
```

**Pros:**
- No implementation cost
- Clear communication
- R7RS compliant

**Cons:**
- Cannot claim SRFI-149 support
- Some advanced macros impossible

---

## Recommendation

**For now: Keep current implementation** ✅

**Rationale:**
1. R7RS compliance achieved
2. All practical macros work
3. Nested ellipsis is rarely needed
4. Migration is complex (3-4 weeks)

**Future trigger points for migration:**
1. User requests for SRFI-149 features
2. Need to implement specific macros requiring nesting
3. Desire for full Scheme compatibility (R6RS, Racket, etc.)
4. Academic interest in complete implementation

**If/when migration happens:**
- Follow this roadmap
- Budget 4-6 weeks
- Start with Phase 1 behind feature flag
- Extensive testing at each phase

---

## Conclusion

This roadmap provides a **complete, step-by-step plan** for migrating from our current approach to Gauche's index-based tree walking. The migration is **well-defined but non-trivial**, requiring:

- **Recursive BindingValue** for multi-dimensional storage
- **Depth tracking** in patterns and templates
- **Index-based lookup** for variable access
- **Comprehensive testing** to ensure correctness

The current implementation is **sufficient for R7RS** and handles all practical use cases. Migration should only be undertaken if there's a clear need for SRFI-149 support or true nested ellipsis patterns.

**Total effort:** 60-108 hours over 4-6 weeks
**Complexity:** High
**Value:** Low (for R7RS), High (for complete Scheme)

---

## References

### Gauche Implementation
- `~/Project/reference/gauche/src/macro.c`
  - Lines 516-540: Depth and nesting calculation
  - Lines 700-750: MatchVar tree structure
  - Lines 730-750: `get_pvref_value()` index-based lookup
  - Lines 983-998: Template expansion with indices

### SRFI Specifications
- SRFI-46: Basic Syntax-rules Extensions
  - Adds `_` wildcard and `...` in literals
- SRFI-149: Extended Syntax-rules
  - Adds nested ellipsis support
  - Defines formal semantics

### Research Papers
- "Macros That Work" (Clinger & Rees, 1991)
  - Original hygiene algorithm
- "Syntactic Abstraction in Scheme" (Dybvig, 1992)
  - syntax-case system design

### Current Implementation
- `src/macro_system/mod.rs` - Pattern/Template definitions
- `src/macro_system/pattern.rs` - Pattern matching logic
- `src/macro_system/template.rs` - Template expansion logic
- `internal/ARCHIVE/macro_research/MACRO_ARCHITECTURE_DECISIONS.md` - Design rationale
- `internal/ARCHIVE/macro_research/TEMPLATE_ELLIPSIS_FIX.md` - Implementation history
