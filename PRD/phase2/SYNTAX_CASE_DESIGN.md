# Syntax-Case Implementation Design

**Status:** Future Enhancement
**Priority:** Medium (after Phase 1 R7RS compliance)
**Estimated Complexity:** High
**Prerequisites:** Current `syntax-rules` implementation complete

---

## Executive Summary

This document outlines the design for implementing `syntax-case`, the procedural macro system that underlies `syntax-rules` in R6RS and many Scheme implementations. While `syntax-rules` is pattern-based and declarative, `syntax-case` provides full programmatic control over macro expansion.

**Key Insight:** Patina's existing scope-set hygiene infrastructure is fully compatible with `syntax-case`. The main work is adding syntax objects and procedural interfaces—the hygiene mechanism remains unchanged.

---

## Table of Contents

1. [Motivation](#motivation)
2. [Current State](#current-state)
3. [Syntax Objects](#syntax-objects)
4. [Core Forms](#core-forms)
5. [Hygiene Utilities](#hygiene-utilities)
6. [Quasisyntax](#quasisyntax)
7. [Implementation Phases](#implementation-phases)
8. [Integration Strategy](#integration-strategy)
9. [Testing Strategy](#testing-strategy)
10. [References](#references)

---

## Motivation

### Limitations of syntax-rules

`syntax-rules` cannot:

1. **Inspect syntax programmatically**
   ```scheme
   ;; Cannot check if identifier is bound
   (define-syntax safe-set!
     (syntax-rules ()
       ((_ var val)
        ;; How to check if var is defined?
        (set! var val))))
   ```

2. **Generate identifiers dynamically**
   ```scheme
   ;; Cannot create foo-getter, foo-setter from foo
   (define-syntax define-property
     (syntax-rules ()
       ((_ name)
        ;; Need to construct name-getter, name-setter
        ...)))
   ```

3. **Use guards/fenders on patterns**
   ```scheme
   ;; Cannot reject non-identifier test
   (define-syntax my-when
     (syntax-rules ()
       ((_ test body ...)
        ;; What if test is not an identifier?
        (if test (begin body ...)))))
   ```

4. **Implement complex transformations**
   - Loop unrolling based on literal count
   - Conditional code generation
   - Error messages with source location

### Benefits of syntax-case

- Full Scheme available during expansion
- Pattern matching with guards
- Programmatic syntax construction
- Better error messages
- Foundation for syntax-rules (can be a macro!)

---

## Current State

### What We Have

1. **Scope-set hygiene** (Racket-style)
   - `ScopeId` - unique identifier per expansion
   - `ScopeSet` - set of scopes on each identifier
   - `IdentifierData` - name + scopes
   - Flip-scope algorithm for hygiene

2. **Pattern matching infrastructure**
   - `Pattern` enum with PVREF encoding
   - `Matcher` with Gauche-style optimization
   - `MatchEnv` tree structure

3. **Template expansion**
   - `Template` enum
   - `Expander` with scope marking
   - Ellipsis iteration

4. **Value::Macro**
   - Stores compiled macros
   - Integrated with desugarer

### What We Need

1. **Syntax objects** - First-class syntax with source info
2. **syntax-case form** - Pattern matching with fenders
3. **Syntax utilities** - `datum->syntax`, `syntax->datum`, etc.
4. **Quasisyntax** - Template construction

---

## Syntax Objects

### Design

A syntax object wraps a datum with lexical context and source location:

```rust
/// A syntax object - datum with lexical context
#[derive(Debug, Clone)]
pub struct SyntaxObject {
    /// The wrapped datum
    pub datum: Value,

    /// Lexical context (scope set)
    pub context: ScopeSet,

    /// Source location (optional)
    pub source: Option<SourceLocation>,
}

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: Option<Rc<str>>,
    pub line: u32,
    pub column: u32,
    pub span: Option<(u32, u32)>,  // start, end offset
}
```

### Value Integration

Add to the Value enum:

```rust
pub enum Value {
    // ... existing variants ...

    /// Syntax object (first-class syntax)
    Syntax(Box<SyntaxObject>),
}
```

### Wrapped vs Unwrapped

Syntax objects can wrap:
- **Atoms**: symbols, numbers, strings, etc.
- **Pairs**: each element is itself a syntax object
- **Vectors**: each element is itself a syntax object

```scheme
;; Creating syntax
#'(if test then else)
;; Produces:
;; Syntax(Pair(
;;   Syntax(Symbol("if")),
;;   Syntax(Pair(
;;     Syntax(Symbol("test")),
;;     ...))))
```

### Identifier Syntax Objects

When the datum is an identifier, the syntax object carries binding information:

```rust
impl SyntaxObject {
    /// Check if this is an identifier
    pub fn is_identifier(&self) -> bool {
        matches!(self.datum, Value::Symbol(_) | Value::Identifier(_))
    }

    /// Get the identifier name
    pub fn identifier_name(&self) -> Option<Rc<str>> {
        match &self.datum {
            Value::Symbol(s) => Some(s.clone()),
            Value::Identifier(id) => Some(id.name.clone()),
            _ => None,
        }
    }

    /// Get the combined scopes (context + datum scopes)
    pub fn scopes(&self) -> ScopeSet {
        match &self.datum {
            Value::Identifier(id) => self.context.union(&id.scopes),
            _ => self.context.clone(),
        }
    }
}
```

---

## Core Forms

### syntax (quote-syntax)

Creates a syntax object from a template:

```scheme
(syntax datum)    ; or #'datum
```

**Semantics:**
- Wraps datum in syntax object
- Captures lexical context at definition site
- Preserves source location from input

**Implementation:**

```rust
// In special_forms/
fn eval_syntax(form: &Value, env: &Rc<Environment>) -> Result<Value> {
    let datum = get_arg(form, 0)?;

    // Get current scope set from environment
    let context = get_current_scopes(env);

    // Get source location if available
    let source = get_source_location(datum);

    Ok(Value::Syntax(Box::new(SyntaxObject {
        datum: datum.clone(),
        context,
        source,
    })))
}
```

### syntax-case

Pattern matching on syntax objects:

```scheme
(syntax-case stx (literal ...)
  (pattern fender template)
  (pattern template)
  ...)
```

**Semantics:**
- `stx` - syntax object to match
- `literal ...` - literal identifiers (like syntax-rules)
- `pattern` - pattern to match (like syntax-rules)
- `fender` - optional guard expression (must return true)
- `template` - expression to evaluate if match succeeds

**Key difference from syntax-rules:** The template is an *expression* that is *evaluated*, not a template that is *expanded*.

**Implementation:**

```rust
fn eval_syntax_case(
    stx_expr: &Value,
    literals: &[Rc<str>],
    clauses: &[(Pattern, Option<Value>, Value)],
    env: &Rc<Environment>,
) -> Result<Value> {
    let stx = eval(stx_expr, env)?;

    for (pattern, fender, body) in clauses {
        // Try to match pattern against stx
        if let Ok(match_env) = match_syntax(pattern, &stx, literals) {
            // Bind pattern variables in environment
            let clause_env = extend_with_matches(env, &match_env);

            // Check fender if present
            if let Some(fender_expr) = fender {
                let fender_result = eval(fender_expr, &clause_env)?;
                if !is_true(&fender_result) {
                    continue;  // Fender failed, try next clause
                }
            }

            // Evaluate body
            return eval(body, &clause_env);
        }
    }

    Err(Error::NoMatchingSyntaxCase)
}
```

### with-syntax

Bind pattern variables for use in templates:

```scheme
(with-syntax ((pattern stx) ...)
  body ...)
```

**Example:**
```scheme
(with-syntax (((name val) #'(x 42)))
  #'(define name val))
;; => #'(define x 42)
```

**Implementation:** Syntactic sugar over `syntax-case`:

```scheme
(define-syntax with-syntax
  (syntax-rules ()
    ((_ ((pat expr) ...) body ...)
     (syntax-case (list expr ...) ()
       ((pat ...) (begin body ...))))))
```

---

## Hygiene Utilities

### identifier?

Check if syntax object is an identifier:

```scheme
(identifier? stx) → boolean
```

```rust
fn identifier_p(stx: &Value) -> Value {
    match stx {
        Value::Syntax(s) => Value::Boolean(s.is_identifier()),
        _ => Value::Boolean(false),
    }
}
```

### bound-identifier=?

Check if two identifiers have the same binding:

```scheme
(bound-identifier=? id1 id2) → boolean
```

**Semantics:** Returns true if both would refer to the same binding at macro use site.

```rust
fn bound_identifier_eq(id1: &Value, id2: &Value) -> Value {
    match (id1, id2) {
        (Value::Syntax(s1), Value::Syntax(s2)) => {
            let scopes1 = s1.scopes();
            let scopes2 = s2.scopes();
            let name1 = s1.identifier_name();
            let name2 = s2.identifier_name();

            Value::Boolean(
                name1.is_some() &&
                name1 == name2 &&
                scopes1 == scopes2
            )
        }
        _ => Value::Boolean(false),
    }
}
```

### free-identifier=?

Check if two identifiers refer to the same free binding:

```scheme
(free-identifier=? id1 id2) → boolean
```

**Semantics:** Returns true if both would resolve to the same top-level binding.

```rust
fn free_identifier_eq(id1: &Value, id2: &Value, env: &Rc<Environment>) -> Value {
    // Resolve both identifiers to their bindings
    let binding1 = resolve_identifier(id1, env);
    let binding2 = resolve_identifier(id2, env);

    Value::Boolean(binding1 == binding2)
}
```

### syntax->datum

Strip syntax wrapper, returning plain datum:

```scheme
(syntax->datum stx) → datum
```

```rust
fn syntax_to_datum(stx: &Value) -> Value {
    match stx {
        Value::Syntax(s) => unwrap_syntax_recursive(&s.datum),
        _ => stx.clone(),  // Already a datum
    }
}

fn unwrap_syntax_recursive(v: &Value) -> Value {
    match v {
        Value::Syntax(s) => unwrap_syntax_recursive(&s.datum),
        Value::Pair(p) => {
            let (car, cdr) = &*p.borrow();
            Value::Pair(Rc::new(RefCell::new((
                unwrap_syntax_recursive(car),
                unwrap_syntax_recursive(cdr),
            ))))
        }
        // ... vectors, etc.
        _ => v.clone(),
    }
}
```

### datum->syntax

Create syntax object with given context:

```scheme
(datum->syntax template-id datum) → syntax
```

**Semantics:** Creates syntax object from datum, copying lexical context from template-id.

```rust
fn datum_to_syntax(template_id: &Value, datum: &Value) -> Result<Value> {
    let context = match template_id {
        Value::Syntax(s) => s.scopes(),
        _ => return Err(Error::ExpectedIdentifier),
    };

    Ok(wrap_syntax_recursive(datum, &context))
}

fn wrap_syntax_recursive(datum: &Value, context: &ScopeSet) -> Value {
    Value::Syntax(Box::new(SyntaxObject {
        datum: match datum {
            Value::Pair(p) => {
                let (car, cdr) = &*p.borrow();
                Value::Pair(Rc::new(RefCell::new((
                    wrap_syntax_recursive(car, context),
                    wrap_syntax_recursive(cdr, context),
                ))))
            }
            // ... vectors, etc.
            _ => datum.clone(),
        },
        context: context.clone(),
        source: None,
    }))
}
```

### generate-temporaries

Create fresh identifiers:

```scheme
(generate-temporaries stx-list) → list of identifiers
```

```rust
fn generate_temporaries(stx_list: &Value) -> Result<Value> {
    let items = list_to_vec(stx_list)?;
    let mut result = Vec::new();

    for _ in &items {
        let name: Rc<str> = format!("g{}", fresh_id()).into();
        let scope = ScopeId::fresh();

        result.push(Value::Syntax(Box::new(SyntaxObject {
            datum: Value::Identifier(Box::new(IdentifierData {
                name,
                scopes: ScopeSet::new().with_scope(scope),
            })),
            context: ScopeSet::new(),
            source: None,
        })));
    }

    Ok(vec_to_list(result))
}
```

---

## Quasisyntax

Quasisyntax provides template-based syntax construction:

```scheme
#`(if #,test #,@body)   ; quasisyntax with unsyntax and unsyntax-splicing
```

### Reader Syntax

| Syntax | Expansion |
|--------|-----------|
| `#'datum` | `(syntax datum)` |
| `#`datum` | `(quasisyntax datum)` |
| `#,expr` | `(unsyntax expr)` |
| `#,@expr` | `(unsyntax-splicing expr)` |

### Implementation

Quasisyntax is similar to quasiquote but works with syntax objects:

```rust
fn expand_quasisyntax(template: &Value, env: &Rc<Environment>) -> Result<Value> {
    match template {
        // Unsyntax: evaluate and insert
        Value::Pair(p) if is_unsyntax(&p.borrow().0) => {
            let expr = &p.borrow().1;
            eval(car(expr)?, env)
        }

        // Unsyntax-splicing: evaluate and splice
        Value::Pair(p) if is_unsyntax_splicing(&p.borrow().0) => {
            // Similar but returns list to splice
        }

        // Pairs: recurse
        Value::Pair(p) => {
            let (car, cdr) = &*p.borrow();
            let new_car = expand_quasisyntax(car, env)?;
            let new_cdr = expand_quasisyntax(cdr, env)?;
            // Wrap in syntax
            Ok(Value::Syntax(Box::new(SyntaxObject {
                datum: Value::Pair(Rc::new(RefCell::new((new_car, new_cdr)))),
                context: get_current_scopes(env),
                source: None,
            })))
        }

        // Atoms: wrap in syntax
        _ => Ok(Value::Syntax(Box::new(SyntaxObject {
            datum: template.clone(),
            context: get_current_scopes(env),
            source: None,
        }))),
    }
}
```

---

## Implementation Phases

### Phase 1: Syntax Objects (Foundation)

**Goal:** Add syntax object type and basic operations

**Tasks:**
1. [ ] Add `SyntaxObject` struct to patina-runtime
2. [ ] Add `Value::Syntax` variant
3. [ ] Implement `syntax` special form
4. [ ] Implement `syntax->datum`
5. [ ] Implement `identifier?`
6. [ ] Add reader support for `#'`
7. [ ] Basic tests

**Estimated effort:** 2-3 days

**Files to modify:**
- `crates/patina-runtime/src/value/mod.rs`
- `crates/patina-runtime/src/syntax_object.rs` (new)
- `crates/patina-tree-walker/src/eval/special_forms/`
- `crates/patina-frontend/src/lexer/`
- `crates/patina-frontend/src/parser/`

### Phase 2: datum->syntax and Context

**Goal:** Enable context transfer for hygiene

**Tasks:**
1. [ ] Implement `datum->syntax`
2. [ ] Implement `bound-identifier=?`
3. [ ] Implement `free-identifier=?`
4. [ ] Implement `generate-temporaries`
5. [ ] Tests for hygiene utilities

**Estimated effort:** 2-3 days

**Dependencies:** Phase 1

### Phase 3: syntax-case

**Goal:** Core procedural macro form

**Tasks:**
1. [ ] Implement `syntax-case` special form
2. [ ] Adapt pattern matcher for syntax objects
3. [ ] Implement pattern variable binding
4. [ ] Implement fenders (guards)
5. [ ] Comprehensive tests

**Estimated effort:** 3-5 days

**Dependencies:** Phase 2

### Phase 4: Quasisyntax

**Goal:** Template-based syntax construction

**Tasks:**
1. [ ] Add reader support for `#``, `#,`, `#,@`
2. [ ] Implement `quasisyntax` expansion
3. [ ] Implement `unsyntax` and `unsyntax-splicing`
4. [ ] Integration tests

**Estimated effort:** 2-3 days

**Dependencies:** Phase 3

### Phase 5: syntax-rules as Macro

**Goal:** Implement syntax-rules using syntax-case

**Tasks:**
1. [ ] Write syntax-rules as syntax-case macro
2. [ ] Verify backward compatibility
3. [ ] Performance comparison
4. [ ] Deprecate old implementation (optional)

**Estimated effort:** 1-2 days

**Dependencies:** Phase 4

### Phase 6: Polish and Documentation

**Goal:** Production-ready implementation

**Tasks:**
1. [ ] Error messages with source locations
2. [ ] Debug output integration
3. [ ] Documentation updates
4. [ ] Performance optimization
5. [ ] Edge case testing

**Estimated effort:** 2-3 days

**Dependencies:** Phase 5

---

## Integration Strategy

### Backward Compatibility

The existing `syntax-rules` implementation will continue to work. After Phase 5, it can optionally be replaced by the syntax-case based implementation.

### Desugarer Integration

The desugarer already handles macros. For syntax-case:

```rust
fn desugar_application(&mut self, list: &Value, env: &Rc<Environment>) -> Result<CoreExpr> {
    let operator = car(list)?;

    // Check for syntax-case macro
    if let Some(Value::SyntaxCaseMacro(transformer)) = self.lookup_macro(&operator, env) {
        // Apply transformer (it's a procedure)
        let stx = self.to_syntax_object(list);
        let expanded = apply_transformer(transformer, stx, env)?;
        return self.desugar(&expanded, env);
    }

    // ... existing logic
}
```

### Scope Set Reuse

The existing `ScopeSet` and flip-scope mechanism work unchanged:

1. Before expansion: flip macro_scope on input syntax object
2. Transformer runs, may call `datum->syntax` to transfer context
3. After expansion: flip macro_scope on output syntax object

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_syntax_object_creation() {
    assert_eval_to("#'x", ...);
    assert_eval_to("(identifier? #'x)", "#t");
    assert_eval_to("(identifier? #'42)", "#f");
}

#[test]
fn test_bound_identifier() {
    assert_eval_to("(bound-identifier=? #'x #'x)", "#t");
    assert_eval_to("(bound-identifier=? #'x #'y)", "#f");
}

#[test]
fn test_syntax_case_basic() {
    assert_program_eval_to(r#"
        (define-syntax my-if
          (lambda (stx)
            (syntax-case stx ()
              ((_ test then else)
               #'(if test then else)))))
        (my-if #t 1 2)
    "#, "1");
}
```

### Integration Tests

```scheme
;; Test hygiene
(let ((if +))
  (my-if #t 1 2))  ; Should still use real if

;; Test fenders
(define-syntax id-only
  (lambda (stx)
    (syntax-case stx ()
      ((_ x) (identifier? #'x) #'x)
      ((_ x) (syntax-error "expected identifier")))))

;; Test generate-temporaries
(define-syntax swap!
  (lambda (stx)
    (syntax-case stx ()
      ((_ a b)
       (with-syntax ((tmp (car (generate-temporaries #'(tmp)))))
         #'(let ((tmp a))
             (set! a b)
             (set! b tmp)))))))
```

### Compatibility Tests

Run existing syntax-rules tests with the new syntax-case based implementation to ensure backward compatibility.

---

## References

### Specifications

- **R7RS Large Macro Fascicle** - https://r7rs.org/large/fascicles/macro/1/
  - Comprehensive syntax-case specification for R7RS-large (announced October 2024)
  - Includes explicit-renaming macros as alternative
  - New features: syntax parameters, identifier properties
  - Procedural interface to syntax object destructuring
  - Target completion: December 2025 (Scheme's 50th birthday)

- **R6RS Chapter 12** - Syntax-case
  - Original R6RS syntax-case specification
  - https://www.r6rs.org/

- **SRFI-93** - R6RS Syntax-Case Macros
  - https://srfi.schemers.org/srfi-93/srfi-93.html

- **SRFI-72** - Hygienic macros (reference)

### Papers

- **"Syntactic Abstraction in Scheme"** - Dybvig, Hieb, Bruggeman (1993)
  - Original syntax-case design paper

- **"Binding as Sets of Scopes"** - Flatt (2016)
  - Current hygiene approach (already implemented in Patina)
  - https://www.cs.utah.edu/plt/scope-sets/

### Reference Implementations

#### Portable syntax-case (psyntax) - **Primary Reference**

The canonical reference implementation, originally developed for Chez Scheme:

- **Main page**: https://www.scheme.com/syntax-case/
- **R6RS libraries version**: https://scheme.com/syntax-case/r6rs-libraries/index.html
- **Mirror**: https://conservatory.scheme.org/psyntax/r6rs-libraries/

Key characteristics:
- **Single-file implementation** (`psyntax.ss`) - easy to study
- Ported to 12+ Scheme implementations (Chez, Gambit, Gauche, Chicken, MIT, Larceny, etc.)
- `psyntax.pp` is pre-expanded version for bootstrapping
- Documented in "The Scheme Programming Language" Chapter 8

**Why study psyntax:**
- Shows the minimal core needed for syntax-case
- Portable design reveals essential algorithm without implementation-specific details
- Well-tested across many implementations

#### Chez Scheme - **Gold Standard**

The original and most mature syntax-case implementation:

- **User's Guide**: https://cisco.github.io/ChezScheme/csug9.5/intro.html
- Created by the syntax-case paper authors (Dybvig, Hieb, Bruggeman)
- Reference for edge cases and optimization strategies
- Source: https://github.com/cisco/ChezScheme

#### Racket - **Modern Scope-Set Implementation**

Uses the same "Binding as Sets of Scopes" approach as Patina:

- Expander source: `racket/src/expander/`
- Most relevant for Patina since we share the hygiene mechanism
- https://github.com/racket/racket

#### Chibi-scheme - **Alternative Approach**

Uses explicit renaming (ER) macros instead of syntax-case:

- **ER macro discussion**: https://github.com/ashinn/chibi-scheme/issues/114
- **datum->syntax PR**: https://github.com/ashinn/chibi-scheme/pull/496
- syntax-case can be implemented as a macro over ER transformers
- Simpler but less powerful than full syntax-case

ER macro example:
```scheme
(define-syntax unless
  (er-macro-transformer
    (lambda (expr rename compare)
      `(,(rename 'if) (,(rename 'not) ,(cadr expr))
        ,(caddr expr)))))
```

### Additional Resources

- **Macro systems in Scheme** - https://terbium.io/2020/05/macros-scheme/
  - Comprehensive overview of different macro systems

- **Explicit renaming tutorial** - https://wiki.call-cc.org/explicit-renaming-macros
  - CHICKEN Scheme wiki tutorial on ER macros

- **Scheme surveys - Syntax definitions** - https://docs.scheme.org/surveys/syntax-definitions/
  - Survey of macro support across Scheme implementations

### Recommended Study Order

1. **R7RS Large Macro Fascicle** - Target specification
2. **psyntax.ss** - Portable implementation to understand core algorithm
3. **Chez Scheme** - Edge cases and optimizations
4. **Racket expander** - Scope-set based implementation (closest to Patina)

### Patina Documentation

- `docs/MACRO_SYSTEM.md` - Current macro system architecture
- `internal/MACRO_SYSTEM_KNOWN_LIMITATIONS.md` - Known edge cases
