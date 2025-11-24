# Hygienic Macro Expansion: A Comparative Study

**Date**: 2025-11-23
**Purpose**: Research document comparing three major approaches to hygienic macro expansion in Scheme implementations, with recommendations for upgrading Patina's current gensym-based system.

---

## Executive Summary

This document analyzes three established approaches to hygienic macro expansion:

1. **Racket's Scope Sets** - Modern, principled approach treating hygiene as a scoping problem
2. **Chez Scheme's Marks and Ribs** - Classic algorithm from the syntactic closures tradition
3. **Chibi Scheme's Syntactic Closures** - Simplified explicit renaming approach

**Recommendation**: Implement a **simplified marks-and-ribs system** inspired by Chez Scheme, but adapted for Rust. This provides the best balance of:
- Correctness (full R7RS compliance)
- Implementation complexity (moderate)
- Performance (efficient for typical programs)
- Compatibility (well-understood algorithm)

---

## Table of Contents

1. [Current State: Patina's Gensym Approach](#current-state-patinas-gensym-approach)
2. [Approach 1: Racket's Scope Sets](#approach-1-rackets-scope-sets)
3. [Approach 2: Chez Scheme's Marks and Ribs](#approach-2-chez-schemes-marks-and-ribs)
4. [Approach 3: Chibi Scheme's Syntactic Closures](#approach-3-chibi-schemes-syntactic-closures)
5. [Comparison Table](#comparison-table)
6. [Recommendation](#recommendation)
7. [Implementation Strategy](#implementation-strategy)
8. [References](#references)

---

## Current State: Patina's Gensym Approach

**Location**: `~/Project/patina/crates/patina-frontend/src/macro_expander/hygiene.rs`

### How It Works

```rust
// Generate unique identifier
pub fn gensym(base: &Rc<str>) -> Rc<str> {
    let counter = GENSYM_COUNTER.fetch_add(1, Ordering::Relaxed);
    Rc::from(format!("##{base}#{counter}"))
}
```

**Algorithm**:
1. After macro expansion, scan the result for "free identifiers" (symbols not from pattern variables)
2. Generate a unique name for each free identifier using global counter
3. Replace all occurrences of free identifiers with their gensymed versions

**Format**: `##original#counter` (e.g., `##x#42`)

### Strengths
- Simple to understand and implement
- Works for basic macros
- No complex data structures required
- Fast for simple cases

### Weaknesses
1. **Post-expansion approach is fragile**: Hygiene is applied after expansion completes, making it hard to handle:
   - Nested macros
   - Recursive macro expansion
   - Macro composition

2. **No phase distinction**: Cannot distinguish between identifiers at different expansion phases

3. **Limited scope tracking**: No notion of binding contours or lexical scope during expansion

4. **Brittle special-form list**: Hardcoded list of special forms must be manually maintained:
   ```rust
   fn is_special_form(name: &str) -> bool {
       matches!(name, "quote" | "if" | "define" | "set!" | ...)
   }
   ```

5. **Doesn't handle all R7RS cases**:
   - Module boundaries
   - `syntax-case` (future)
   - Complex identifier aliasing
   - Pattern matching with ellipses at different depths

### Example Failure Case

```scheme
(define-syntax swap
  (syntax-rules ()
    ((swap a b)
     (let ((tmp a))
       (set! a b)
       (set! b tmp)))))

(let ((tmp 1) (x 2))
  (swap tmp x)
  tmp)
;; Expected: 2 (tmp and x should swap)
;; With gensym: Works correctly (tmp renamed to ##tmp#0)
;; But consider:

(define-syntax bad-swap
  (syntax-rules ()
    ((bad-swap a b)
     (let ((##tmp#0 a))  ; Directly uses gensym format
       (set! a b)
       (set! b ##tmp#0)))))

(let ((x 1) (y 2))
  (bad-swap x y))
;; This could collide with our gensym scheme!
```

---

## Approach 1: Racket's Scope Sets

**Location**: `~/Project/reference/racket/racket/src/expander/syntax/scope.rkt`

### Core Concepts

Racket treats hygiene as a **scoping problem** rather than a renaming problem. Each syntax object carries a **set of scopes** that determine its binding context.

#### Key Data Structures

```racket
;; From scope.rkt

;; A scope represents a distinct "dimension" of binding
(struct scope (id             ; unique ID (exact rational)
               kind           ; 'macro for macro-introduction scopes
               binding-table) ; maps (scopes × symbol × phase) -> binding
  #:mutable)

;; Syntax object with scope information
(struct syntax ([content* #:mutable]  ; datum + nested syntax
                scopes                ; set of scopes at all phases
                shifted-multi-scopes  ; phase-specific scopes
                mpi-shifts            ; module-path-index shifts
                srcloc                ; source location
                props                 ; properties
                inspector))           ; for protected bindings

;; A multi-scope represents scopes at different phases
(struct multi-scope (id       ; identity
                     name     ; for debugging
                     scopes   ; box of table: phase -> representative-scope
                     shifted  ; box of table: phase-specific shifts
                     label-shifted))
```

### Algorithm

1. **Scope Introduction**: When a macro is applied, a new scope is created and added to all introduced identifiers

2. **Scope Propagation**: Scopes are propagated through syntax object structure lazily for efficiency

3. **Binding Resolution**: An identifier resolves to a binding by finding the entry in a scope's binding table that matches:
   - The identifier's symbol
   - The identifier's complete set of scopes
   - The current phase

4. **Hygiene via Scope Sets**:
   - Identifiers from the macro input carry the use-site scopes
   - Identifiers introduced by the macro carry an additional macro-introduction scope
   - These different scope sets prevent accidental capture

### Code Example

```racket
;; Add a scope to a syntax object
(define (add-scope s sc)
  (apply-scope s (generalize-scope sc) set-add propagation-add))

;; Resolve an identifier's binding
(define (resolve s phase)
  (let ([scopes (syntax-scope-set s phase)])
    ;; Find binding with most specific (largest) matching scope set
    (for*/fold ([best-scopes #f] [best-binding #f])
               ([sc (in-set scopes)]
                [(b-scopes binding) (in-binding-table sym
                                                      (scope-binding-table sc)
                                                      s)])
      (if (and b-scopes binding (subset? b-scopes scopes))
          (if (and best-scopes (subset? b-scopes best-scopes))
              (values best-scopes best-binding)
              (values b-scopes binding))
          (values best-scopes best-binding)))))
```

### Advantages

1. **Principled foundation**: Based on formal semantics of hygiene
2. **Handles all cases**: Modules, phases, pattern matching, nested macros
3. **Lazy propagation**: Efficient scope propagation through lazy updates
4. **Precise**: No over-renaming or under-renaming
5. **Extensible**: Easy to add new features (e.g., syntax parameters)

### Disadvantages

1. **High complexity**:
   - ~1000 lines just for scope.rkt
   - Multiple specialized scope types (scope, interned-scope, multi-scope, representative-scope, shifted-multi-scope)
   - Complex binding table implementation

2. **Memory overhead**: Every syntax object carries sets of scopes

3. **Serialization complexity**: Requires careful handling for module compilation

4. **Learning curve**: Difficult to understand and debug without deep theory knowledge

### Complexity Assessment

- **Code size**: ~3000 lines (scope.rkt + binding-table.rkt + binding.rkt)
- **Data structures**: 7+ specialized types
- **Algorithmic complexity**: O(n × s) where n = syntax size, s = average scope set size
- **Memory**: O(n × s) for syntax objects

---

## Approach 2: Chez Scheme's Marks and Ribs

**Location**: `~/Project/reference/ChezScheme/s/syntax.ss`

### Core Concepts

Chez uses **marks** to track macro expansion phases and **ribs** (substitution frames) to record identifier renamings. This is the classic algorithm from Dybvig's papers.

#### Key Data Structures

```scheme
;;; From syntax.ss (comments are from source)

;;; Syntax object with wrap
(define-record-type syntax-object
  (fields expression wrap))

;;; Wrap: combines marks and substitutions
;;;   wrap ::= ((mark ...) . (subst ...))
(define make-wrap cons)
(define wrap-marks car)
(define wrap-subst cdr)

;;; Substitution (ribcage)
;;;   subst ::= ribcage | shift
;;;   ribcage ::= fixed-ribcage | extensible-ribcage | top-ribcage

;;; Fixed ribcage: maps (symbol × marks) -> label
(define-record-type fixed-ribcage
  (fields (immutable symnames)    ; vector of symbols
          (immutable marks)       ; vector of mark lists
          (immutable label/pls))) ; vector of labels

;;; Extensible ribcage: built incrementally for internal definitions
(define-record-type extensible-ribcage
  (fields (mutable chunks)))  ; list of hashtables/interfaces/barriers

;;; Top-level ribcage: tracks top-level definitions
(define-record-type top-ribcage
  (fields (immutable key)      ; token identifying environment
          (mutable mutable?))) ; whether bindings can be added

;;; Mark: unique object for each macro expansion
(define new-mark (lambda () (string #\m)))  ; Each mark is unique string

;;; Label: binding identifier (can be symbol or local-label)
(define gen-global-label (lambda (sym) (generate-id sym)))

(define-record-type local-label
  (fields (mutable binding)  ; the actual binding
          (mutable level)))  ; meta-level for phase
```

### Algorithm

#### 1. Macro Application (Mark Introduction)

```scheme
;;; When applying a macro transformer:
;;; 1. Add a fresh mark to the input
;;; 2. Call the transformer
;;; 3. Add the same mark to the output (anti-mark)

(define chi-macro
  (lambda (p e r w ...)
    (let ([mark (new-mark)])
      ;; Add mark to input
      (let ([input (add-mark mark e w)])
        ;; Apply transformer
        (let ([output (p input)])
          ;; Add anti-mark to output
          (add-mark mark output (anti-mark w)))))))
```

#### 2. Identifier Resolution (Label Lookup)

```scheme
;;; To resolve an identifier to its binding:
;;; 1. Extract symbol and marks from the identifier
;;; 2. Walk through the wrap's substitutions (ribs)
;;; 3. Find matching entry: same symbol AND same marks
;;; 4. Return the associated label (binding)

(define id->label
  (lambda (id w)
    (let-values ([(sym marks) (id-sym-name&marks id w)])
      ;; Walk through substitutions
      (let search-ribs ([substs (wrap-subst w)])
        (cond
          [(null? substs)
           ;; No local binding, lookup global
           (lookup-global-label sym marks (top-ribcage-key ...))]

          [(fixed-ribcage? (car substs))
           ;; Search fixed ribcage
           (let ([rib (car substs)])
             (let search-rib ([i 0])
               (cond
                 [(= i (vector-length (fixed-ribcage-symnames rib)))
                  ;; Not in this rib, try next
                  (search-ribs (cdr substs))]

                 [(and (eq? sym (vector-ref (fixed-ribcage-symnames rib) i))
                       (same-marks? marks (vector-ref (fixed-ribcage-marks rib) i)))
                  ;; Found it!
                  (vector-ref (fixed-ribcage-label/pls rib) i)]

                 [else (search-rib (+ i 1))])))]

          [(eq? (car substs) 'shift)
           ;; Anti-mark: cancel a mark and continue
           (search-ribs (cdr substs))]

          [else ; extensible-ribcage, etc.
           ...])))))
```

#### 3. Mark Cancellation

```scheme
;;; Join two wraps, canceling matching marks:
;;;   join-wraps(((m1 m2 m3) . S1), ((m3) . (shift . S2)))
;;;      = join-wraps(((m1 m2) . S1), (() . S2))
;;;      = ((m1 m2) . (S1 ... S2))

(define join-marks
  (lambda (m1 m2)
    (cond
      [(and (pair? m1) (pair? m2) (eq? (car m1) the-anti-mark))
       ;; Cancel matching marks
       (join-marks (cdr m1) (cdr m2))]
      [else (append m1 m2)])))
```

### Detailed Example

```scheme
;; Original macro
(define-syntax swap
  (syntax-rules ()
    ((swap a b)
     (let ((tmp a))
       (set! a b)
       (set! b tmp)))))

;; Expansion trace:
;; 1. Input: (swap x y)
;;    Marks: ()
;;    Substs: (top-rib)

;; 2. Add fresh mark m1 to input:
;;    Input becomes: (swap[m1] x[m1] y[m1])

;; 3. Pattern match extracts:
;;    a -> x[m1]  (carries mark m1)
;;    b -> y[m1]  (carries mark m1)

;; 4. Template expansion produces:
;;    (let ((tmp a[subst:a->x[m1]])
;;          (set! a[subst:a->x[m1]] b[subst:b->y[m1]])
;;          (set! b[subst:b->y[m1]] tmp)))
;;
;;    Where:
;;    - 'let', 'tmp', 'set!' are introduced by macro
;;    - a, b are pattern variables substituted

;; 5. Add anti-mark m1:
;;    (let[m1] ((tmp[m1] a[subst:a->x[m1]][m1])
;;              (set![m1] a[subst:a->x[m1]][m1] b[subst:b->y[m1]][m1])
;;              (set![m1] b[subst:b->y[m1]][m1] tmp[m1])))

;; 6. Marks cancel in pattern variables:
;;    x[m1][m1] -> x[]  (mark cancellation)
;;    y[m1][m1] -> y[]  (mark cancellation)
;;
;;    But NOT in introduced identifiers:
;;    let[m1] remains marked
;;    tmp[m1] remains marked
;;
;; 7. Final output (conceptual):
;;    (let ((##tmp#m1 x)
;;          (set! x y)
;;          (set! y ##tmp#m1)))
;;
;;    Where ##tmp#m1 represents tmp with mark m1
```

### Advantages

1. **Well-understood**: Decades of research and practical use
2. **Moderate complexity**: Simpler than scope sets, more powerful than gensym
3. **Efficient**: Mark cancellation prevents unbounded mark accumulation
4. **Correct**: Handles all R7RS macro cases correctly
5. **Proven**: Used in production Scheme systems (Chez, Vicare, etc.)

### Disadvantages

1. **Still complex**: Requires understanding marks, ribs, wraps, and labels
2. **Substitution overhead**: Walking ribs can be O(n) for n bindings
3. **Mark comparison**: Need efficient mark equality and list comparison
4. **Implementation details**: Many edge cases in mark joining and cancellation

### Complexity Assessment

- **Code size**: ~2000 lines in syntax.ss (includes full expander)
- **Core hygiene logic**: ~500 lines
- **Data structures**: 6 record types (syntax-object, fixed-ribcage, extensible-ribcage, top-ribcage, local-label, barrier)
- **Algorithmic complexity**: O(n × r × m) where n = identifier lookups, r = ribs in wrap, m = marks
- **Memory**: O(n × (r + m)) for syntax objects

---

## Approach 3: Chibi Scheme's Syntactic Closures

**Location**: `~/Project/reference/chibi-scheme/eval.c` and `include/chibi/sexp.h`

### Core Concepts

Chibi uses **syntactic closures** - a simplified approach where each identifier explicitly captures its environment and free variable set. This is based on the "syntactic closures" paper by Bawden & Rees.

#### Key Data Structures

```c
/* From sexp.h */

/* Syntactic closure: wraps expression with environment */
struct {
  sexp env;         // Environment where defined
  sexp free_vars;   // List of free variables (not renamed)
  sexp expr;        // The actual expression
  sexp rename;      // Rename alist (for explicit renaming)
} synclo;

/* Environment has optional rename bindings */
struct {
  sexp parent;      // Parent environment
  sexp bindings;    // Variable bindings
  sexp renames;     // Rename bindings (name -> renamed-name)
} env;
```

### Algorithm

#### 1. Creating Syntactic Closures

```c
/* make-syntactic-closure: env free-vars expr -> synclo */
sexp sexp_make_synclo_op(sexp ctx, sexp env, sexp free_vars, sexp expr) {
  sexp res = sexp_alloc_type(ctx, synclo, SEXP_SYNCLO);
  sexp_synclo_env(res) = env;
  sexp_synclo_free_vars(res) = free_vars;
  sexp_synclo_expr(res) = expr;
  sexp_synclo_rename(res) = SEXP_FALSE;
  return res;
}
```

#### 2. Identifier Resolution

```c
/* Look up identifier, unwrapping syntactic closures */
sexp sexp_env_cell(sexp ctx, sexp env, sexp key, int localp) {
  sexp cell;

  /* Unwrap syntactic closures */
  while (!cell && key && sexp_synclop(key)) {
    /* If key is not in free_vars, use the closure's environment */
    if (!sexp_pairp(ls) &&
        sexp_not(sexp_memq(ctx, sexp_synclo_expr(key),
                          sexp_synclo_free_vars(key))))
      env = sexp_synclo_env(key);

    /* Unwrap to inner expression */
    key = sexp_synclo_expr(key);
    cell = sexp_env_cell_loc1(env, key, localp, NULL);
  }

  return cell;
}
```

#### 3. Macro Expansion with Closures

```scheme
;; In Scheme (simplified conceptual version):

(define-syntax swap
  (er-macro-transformer  ;; explicit-renaming style
   (lambda (expr rename compare)
     (let ((tmp (rename 'tmp))    ; Generate fresh name
           (a (cadr expr))        ; Extract pattern vars
           (b (caddr expr)))
       `(let ((,tmp ,a))          ; Use renamed tmp
          (set! ,a ,b)
          (set! ,b ,tmp))))))
```

The `rename` function:
- Takes a symbol
- Returns a syntactic closure that captures the macro's environment
- Prevents the symbol from capturing user variables

### Example Trace

```scheme
;; Macro invocation: (swap x y)

;; 1. Macro transformer called with:
;;    expr = (swap x y)
;;    rename = <function to create closures>
;;    compare = <function to compare identifiers>

;; 2. Transformer execution:
;;    tmp = (rename 'tmp)
;;        = <synclo env=macro-env free-vars=() expr=tmp>
;;    a = x  (from input, has input environment)
;;    b = y  (from input, has input environment)

;; 3. Template construction:
;;    (let ((<synclo tmp> x))
;;         (set! x y)
;;         (set! y <synclo tmp>))

;; 4. Evaluation:
;;    - When looking up <synclo tmp>, use macro-env
;;    - When looking up x or y, use input environment
;;    - No collision possible!
```

### Advantages

1. **Simplicity**: Easiest to understand conceptually
2. **Explicit**: Rename operations are explicit in macro transformers
3. **Flexible**: Can choose exactly which identifiers to rename
4. **Low overhead**: Only wraps identifiers that need special treatment
5. **Small code**: ~200 lines for core hygiene logic

### Disadvantages

1. **Manual hygiene**: Macro writers must call `rename` explicitly
   - Error-prone: easy to forget to rename
   - Not automatic like `syntax-rules`

2. **Not fully hygienic by default**:
   - `define-syntax` with lambda doesn't automatically provide hygiene
   - Need `er-macro-transformer` or `sc-macro-transformer` wrappers

3. **Limited to explicit renaming**:
   - Doesn't handle `syntax-case` patterns
   - No automatic ellipsis handling

4. **Environment capture overhead**:
   - Every renamed identifier carries an environment
   - Can be memory-intensive for large macros

5. **R7RS compliance issues**:
   - `syntax-rules` must be implemented on top
   - Pattern matching hygiene needs extra machinery

### Complexity Assessment

- **Code size**: ~200 lines for synclo implementation
- **Data structures**: 1 record type (syntactic-closure)
- **Algorithmic complexity**: O(n) for identifier lookup (unwrap closures)
- **Memory**: O(k) where k = number of renamed identifiers
- **Wrapper overhead**: Need er-macro-transformer for hygiene

---

## Comparison Table

| Aspect | Racket Scope Sets | Chez Marks & Ribs | Chibi Syntactic Closures | Patina Gensym |
|--------|------------------|-------------------|--------------------------|---------------|
| **Complexity** | Very High | Moderate | Low | Very Low |
| **Code Lines** | ~3000 | ~500 | ~200 | ~200 |
| **Data Structures** | 7+ types | 6 types | 1 type | 0 types |
| **Memory Overhead** | High (scope sets) | Moderate (marks+ribs) | Low (only renamed) | Low (gensyms) |
| **Performance** | Good (lazy prop.) | Good (mark cancel) | Good (simple) | Good (simple) |
| **Correctness** | Excellent | Excellent | Good* | Poor |
| **R7RS Compliance** | Full | Full | Partial** | Partial |
| **Module Support** | Excellent | Excellent | Basic | None |
| **Phase Support** | Full | Full | Limited | None |
| **Nested Macros** | Excellent | Excellent | Good | Poor |
| **Learning Curve** | Steep | Moderate | Gentle | Gentle |
| **Debugging** | Hard | Moderate | Easy | Easy |
| **Implementation Risk** | High | Moderate | Low | Very Low |
| **Maintenance** | High | Moderate | Low | Low |

\* With proper macro transformer wrappers
\*\* Requires additional machinery for `syntax-rules`

---

## Recommendation

### Recommended Approach: Simplified Marks and Ribs

For Patina, I recommend implementing a **simplified marks-and-ribs system** inspired by Chez Scheme, but adapted for Rust's type system and Patina's architecture.

### Rationale

1. **Correctness**: Full R7RS compliance for `syntax-rules` macros
2. **Complexity**: Moderate - not as complex as scope sets, more principled than gensym
3. **Performance**: Efficient mark cancellation prevents unbounded growth
4. **Proven**: Decades of use in production Scheme systems
5. **Extensibility**: Can be extended to `syntax-case` later
6. **Rust-friendly**: Maps well to Rust's type system and ownership model

### Why Not the Others?

**Not Scope Sets** (Racket):
- Too complex for current needs (3000+ lines)
- Overkill for R7RS-small (designed for full Racket)
- Harder to debug and maintain
- High memory overhead

**Not Syntactic Closures** (Chibi):
- Manual hygiene is error-prone
- Requires wrapper macros (`er-macro-transformer`)
- Not natural for `syntax-rules`
- Doesn't match R7RS specification philosophy

**Not Gensym** (Current):
- Doesn't handle all R7RS cases correctly
- Fragile post-expansion approach
- No phase distinction
- Hard to extend to `syntax-case`

---

## Implementation Strategy

### Phase 1: Core Infrastructure (Week 1-2)

#### 1.1 Define Core Types

```rust
// In patina-runtime/src/syntax.rs (new file)

use std::rc::Rc;

/// Unique identifier for a mark
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mark(Rc<MarkInner>);

#[derive(Debug, PartialEq, Eq, Hash)]
struct MarkInner {
    id: usize,  // Unique ID from counter
    kind: MarkKind,
}

#[derive(Debug, PartialEq, Eq, Hash)]
enum MarkKind {
    Macro,      // From macro expansion
    Module,     // From module boundary
    AntiMark,   // For cancellation
}

impl Mark {
    pub fn fresh_macro() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        Mark(Rc::new(MarkInner { id, kind: MarkKind::Macro }))
    }

    pub fn anti_mark(&self) -> Self {
        Mark(Rc::new(MarkInner {
            id: self.0.id,
            kind: MarkKind::AntiMark
        }))
    }
}

/// List of marks (most recent first)
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Marks(Vec<Mark>);

impl Marks {
    pub fn add(&self, mark: Mark) -> Self {
        let mut new_marks = self.0.clone();
        new_marks.insert(0, mark);
        Self(new_marks)
    }

    /// Join two mark lists, canceling anti-marks
    pub fn join(&self, other: &Marks) -> Self {
        // Cancel (mark, anti-mark) pairs
        let mut result = self.0.clone();
        let mut other_marks = other.0.iter();

        while let Some(mark) = other_marks.next() {
            if !result.is_empty() && Self::can_cancel(&result[0], mark) {
                result.remove(0);  // Cancel
            } else {
                result.insert(0, mark.clone());
            }
        }

        Self(result)
    }

    fn can_cancel(m1: &Mark, m2: &Mark) -> bool {
        m1.0.id == m2.0.id &&
        matches!(
            (&m1.0.kind, &m2.0.kind),
            (MarkKind::Macro, MarkKind::AntiMark) |
            (MarkKind::AntiMark, MarkKind::Macro)
        )
    }
}

/// Label: unique identifier for a binding
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Label {
    Global(Rc<str>),           // Global variable: symbol name
    Local(usize),              // Local variable: unique ID
    Generated(Rc<str>, usize), // Generated: (base-name, unique-id)
}

impl Label {
    pub fn fresh_local() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        Label::Local(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    pub fn fresh_generated(base: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        Label::Generated(
            Rc::from(base),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    }
}

/// Substitution rib: maps (symbol, marks) -> label
#[derive(Clone, Debug)]
pub enum Rib {
    /// Fixed ribcage: vector-based for compiled macros
    Fixed(FixedRib),

    /// Extensible ribcage: hash-based for internal definitions
    Extensible(Rc<RefCell<ExtensibleRib>>),
}

#[derive(Clone, Debug)]
pub struct FixedRib {
    /// Parallel vectors: symbols, marks, labels
    pub symbols: Vec<Rc<str>>,
    pub marks: Vec<Marks>,
    pub labels: Vec<Label>,
}

impl FixedRib {
    pub fn new() -> Self {
        Self {
            symbols: Vec::new(),
            marks: Vec::new(),
            labels: Vec::new(),
        }
    }

    pub fn add(&mut self, symbol: Rc<str>, marks: Marks, label: Label) {
        self.symbols.push(symbol);
        self.marks.push(marks);
        self.labels.push(label);
    }

    /// Look up (symbol, marks) -> label
    pub fn lookup(&self, symbol: &str, marks: &Marks) -> Option<&Label> {
        for i in 0..self.symbols.len() {
            if self.symbols[i].as_ref() == symbol && &self.marks[i] == marks {
                return Some(&self.labels[i]);
            }
        }
        None
    }
}

#[derive(Clone, Debug, Default)]
pub struct ExtensibleRib {
    /// HashMap: symbol -> Vec<(marks, label)>
    pub bindings: HashMap<Rc<str>, Vec<(Marks, Label)>>,
}

impl ExtensibleRib {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, symbol: Rc<str>, marks: Marks, label: Label) {
        self.bindings
            .entry(symbol)
            .or_insert_with(Vec::new)
            .push((marks, label));
    }

    pub fn lookup(&self, symbol: &str, marks: &Marks) -> Option<&Label> {
        self.bindings.get(symbol)?.iter()
            .find(|(m, _)| m == marks)
            .map(|(_, label)| label)
    }
}

/// Wrap: combines marks and ribs
#[derive(Clone, Debug)]
pub struct Wrap {
    pub marks: Marks,
    pub ribs: Vec<Rib>,
}

impl Wrap {
    pub fn empty() -> Self {
        Self {
            marks: Marks::default(),
            ribs: Vec::new(),
        }
    }

    pub fn add_mark(&self, mark: Mark) -> Self {
        Self {
            marks: self.marks.add(mark),
            ribs: self.ribs.clone(),
        }
    }

    pub fn add_rib(&self, rib: Rib) -> Self {
        let mut new_ribs = self.ribs.clone();
        new_ribs.insert(0, rib);
        Self {
            marks: self.marks.clone(),
            ribs: new_ribs,
        }
    }

    pub fn join(&self, other: &Wrap) -> Self {
        Self {
            marks: self.marks.join(&other.marks),
            ribs: [self.ribs.clone(), other.ribs.clone()].concat(),
        }
    }
}
```

#### 1.2 Extend Value Type

```rust
// In patina-runtime/src/value.rs

pub enum Value {
    // ... existing variants ...

    /// Syntax object: datum + hygiene information
    Syntax(Rc<SyntaxObject>),
}

pub struct SyntaxObject {
    pub datum: Box<Value>,  // The actual expression
    pub wrap: Wrap,         // Hygiene information
    pub source: Option<SourceLocation>,  // For error messages
}

impl SyntaxObject {
    pub fn new(datum: Value) -> Self {
        Self {
            datum: Box::new(datum),
            wrap: Wrap::empty(),
            source: None,
        }
    }

    pub fn with_wrap(datum: Value, wrap: Wrap) -> Self {
        Self {
            datum: Box::new(datum),
            wrap,
            source: None,
        }
    }

    /// Add a mark to this syntax object
    pub fn add_mark(&self, mark: Mark) -> Self {
        Self {
            datum: self.datum.clone(),
            wrap: self.wrap.add_mark(mark),
            source: self.source.clone(),
        }
    }

    /// Get the underlying datum
    pub fn datum(&self) -> &Value {
        &self.datum
    }

    /// Resolve identifier to label
    pub fn resolve(&self) -> Option<Label> {
        if let Value::Symbol(sym) = self.datum.as_ref() {
            resolve_identifier(sym, &self.wrap)
        } else {
            None
        }
    }
}

/// Resolve (symbol, wrap) to label
fn resolve_identifier(symbol: &Rc<str>, wrap: &Wrap) -> Option<Label> {
    // Search ribs in order (most recent first)
    for rib in &wrap.ribs {
        match rib {
            Rib::Fixed(fixed) => {
                if let Some(label) = fixed.lookup(symbol, &wrap.marks) {
                    return Some(label.clone());
                }
            }
            Rib::Extensible(ext) => {
                if let Some(label) = ext.borrow().lookup(symbol, &wrap.marks) {
                    return Some(label.clone());
                }
            }
        }
    }

    // No local binding, return global label
    Some(Label::Global(symbol.clone()))
}
```

### Phase 2: Macro Expander Integration (Week 3-4)

#### 2.1 Update Template Expansion

```rust
// In patina-frontend/src/macro_expander/expander.rs

impl Expander {
    pub fn expand(
        &self,
        template: &Template,
        match_env: &MatchEnv,
        input_wrap: &Wrap,      // NEW: wrap from input
        macro_mark: &Mark,      // NEW: mark for this expansion
    ) -> Result<Value, ExpandError> {
        // Create wrap for introduced identifiers
        let intro_wrap = input_wrap
            .add_mark(macro_mark.clone())
            .add_mark(macro_mark.anti_mark());

        self.expand_template(template, match_env, input_wrap, &intro_wrap)
    }

    fn expand_template(
        &self,
        template: &Template,
        match_env: &MatchEnv,
        pattern_var_wrap: &Wrap,  // Wrap for pattern variables
        intro_wrap: &Wrap,        // Wrap for introduced identifiers
    ) -> Result<Value, ExpandError> {
        match template {
            Template::Literal(val) => Ok(val.clone()),

            Template::Var(pv) => {
                // Pattern variable: use input wrap
                let value = match_env.lookup(pv)?;
                Ok(self.wrap_value(value, pattern_var_wrap))
            }

            Template::Symbol(ident) => {
                // Introduced identifier: use intro wrap
                Ok(Value::Syntax(Rc::new(SyntaxObject::with_wrap(
                    Value::Symbol(ident.name().clone()),
                    intro_wrap.clone()
                ))))
            }

            Template::List(templates) => {
                let items: Result<Vec<_>, _> = templates
                    .iter()
                    .map(|t| self.expand_template(
                        t,
                        match_env,
                        pattern_var_wrap,
                        intro_wrap
                    ))
                    .collect();
                Ok(self.list_from_vec(items?))
            }

            // ... other cases ...
        }
    }

    fn wrap_value(&self, value: &Value, wrap: &Wrap) -> Value {
        match value {
            Value::Symbol(sym) => {
                Value::Syntax(Rc::new(SyntaxObject::with_wrap(
                    Value::Symbol(sym.clone()),
                    wrap.clone()
                )))
            }
            Value::Pair(pair) => {
                let (car, cdr) = &*pair.borrow();
                let new_car = self.wrap_value(car, wrap);
                let new_cdr = self.wrap_value(cdr, wrap);
                Value::Pair(Rc::new(RefCell::new((new_car, new_cdr))))
            }
            // ... other cases ...
            _ => value.clone()
        }
    }
}
```

#### 2.2 Update Macro Application

```rust
// In patina-frontend/src/macro_expander/mod.rs

pub fn expand_macro(
    compiled_macro: &CompiledMacro,
    args: &Value,
    env: &Rc<Environment>,
) -> Result<Value, FrontendError> {
    // Extract wrap from input (or create empty if plain value)
    let input_wrap = extract_wrap(args);

    // Create fresh mark for this expansion
    let macro_mark = Mark::fresh_macro();

    // Add mark to input for pattern matching
    let marked_input = add_mark_to_value(args, &macro_mark);

    let expander = Expander::new(env.clone());

    for rule in &compiled_macro.rules {
        let matcher = Matcher::new(rule.num_pvars);

        if let Ok(match_env) = matcher.match_pattern(&rule.pattern, &marked_input) {
            // Pattern matched! Expand template with hygiene
            return expander.expand(
                &rule.template,
                &match_env,
                &input_wrap,
                &macro_mark,
            );
        }
    }

    Err(FrontendError::InvalidSyntax(
        format!("No matching pattern for macro {}", compiled_macro.name)
    ))
}

fn extract_wrap(value: &Value) -> Wrap {
    match value {
        Value::Syntax(stx) => stx.wrap.clone(),
        _ => Wrap::empty(),
    }
}

fn add_mark_to_value(value: &Value, mark: &Mark) -> Value {
    match value {
        Value::Symbol(sym) => {
            Value::Syntax(Rc::new(SyntaxObject::with_wrap(
                Value::Symbol(sym.clone()),
                Wrap::empty().add_mark(mark.clone())
            )))
        }
        Value::Syntax(stx) => {
            Value::Syntax(Rc::new(stx.add_mark(mark.clone())))
        }
        Value::Pair(pair) => {
            let (car, cdr) = &*pair.borrow();
            let new_car = add_mark_to_value(car, mark);
            let new_cdr = add_mark_to_value(cdr, mark);
            Value::Pair(Rc::new(RefCell::new((new_car, new_cdr))))
        }
        _ => value.clone()
    }
}
```

### Phase 3: Evaluator Integration (Week 5)

#### 3.1 Update Variable Lookup

```rust
// In patina-tree-walker/src/eval/mod.rs

fn eval_symbol(
    &self,
    value: &Value,
    env: &Rc<Environment>,
) -> EvalResult {
    match value {
        Value::Symbol(name) => {
            // Plain symbol: look up in environment
            self.lookup_variable(name, env)
        }
        Value::Syntax(stx) => {
            // Syntax object: resolve label first
            if let Some(label) = stx.resolve() {
                self.lookup_by_label(&label, env)
            } else {
                self.lookup_variable(
                    stx.datum().as_symbol()?,
                    env
                )
            }
        }
        _ => unreachable!()
    }
}

fn lookup_by_label(
    &self,
    label: &Label,
    env: &Rc<Environment>
) -> EvalResult {
    match label {
        Label::Global(name) => {
            // Global: lookup by name
            env.get(name)
                .ok_or_else(|| EvalError::UnboundVariable(name.to_string()))
                .map(|v| StepResult::Value(v))
        }

        Label::Local(id) | Label::Generated(_, id) => {
            // Local/generated: lookup in label table
            self.label_table
                .get(id)
                .ok_or_else(|| EvalError::UnboundVariable(
                    format!("label {}", id)
                ))
                .map(|v| StepResult::Value(v.clone()))
        }
    }
}
```

#### 3.2 Add Label Table to Evaluator

```rust
// In patina-tree-walker/src/eval/mod.rs

pub struct Evaluator {
    // ... existing fields ...

    /// Label table: maps local labels to values
    label_table: HashMap<usize, Value>,
}

impl Evaluator {
    pub fn bind_label(&mut self, label: Label, value: Value) {
        if let Label::Local(id) | Label::Generated(_, id) = label {
            self.label_table.insert(id, value);
        }
    }
}
```

### Phase 4: Testing and Refinement (Week 6)

#### 4.1 Test Cases

```scheme
;; Test 1: Basic hygiene
(define-syntax my-let
  (syntax-rules ()
    ((my-let ((var val)) body)
     ((lambda (var) body) val))))

(let ((x 1))
  (my-let ((x 2))
    x))
;; Should return 2 (inner x shadows outer)

;; Test 2: Introduced identifier hygiene
(define-syntax swap
  (syntax-rules ()
    ((swap a b)
     (let ((tmp a))
       (set! a b)
       (set! b tmp)))))

(let ((tmp 1) (x 2))
  (swap tmp x)
  tmp)
;; Should return 2 (macro's tmp doesn't capture)

;; Test 3: Nested macros
(define-syntax when
  (syntax-rules ()
    ((when test body ...)
     (if test (begin body ...)))))

(define-syntax unless
  (syntax-rules ()
    ((unless test body ...)
     (when (not test) body ...))))

(let ((x 1))
  (unless (= x 0)
    (set! x 2))
  x)
;; Should return 2

;; Test 4: Ellipsis with hygiene
(define-syntax let*
  (syntax-rules ()
    ((let* () body)
     body)
    ((let* ((var val) rest ...) body)
     (let ((var val))
       (let* (rest ...) body)))))

(let* ((x 1) (y 2) (z 3))
  (+ x y z))
;; Should return 6

;; Test 5: Pattern variable capturing (should work)
(define-syntax my-cond
  (syntax-rules (else)
    ((my-cond)
     (if #f #f))
    ((my-cond (else result))
     result)
    ((my-cond (test result) rest ...)
     (if test result (my-cond rest ...)))))

(my-cond
  ((= 1 0) 'no)
  (else 'yes))
;; Should return 'yes
```

#### 4.2 Benchmark Comparison

Compare performance before/after:
- Macro expansion time
- Memory usage
- R7RS compliance test pass rate

### Phase 5: Documentation and Migration (Week 7)

#### 5.1 Update Documentation

- Document the marks-and-ribs algorithm
- Add examples to CLAUDE.md
- Update TEST_ORGANIZATION.md with new test categories

#### 5.2 Migration Guide

```markdown
## Migrating from Gensym to Marks-and-Ribs

### For Users
No changes needed. Your macros will work the same or better.

### For Developers

**Old Code**:
```rust
let expanded = expand_template(...);
let hygienic = apply_hygiene(&expanded, &pattern_vars, &env);
```

**New Code**:
```rust
let expanded = expander.expand(
    &template,
    &match_env,
    &input_wrap,
    &macro_mark,
);
// Hygiene is automatic!
```

**Key Changes**:
1. `Value::Syntax` is now a first-class variant
2. Identifiers carry `Wrap` information
3. Label resolution happens during evaluation
4. No post-expansion hygiene pass needed
```

---

## Expected Outcomes

### Immediate Benefits

1. **Correctness**: All R7RS `syntax-rules` tests should pass
2. **No hardcoded special forms**: Special form list no longer needed
3. **Better error messages**: Can report which macro introduced an identifier
4. **Cleaner code**: No post-expansion hygiene pass

### Long-term Benefits

1. **Extensibility**: Foundation for `syntax-case` (Phase 2+)
2. **Module support**: Can handle library exports/imports correctly
3. **Debuggability**: Can trace identifier bindings through expansions
4. **Compliance**: Full R7RS-small compliance milestone

### Metrics for Success

- [ ] All existing macro tests pass
- [ ] Additional R7RS compliance tests pass (target: 80%+ for macro tests)
- [ ] No performance regression (< 10% slowdown acceptable)
- [ ] Memory usage remains reasonable (< 20% increase acceptable)
- [ ] Code maintainability improves (fewer special cases)

---

## References

### Academic Papers

1. **Dybvig, R. K., Hieb, R., & Bruggeman, C. (1992)**. "Syntactic abstraction in Scheme."
   *Lisp and Symbolic Computation*, 5(4), 295-326.
   - Original marks-and-ribs algorithm

2. **Flatt, M. (2016)**. "Binding as sets of scopes."
   *ACM SIGPLAN Notices*, 51(1), 705-717.
   - Racket's scope sets approach

3. **Bawden, A., & Rees, J. (1988)**. "Syntactic closures."
   *Proceedings of the 1988 ACM conference on LISP and functional programming*, 86-95.
   - Syntactic closures approach

### Implementation References

1. **Chez Scheme**: `s/syntax.ss` - marks and ribs implementation
   - Most comprehensive reference
   - Production-quality code
   - Well-commented

2. **Racket Expander**: `src/expander/syntax/` - scope sets implementation
   - Modern approach
   - Highly optimized
   - Complex but instructive

3. **Chibi Scheme**: `eval.c` - syntactic closures implementation
   - Simple and clear
   - Good for understanding explicit renaming
   - Not suitable for automatic hygiene

### Specifications

1. **R7RS-small**: Section 4.3 "Macros"
   - Official specification
   - Defines `syntax-rules` semantics
   - Test cases in appendix

2. **R6RS**: Section 12 "Syntax-case"
   - More complex macro system
   - Useful for future extension
   - Hygiene algorithm details

---

## Appendix A: Detailed Algorithm Comparison

### Mark Cancellation Example

```scheme
;; Input: (swap x y)
;; Macro: (define-syntax swap
;;          (syntax-rules ()
;;            ((swap a b)
;;             (let ((tmp a)) (set! a b) (set! b tmp)))))

;; Step 1: Add mark m to input
;;   Input: (swap[m] x[m] y[m])

;; Step 2: Match pattern
;;   a -> x[m]
;;   b -> y[m]

;; Step 3: Expand template (with substitutions)
;;   (let ((tmp a)) (set! a b) (set! b tmp))
;;
;;   Becomes:
;;   (let ((tmp x[m]) (set! x[m] y[m]) (set! y[m] tmp)))

;; Step 4: Add anti-mark m to result
;;   (let[m] ((tmp[m] x[m,m]) (set![m] x[m,m] y[m,m]) (set![m] y[m,m] tmp[m])))

;; Step 5: Cancel marks
;;   x[m,m] -> x[]    (marks cancel)
//   y[m,m] -> y[]    (marks cancel)
;;   tmp[m] -> tmp[m] (no cancellation - introduced identifier)
;;   let[m] -> let[m] (no cancellation - introduced identifier)

;; Result (conceptual):
;;   (let[m] ((tmp[m] x) (set![m] x y) (set![m] y tmp[m])))
;;
;; At evaluation time:
;;   - let[m] resolves to standard 'let' (special form)
;;   - tmp[m] resolves to fresh label L1
;;   - x resolves in user environment
;;   - y resolves in user environment
```

### Scope Sets Comparison

```racket
;; Same example with scope sets:

;; Input: (swap x y)
;;   x has scopes: {use-site}
;;   y has scopes: {use-site}

;; After macro expansion:
;;   (let ((tmp x)) (set! x y) (set! y tmp))
;;
;;   let has scopes:  {use-site, macro-intro}
;;   tmp has scopes:  {use-site, macro-intro}
;;   x has scopes:    {use-site}
;;   y has scopes:    {use-site}

;; At evaluation time:
;;   - let resolves: check for binding with scopes ⊇ {use-site, macro-intro}
;;     -> finds standard 'let' (has all scopes)
;;   - tmp resolves: check for binding with scopes ⊇ {use-site, macro-intro}
;;     -> creates fresh binding in inner let
;;   - x resolves: check for binding with scopes ⊇ {use-site}
;;     -> finds user's x
;;   - y resolves: check for binding with scopes ⊇ {use-site}
;;     -> finds user's y
```

**Key Difference**:
- Marks: temporal (when was identifier introduced)
- Scopes: spatial (where was identifier introduced)

Both achieve hygiene, but scope sets are more compositional and easier to reason about for complex cases like modules.

---

## Appendix B: Rust Type System Considerations

### Memory Management

```rust
// Marks and wraps need careful Rc handling

// Bad: Clone everything
fn expand_with_wrap(&self, template: &Template, wrap: Wrap) -> Value {
    // This clones wrap for every identifier!
    match template {
        Template::Symbol(id) => {
            Value::Syntax(Rc::new(SyntaxObject {
                datum: Box::new(Value::Symbol(id.clone())),
                wrap: wrap.clone(),  // Expensive!
                source: None,
            }))
        }
        // ...
    }
}

// Good: Share Rc<Wrap>
#[derive(Clone)]
pub struct Wrap(Rc<WrapInner>);

struct WrapInner {
    marks: Marks,
    ribs: Vec<Rib>,
}

fn expand_with_wrap(&self, template: &Template, wrap: &Wrap) -> Value {
    match template {
        Template::Symbol(id) => {
            Value::Syntax(Rc::new(SyntaxObject {
                datum: Box::new(Value::Symbol(id.clone())),
                wrap: wrap.clone(),  // Just clones Rc!
                source: None,
            }))
        }
        // ...
    }
}
```

### Performance Optimization

```rust
// Optimize mark comparison with Rc pointer equality

impl PartialEq for Mark {
    fn eq(&self, other: &Self) -> bool {
        // Fast path: pointer equality
        Rc::ptr_eq(&self.0, &other.0) ||
        // Slow path: value equality
        self.0 == other.0
    }
}

impl Hash for Mark {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash the ID, not the Rc pointer
        self.0.id.hash(state);
        self.0.kind.hash(state);
    }
}
```

### Error Handling

```rust
// Include wrap information in error messages

#[derive(Debug)]
pub enum EvalError {
    UnboundVariable {
        name: String,
        wrap: Option<Wrap>,
        source: Option<SourceLocation>,
    },
    // ...
}

impl Display for EvalError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            EvalError::UnboundVariable { name, wrap, source } => {
                write!(f, "Unbound variable: {}", name)?;
                if let Some(src) = source {
                    write!(f, " at {}", src)?;
                }
                if let Some(w) = wrap {
                    write!(f, "\n  Introduced by macro with {} marks",
                           w.marks.0.len())?;
                }
                Ok(())
            }
            // ...
        }
    }
}
```

---

## Appendix C: Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mark_cancellation() {
        let m1 = Mark::fresh_macro();
        let m2 = m1.anti_mark();

        let marks1 = Marks::default().add(m1.clone());
        let marks2 = Marks::default().add(m2);

        let joined = marks1.join(&marks2);
        assert_eq!(joined, Marks::default());
    }

    #[test]
    fn test_rib_lookup() {
        let mut rib = FixedRib::new();
        let sym = Rc::from("x");
        let marks = Marks::default();
        let label = Label::fresh_local();

        rib.add(sym.clone(), marks.clone(), label.clone());

        assert_eq!(rib.lookup("x", &marks), Some(&label));
        assert_eq!(rib.lookup("y", &marks), None);
    }

    #[test]
    fn test_wrap_join() {
        let w1 = Wrap::empty().add_mark(Mark::fresh_macro());
        let w2 = Wrap::empty().add_mark(Mark::fresh_macro());

        let joined = w1.join(&w2);
        assert_eq!(joined.marks.0.len(), 2);
    }
}
```

### Integration Tests

```rust
#[test]
fn test_basic_hygiene() {
    let interp = Interpreter::new();

    // Define swap macro
    interp.eval_str("
        (define-syntax swap
          (syntax-rules ()
            ((swap a b)
             (let ((tmp a))
               (set! a b)
               (set! b tmp)))))
    ").unwrap();

    // Test that macro's tmp doesn't capture
    let result = interp.eval_str("
        (let ((tmp 1) (x 2))
          (swap tmp x)
          tmp)
    ").unwrap();

    assert_eq!(result, Value::Integer(2));
}

#[test]
fn test_nested_macros() {
    let interp = Interpreter::new();

    interp.eval_str("
        (define-syntax when
          (syntax-rules ()
            ((when test body ...)
             (if test (begin body ...)))))

        (define-syntax unless
          (syntax-rules ()
            ((unless test body ...)
             (when (not test) body ...))))
    ").unwrap();

    let result = interp.eval_str("
        (let ((x 1))
          (unless (= x 0)
            (set! x 2))
          x)
    ").unwrap();

    assert_eq!(result, Value::Integer(2));
}
```

### R7RS Compliance Tests

Use the chibi-scheme r7rs-tests.scm as a reference:

```bash
#!/bin/bash
# Run hygiene-specific tests from chibi

./scripts/run_chibi_tests.sh --filter="hygiene|macro|syntax-rules"
```

Expected improvement:
- Current: ~60% of macro tests passing
- Target: ~95% of macro tests passing

---

*End of Document*
