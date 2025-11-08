# Macro System Implementation Design

**Status:** Design Phase - Single Source of Truth
**Target:** R7RS-small `syntax-rules` compliance
**Approach:** Native Rust implementation (inspired by Steel)
**Date:** 2025-11-08

---

## Table of Contents

1. [Goals and Non-Goals](#goals-and-non-goals)
2. [Architecture Overview](#architecture-overview)
3. [Data Structures](#data-structures)
4. [Core Algorithms](#core-algorithms)
5. [Integration Points](#integration-points)
6. [Hygiene Strategy](#hygiene-strategy)
7. [Implementation Phases](#implementation-phases)
8. [Testing Strategy](#testing-strategy)
9. [Success Criteria](#success-criteria)

---

## Goals and Non-Goals

### Goals

✅ **R7RS-small compliance:**
- Support `define-syntax` and `syntax-rules`
- Pattern matching with ellipsis (`...`)
- Template expansion with proper hygiene
- Literal identifier matching

✅ **Integration with Patina:**
- Works with existing `Value` enum
- Integrates with current evaluator
- Uses existing environment infrastructure
- Compatible with debug mode

✅ **Production quality:**
- Comprehensive error messages
- Well-tested (unit + integration)
- Clear separation of concerns
- Maintainable code structure

### Non-Goals

❌ **Advanced macro features (for now):**
- `syntax-case` (R6RS feature)
- `let-syntax` / `letrec-syntax` (can add later)
- Procedural macros
- Macro debugging tools

❌ **Optimization (initially):**
- Macro expansion caching
- Compile-time macro expansion
- These can be added after basic functionality works

---

## Architecture Overview

### Three-Module Design

Following Steel's structure, we'll create three focused modules:

```
src/macro_system/
├── mod.rs           # Public API, Macro struct
├── pattern.rs       # Pattern matching
├── template.rs      # Template expansion
└── hygiene.rs       # Identifier renaming
```

### Data Flow

```
User code: (define-syntax when ...)
           ↓
Parse into Macro struct (mod.rs)
           ↓
Store in macro environment
           ↓
User code: (when #t 1 2)
           ↓
Lookup macro in environment
           ↓
Pattern matching (pattern.rs)
           ↓
Collect bindings
           ↓
Template expansion (template.rs)
           ↓
Apply hygiene (hygiene.rs)
           ↓
Expanded: (if #t (begin 1 2))
           ↓
Evaluate expanded code
```

---

## Data Structures

### Core Types (in `src/macro_system/mod.rs`)

```rust
use std::rc::Rc;
use std::collections::HashMap;
use crate::value::Value;
use crate::env::Environment;
use crate::eval::error::EvalError;

/// Pattern in a syntax-rules macro
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Underscore wildcard: matches anything, binds nothing
    Wildcard,

    /// Literal constant: must match exactly
    Literal(Value),

    /// Pattern variable: binds to matched expression
    Variable(Rc<str>),

    /// List pattern: (p1 p2 p3)
    List(Vec<Pattern>),

    /// Vector pattern: #(p1 p2 p3)
    Vector(Vec<Pattern>),

    /// Ellipsis pattern: (p1 p2 ... p3)
    Ellipsis {
        /// Patterns before ellipsis
        before: Vec<Pattern>,
        /// Pattern to repeat (zero or more times)
        repeated: Box<Pattern>,
        /// Patterns after ellipsis
        after: Vec<Pattern>,
    },
}

/// Template in a syntax-rules macro
#[derive(Debug, Clone, PartialEq)]
pub enum Template {
    /// Literal value
    Literal(Value),

    /// Pattern variable reference
    Variable(Rc<str>),

    /// List template: (t1 t2 t3)
    List(Vec<Template>),

    /// Vector template: #(t1 t2 t3)
    Vector(Vec<Template>),

    /// Ellipsis template: (t1 t2 ... t3)
    Ellipsis {
        /// Templates before ellipsis
        before: Vec<Template>,
        /// Template to repeat
        repeated: Box<Template>,
        /// Templates after ellipsis
        after: Vec<Template>,
    },

    /// Ellipsis escape: (... template)
    /// Used to include literal ... in output
    EllipsisEscape(Box<Template>),
}

/// A single pattern-template pair (one case in syntax-rules)
#[derive(Debug, Clone)]
pub struct MacroRule {
    pub pattern: Pattern,
    pub template: Template,
}

/// A macro definition
#[derive(Debug, Clone)]
pub struct Macro {
    /// Macro name (for error messages)
    pub name: Rc<str>,

    /// Literal identifiers (e.g., "else" in cond)
    pub literals: Vec<Rc<str>>,

    /// Pattern-template rules (tried in order)
    pub rules: Vec<MacroRule>,

    /// Definition environment (for hygiene)
    pub env: Rc<Environment>,
}

/// Bindings collected during pattern matching
pub type Bindings = HashMap<Rc<str>, BindingValue>;

/// Value bound to a pattern variable
#[derive(Debug, Clone)]
pub enum BindingValue {
    /// Single value (from pattern variable)
    Single(Value),

    /// Multiple values (from ellipsis pattern)
    Multiple(Vec<Value>),
}
```

### Key Design Decisions

**Why separate `Ellipsis` pattern/template?**
- Makes ellipsis handling explicit in the type system
- Simplifies algorithms (no need to scan for ellipsis)
- Follows Steel's design

**Why `BindingValue` enum?**
- Distinguishes single vs. repeated bindings
- Enables better error messages
- Required for correct ellipsis expansion

**Why store `env` in `Macro`?**
- Needed for hygiene (free variable references)
- Captures lexical scope at macro definition site
- Matches R7RS semantics

---

## Core Algorithms

### Pattern Matching (in `src/macro_system/pattern.rs`)

#### Public API

```rust
/// Match a pattern against an expression
/// Returns Some(bindings) if match succeeds, None otherwise
pub fn match_pattern(
    pattern: &Pattern,
    expr: &Value,
    literals: &[Rc<str>],
) -> Option<Bindings> {
    let mut bindings = Bindings::new();
    if match_pattern_impl(pattern, expr, literals, &mut bindings) {
        Some(bindings)
    } else {
        None
    }
}
```

#### Implementation Strategy

```rust
fn match_pattern_impl(
    pattern: &Pattern,
    expr: &Value,
    literals: &[Rc<str>],
    bindings: &mut Bindings,
) -> bool {
    match pattern {
        // Wildcard: always matches, binds nothing
        Pattern::Wildcard => true,

        // Literal: must match exactly
        Pattern::Literal(lit) => values_equal(lit, expr),

        // Variable: bind or check literal
        Pattern::Variable(name) => {
            if literals.contains(name) {
                // Literal identifier: check if expr is same identifier
                match expr {
                    Value::Symbol(sym) if sym == name => true,
                    _ => false,
                }
            } else {
                // Pattern variable: bind
                bindings.insert(name.clone(), BindingValue::Single(expr.clone()));
                true
            }
        }

        // List: match elements
        Pattern::List(patterns) => {
            match expr {
                Value::Pair(_) => {
                    let exprs = match collect_list_items(expr) {
                        Ok(items) => items,
                        Err(_) => return false,  // Improper list
                    };
                    match_list_patterns(patterns, &exprs, literals, bindings)
                }
                _ => false,
            }
        }

        // Vector: match elements
        Pattern::Vector(patterns) => {
            match expr {
                Value::Vector(items) => {
                    match_list_patterns(patterns, items, literals, bindings)
                }
                _ => false,
            }
        }

        // Ellipsis: complex matching
        Pattern::Ellipsis { before, repeated, after } => {
            match expr {
                Value::Pair(_) => {
                    let exprs = match collect_list_items(expr) {
                        Ok(items) => items,
                        Err(_) => return false,
                    };
                    match_ellipsis_pattern(before, repeated, after, &exprs, literals, bindings)
                }
                _ => false,
            }
        }
    }
}

/// Match a list of patterns against a list of expressions
fn match_list_patterns(
    patterns: &[Pattern],
    exprs: &[Value],
    literals: &[Rc<str>],
    bindings: &mut Bindings,
) -> bool {
    // Exact length match required
    if patterns.len() != exprs.len() {
        return false;
    }

    for (pattern, expr) in patterns.iter().zip(exprs.iter()) {
        if !match_pattern_impl(pattern, expr, literals, bindings) {
            return false;
        }
    }

    true
}

/// Match ellipsis pattern: (before ... repeated ... after)
fn match_ellipsis_pattern(
    before: &[Pattern],
    repeated: &Pattern,
    after: &[Pattern],
    exprs: &[Value],
    literals: &[Rc<str>],
    bindings: &mut Bindings,
) -> bool {
    // Check minimum length
    let min_len = before.len() + after.len();
    if exprs.len() < min_len {
        return false;
    }

    // Match 'before' patterns
    for (i, pattern) in before.iter().enumerate() {
        if !match_pattern_impl(pattern, &exprs[i], literals, bindings) {
            return false;
        }
    }

    // Match 'after' patterns (from end)
    for (i, pattern) in after.iter().enumerate() {
        let expr_idx = exprs.len() - after.len() + i;
        if !match_pattern_impl(pattern, &exprs[expr_idx], literals, bindings) {
            return false;
        }
    }

    // Match repeated pattern (middle section, zero or more times)
    let middle_start = before.len();
    let middle_end = exprs.len() - after.len();
    let middle_exprs = &exprs[middle_start..middle_end];

    // Collect bindings for repeated section
    let mut repeated_bindings: HashMap<Rc<str>, Vec<Value>> = HashMap::new();

    for expr in middle_exprs {
        let mut temp_bindings = Bindings::new();
        if !match_pattern_impl(repeated, expr, literals, &mut temp_bindings) {
            return false;
        }

        // Accumulate bindings
        for (name, binding) in temp_bindings {
            match binding {
                BindingValue::Single(val) => {
                    repeated_bindings.entry(name).or_insert_with(Vec::new).push(val);
                }
                BindingValue::Multiple(_) => {
                    // Nested ellipsis - not supported in basic implementation
                    return false;
                }
            }
        }
    }

    // Insert accumulated bindings as Multiple
    for (name, values) in repeated_bindings {
        bindings.insert(name, BindingValue::Multiple(values));
    }

    true
}
```

**Key points:**
- First-match-wins (try patterns in order)
- Literals checked by identifier binding, not string equality
- Ellipsis captures zero-or-more matches
- Nested ellipsis deferred to future work

---

### Template Expansion (in `src/macro_system/template.rs`)

#### Public API

```rust
/// Expand a template using bindings from pattern matching
pub fn expand_template(
    template: &Template,
    bindings: &Bindings,
) -> Result<Value, EvalError> {
    expand_template_impl(template, bindings, 0)
}
```

#### Implementation Strategy

```rust
fn expand_template_impl(
    template: &Template,
    bindings: &Bindings,
    ellipsis_depth: usize,
) -> Result<Value, EvalError> {
    match template {
        // Literal: return as-is
        Template::Literal(val) => Ok(val.clone()),

        // Variable: substitute from bindings
        Template::Variable(name) => {
            match bindings.get(name) {
                Some(BindingValue::Single(val)) => Ok(val.clone()),
                Some(BindingValue::Multiple(_)) => {
                    Err(EvalError::MacroExpansion(format!(
                        "Pattern variable {} used outside ellipsis context",
                        name
                    )))
                }
                None => {
                    // Not a pattern variable - treat as literal identifier
                    // This will be renamed for hygiene later
                    Ok(Value::Symbol(name.clone()))
                }
            }
        }

        // List: expand elements
        Template::List(templates) => {
            let mut expanded = Vec::new();
            for t in templates {
                expanded.push(expand_template_impl(t, bindings, ellipsis_depth)?);
            }
            Ok(list_from_vec(expanded))
        }

        // Vector: expand elements
        Template::Vector(templates) => {
            let mut expanded = Vec::new();
            for t in templates {
                expanded.push(expand_template_impl(t, bindings, ellipsis_depth)?);
            }
            Ok(Value::Vector(Rc::new(expanded)))
        }

        // Ellipsis: repeat expansion
        Template::Ellipsis { before, repeated, after } => {
            let mut result = Vec::new();

            // Expand 'before' templates
            for t in before {
                result.push(expand_template_impl(t, bindings, ellipsis_depth)?);
            }

            // Find pattern variables in repeated template
            let vars = find_pattern_vars(repeated);

            if vars.is_empty() {
                return Err(EvalError::MacroExpansion(
                    "Ellipsis template contains no pattern variables".to_string()
                ));
            }

            // Get length of repeated section (all vars should have same length)
            let repeat_count = match bindings.get(&vars[0]) {
                Some(BindingValue::Multiple(values)) => values.len(),
                Some(BindingValue::Single(_)) => {
                    return Err(EvalError::MacroExpansion(format!(
                        "Pattern variable {} used with ellipsis but not bound with ellipsis",
                        vars[0]
                    )));
                }
                None => {
                    return Err(EvalError::MacroExpansion(format!(
                        "Unbound pattern variable: {}",
                        vars[0]
                    )));
                }
            };

            // Verify all variables have same length
            for var in &vars[1..] {
                match bindings.get(var) {
                    Some(BindingValue::Multiple(values)) if values.len() == repeat_count => {}
                    _ => {
                        return Err(EvalError::MacroExpansion(format!(
                            "Pattern variables in ellipsis have different lengths"
                        )));
                    }
                }
            }

            // Expand repeated template for each iteration
            for i in 0..repeat_count {
                // Create temporary bindings for this iteration
                let mut iter_bindings = bindings.clone();

                for var in &vars {
                    if let Some(BindingValue::Multiple(values)) = bindings.get(var) {
                        iter_bindings.insert(
                            var.clone(),
                            BindingValue::Single(values[i].clone())
                        );
                    }
                }

                result.push(expand_template_impl(repeated, &iter_bindings, ellipsis_depth + 1)?);
            }

            // Expand 'after' templates
            for t in after {
                result.push(expand_template_impl(t, bindings, ellipsis_depth)?);
            }

            Ok(list_from_vec(result))
        }

        // Ellipsis escape: (... template) → literal ...
        Template::EllipsisEscape(inner) => {
            // Return the literal ellipsis symbol followed by expanded template
            let ellipsis = Value::Symbol("...".into());
            let expanded = expand_template_impl(inner, bindings, ellipsis_depth)?;
            Ok(list_from_vec(vec![ellipsis, expanded]))
        }
    }
}

/// Find all pattern variables in a template
fn find_pattern_vars(template: &Template) -> Vec<Rc<str>> {
    let mut vars = Vec::new();
    find_pattern_vars_impl(template, &mut vars);
    vars
}

fn find_pattern_vars_impl(template: &Template, vars: &mut Vec<Rc<str>>) {
    match template {
        Template::Variable(name) => {
            if !vars.contains(name) {
                vars.push(name.clone());
            }
        }
        Template::List(templates) | Template::Vector(templates) => {
            for t in templates {
                find_pattern_vars_impl(t, vars);
            }
        }
        Template::Ellipsis { before, repeated, after } => {
            for t in before {
                find_pattern_vars_impl(t, vars);
            }
            find_pattern_vars_impl(repeated, vars);
            for t in after {
                find_pattern_vars_impl(t, vars);
            }
        }
        Template::EllipsisEscape(inner) => {
            find_pattern_vars_impl(inner, vars);
        }
        Template::Literal(_) => {}
    }
}
```

**Key points:**
- Variables not in bindings become literal symbols (for hygiene)
- Ellipsis expansion validates all variables have same length
- Temporary bindings created for each iteration
- Clear error messages for misuse

---

## Integration Points

### 1. Add Macro Variant to Value Enum

**File:** `src/value/mod.rs`

```rust
pub enum Value {
    // ... existing variants ...

    /// Macro definition
    Macro(Rc<crate::macro_system::Macro>),
}
```

**Display implementation:**
```rust
impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            // ... existing cases ...
            Value::Macro(m) => write!(f, "#<macro {}>", m.name),
        }
    }
}
```

### 2. Add define-syntax Special Form

**File:** `src/eval/special_forms.rs`

```rust
pub(super) fn eval_define_syntax(
    &self,
    args: Vec<Value>,
    env: &Rc<Environment>,
) -> Result<Value, EvalError> {
    // Validate: (define-syntax name (syntax-rules ...))
    if args.len() != 2 {
        return Err(EvalError::SyntaxError(
            "define-syntax: expected (define-syntax name transformer)".to_string()
        ));
    }

    let name = match &args[0] {
        Value::Symbol(s) => s.clone(),
        _ => return Err(EvalError::TypeError(
            "define-syntax: name must be a symbol".to_string()
        )),
    };

    // Parse syntax-rules
    let macro_def = crate::macro_system::parse_syntax_rules(&args[1], env)?;

    // Store in macro environment (separate from value environment)
    self.macro_env.borrow_mut().insert(
        name.clone(),
        Value::Macro(Rc::new(macro_def))
    );

    Ok(Value::Unspecified)
}
```

### 3. Parse syntax-rules

**File:** `src/macro_system/mod.rs`

```rust
use crate::value::Value;
use crate::env::Environment;
use crate::eval::error::EvalError;

/// Parse a syntax-rules form
pub fn parse_syntax_rules(
    expr: &Value,
    env: &Rc<Environment>,
) -> Result<Macro, EvalError> {
    // Expect: (syntax-rules (literals...) (pattern template) ...)
    let items = collect_list_items(expr)
        .map_err(|_| EvalError::SyntaxError(
            "syntax-rules: expected proper list".to_string()
        ))?;

    if items.len() < 2 {
        return Err(EvalError::SyntaxError(
            "syntax-rules: expected (syntax-rules (literals...) rules...)".to_string()
        ));
    }

    // Check first element is 'syntax-rules
    match &items[0] {
        Value::Symbol(s) if s.as_ref() == "syntax-rules" => {}
        _ => return Err(EvalError::SyntaxError(
            "Expected syntax-rules keyword".to_string()
        )),
    }

    // Parse literals
    let literals = parse_literals(&items[1])?;

    // Parse rules
    let rules: Result<Vec<_>, _> = items[2..]
        .iter()
        .enumerate()
        .map(|(i, rule)| parse_macro_rule(rule, i))
        .collect();

    Ok(Macro {
        name: "anonymous".into(),  // Will be set by define-syntax
        literals,
        rules: rules?,
        env: env.clone(),
    })
}

fn parse_literals(expr: &Value) -> Result<Vec<Rc<str>>, EvalError> {
    match expr {
        Value::Null => Ok(vec![]),
        Value::Pair(_) => {
            let items = collect_list_items(expr)?;
            items.into_iter()
                .map(|item| match item {
                    Value::Symbol(s) => Ok(s),
                    _ => Err(EvalError::SyntaxError(
                        "syntax-rules: literals must be symbols".to_string()
                    ))
                })
                .collect()
        }
        _ => Err(EvalError::SyntaxError(
            "syntax-rules: expected list of literals".to_string()
        ))
    }
}

fn parse_macro_rule(expr: &Value, rule_num: usize) -> Result<MacroRule, EvalError> {
    let items = collect_list_items(expr)
        .map_err(|_| EvalError::SyntaxError(format!(
            "syntax-rules: rule {} must be a list", rule_num
        )))?;

    if items.len() != 2 {
        return Err(EvalError::SyntaxError(format!(
            "syntax-rules: rule {} must have exactly 2 elements (pattern template)", rule_num
        )));
    }

    let pattern = parse_pattern(&items[0])?;
    let template = parse_template(&items[1])?;

    Ok(MacroRule { pattern, template })
}
```

### 4. Macro Expansion in Evaluator

**File:** `src/eval/mod.rs`

```rust
pub struct Evaluator {
    pub global_env: Rc<Environment>,
    pub debug: Rc<DebugConfig>,

    // NEW: Separate macro environment
    macro_env: RefCell<HashMap<Rc<str>, Value>>,
}

impl Evaluator {
    pub fn new() -> Self {
        let global_env = Rc::new(Environment::new_global());
        install_primitives(&global_env);

        Self {
            global_env,
            debug: Rc::new(DebugConfig::new()),
            macro_env: RefCell::new(HashMap::new()),
        }
    }
}

fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    // ... existing debug tracing ...

    let result = match expr {
        // ... existing cases ...

        // Lists: check for macro expansion FIRST
        Value::Pair(_) => {
            let items = collect_list_items(expr)?;
            if items.is_empty() {
                return Err(EvalError::SyntaxError("Cannot evaluate empty list".to_string()));
            }

            // Check if first element is a macro name
            if let Value::Symbol(name) = &items[0] {
                if let Some(Value::Macro(m)) = self.macro_env.borrow().get(name) {
                    // Expand macro
                    let expanded = expand_macro(m.clone(), &items)?;

                    // Recursively evaluate expansion
                    return self.eval_in_env(&expanded, env);
                }
            }

            // Not a macro, proceed with normal evaluation
            self.eval_list(expr, env)
        }

        _ => // ... existing cases ...
    };

    // ... existing debug tracing ...

    result
}

/// Expand a macro application
fn expand_macro(m: Rc<Macro>, args: &[Value]) -> Result<Value, EvalError> {
    use crate::macro_system::{match_pattern, expand_template};

    // Try each rule in order (first-match-wins)
    for rule in &m.rules {
        // Pattern should match (macro-name arg1 arg2 ...)
        // So we match against the full application
        let full_expr = list_from_vec(args.to_vec());

        if let Some(bindings) = match_pattern(&rule.pattern, &full_expr, &m.literals) {
            // Match succeeded, expand template
            let expanded = expand_template(&rule.template, &bindings)?;

            // Apply hygiene
            let hygienic = apply_hygiene(expanded, &m.env)?;

            return Ok(hygienic);
        }
    }

    // No rule matched
    Err(EvalError::MacroExpansion(format!(
        "No matching pattern for macro {}",
        m.name
    )))
}
```

---

## Hygiene Strategy

### Simplified Identifier Renaming

**File:** `src/macro_system/hygiene.rs`

Following Steel's approach, we use identifier mangling:

```rust
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::value::Value;
use crate::env::Environment;

static GENSYM_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a fresh symbol name
fn gensym(prefix: &str) -> Rc<str> {
    let id = GENSYM_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("##{}#{}", prefix, id).into()
}

/// Apply hygiene to expanded macro output
pub fn apply_hygiene(
    expr: Value,
    macro_env: &Rc<Environment>,
) -> Result<Value, crate::eval::error::EvalError> {
    rename_identifiers(expr, macro_env)
}

/// Rename free identifiers to prevent capture
fn rename_identifiers(
    expr: Value,
    macro_env: &Rc<Environment>,
) -> Result<Value, crate::eval::error::EvalError> {
    match expr {
        // Symbols: check if should be renamed
        Value::Symbol(name) => {
            // If bound in macro environment, rename it
            // This prevents capturing user variables
            if macro_env.get(&name).is_some() {
                Ok(Value::Symbol(gensym(&name)))
            } else {
                // Not bound in macro env - keep as-is
                // (it's a pattern variable, will bind at use site)
                Ok(Value::Symbol(name))
            }
        }

        // Lists: recursively rename
        Value::Pair(_) => {
            let items = crate::eval::special_forms::collect_list_items(&expr)?;
            let renamed: Result<Vec<_>, _> = items
                .into_iter()
                .map(|item| rename_identifiers(item, macro_env))
                .collect();
            Ok(crate::eval::special_forms::list_from_vec(renamed?))
        }

        // Vectors: recursively rename
        Value::Vector(items) => {
            let renamed: Result<Vec<_>, _> = items
                .iter()
                .map(|item| rename_identifiers(item.clone(), macro_env))
                .collect();
            Ok(Value::Vector(Rc::new(renamed?)))
        }

        // Other values: no renaming needed
        _ => Ok(expr),
    }
}
```

**How it works:**
1. After template expansion, scan the output
2. For each identifier, check if it's bound in macro's definition environment
3. If yes: rename it to a fresh symbol (prevents capture)
4. If no: keep as-is (it's from pattern, should bind at use site)

**Example:**
```scheme
(define-syntax when
  (syntax-rules ()
    ((when test body ...)
     (if test (begin body ...)))))

;; Macro environment binds: if, begin
;; Expansion of (when #t 1 2):
;; Before hygiene: (if #t (begin 1 2))
;; After hygiene:  (##if#0 #t (##begin#1 1 2))
```

This prevents user's `if` variable from being captured.

---

## Implementation Phases

### Phase 1: Data Structures (Week 1, 3-5 days)

**Goal:** Define all types, no functionality yet

**Tasks:**
- [ ] Create `src/macro_system/mod.rs` with Pattern, Template, Macro structs
- [ ] Add `Value::Macro` variant
- [ ] Update Display for new variant
- [ ] Add stub files for pattern.rs, template.rs, hygiene.rs

**Success:** Code compiles, all tests still pass

### Phase 2: Pattern Matching (Week 2, 5-7 days)

**Goal:** Implement pattern matching algorithm

**Tasks:**
- [ ] Implement `match_pattern` in pattern.rs
- [ ] Handle literals, variables, wildcards
- [ ] Handle lists and vectors
- [ ] Handle ellipsis patterns
- [ ] Write unit tests for each pattern type

**Success:** All pattern matching unit tests pass

**Test example:**
```rust
#[test]
fn test_match_simple() {
    let pattern = Pattern::List(vec![
        Pattern::Variable("when".into()),
        Pattern::Variable("test".into()),
        Pattern::Ellipsis {
            before: vec![],
            repeated: Box::new(Pattern::Variable("body".into())),
            after: vec![],
        },
    ]);

    let expr = list_from_vec(vec![
        Value::Symbol("when".into()),
        Value::Boolean(true),
        Value::Integer(1),
        Value::Integer(2),
    ]);

    let bindings = match_pattern(&pattern, &expr, &[]).unwrap();

    assert_eq!(bindings.get("when"), Some(&BindingValue::Single(Value::Symbol("when".into()))));
    assert_eq!(bindings.get("test"), Some(&BindingValue::Single(Value::Boolean(true))));
    assert!(matches!(bindings.get("body"), Some(BindingValue::Multiple(_))));
}
```

### Phase 3: Template Expansion (Week 3, 5-7 days)

**Goal:** Implement template expansion algorithm

**Tasks:**
- [ ] Implement `expand_template` in template.rs
- [ ] Handle literals and variables
- [ ] Handle lists and vectors
- [ ] Handle ellipsis expansion
- [ ] Handle ellipsis escape (... ...)
- [ ] Write unit tests

**Success:** All template expansion unit tests pass

**Test example:**
```rust
#[test]
fn test_expand_simple() {
    let template = Template::List(vec![
        Template::Literal(Value::Symbol("if".into())),
        Template::Variable("test".into()),
        Template::List(vec![
            Template::Literal(Value::Symbol("begin".into())),
            Template::Ellipsis {
                before: vec![],
                repeated: Box::new(Template::Variable("body".into())),
                after: vec![],
            },
        ]),
    ]);

    let mut bindings = Bindings::new();
    bindings.insert("test".into(), BindingValue::Single(Value::Boolean(true)));
    bindings.insert("body".into(), BindingValue::Multiple(vec![
        Value::Integer(1),
        Value::Integer(2),
    ]));

    let result = expand_template(&template, &bindings).unwrap();

    // Should produce: (if #t (begin 1 2))
    let expected = list_from_vec(vec![
        Value::Symbol("if".into()),
        Value::Boolean(true),
        list_from_vec(vec![
            Value::Symbol("begin".into()),
            Value::Integer(1),
            Value::Integer(2),
        ]),
    ]);

    assert_eq!(result, expected);
}
```

### Phase 4: Parsing (Week 4, 3-5 days)

**Goal:** Parse syntax-rules forms into Macro struct

**Tasks:**
- [ ] Implement `parse_syntax_rules` in mod.rs
- [ ] Implement `parse_pattern` (Value → Pattern)
- [ ] Implement `parse_template` (Value → Template)
- [ ] Handle all edge cases (improper lists, etc.)
- [ ] Write parsing tests

**Success:** Can parse all test macros from fixtures

**Test example:**
```rust
#[test]
fn test_parse_when_macro() {
    let syntax_rules = parse_str(r#"
        (syntax-rules ()
          ((when test body ...)
           (if test (begin body ...))))
    "#).unwrap();

    let env = Rc::new(Environment::new_global());
    let macro_def = parse_syntax_rules(&syntax_rules, &env).unwrap();

    assert_eq!(macro_def.literals.len(), 0);
    assert_eq!(macro_def.rules.len(), 1);
}
```

### Phase 5: Integration (Week 5, 5-7 days)

**Goal:** Wire everything into the evaluator

**Tasks:**
- [ ] Add `macro_env` to Evaluator
- [ ] Implement `eval_define_syntax` special form
- [ ] Add macro expansion to eval_in_env
- [ ] Update eval_list dispatcher
- [ ] Add error handling for macro expansion
- [ ] Write integration tests

**Success:** Basic macros work end-to-end

**Test example:**
```rust
#[test]
fn test_when_macro_integration() {
    let mut interp = Interpreter::new();

    // Define macro
    interp.eval_str(r#"
        (define-syntax when
          (syntax-rules ()
            ((when test body ...)
             (if test (begin body ...)))))
    "#).unwrap();

    // Use macro
    let result = interp.eval_str("(when #t 42)").unwrap();
    assert_eq!(result, Value::Integer(42));

    // Should not execute when test is false
    let result = interp.eval_str("(when #f 42)").unwrap();
    assert_eq!(result, Value::Unspecified);
}
```

### Phase 6: Hygiene (Week 6, 3-5 days)

**Goal:** Add identifier renaming for hygiene

**Tasks:**
- [ ] Implement `apply_hygiene` in hygiene.rs
- [ ] Implement `rename_identifiers`
- [ ] Add gensym counter
- [ ] Apply hygiene after template expansion
- [ ] Write hygiene tests

**Success:** All hygiene tests from 02_hygiene_tests.scm pass

**Test example:**
```rust
#[test]
fn test_hygiene_no_capture() {
    let mut interp = Interpreter::new();

    interp.eval_str(r#"
        (define-syntax when
          (syntax-rules ()
            ((when test body ...)
             (if test (begin body ...)))))
    "#).unwrap();

    // User's 'if' variable should not be captured
    let result = interp.eval_str(r#"
        (let ((if #t))
          (when if (set! if 'now))
          if)
    "#).unwrap();

    assert_eq!(result, Value::Symbol("now".into()));
}
```

### Phase 7: Testing & Refinement (Week 7, 3-5 days)

**Goal:** Ensure all tests pass, fix edge cases

**Tasks:**
- [ ] Run 01_basic_when_unless.scm - all tests pass
- [ ] Run 02_hygiene_tests.scm - all tests pass
- [ ] Check r7rs-tests.scm - when/unless/do work
- [ ] Add comprehensive error messages
- [ ] Document public API
- [ ] Performance testing

**Success:** 245/245 r7rs-tests passing (99%+)

---

## Testing Strategy

### Testing Infrastructure

To make testing easier, we'll add test helper utilities in `tests/common/macro_helpers.rs`:

```rust
use patina::{Interpreter, Value};
use std::rc::Rc;

/// Assert that a macro expands to the expected form
///
/// Example:
/// ```
/// assert_macro_expands_to!(
///     interp,
///     "(when #t 1 2)",
///     "(if #t (begin 1 2))"
/// );
/// ```
#[macro_export]
macro_rules! assert_macro_expands_to {
    ($interp:expr, $input:expr, $expected:expr) => {{
        let expanded = $interp.expand_macro_str($input)
            .expect(&format!("Failed to expand: {}", $input));
        let expected = $interp.parse_str($expected)
            .expect(&format!("Failed to parse expected: {}", $expected));

        assert_eq!(
            expanded, expected,
            "\nMacro expansion mismatch:\n  Input:    {}\n  Expected: {}\n  Got:      {}",
            $input, $expected, expanded
        );
    }};
}

/// Assert that macro expansion fails with expected error
///
/// Example:
/// ```
/// assert_macro_expansion_fails!(
///     interp,
///     "(when)",
///     "No pattern matched"
/// );
/// ```
#[macro_export]
macro_rules! assert_macro_expansion_fails {
    ($interp:expr, $input:expr, $error_contains:expr) => {{
        let result = $interp.expand_macro_str($input);
        assert!(
            result.is_err(),
            "Expected macro expansion to fail for: {}",
            $input
        );

        let error = result.unwrap_err().to_string();
        assert!(
            error.contains($error_contains),
            "Error message '{}' does not contain '{}'",
            error, $error_contains
        );
    }};
}

/// Assert that pattern matches input and produces expected bindings
///
/// Example:
/// ```
/// let pattern = parse_pattern("(when test body ...)");
/// let input = parse_value("(when #t 1 2)");
///
/// assert_pattern_matches!(
///     pattern, input, [],
///     {
///         "test" => Single(Boolean(true)),
///         "body" => Multiple([Integer(1), Integer(2)])
///     }
/// );
/// ```
#[macro_export]
macro_rules! assert_pattern_matches {
    ($pattern:expr, $input:expr, $literals:expr, { $($var:expr => $binding:expr),* $(,)? }) => {{
        use patina::macro_system::{match_pattern, BindingValue};

        let bindings = match_pattern(&$pattern, &$input, &$literals)
            .expect(&format!("Pattern did not match input"));

        $(
            let expected_binding = $binding;
            let actual_binding = bindings.get($var)
                .expect(&format!("Binding not found: {}", $var));

            assert_eq!(
                actual_binding, &expected_binding,
                "Binding mismatch for '{}': expected {:?}, got {:?}",
                $var, expected_binding, actual_binding
            );
        )*
    }};
}

/// Assert that pattern does NOT match input
///
/// Example:
/// ```
/// assert_pattern_does_not_match!(
///     pattern,
///     parse_value("(when)"),
///     []
/// );
/// ```
#[macro_export]
macro_rules! assert_pattern_does_not_match {
    ($pattern:expr, $input:expr, $literals:expr) => {{
        use patina::macro_system::match_pattern;

        let result = match_pattern(&$pattern, &$input, &$literals);
        assert!(
            result.is_none(),
            "Pattern should not have matched input, but it did"
        );
    }};
}

/// Assert that template expansion produces expected output
///
/// Example:
/// ```
/// let template = parse_template("(if test (begin body ...))");
/// let bindings = hashmap! {
///     "test" => BindingValue::Single(Value::Boolean(true)),
///     "body" => BindingValue::Multiple(vec![Value::Integer(1), Value::Integer(2)])
/// };
///
/// assert_template_expands_to!(
///     template, bindings,
///     "(if #t (begin 1 2))"
/// );
/// ```
#[macro_export]
macro_rules! assert_template_expands_to {
    ($template:expr, $bindings:expr, $expected:expr) => {{
        use patina::macro_system::expand_template;

        let expanded = expand_template(&$template, &$bindings)
            .expect("Template expansion failed");

        let expected = parse_value($expected);

        assert_eq!(
            expanded, expected,
            "\nTemplate expansion mismatch:\n  Expected: {}\n  Got:      {}",
            expected, expanded
        );
    }};
}
```

### Helper Functions for Tests

```rust
// In tests/common/macro_helpers.rs

use patina::{Interpreter, Value};
use patina::macro_system::{Pattern, Template, Bindings, BindingValue};
use std::rc::Rc;

/// Parse a string into a Value for testing
pub fn parse_value(s: &str) -> Value {
    let mut interp = Interpreter::new();
    interp.parse_str(s).expect(&format!("Failed to parse: {}", s))
}

/// Parse a pattern string for testing
///
/// Example: parse_pattern("(when test body ...)")
pub fn parse_pattern(s: &str) -> Pattern {
    let value = parse_value(s);
    patina::macro_system::parse_pattern(&value)
        .expect(&format!("Failed to parse pattern: {}", s))
}

/// Parse a template string for testing
///
/// Example: parse_template("(if test (begin body ...))")
pub fn parse_template(s: &str) -> Template {
    let value = parse_value(s);
    patina::macro_system::parse_template(&value)
        .expect(&format!("Failed to parse template: {}", s))
}

/// Create bindings for testing
///
/// Example:
/// ```
/// let bindings = test_bindings(vec![
///     ("test", BindingValue::Single(Value::Boolean(true))),
///     ("body", BindingValue::Multiple(vec![Value::Integer(1), Value::Integer(2)])),
/// ]);
/// ```
pub fn test_bindings(pairs: Vec<(&str, BindingValue)>) -> Bindings {
    pairs.into_iter()
        .map(|(k, v)| (Rc::from(k), v))
        .collect()
}
```

### Extension to Interpreter (for testing)

Add these methods to `Interpreter` (in `src/lib.rs`):

```rust
impl Interpreter {
    /// Expand a macro without evaluating (for testing)
    ///
    /// This is useful for testing macro expansion in isolation
    pub fn expand_macro_str(&mut self, input: &str) -> Result<Value, InterpreterError> {
        let expr = self.parse_str(input)?;

        // Check if it's a macro application
        if let Value::Pair(_) = &expr {
            let items = crate::eval::special_forms::collect_list_items(&expr)?;
            if let Some(Value::Symbol(name)) = items.first() {
                if let Some(Value::Macro(m)) = self.evaluator.macro_env.borrow().get(name) {
                    let expanded = crate::eval::expand_macro(m.clone(), &items)?;
                    return Ok(expanded);
                }
            }
        }

        // Not a macro application
        Ok(expr)
    }

    /// Parse a string into Value (exposed for testing)
    pub fn parse_str(&self, input: &str) -> Result<Value, InterpreterError> {
        let tokens = crate::lexer::tokenize(input)?;
        let (value, _) = crate::parser::parse(&tokens)?;
        Ok(value)
    }
}
```

### Unit Tests (in src/macro_system/)

With our testing infrastructure, tests become very readable:

**Pattern matching tests:**

```rust
#[cfg(test)]
mod pattern_tests {
    use super::*;
    use crate::macro_helpers::*;

    #[test]
    fn test_pattern_literal_matching() {
        let pattern = parse_pattern("42");
        let input = parse_value("42");

        assert_pattern_matches!(pattern, input, [], {});

        // Should NOT match different literal
        assert_pattern_does_not_match!(
            pattern,
            parse_value("43"),
            []
        );
    }

    #[test]
    fn test_pattern_variable_binding() {
        let pattern = parse_pattern("x");
        let input = parse_value("42");

        assert_pattern_matches!(
            pattern, input, [],
            { "x" => BindingValue::Single(Value::Integer(42)) }
        );
    }

    #[test]
    fn test_pattern_wildcard() {
        let pattern = parse_pattern("_");
        let input = parse_value("anything");

        // Wildcard matches but binds nothing
        assert_pattern_matches!(pattern, input, [], {});
    }

    #[test]
    fn test_pattern_list() {
        let pattern = parse_pattern("(when test body)");
        let input = parse_value("(when #t 42)");

        assert_pattern_matches!(
            pattern, input, [],
            {
                "when" => BindingValue::Single(Value::Symbol("when".into())),
                "test" => BindingValue::Single(Value::Boolean(true)),
                "body" => BindingValue::Single(Value::Integer(42))
            }
        );
    }

    #[test]
    fn test_pattern_ellipsis_zero_elements() {
        let pattern = parse_pattern("(when test body ...)");
        let input = parse_value("(when #t)");

        assert_pattern_matches!(
            pattern, input, [],
            {
                "when" => BindingValue::Single(Value::Symbol("when".into())),
                "test" => BindingValue::Single(Value::Boolean(true)),
                "body" => BindingValue::Multiple(vec![])
            }
        );
    }

    #[test]
    fn test_pattern_ellipsis_multiple_elements() {
        let pattern = parse_pattern("(when test body ...)");
        let input = parse_value("(when #t 1 2 3)");

        assert_pattern_matches!(
            pattern, input, [],
            {
                "when" => BindingValue::Single(Value::Symbol("when".into())),
                "test" => BindingValue::Single(Value::Boolean(true)),
                "body" => BindingValue::Multiple(vec![
                    Value::Integer(1),
                    Value::Integer(2),
                    Value::Integer(3)
                ])
            }
        );
    }

    #[test]
    fn test_pattern_literal_identifier() {
        let pattern = parse_pattern("(cond (test result) (else default))");
        let input = parse_value("(cond (#t 42) (else 0))");
        let literals = vec!["else".into()];

        // 'else' should match as literal, not bind as variable
        let bindings = match_pattern(&pattern, &input, &literals).unwrap();

        // 'else' should NOT be in bindings
        assert!(bindings.get("else").is_none());
    }
}
```

**Template expansion tests:**

```rust
#[cfg(test)]
mod template_tests {
    use super::*;
    use crate::macro_helpers::*;

    #[test]
    fn test_template_literal() {
        let template = parse_template("42");
        let bindings = test_bindings(vec![]);

        assert_template_expands_to!(
            template, bindings,
            "42"
        );
    }

    #[test]
    fn test_template_variable_substitution() {
        let template = parse_template("test");
        let bindings = test_bindings(vec![
            ("test", BindingValue::Single(Value::Boolean(true)))
        ]);

        assert_template_expands_to!(
            template, bindings,
            "#t"
        );
    }

    #[test]
    fn test_template_list_construction() {
        let template = parse_template("(if test result)");
        let bindings = test_bindings(vec![
            ("test", BindingValue::Single(Value::Boolean(true))),
            ("result", BindingValue::Single(Value::Integer(42)))
        ]);

        assert_template_expands_to!(
            template, bindings,
            "(if #t 42)"
        );
    }

    #[test]
    fn test_template_ellipsis_expansion() {
        let template = parse_template("(begin body ...)");
        let bindings = test_bindings(vec![
            ("body", BindingValue::Multiple(vec![
                Value::Integer(1),
                Value::Integer(2),
                Value::Integer(3)
            ]))
        ]);

        assert_template_expands_to!(
            template, bindings,
            "(begin 1 2 3)"
        );
    }

    #[test]
    fn test_template_ellipsis_zero_elements() {
        let template = parse_template("(begin body ...)");
        let bindings = test_bindings(vec![
            ("body", BindingValue::Multiple(vec![]))
        ]);

        assert_template_expands_to!(
            template, bindings,
            "(begin)"
        );
    }

    #[test]
    fn test_template_ellipsis_escape() {
        let template = parse_template("(... (list ...))");
        let bindings = test_bindings(vec![]);

        // Should produce literal (... (list ...))
        assert_template_expands_to!(
            template, bindings,
            "(... (list ...))"
        );
    }

    #[test]
    fn test_template_expansion_error_unbound_variable() {
        let template = parse_template("undefined");
        let bindings = test_bindings(vec![]);

        // Should fail - undefined variable
        let result = expand_template(&template, &bindings);
        assert!(result.is_err());
    }

    #[test]
    fn test_template_expansion_error_ellipsis_mismatch() {
        let template = parse_template("(test ...)");
        let bindings = test_bindings(vec![
            // test is Single, but used with ellipsis
            ("test", BindingValue::Single(Value::Integer(42)))
        ]);

        let result = expand_template(&template, &bindings);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("ellipsis"));
    }
}
```

**Parsing tests:**

```rust
#[cfg(test)]
mod parsing_tests {
    use super::*;

    #[test]
    fn test_parse_simple_pattern() {
        let value = parse_value("(when test body)");
        let pattern = parse_pattern(&value).unwrap();

        assert!(matches!(pattern, Pattern::List(_)));
    }

    #[test]
    fn test_parse_ellipsis_pattern() {
        let value = parse_value("(when test body ...)");
        let pattern = parse_pattern(&value).unwrap();

        if let Pattern::List(patterns) = pattern {
            assert_eq!(patterns.len(), 3);
            // Last element should be ellipsis
            assert!(matches!(patterns[2], Pattern::Ellipsis { .. }));
        } else {
            panic!("Expected List pattern");
        }
    }

    #[test]
    fn test_parse_syntax_rules() {
        let mut interp = Interpreter::new();

        let syntax_rules = parse_value(r#"
            (syntax-rules ()
              ((when test body ...)
               (if test (begin body ...))))
        "#);

        let env = Rc::new(Environment::new_global());
        let macro_def = parse_syntax_rules(&syntax_rules, &env).unwrap();

        assert_eq!(macro_def.literals.len(), 0);
        assert_eq!(macro_def.rules.len(), 1);
    }

    #[test]
    fn test_parse_syntax_rules_with_literals() {
        let syntax_rules = parse_value(r#"
            (syntax-rules (else =>)
              ((cond (else result))
               result))
        "#);

        let env = Rc::new(Environment::new_global());
        let macro_def = parse_syntax_rules(&syntax_rules, &env).unwrap();

        assert_eq!(macro_def.literals.len(), 2);
        assert!(macro_def.literals.contains(&"else".into()));
        assert!(macro_def.literals.contains(&"=>".into()));
    }
}
```

**Hygiene tests:**

```rust
#[cfg(test)]
mod hygiene_tests {
    use super::*;
    use crate::macro_helpers::*;

    #[test]
    fn test_gensym_uniqueness() {
        let sym1 = gensym("test");
        let sym2 = gensym("test");

        assert_ne!(sym1, sym2);
        assert!(sym1.starts_with("##test#"));
        assert!(sym2.starts_with("##test#"));
    }

    #[test]
    fn test_rename_bound_identifier() {
        let env = Rc::new(Environment::new_global());
        env.define("if".into(), Value::Symbol("special-form".into()));

        let expr = Value::Symbol("if".into());
        let renamed = rename_identifiers(expr, &env).unwrap();

        // Should be renamed (not equal to original)
        assert_ne!(renamed, Value::Symbol("if".into()));
        assert!(matches!(renamed, Value::Symbol(_)));
    }

    #[test]
    fn test_keep_unbound_identifier() {
        let env = Rc::new(Environment::new_global());

        let expr = Value::Symbol("user-variable".into());
        let renamed = rename_identifiers(expr, &env).unwrap();

        // Should NOT be renamed (not bound in macro env)
        assert_eq!(renamed, Value::Symbol("user-variable".into()));
    }

    #[test]
    fn test_rename_in_list() {
        let env = Rc::new(Environment::new_global());
        env.define("if".into(), Value::Symbol("special-form".into()));
        env.define("begin".into(), Value::Symbol("special-form".into()));

        let expr = parse_value("(if test (begin body))");
        let renamed = rename_identifiers(expr, &env).unwrap();

        let items = collect_list_items(&renamed).unwrap();

        // 'if' and 'begin' should be renamed
        if let Value::Symbol(s) = &items[0] {
            assert!(s.starts_with("##if#"));
        } else {
            panic!("Expected renamed symbol");
        }
    }
}
```

### Integration Tests (in tests/)

**Using the testing macros for integration tests:**

```rust
// tests/macro_integration_test.rs

use patina::Interpreter;

#[test]
fn test_when_macro_basic() {
    let mut interp = Interpreter::new();

    // Define the macro
    interp.eval_str(r#"
        (define-syntax when
          (syntax-rules ()
            ((when test body ...)
             (if test (begin body ...)))))
    "#).unwrap();

    // Test expansion
    assert_macro_expands_to!(
        interp,
        "(when #t 1 2 3)",
        "(if #t (begin 1 2 3))"
    );

    // Test evaluation
    let result = interp.eval_str("(when #t 42)").unwrap();
    assert_eq!(result, Value::Integer(42));

    let result = interp.eval_str("(when #f 42)").unwrap();
    assert_eq!(result, Value::Unspecified);
}

#[test]
fn test_when_macro_multiple_body_forms() {
    let mut interp = Interpreter::new();

    interp.eval_str(r#"
        (define-syntax when
          (syntax-rules ()
            ((when test body ...)
             (if test (begin body ...)))))
    "#).unwrap();

    // Multiple body forms should all execute
    let result = interp.eval_str(r#"
        (define x 0)
        (when #t
          (set! x 1)
          (set! x (+ x 1))
          (set! x (+ x 1)))
        x
    "#).unwrap();

    assert_eq!(result, Value::Integer(3));
}

#[test]
fn test_unless_macro() {
    let mut interp = Interpreter::new();

    interp.eval_str(r#"
        (define-syntax unless
          (syntax-rules ()
            ((unless test body ...)
             (if (not test) (begin body ...)))))
    "#).unwrap();

    assert_macro_expands_to!(
        interp,
        "(unless #f 1 2)",
        "(if (not #f) (begin 1 2))"
    );

    let result = interp.eval_str("(unless #f 42)").unwrap();
    assert_eq!(result, Value::Integer(42));

    let result = interp.eval_str("(unless #t 42)").unwrap();
    assert_eq!(result, Value::Unspecified);
}

#[test]
fn test_macro_hygiene_no_capture() {
    let mut interp = Interpreter::new();

    interp.eval_str(r#"
        (define-syntax when
          (syntax-rules ()
            ((when test body ...)
             (if test (begin body ...)))))
    "#).unwrap();

    // User's 'if' variable should NOT be captured by macro's 'if'
    let result = interp.eval_str(r#"
        (let ((if #t))
          (when if (set! if 'now))
          if)
    "#).unwrap();

    assert_eq!(result, Value::Symbol("now".into()));
}

#[test]
fn test_macro_expansion_error_no_match() {
    let mut interp = Interpreter::new();

    interp.eval_str(r#"
        (define-syntax when
          (syntax-rules ()
            ((when test body ...)
             (if test (begin body ...)))))
    "#).unwrap();

    // Should fail - no body forms
    assert_macro_expansion_fails!(
        interp,
        "(when #t)",
        "No matching pattern"
    );
}

#[test]
fn test_cond_with_else_literal() {
    let mut interp = Interpreter::new();

    interp.eval_str(r#"
        (define-syntax simple-cond
          (syntax-rules (else)
            ((simple-cond (else result))
             result)
            ((simple-cond (test result))
             (if test result))))
    "#).unwrap();

    // Test expansion with else
    assert_macro_expands_to!(
        interp,
        "(simple-cond (else 42))",
        "42"
    );

    // Test that 'else' as a local variable doesn't match
    let result = interp.eval_str(r#"
        (let ((else #f))
          (simple-cond (#t 'ok)))
    "#).unwrap();

    assert_eq!(result, Value::Symbol("ok".into()));
}

#[test]
fn test_macro_with_multiple_rules() {
    let mut interp = Interpreter::new();

    interp.eval_str(r#"
        (define-syntax simple-cond
          (syntax-rules (else)
            ((simple-cond (else result))
             result)
            ((simple-cond (test result))
             (if test result))
            ((simple-cond (test result) clause ...)
             (if test result (simple-cond clause ...)))))
    "#).unwrap();

    // Test first rule (else)
    let result = interp.eval_str("(simple-cond (else 1))").unwrap();
    assert_eq!(result, Value::Integer(1));

    // Test second rule (single clause)
    let result = interp.eval_str("(simple-cond (#t 2))").unwrap();
    assert_eq!(result, Value::Integer(2));

    // Test third rule (multiple clauses)
    let result = interp.eval_str(r#"
        (simple-cond
          (#f 1)
          (#t 2)
          (else 3))
    "#).unwrap();
    assert_eq!(result, Value::Integer(2));
}

#[test]
fn test_or_macro_with_recursion() {
    let mut interp = Interpreter::new();

    interp.eval_str(r#"
        (define-syntax my-or
          (syntax-rules ()
            ((my-or) #f)
            ((my-or e) e)
            ((my-or e1 e2 ...)
             (let ((temp e1))
               (if temp temp (my-or e2 ...))))))
    "#).unwrap();

    // Test zero arguments
    let result = interp.eval_str("(my-or)").unwrap();
    assert_eq!(result, Value::Boolean(false));

    // Test one argument
    let result = interp.eval_str("(my-or 42)").unwrap();
    assert_eq!(result, Value::Integer(42));

    // Test multiple arguments
    let result = interp.eval_str("(my-or #f #f 42 99)").unwrap();
    assert_eq!(result, Value::Integer(42));

    // Test short-circuit evaluation
    let result = interp.eval_str(r#"
        (define x 0)
        (my-or #t (set! x 1))
        x
    "#).unwrap();
    // x should still be 0 (second expression not evaluated)
    assert_eq!(result, Value::Integer(0));
}
```

**Scheme test files:**

```scheme
;; tests/fixtures/examples/macros/01_basic_when_unless.scm
(import (scheme base) (scheme write))

;; when macro - execute body if test is true
(define-syntax when
  (syntax-rules ()
    ((when test result1 result2 ...)
     (if test (begin result1 result2 ...)))))

;; Test 1: when with single expression
(display "Test 1: when with single expression\n")
(when #t (display "  Success!\n"))  ; Should print
(when #f (display "  Fail!\n"))     ; Should not print

;; Test 2: when with multiple expressions
(display "\nTest 2: when with multiple expressions\n")
(define x 0)
(when #t
  (set! x 1)
  (set! x (+ x 1))
  (set! x (+ x 1)))
(display "  x = ")
(display x)  ; Should be 3
(display "\n")

;; unless macro
(define-syntax unless
  (syntax-rules ()
    ((unless test result1 result2 ...)
     (if (not test) (begin result1 result2 ...)))))

(display "\nTest 3: unless\n")
(unless #f (display "  Success!\n"))  ; Should print
(unless #t (display "  Fail!\n"))     ; Should not print

(display "\nAll when/unless tests completed!\n")
```

```scheme
;; tests/fixtures/examples/macros/02_hygiene_tests.scm
(import (scheme base) (scheme write))

(display "=== Hygiene Test Suite ===\n\n")

;; Test 1: Macro-inserted bindings don't capture user variables
(display "Test 1: Inserted binding hygiene\n")
(define-syntax when
  (syntax-rules ()
    ((when test stmt1 stmt2 ...)
     (if test (begin stmt1 stmt2 ...)))))

(define test1-result
  (let ((if #t))
    (when if (set! if 'now))
    if))

(display "  Expected: now\n")
(display "  Got: ")
(display test1-result)
(display "\n")
(if (eq? test1-result 'now)
    (display "  PASS\n")
    (display "  FAIL\n"))

;; Test 2: Temporary variable hygiene
(display "\nTest 2: Temporary variable hygiene\n")
(define-syntax my-or
  (syntax-rules ()
    ((my-or e1 e2)
     (let ((temp e1))
       (if temp temp e2)))))

(define test2-result
  (let ((temp 'outer-temp))
    (my-or #f temp)))  ; Should return 'outer-temp, not capture it

(display "  Expected: outer-temp\n")
(display "  Got: ")
(display test2-result)
(display "\n")
(if (eq? test2-result 'outer-temp)
    (display "  PASS\n")
    (display "  FAIL\n"))

(display "\n=== Hygiene tests complete ===\n")
```

**R7RS compliance tests:**

```bash
# Run full test suite
cargo test

# Run macro-specific integration tests
cargo test --test macro_integration_test

# Run scheme file tests
cargo run --release < tests/fixtures/examples/macros/01_basic_when_unless.scm
cargo run --release < tests/fixtures/examples/macros/02_hygiene_tests.scm

# Check r7rs-tests.scm compliance
cargo test --test r7rs_compliance
```

### Test Coverage Goals

- **Unit tests:** 100% coverage of pattern.rs, template.rs, hygiene.rs
- **Integration tests:** All macro examples from R7RS spec
- **Compliance:** 245/245 r7rs-tests.scm passing

---

## Success Criteria

### Phase 1-4 (Data & Algorithms)
- ✅ All unit tests pass
- ✅ Pattern matching handles all R7RS patterns
- ✅ Template expansion handles all R7RS templates
- ✅ Parsing correctly converts Value → Pattern/Template

### Phase 5 (Integration)
- ✅ `when` macro works correctly
- ✅ `unless` macro works correctly
- ✅ Multiple rules work (cond, case)
- ✅ Error messages are clear and helpful

### Phase 6 (Hygiene)
- ✅ All hygiene tests pass
- ✅ Macro-inserted identifiers don't capture user variables
- ✅ Free references use macro definition environment

### Phase 7 (Completion)
- ✅ 01_basic_when_unless.scm passes
- ✅ 02_hygiene_tests.scm passes
- ✅ r7rs-tests.scm: 245/245 tests pass (99%+)
- ✅ No regressions in existing tests
- ✅ Documentation complete

---

## Error Handling

### Error Messages

**Pattern matching errors:**
```
Error: Macro expansion failed for 'when'
  No pattern matched input: (when)
  Expected at least: (when test body ...)
```

**Template expansion errors:**
```
Error: Macro expansion failed for 'when'
  Pattern variable 'body' used outside ellipsis context
  In template: (if test body)
```

**Hygiene errors:**
```
Error: Macro expansion failed
  Identifier 'undefined-var' not bound in macro environment
```

### Error Recovery

- Pattern matching: try next rule on failure
- Template expansion: fail fast with clear location
- Hygiene: never fail (always rename)

---

## Future Enhancements (Not in Scope)

These can be added after basic functionality works:

1. **let-syntax / letrec-syntax**
   - Local macro definitions
   - Requires tracking macro scope

2. **Macro debugging**
   - `(debug-expand 'macro-name expr)`
   - Show expansion steps

3. **Performance optimizations**
   - Cache expanded macros
   - Compile-time macro expansion

4. **Advanced hygiene**
   - Full syntactic closures
   - Better free variable handling

5. **syntax-case (R6RS)**
   - More powerful pattern matching
   - Requires implementing syntax objects

---

## References

**R7RS Specification:**
- Section 4.3 (Macros): `spec/r7rs-small-spec/expr.tex` lines 1443-1850

**Research Documents:**
- `internal/MACRO_R7RS_ANALYSIS.md` - Spec analysis
- `internal/STEEL_MACRO_ANALYSIS.md` - Reference implementation
- `internal/MACRO_RESEARCH_SUMMARY.md` - Research summary

**Reference Implementation:**
- Steel: `~/Project/reference/steel/crates/steel-core/src/parser/expander.rs`

**Test Cases:**
- `tests/fixtures/examples/macros/01_basic_when_unless.scm`
- `tests/fixtures/examples/macros/02_hygiene_tests.scm`

---

**Status:** Design Complete - Ready for Implementation
**Estimated Timeline:** 6-7 weeks to full R7RS macro compliance
**Next Step:** Begin Phase 1 - Data Structures
