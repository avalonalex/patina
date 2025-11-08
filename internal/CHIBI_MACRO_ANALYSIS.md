# Chibi-Scheme Macro Implementation Analysis

This document analyzes chibi-scheme's macro implementation to guide Patina's implementation of hygienic macros and `syntax-rules`.

**Analysis Date:** 2025-11-08
**Chibi-Scheme Version:** Reference implementation at `~/Project/reference/chibi-scheme`

## Table of Contents

1. [Overview](#overview)
2. [Data Structures](#data-structures)
3. [Macro Expansion Flow](#macro-expansion-flow)
4. [Pattern Matching Algorithm](#pattern-matching-algorithm)
5. [Template Expansion Algorithm](#template-expansion-algorithm)
6. [Hygiene Strategy](#hygiene-strategy)
7. [Implementation Architecture](#implementation-architecture)
8. [Mapping to Rust Implementation](#mapping-to-rust-implementation)

---

## Overview

Chibi-scheme implements `syntax-rules` using a **meta-circular approach**: the `syntax-rules` macro is itself defined as a macro using the lower-level `er-macro-transformer` (explicit renaming) system. This approach has several advantages:

- **Bootstrapping**: `syntax-rules` is implemented in Scheme, not C
- **Hygiene**: Built on explicit renaming + syntactic closures
- **Clarity**: The pattern matching and template expansion logic is readable
- **Flexibility**: Easy to extend or debug

### Key Files

- **C Implementation**: `~/Project/reference/chibi-scheme/eval.c` (lines 396-1060)
  - Macro data structures
  - Macro expansion hooks (`analyze_macro_once`)
  - `define-syntax`, `let-syntax`, `letrec-syntax` handling

- **Scheme Implementation**: `~/Project/reference/chibi-scheme/lib/init-7.scm` (lines 110-1095)
  - `er-macro-transformer` (lines 146-151)
  - `syntax-rules-transformer` (lines 850-1090)
  - Other macros (`cond`, `and`, `or`, `quasiquote`, etc.) built using `er-macro-transformer`

---

## Data Structures

### 1. Macro Type (C)

Located in `eval.c` lines 396-400 and `include/chibi/sexp.h` lines 1310-1313:

```c
// Macro allocation
static sexp sexp_make_macro (sexp ctx, sexp p, sexp e) {
  sexp mac = sexp_alloc_type(ctx, macro, SEXP_MACRO);
  sexp_macro_env(mac) = e;     // Macro definition environment
  sexp_macro_proc(mac) = p;    // Transformer procedure
  sexp_macro_aux(mac) = SEXP_FALSE;
  return mac;
}

// Field accessors (sexp.h)
#define sexp_macro_proc(x)   (sexp_field(x, macro, SEXP_MACRO, proc))
#define sexp_macro_env(x)    (sexp_field(x, macro, SEXP_MACRO, env))
#define sexp_macro_source(x) (sexp_field(x, macro, SEXP_MACRO, source))
#define sexp_macro_aux(x)    (sexp_field(x, macro, SEXP_MACRO, aux))
```

**Fields:**
- `proc`: The transformer procedure (a closure that takes `(expr use-env mac-env)`)
- `env`: The environment where the macro was defined (for hygiene)
- `source`: Source location information (for error reporting)
- `aux`: Auxiliary data (unused in basic `syntax-rules`)

### 2. Syntactic Closure Type (C)

Located in `eval.c` lines 407-418 and `include/chibi/sexp.h` lines 1315-1318:

```c
// Syntactic closure creation (eval.c:407-418)
res = sexp_alloc_type(ctx, synclo, SEXP_SYNCLO);
if (SEXP_USE_FLAT_SYNTACTIC_CLOSURES && sexp_synclop(expr)) {
  sexp_synclo_env(res) = sexp_synclo_env(expr);
  sexp_synclo_free_vars(res) = sexp_synclo_free_vars(expr);
  sexp_synclo_expr(res) = sexp_synclo_expr(expr);
} else {
  sexp_synclo_env(res) = env;
  sexp_synclo_free_vars(res) = SEXP_NULL;  // or free vars list
  sexp_synclo_expr(res) = expr;
}

// Field accessors (sexp.h)
#define sexp_synclo_env(x)        (sexp_field(x, synclo, SEXP_SYNCLO, env))
#define sexp_synclo_free_vars(x)  (sexp_field(x, synclo, SEXP_SYNCLO, free_vars))
#define sexp_synclo_expr(x)       (sexp_field(x, synclo, SEXP_SYNCLO, expr))
#define sexp_synclo_rename(x)     (sexp_field(x, synclo, SEXP_SYNCLO, rename))
```

**Fields:**
- `env`: The environment where this identifier should be looked up
- `free_vars`: List of free variables (for optimization)
- `expr`: The actual identifier/expression
- `rename`: The rename function (for explicit renaming)

### 3. Pattern Variables (Scheme)

In `syntax-rules-transformer`, pattern variables are tracked as association lists:

```scheme
;; Variable representation: (identifier . dimension)
;; dimension = ellipsis depth (0 for non-repeating vars)
;;
;; Example for pattern: (a (b c ...) ...)
;; vars = ((a . 0)    ; non-repeating
;;         (b . 1)    ; inside one ellipsis
;;         (c . 2))   ; inside two ellipses
```

This is computed by `all-vars` (lines 1002-1012) and used throughout pattern matching and template expansion.

---

## Macro Expansion Flow

### High-Level Flow

1. **Parsing**: Macro invocation is parsed as a normal S-expression
2. **Analysis**: During analysis phase, macro use is detected
3. **Expansion**: `analyze_macro_once` is called (eval.c:770-788)
4. **Recursion**: Expanded code is re-analyzed (goto loop)

### Detailed Flow (eval.c)

```c
// 1. During analysis, check if car is a macro (eval.c:1158-1160)
if (sexp_macrop(op)) {
  x = analyze_macro_once(ctx, x, op, depth);
  goto loop;  // Re-analyze expanded code
}

// 2. analyze_macro_once (eval.c:770-788)
static sexp analyze_macro_once (sexp ctx, sexp x, sexp op, int depth) {
  sexp res;
  sexp_gc_var1(tmp);
  sexp_gc_preserve1(ctx, tmp);

  // Build arguments: (expr use-env mac-env)
  tmp = sexp_cons(ctx, sexp_macro_env(op), SEXP_NULL);      // mac-env
  tmp = sexp_cons(ctx, sexp_context_env(ctx), tmp);         // use-env
  tmp = sexp_cons(ctx, x, tmp);                             // expr

  // Create child context and apply transformer
  res = sexp_exceptionp(tmp) ? tmp : sexp_make_child_context(ctx, sexp_context_lambda(ctx));
  if (!sexp_exceptionp(res) && !sexp_exceptionp(sexp_context_exception(ctx)))
    res = sexp_apply(res, sexp_macro_proc(op), tmp);  // Call transformer!

  // Preserve source location
  if (sexp_pairp(sexp_car(tmp)) && sexp_pair_source(sexp_car(tmp))) {
    if (sexp_pairp(res))
      sexp_pair_source(res) = sexp_pair_source(sexp_car(tmp));
    else if (sexp_exceptionp(res) && sexp_not(sexp_exception_source(x)))
      sexp_exception_source(res) = sexp_pair_source(sexp_car(tmp));
  }

  sexp_gc_release1(ctx);
  return res;
}
```

### `define-syntax` Handling (eval.c:1044-1052)

```c
static sexp analyze_define_syntax (sexp ctx, sexp x) {
  sexp res;
  sexp_gc_var1(tmp);
  sexp_gc_preserve1(ctx, tmp);
  tmp = sexp_list1(ctx, sexp_cdr(x));
  res = sexp_exceptionp(tmp) ? tmp : analyze_bind_syntax(tmp, ctx, ctx, 0);
  sexp_gc_release1(ctx);
  return res;
}
```

This evaluates the transformer expression and binds it in the environment as a macro.

---

## Pattern Matching Algorithm

The pattern matcher is implemented in `expand-pattern` (init-7.scm:880-982). It's a code generator that produces Scheme code to perform the match at runtime.

### Core Idea

`expand-pattern` takes a pattern and template, and generates code that:
1. **Matches** the input against the pattern
2. **Binds** pattern variables
3. **Calls** the template expander with the bindings

### Algorithm Structure

```scheme
(define (expand-pattern pat tmpl)
  (let lp ((p (if full-match? pat (cdr pat)))     ; Pattern to match
           (x (if full-match? _expr (list _cdr _expr)))  ; Expression to match against
           (dim 0)                                 ; Current ellipsis depth
           (vars '())                              ; Accumulated variable bindings
           (k (lambda (vars)                       ; Continuation: what to do on success
                (list _cons (expand-template tmpl vars) #f))))
    (let ((v (next-symbol "v.")))
      (list _let (list (list v x))  ; Bind current expression to fresh variable
            (cond
              ;; Case 1: Identifier
              ((identifier? p) ...)

              ;; Case 2: Ellipsis pattern (p1 ... tail)
              ((ellipsis? p) ...)

              ;; Case 3: Pair pattern (p1 . p2)
              ((pair? p) ...)

              ;; Case 4: Vector pattern #(p1 p2 ...)
              ((vector? p) ...)

              ;; Case 5: Null
              ((null? p) ...)

              ;; Case 6: Literal (number, string, etc.)
              (else ...))))))
```

### Pattern Matching Cases

#### Case 1: Identifier Pattern

```scheme
((identifier? p)
 (cond
  ;; Ellipsis marker (error)
  ((ellipsis-mark? p)
   (error "bad ellipsis" p))

  ;; Literal identifier (compare hygienic identifiers)
  ((memq p lits)
   (list _and
         (list _compare v (list _rename (list _quote p)))
         (k vars)))

  ;; Wildcard _ (matches anything, don't bind)
  ((compare p _underscore)
   (k vars))

  ;; Pattern variable (bind it)
  (else
   (list _let (list (list p v))
         (k (cons (cons p dim) vars))))))
```

**Key points:**
- Literals are compared using the `compare` function (hygienic comparison)
- Pattern variables are bound with `let`
- Dimension is tracked for ellipsis handling

#### Case 2: Ellipsis Pattern `(p1 ... tail)`

This is the most complex case. The generated code must:
1. Check that the input is a list
2. Iterate over the list
3. Match each element against `p1`
4. Accumulate bindings for pattern variables
5. Handle tail patterns after ellipsis

```scheme
((ellipsis? p)
 (cond
  ;; Multiple ellipses: (p1 ... p2 ... tail)
  ((not (null? (cdr (cdr p))))
   (let ((len (length* (cdr (cdr p))))
         (_lp (next-symbol "lp.")))
     ;; Generated code checks: length(v) >= len
     ;; Then splits: first part for ellipsis, last part for tail
     `(,_let ((,_len (,_length ,v)))
        (,_and (,_>= ,_len ,len)
               (,_let ,_lp ((,_ls ,v)
                            (,_i (,_- ,_len ,len))
                            (,_res (,_quote ())))
                 (,_if (,_>= 0 ,_i)
                     ,(lp `(,(cddr p) (,(car p) ,(car (cdr p))))
                          `(,_cons ,_ls (,_cons (,_reverse ,_res) (,_quote ())))
                          dim vars k)
                     (,_lp (,_cdr ,_ls)
                           (,_- ,_i 1)
                           (,_cons3 (,_car ,_ls) ,_res ,_ls))))))))

  ;; Simple variable ellipsis: (var ...)
  ((identifier? (car p))
   (list _and (list _list? v)
         (list _let (list (list (car p) v))
               (k (cons (cons (car p) (+ 1 dim)) vars)))))

  ;; Complex pattern ellipsis: ((p1 p2) ...)
  (else
   ;; Generate nested loop that:
   ;; - Iterates over list
   ;; - Matches each element against (car p)
   ;; - Accumulates bindings in reversed lists
   ;; - Reverses at end
   (let* ((w (next-symbol "w."))
          (_lp (next-symbol "lp."))
          (new-vars (all-vars (car p) (+ dim 1)))
          (ls-vars (map ... new-vars)))
     ...))))
```

**Key insight:** Ellipsis variables are bound to **lists of values**, not single values. The dimension tracks how many ellipses deep we are.

#### Case 3: Pair Pattern `(p1 . p2)`

```scheme
((pair? p)
 (list _and (list _pair? v)
       (lp (car p) (list _car v) dim vars
           (lambda (vars)
             (lp (cdr p) (list _cdr v) dim vars k)))))
```

**Recursive descent:** Match car, then match cdr with accumulated vars.

#### Case 4: Vector Pattern

```scheme
((vector? p)
 (list _and (list _vector? v)
       (lp (vector->list p) (list _vector->list v) dim vars k)))
```

Converts to list matching.

#### Case 5: Null and Literals

```scheme
((null? p) (list _and (list _null? v) (k vars)))
(else (list _and (list _equal? v p) (k vars)))
```

Simple checks.

### Helper Functions

#### `all-vars` (lines 1002-1012)

Collects all pattern variables with their dimensions:

```scheme
(define (all-vars x dim)
  (let lp ((x x) (dim dim) (vars '()))
    (cond
     ((identifier? x)
      (if (or (memq x lits) (compare x _underscore))
          vars
          (cons (cons x dim) vars)))
     ((ellipsis? x)
      (lp (car x) (+ dim 1) (lp (cddr x) dim vars)))
     ((pair? x)
      (lp (car x) dim (lp (cdr x) dim vars)))
     ((vector? x)
      (lp (vector->list x) dim vars))
     (else vars))))
```

Returns: `((var1 . dim1) (var2 . dim2) ...)`

#### `ellipsis?` (lines 992-993)

```scheme
(define (ellipsis? x)
  (and (pair? x) (pair? (cdr x)) (ellipsis-mark? (cadr x))))
```

Checks if pattern is `(p ... tail)`.

---

## Template Expansion Algorithm

Template expansion is implemented in `expand-template` (init-7.scm:1025-1070). It generates code that constructs the output using the pattern variable bindings.

### Core Idea

Given a template and the `vars` alist (from pattern matching), generate code that:
1. **Substitutes** pattern variables with their bindings
2. **Repeats** ellipsis templates for each element in ellipsis variables
3. **Renames** introduced identifiers for hygiene

### Algorithm Structure

```scheme
(define (expand-template tmpl vars)
  (let lp ((t tmpl)         ; Template expression
           (dim 0)          ; Current ellipsis depth
           (ell-esc #f))    ; Ellipsis escape flag
    (cond
     ;; Case 1: Identifier
     ((identifier? t) ...)

     ;; Case 2: Pair (recursive)
     ((pair? t)
      (cond
       ;; Ellipsis escape: (... template)
       ((and (ellipsis-escape? t) (not ell-esc)) ...)

       ;; Ellipsis: (template ... tail)
       ((and (ellipsis? t) (not ell-esc)) ...)

       ;; Regular pair
       (else ...)))

     ;; Case 3: Vector
     ((vector? t) ...)

     ;; Case 4: Null
     ((null? t) ...)

     ;; Case 5: Literal
     (else t))))
```

### Template Expansion Cases

#### Case 1: Identifier in Template

```scheme
((identifier? t)
 (cond
  ;; Pattern variable: check if it's bound
  ((find (lambda (v) (eq? t (car v))) vars)
   => (lambda (cell)
        (if (<= (cdr cell) dim)
            t  ; In scope, use directly
            (error "too few ...'s"))))

  ;; Introduced identifier: rename for hygiene
  (else
   (list _rename (list _quote t)))))
```

**Key insight:** Pattern variables are substituted directly, but **introduced identifiers** go through the `rename` function for hygiene.

#### Case 2: Ellipsis in Template `(template ... tail)`

```scheme
((and (ellipsis? t) (not ell-esc))
 (let* ((depth (ellipsis-depth t))
        (ell-dim (+ dim depth))
        (ell-vars (free-vars (car t) vars ell-dim)))
   (cond
    ;; No variables at ellipsis depth (error)
    ((null? ell-vars)
     (error "too many ...'s"))

    ;; Simple variable ellipsis: (var ...)
    ((and (null? (cdr (cdr t))) (identifier? (car t)))
     (lp (car t) ell-dim ell-esc))

    ;; Complex ellipsis: (template ...)
    (else
     (let* ((once (lp (car t) ell-dim ell-esc))  ; Expand once
            (nest (if (and (null? (cdr ell-vars))
                           (identifier? once)
                           (eq? once (car vars)))
                      once  ; Shortcut
                      (cons _map
                            (cons (list _lambda ell-vars once)
                                  ell-vars))))
            (many (do ((d depth (- d 1))
                       (many nest
                             (list _apply _append many)))
                      ((= d 1) many))))
       (if (null? (ellipsis-tail t))
           many  ; Shortcut
           (list _append many (lp (ellipsis-tail t) dim ell-esc))))))))
```

**Key steps:**
1. Expand the template once: `(lp (car t) ell-dim ell-esc)`
2. Wrap in `map` over ellipsis variables: `(map (lambda (var1 var2 ...) once) var1 var2 ...)`
3. Flatten nested lists: `(apply append ...)` for nested ellipses
4. Append tail: `(append many tail)`

**Example:**

Pattern: `(a b ...)`
Template: `((b a) ...)`
Bindings: `a = 1, b = '(2 3 4)`

Generated code:
```scheme
(map (lambda (b) (cons b (cons a '())))
     b)
; => ((2 1) (3 1) (4 1))
```

#### Case 3: Regular Pair

```scheme
(else
 (list _cons3 (lp (car t) dim ell-esc)
              (lp (cdr t) dim ell-esc)
              (list _quote t)))
```

`cons3` is `cons-source` which preserves source location.

#### Case 4: Vector

```scheme
((vector? t)
 (list _list->vector (lp (vector->list t) dim ell-esc)))
```

#### Case 5: Literals

```scheme
((null? t) (list _quote '()))
(else t)  ; Numbers, strings, etc.
```

### Helper Functions

#### `free-vars` (lines 1013-1024)

Finds pattern variables used in a template that are at the specified dimension:

```scheme
(define (free-vars x vars dim)
  (let lp ((x x) (free '()))
    (cond
     ((identifier? x)
      (if (and (not (memq x free))
               (cond ((assq x vars) => (lambda (cell) (>= (cdr cell) dim)))
                     (else #f)))
          (cons x free)
          free))
     ((pair? x) (lp (car x) (lp (cdr x) free)))
     ((vector? x) (lp (vector->list x) free))
     (else free))))
```

Returns list of identifiers that need to be iterated over for ellipsis expansion.

#### `ellipsis-depth` (lines 994-997)

```scheme
(define (ellipsis-depth x)
  (if (ellipsis? x)
      (+ 1 (ellipsis-depth (cdr x)))
      0))
```

Counts nested ellipses: `(a ... ...)` has depth 2.

---

## Hygiene Strategy

Chibi-scheme uses a **two-layer hygiene system**:

1. **Explicit Renaming** (`er-macro-transformer`)
2. **Syntactic Closures** (underlying mechanism)

### Explicit Renaming (init-7.scm:146-151)

```scheme
(define er-macro-transformer
  (lambda (f)
    (lambda (expr use-env mac-env)
      (f expr
         (make-renamer mac-env)      ; rename function
         (lambda (x y) (identifier=? use-env x use-env y))))))  ; compare function
```

**Interface:** Transformer receives:
- `expr`: The macro use form
- `rename`: Function to create fresh identifiers in `mac-env`
- `compare`: Function to compare identifiers hygienic ally

**Usage in `syntax-rules`:** All introduced identifiers are renamed:

```scheme
(_er-macro-transformer (rename 'er-macro-transformer))
(_lambda (rename 'lambda))
(_let (rename 'let))
(_if (rename 'if'))
...
```

This ensures introduced identifiers refer to bindings from the **macro definition environment**, not the use site.

### Syntactic Closures (init-7.scm:113-134)

```scheme
(define close-syntax
  (lambda (form env)
    (make-syntactic-closure env '() form)))

(define make-renamer
  (lambda (mac-env)
    (define rename
      ((lambda (renames)
         (lambda (identifier)
           ((lambda (cell)
              (if cell
                  (cdr cell)  ; Already renamed
                  ((lambda (name)
                     (set! renames (cons (cons identifier name) renames))
                     name)
                   ((lambda (id)
                      (syntactic-closure-set-rename! id rename)
                      id)
                    (close-syntax identifier mac-env)))))
            (assq identifier renames))))
       '()))
    rename))
```

**Mechanism:**
1. `make-renamer` creates a rename function with private state (`renames` alist)
2. First time identifier is renamed: create syntactic closure in `mac-env`
3. Subsequent renames: return same closure (ensures consistent renaming)
4. Syntactic closure tracks: `(env free-vars expr rename)`

### Identifier Lookup with Syntactic Closures (eval.c:113-116)

```c
while (!cell && key && sexp_synclop(key)) {
  if (!sexp_pairp(ls) && sexp_not(sexp_memq(ctx, sexp_synclo_expr(key), sexp_synclo_free_vars(key))))
    env = sexp_synclo_env(key);  // Use closure's environment
  key = sexp_synclo_expr(key);
}
```

**Effect:** Identifier lookup uses the environment captured in the syntactic closure, not the lexical environment at use site.

### Hygiene Example

```scheme
(define-syntax swap!
  (syntax-rules ()
    ((swap! a b)
     (let ((tmp a))
       (set! a b)
       (set! b tmp)))))

(let ((tmp 1)
      (other 2))
  (swap! tmp other)
  tmp)  ; => 2, not 1
```

Without hygiene, `tmp` would conflict. With hygiene:
- `tmp` in `let ((tmp a))` is renamed to, say, `tmp#123`
- Pattern variables `a` and `b` are substituted with `tmp` and `other` (from use site)
- No conflict!

---

## Implementation Architecture

### Layered Design

```
┌─────────────────────────────────────────┐
│         syntax-rules macro              │  ← User-facing
├─────────────────────────────────────────┤
│      er-macro-transformer               │  ← Explicit renaming layer
├─────────────────────────────────────────┤
│      Syntactic closures                 │  ← Hygiene mechanism
├─────────────────────────────────────────┤
│      Macro expansion hooks              │  ← C runtime
│      (analyze_macro_once, etc.)         │
└─────────────────────────────────────────┘
```

### Bootstrap Process

1. **C runtime**: Provides primitives
   - `make-syntactic-closure`
   - `identifier=?`
   - `strip-syntactic-closures`
   - Macro type and expansion hooks

2. **init-7.scm**: Implements transformers
   - `er-macro-transformer` (lines 146-151)
   - `make-renamer` (lines 117-134)
   - `syntax-rules-transformer` (lines 850-1090)

3. **`syntax-rules` macro**: Uses `er-macro-transformer`
   - Defined using `define-syntax` (lines 1092-1095)
   - Expands to `(er-macro-transformer (lambda (expr rename compare) ...))`

4. **User macros**: Use `syntax-rules`
   - Clean, declarative interface
   - Full hygiene automatically

### Compilation vs. Interpretation

Chibi-scheme uses **compiled pattern matching**:
- `syntax-rules-transformer` generates Scheme code
- This code is evaluated once when macro is defined
- Result is a transformer closure
- Each macro use applies this closure (interpreted)

**Advantage:** Pattern matching code is optimized by the Scheme compiler/interpreter.

---

## Mapping to Rust Implementation

### Phase 1: Core Data Structures

```rust
// src/value/mod.rs
pub enum Value {
    // ... existing variants ...

    /// Macro transformer
    Macro(Rc<Macro>),

    /// Syntactic closure (for hygiene)
    SyntacticClosure(Rc<SyntacticClosure>),
}

pub struct Macro {
    /// Transformer procedure: takes (expr use-env mac-env)
    pub transformer: Box<dyn Fn(&Value, &Environment, &Environment) -> Result<Value, EvalError>>,

    /// Environment where macro was defined
    pub definition_env: Rc<Environment>,

    /// Optional source location
    pub source: Option<SourceLocation>,
}

pub struct SyntacticClosure {
    /// The wrapped expression/identifier
    pub expr: Value,

    /// Environment for identifier lookup
    pub env: Rc<Environment>,

    /// Free variables (if any)
    pub free_vars: Vec<String>,

    /// Optional rename function
    pub rename: Option<Rc<RenameFunction>>,
}
```

### Phase 2: Pattern Matching Engine

We have two implementation strategies:

#### Option A: Interpreted Pattern Matching (Like Chibi)

Implement `syntax-rules` as a Rust function that generates Scheme code:

```rust
// src/eval/macros/syntax_rules.rs

pub fn syntax_rules_transformer(
    rules: &[(Value, Value)],  // Vec of (pattern, template) pairs
    literals: &[String],
    ellipsis: Option<String>,
) -> Result<Value, EvalError> {
    // Generate Scheme code like chibi's syntax-rules-transformer
    // Return a lambda that does pattern matching
    let generated_code = generate_pattern_matcher(rules, literals, ellipsis)?;

    // Evaluate generated code to create transformer closure
    eval(&generated_code, &macro_env)
}
```

**Pros:**
- Matches chibi's architecture
- Can reuse Scheme evaluation engine
- Easier to debug (inspect generated code)

**Cons:**
- Requires Scheme code generation
- Runtime overhead of interpretation

#### Option B: Native Rust Pattern Matching (Direct)

Implement pattern matching directly in Rust:

```rust
// src/eval/macros/pattern.rs

pub enum Pattern {
    Identifier(String),
    Literal(Value),
    Pair(Box<Pattern>, Box<Pattern>),
    Vector(Vec<Pattern>),
    Ellipsis {
        pattern: Box<Pattern>,
        tail: Vec<Pattern>,
    },
    Wildcard,
}

pub struct PatternMatcher {
    pattern: Pattern,
    literals: HashSet<String>,
    ellipsis_symbol: String,
}

impl PatternMatcher {
    pub fn match_pattern(
        &self,
        expr: &Value,
        env: &Environment,
    ) -> Result<Bindings, MatchError> {
        let mut bindings = Bindings::new();
        self.match_internal(&self.pattern, expr, &mut bindings, 0)?;
        Ok(bindings)
    }

    fn match_internal(
        &self,
        pat: &Pattern,
        expr: &Value,
        bindings: &mut Bindings,
        depth: usize,
    ) -> Result<(), MatchError> {
        match pat {
            Pattern::Identifier(id) => {
                if self.literals.contains(id) {
                    // Compare hygienic ally
                    self.compare_identifiers(id, expr, env)?;
                } else {
                    // Bind pattern variable
                    bindings.insert(id.clone(), BindingValue::Single(expr.clone()), depth);
                }
                Ok(())
            }

            Pattern::Ellipsis { pattern, tail } => {
                // Extract list elements
                let elements = self.extract_list(expr)?;
                let tail_len = tail.len();

                if elements.len() < tail_len {
                    return Err(MatchError::TooFewElements);
                }

                // Match repeating part
                let repeat_elements = &elements[..elements.len() - tail_len];
                let mut lists = HashMap::new();

                for elem in repeat_elements {
                    let mut sub_bindings = Bindings::new();
                    self.match_internal(pattern, elem, &mut sub_bindings, depth + 1)?;

                    // Accumulate into lists
                    for (var, value) in sub_bindings.iter() {
                        lists.entry(var.clone())
                            .or_insert_with(Vec::new)
                            .push(value.clone());
                    }
                }

                // Bind as lists
                for (var, values) in lists {
                    bindings.insert(var, BindingValue::List(values), depth + 1);
                }

                // Match tail
                // ... (similar recursive matching)

                Ok(())
            }

            Pattern::Pair(car, cdr) => {
                let (car_val, cdr_val) = expr.as_pair()?;
                self.match_internal(car, car_val, bindings, depth)?;
                self.match_internal(cdr, cdr_val, bindings, depth)?;
                Ok(())
            }

            // ... other pattern types
        }
    }
}

pub enum BindingValue {
    Single(Value),
    List(Vec<Value>),
}

pub struct Bindings {
    vars: HashMap<String, (BindingValue, usize)>,  // (value, depth)
}
```

**Pros:**
- More performant (no Scheme interpretation)
- Type-safe pattern representation
- Easier to optimize

**Cons:**
- More Rust code to write
- Less direct correspondence to chibi

### Phase 3: Template Expansion

```rust
// src/eval/macros/template.rs

pub enum Template {
    Identifier(String),
    Literal(Value),
    Pair(Box<Template>, Box<Template>),
    Vector(Vec<Template>),
    Ellipsis {
        template: Box<Template>,
        tail: Vec<Template>,
    },
}

pub struct TemplateExpander {
    template: Template,
    ellipsis_symbol: String,
}

impl TemplateExpander {
    pub fn expand(
        &self,
        bindings: &Bindings,
        rename_fn: &RenameFunction,
    ) -> Result<Value, EvalError> {
        self.expand_internal(&self.template, bindings, rename_fn, 0)
    }

    fn expand_internal(
        &self,
        tmpl: &Template,
        bindings: &Bindings,
        rename_fn: &RenameFunction,
        depth: usize,
    ) -> Result<Value, EvalError> {
        match tmpl {
            Template::Identifier(id) => {
                if let Some((value, var_depth)) = bindings.get(id) {
                    if *var_depth <= depth {
                        // In scope, substitute directly
                        match value {
                            BindingValue::Single(v) => Ok(v.clone()),
                            BindingValue::List(_) => Err(EvalError::TooManyEllipses),
                        }
                    } else {
                        Err(EvalError::TooFewEllipses)
                    }
                } else {
                    // Introduced identifier, rename for hygiene
                    Ok(rename_fn(id))
                }
            }

            Template::Ellipsis { template, tail } => {
                // Find variables at current ellipsis depth
                let ell_vars = self.find_ellipsis_vars(template, bindings, depth + 1)?;

                if ell_vars.is_empty() {
                    return Err(EvalError::TooManyEllipses);
                }

                // Get length of first ellipsis variable (all must be same length)
                let list_len = match bindings.get(&ell_vars[0]).unwrap().0 {
                    BindingValue::List(ref v) => v.len(),
                    _ => return Err(EvalError::ExpectedList),
                };

                // Expand template once for each element
                let mut results = Vec::new();
                for i in 0..list_len {
                    // Create sub-bindings for this iteration
                    let mut sub_bindings = bindings.clone();
                    for var in &ell_vars {
                        let list = match bindings.get(var).unwrap().0 {
                            BindingValue::List(ref v) => v,
                            _ => unreachable!(),
                        };
                        sub_bindings.insert(
                            var.clone(),
                            BindingValue::Single(list[i].clone()),
                            depth,
                        );
                    }

                    results.push(self.expand_internal(template, &sub_bindings, rename_fn, depth + 1)?);
                }

                // Expand tail
                let tail_value = if tail.is_empty() {
                    Value::Null
                } else {
                    // ... expand tail templates
                };

                // Cons results into list
                Ok(Value::list_from_vec(results, tail_value))
            }

            Template::Pair(car, cdr) => {
                let car_val = self.expand_internal(car, bindings, rename_fn, depth)?;
                let cdr_val = self.expand_internal(cdr, bindings, rename_fn, depth)?;
                Ok(Value::cons(car_val, cdr_val))
            }

            Template::Literal(v) => Ok(v.clone()),

            // ... other cases
        }
    }

    fn find_ellipsis_vars(
        &self,
        tmpl: &Template,
        bindings: &Bindings,
        target_depth: usize,
    ) -> Result<Vec<String>, EvalError> {
        // Walk template, collect identifiers with depth >= target_depth
        // ...
    }
}
```

### Phase 4: Hygiene Implementation

```rust
// src/eval/macros/hygiene.rs

pub type RenameFunction = Box<dyn Fn(&str) -> Value>;

pub fn make_renamer(mac_env: Rc<Environment>) -> RenameFunction {
    let mut counter = 0;
    let mut renames: HashMap<String, Value> = HashMap::new();

    Box::new(move |identifier: &str| {
        if let Some(renamed) = renames.get(identifier) {
            renamed.clone()
        } else {
            counter += 1;
            let fresh_name = format!("{}#{}", identifier, counter);
            let closure = Value::SyntacticClosure(Rc::new(SyntacticClosure {
                expr: Value::symbol(fresh_name),
                env: mac_env.clone(),
                free_vars: vec![],
                rename: None,
            }));
            renames.insert(identifier.to_string(), closure.clone());
            closure
        }
    })
}

pub fn identifier_equal(
    id1: &Value,
    id2: &Value,
    env1: &Environment,
    env2: &Environment,
) -> bool {
    // Unwrap syntactic closures
    let (sym1, env1) = unwrap_identifier(id1, env1);
    let (sym2, env2) = unwrap_identifier(id2, env2);

    if sym1 != sym2 {
        return false;
    }

    // Compare bindings (same identifier = same binding?)
    let binding1 = env1.lookup(&sym1);
    let binding2 = env2.lookup(&sym2);

    match (binding1, binding2) {
        (Some(b1), Some(b2)) => Rc::ptr_eq(&b1, &b2),
        (None, None) => true,  // Both unbound
        _ => false,
    }
}

fn unwrap_identifier(value: &Value, env: &Environment) -> (String, Rc<Environment>) {
    match value {
        Value::SyntacticClosure(sc) => {
            unwrap_identifier(&sc.expr, &sc.env)
        }
        Value::Symbol(s) => (s.to_string(), env.clone()),
        _ => panic!("Not an identifier"),
    }
}
```

### Phase 5: Integration with Evaluator

```rust
// src/eval/special_forms.rs

pub fn eval_define_syntax(
    expr: &Value,
    env: &Rc<RefCell<Environment>>,
    evaluator: &Evaluator,
) -> Result<Value, EvalError> {
    // (define-syntax name transformer-spec)
    let (name, transformer_spec) = extract_define_syntax_parts(expr)?;

    // Evaluate transformer spec (often (syntax-rules ...))
    let transformer = evaluator.eval_in_env(transformer_spec, env)?;

    // Create macro value
    let macro_value = match transformer {
        Value::Lambda { .. } => {
            Value::Macro(Rc::new(Macro {
                transformer: /* wrap lambda */,
                definition_env: env.borrow().clone(),
                source: None,
            }))
        }
        _ => return Err(EvalError::ExpectedProcedure),
    };

    // Bind in environment
    env.borrow_mut().define(name, macro_value);

    Ok(Value::Unspecified)
}

pub fn eval_syntax_rules(
    expr: &Value,
    env: &Rc<RefCell<Environment>>,
) -> Result<Value, EvalError> {
    // (syntax-rules (literals ...) (pattern template) ...)
    let (literals, rules) = parse_syntax_rules(expr)?;

    // Build pattern matchers and template expanders
    let matchers: Vec<(PatternMatcher, TemplateExpander)> = rules
        .iter()
        .map(|(pat, tmpl)| {
            let matcher = PatternMatcher::from_pattern(pat, &literals)?;
            let expander = TemplateExpander::from_template(tmpl)?;
            Ok((matcher, expander))
        })
        .collect::<Result<_, _>>()?;

    // Create transformer closure
    let mac_env = env.borrow().clone();
    let transformer = move |expr: &Value, use_env: &Environment, _mac_env: &Environment| {
        let rename_fn = make_renamer(Rc::new(mac_env.clone()));

        // Try each rule in order
        for (matcher, expander) in &matchers {
            if let Ok(bindings) = matcher.match_pattern(expr, use_env) {
                return expander.expand(&bindings, &rename_fn);
            }
        }

        Err(EvalError::NoMatchingMacroRule)
    };

    Ok(Value::Lambda {
        params: /* (expr use-env mac-env) */,
        body: /* call transformer */,
        env: env.clone(),
        name: Some("syntax-rules transformer".to_string()),
    })
}

// In eval_list:
pub fn eval_list(expr: &Value, env: &Rc<RefCell<Environment>>) -> Result<Value, EvalError> {
    let (car, _) = expr.as_pair()?;

    // Check if car is a macro
    if let Value::Symbol(sym) = car {
        if let Some(Value::Macro(mac)) = env.borrow().lookup(sym) {
            // Expand macro
            let use_env = env.borrow().clone();
            let expanded = (mac.transformer)(expr, &use_env, &mac.definition_env)?;

            // Re-evaluate expanded code
            return evaluator.eval_in_env(&expanded, env);
        }
    }

    // ... normal evaluation
}
```

### Recommended Implementation Order

1. **Week 1-2: Data structures and basic hygiene**
   - Add `Value::Macro` and `Value::SyntacticClosure`
   - Implement `make-renamer` and `identifier=?`
   - Add macro lookup to evaluator

2. **Week 3-4: Pattern matching**
   - Implement `Pattern` enum and parser
   - Implement `PatternMatcher` with basic patterns
   - Add ellipsis support incrementally

3. **Week 5-6: Template expansion**
   - Implement `Template` enum and parser
   - Implement `TemplateExpander` with basic templates
   - Add ellipsis expansion

4. **Week 7: Integration**
   - Implement `define-syntax` special form
   - Implement `syntax-rules` as built-in or macro
   - Connect to evaluator's macro expansion

5. **Week 8+: Testing and refinement**
   - Test with R7RS macro tests
   - Handle edge cases (nested ellipses, improper lists, etc.)
   - Optimize performance

### Alternative: Chibi-style Bootstrap

If we want to match chibi exactly:

1. Implement syntactic closures in Rust (C primitives)
2. Implement `er-macro-transformer` in Scheme (init-7.scm)
3. Load as bootstrap library
4. `syntax-rules` is then defined in Scheme

This requires:
- Loading and evaluating Scheme code at startup
- Scheme->Rust FFI for primitives
- More complex but more "correct"

---

## Key Insights for Patina

### 1. Hygiene is Compositional

Chibi's approach shows that hygiene can be built in layers:
- **Syntactic closures**: Low-level mechanism (track environment)
- **Explicit renaming**: Mid-level API (rename function)
- **`syntax-rules`**: High-level declarative interface

We can implement each layer independently and test incrementally.

### 2. Code Generation is Powerful

Chibi implements `syntax-rules` by **generating code** that performs pattern matching. This is elegant because:
- Pattern matching logic is written once (in `syntax-rules-transformer`)
- Each macro gets a specialized matcher (efficient)
- The Scheme interpreter optimizes the generated code

For Patina, we can either:
- Generate Scheme code (like chibi)
- Generate Rust code (compile-time macros)
- Use direct interpretation (runtime macros)

### 3. Ellipsis Handling is the Hard Part

The most complex code in chibi's `syntax-rules` handles ellipses:
- Detecting ellipsis patterns: `(p ... tail)`
- Tracking dimension: How many ellipses deep are we?
- Accumulating lists: Pattern variables become lists of values
- Flattening: Nested ellipses require `(apply append ...)`

We need robust representation of:
- **Pattern variables with depth**: `(var . depth)`
- **Bindings as lists or values**: `BindingValue::Single` vs `List`
- **Template iteration**: `map` over ellipsis variables

### 4. Testing Strategy

Chibi's architecture suggests a testing approach:
1. **Unit test pattern matching**: Given pattern and expr, check bindings
2. **Unit test template expansion**: Given template and bindings, check output
3. **Integration test macros**: Given macro definition and use, check expanded code
4. **Compliance test**: Use R7RS test suite

### 5. Error Handling

Chibi provides clear errors:
- "bad ellipsis" (ellipsis in wrong position)
- "too few ...'s" (variable not deep enough)
- "too many ...'s" (ellipsis with no variables)
- "no expansion for" (no pattern matched)

Our implementation should match these error messages.

---

## Example Walkthrough

Let's trace `syntax-rules` expansion for a simple macro:

```scheme
(define-syntax swap!
  (syntax-rules ()
    ((swap! a b)
     (let ((tmp a))
       (set! a b)
       (set! b tmp)))))

(swap! x y)
```

### Step 1: Macro Definition

1. `(define-syntax swap! ...)` is parsed
2. `(syntax-rules () ((swap! a b) ...))` is evaluated
3. `syntax-rules-transformer` generates code:

```scheme
(er-macro-transformer
 (lambda (expr rename compare)
   (car
    (or
     ;; Generated pattern matcher for ((swap! a b) ...)
     (let ((v.1 expr))
       (and (pair? v.1)
            (let ((v.2 (car v.1)))
              (and (compare v.2 (rename 'swap!))
                   (let ((v.3 (cdr v.1)))
                     (and (pair? v.3)
                          (let ((v.4 (car v.3)))
                            (let ((a v.4))  ; Bind pattern variable a
                              (let ((v.5 (cdr v.3)))
                                (and (pair? v.5)
                                     (let ((v.6 (car v.5)))
                                       (let ((b v.6))  ; Bind pattern variable b
                                         (let ((v.7 (cdr v.5)))
                                           (and (null? v.7)
                                                ;; Template expansion:
                                                (cons
                                                 (list
                                                  (rename 'let)
                                                  (list (list (rename 'tmp) a))
                                                  (list (rename 'set!) a b)
                                                  (list (rename 'set!) b (rename 'tmp)))
                                                 #f)))))))))))))

     ;; Fallback error
     (cons (error "no expansion for" (strip-syntactic-closures expr))
           #f)))))
```

4. This code is evaluated, creating a transformer closure
5. Macro object is created and bound to `swap!`

### Step 2: Macro Use

1. `(swap! x y)` is encountered during evaluation
2. Evaluator detects `swap!` is a macro
3. `analyze_macro_once` calls transformer with `((swap! x y) use-env mac-env)`
4. Generated pattern matcher executes:
   - Binds `a` to `x` (syntactic closure in use-env)
   - Binds `b` to `y` (syntactic closure in use-env)
5. Template expander executes:
   - `(rename 'let)` → `let#123` (fresh identifier in mac-env)
   - `(rename 'tmp)` → `tmp#456` (fresh identifier in mac-env)
   - `a` → `x` (from use-env, substituted directly)
   - `b` → `y` (from use-env, substituted directly)
   - `(rename 'set!)` → `set!#789` (fresh identifier in mac-env)
6. Returns:
```scheme
(let#123 ((tmp#456 x))
  (set!#789 x y)
  (set!#789 y tmp#456))
```
7. This is re-analyzed and evaluated

### Hygiene in Action

Notice:
- `let`, `set!`, `tmp`: Renamed to fresh identifiers in `mac-env`
- `x`, `y`: From use-env, not renamed
- Even if use-env has `(let ((tmp 1)) (swap! tmp x))`, no conflict!
- The `tmp` from the macro (`tmp#456`) is different from user's `tmp`

---

## Conclusion

Chibi-scheme's macro implementation demonstrates:

1. **Layered architecture**: Syntactic closures → Explicit renaming → `syntax-rules`
2. **Code generation**: Pattern matching is compiled to Scheme code
3. **Hygiene through renaming**: Introduced identifiers are renamed in macro-env
4. **Dimension tracking**: Ellipsis variables track depth for proper nesting
5. **Clean separation**: Pattern matching and template expansion are independent

For Patina, we should:
- Implement syntactic closures in Rust (or simulate with environments)
- Implement pattern matching and template expansion in Rust
- Consider meta-circular approach for `syntax-rules` (future)
- Start with native Rust implementation for direct control
- Test incrementally with simple macros before tackling ellipses

The most important data structures are:
- `Macro { transformer, definition_env }`
- `SyntacticClosure { expr, env, free_vars }`
- `Pattern` (recursive structure with ellipsis support)
- `Template` (recursive structure with ellipsis support)
- `Bindings` (maps variables to values/lists with depth)

The critical algorithms are:
- Pattern matching with ellipsis (accumulate lists, track depth)
- Template expansion with ellipsis (map over lists, flatten)
- Identifier renaming (fresh names in mac-env)
- Hygienic comparison (unwrap closures, compare bindings)
