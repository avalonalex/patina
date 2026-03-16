# R7RS-Large Macro Fascicle: Gap Analysis

**Source**: https://r7rs.org/large/fascicles/macro/1/macros-and-hygiene.html
**Date**: 2026-03-15
**Library**: `(r7rs-drafts macro-fascicle)` (temporary, for experimentation)

## Summary

The R7RS-large macro fascicle specifies a comprehensive macro system built on `syntax-case`, with additional systems (ER, IR macros) implementable on top. Patina currently has R7RS-small `syntax-rules` with scope-set hygiene. This document catalogs every feature in the fascicle and Patina's status.

## Feature Status

### Legend
- **Done** — Implemented and tested
- **PRD** — Design document exists, not yet implemented
- **Gap** — Not implemented, no design document
- **N/A** — Not applicable or derivable from other features

---

### Chapter 2: Syntax Transformation

| Feature | Status | Notes |
|---|---|---|
| `define-syntax` | Done | R7RS-small, fully working |
| `let-syntax` | Done | R7RS-small |
| `letrec-syntax` | Done | R7RS-small |
| `splicing-let-syntax` | Gap | Like `let-syntax` but splices definitions into enclosing scope |
| `splicing-letrec-syntax` | Gap | Like `letrec-syntax` but splices definitions into enclosing scope |
| `define-syntax-parameter` | Gap | Parameterizable syntax keywords |
| `syntax-parameterize` | Gap | Rebind syntax parameters within expansion |
| `make-variable-transformer` | Gap | Enable `(set! keyword val)` handling |
| `define-property` | Gap | Lexically-scoped identifier properties |
| `identifier-property` | Gap | Query identifier properties |
| Expansion process (left-to-right, two-pass) | Done | Desugarer handles this |
| Implicit phasing | Gap | Single-phase only; needs expand-time eval for `syntax-case` |

### Chapter 3: Syntax Objects

| Feature | Status | Notes |
|---|---|---|
| `identifier?` | Gap | Predicate on syntax objects |
| `identifier-defined?` | Gap | Check if identifier has a binding |
| `generate-identifier` | Gap | Create fresh identifier (optional name) |
| `generate-temporaries` | Gap | Create list of fresh identifiers |
| `bound-identifier=?` | Gap | Same binding? (scope-set equality) |
| `free-identifier=?` | Gap | Same resolved binding? |
| `symbolic-identifier=?` | Gap | Same symbolic name? (trivial) |
| `quote-syntax` | Gap | Like `syntax` but no pattern variable substitution |
| `unwrap-syntax` | Gap | Shallow unwrap (one level of structure) |
| `syntax->datum` | Gap | Recursive strip of all syntax info |
| `datum->syntax` | Gap | Wrap datum with context from identifier |
| `syntax-error` | Done | R7RS-small, in `(scheme base)` |
| `erroneous-syntax` | Gap | Returns always-error transformer |

### Chapter 4: The syntax-case System

| Feature | Status | Notes |
|---|---|---|
| `syntax-case` | PRD | Core procedural macro form |
| `syntax` (template form / `#'`) | PRD | Template with pattern variable substitution |
| `quasisyntax` / `#`` | PRD | Template with `unsyntax` escapes |
| `unsyntax` / `#,` | PRD | Evaluate expression within quasisyntax |
| `unsyntax-splicing` / `#,@` | PRD | Splice list within quasisyntax |
| `with-syntax` | PRD | Bind pattern variables for templates |
| `custom-ellipsis` | Gap | Rename `...` within syntax-case/syntax-rules |
| Fenders (guards) | PRD | Optional predicate on syntax-case clauses |
| Tail patterns in patterns | PRD | `(p1 ... p2 p3)` — see TAIL_PATTERNS.md |

### Chapter 5: The syntax-rules System

| Feature | Status | Notes |
|---|---|---|
| `syntax-rules` (R7RS-small) | Done | Full compliance |
| `_` as wildcard | Done | |
| Ellipsis `...` | Done | Single level |
| Nested ellipsis `((x ...) ...)` | Done | |
| Tail patterns after `...` | PRD | See TAIL_PATTERNS.md |
| Multiple `...` after subtemplate | PRD | See CONSECUTIVE_ELLIPSES.md |
| Custom ellipsis in syntax-rules | Gap | `(syntax-rules custom-ellipsis (lit ...) ...)` |
| `identifier-syntax` | Gap | Identifier macros (read-only and `set!` forms) |

### Chapter 6: Other Macro Systems

| Feature | Status | Notes |
|---|---|---|
| `er-macro-transformer` | Gap | Explicit renaming — implementable as macro over syntax-case |
| `ir-macro-transformer` | Gap | Implicit renaming — implementable as macro over syntax-case |

---

## Detailed Gap Descriptions

### Tier 1: Small, Independent Additions

These can be implemented without `syntax-case`.

#### `erroneous-syntax`

```scheme
(erroneous-syntax)
(erroneous-syntax message)
```

Returns a transformer that always signals a syntax violation. Used for auxiliary keywords like `else`, `=>`.

**Effort**: ~20 lines. Create a transformer that raises an error with the given message.

**Example**:
```scheme
(define-syntax => (erroneous-syntax))
(define-syntax else (erroneous-syntax))
```

#### `custom-ellipsis` in `syntax-rules`

```scheme
(syntax-rules ::: ()
  ((foo a ::: b) (list a ::: b)))
```

The first argument to `syntax-rules` can be an identifier that replaces `...` as the ellipsis within that macro's patterns and templates. Already supported in the fascicle's `syntax-rules` spec.

**Effort**: ~50-100 lines. Thread a custom ellipsis identifier through the pattern/template compiler instead of hardcoding `...`.

#### `splicing-let-syntax` / `splicing-letrec-syntax`

```scheme
(let ((x 21))
  (splicing-let-syntax
      ((def (syntax-rules ()
              ((def stuff ...) (define stuff ...)))))
    (def foo 42))
  foo)  ;; → 42 (foo is visible in enclosing scope!)
```

Unlike `let-syntax`, definitions produced by macros within the body escape into the enclosing scope. The fascicle shows that `let-syntax` and `letrec-syntax` are derivable from the splicing variants:

```scheme
(define-syntax let-syntax
  (syntax-rules ()
    ((_ bindings body_0 body_1 ...)
     (splicing-let-syntax bindings
       (let () body_0 body_1 ...)))))
```

**Effort**: ~100-150 lines. Modify the desugarer to handle the splicing semantics — expand macro body forms and splice results into the enclosing definition context.

### Tier 2: Require Syntax Objects

These depend on having first-class syntax objects (Phase 1 of SYNTAX_CASE_DESIGN.md).

#### `identifier?`

```scheme
(identifier? #'x)    ;; → #t
(identifier? 'x)     ;; → #f
(identifier? #'(x))  ;; → #f
```

Predicate: is this a wrapped syntax object containing an identifier?

#### `quote-syntax`

```scheme
(quote-syntax datum)
```

Like `quote` but produces a syntax object retaining hygiene information. Unlike `syntax` (`#'`), does NOT substitute pattern variables.

```scheme
(let-syntax ((car (lambda (x) (quote-syntax car))))
  ((car) '(0)))  ;; → 0 (refers to the real car)
```

#### `unwrap-syntax`

```scheme
(unwrap-syntax stx)
```

Shallow unwrap — peels one layer of syntax object structure. If `stx` is a wrapped pair, returns a pair whose car and cdr are still syntax objects. If `stx` is an identifier, returns it unchanged.

Contrast with `syntax->datum` which recursively strips ALL syntax info.

#### `identifier-defined?`

```scheme
(identifier-defined? #'x)                ;; → #f (unbound)
(let ((x 1)) (identifier-defined? #'x))  ;; → #t
```

Returns whether the identifier has a binding in its lexical environment.

#### `symbolic-identifier=?`

```scheme
(symbolic-identifier=? id1 id2)
```

Compare symbolic names only, ignoring scopes. Equivalent to:
```scheme
(define (symbolic-identifier=? id1 id2)
  (symbol=? (syntax->datum id1) (syntax->datum id2)))
```

#### `generate-identifier`

```scheme
(generate-identifier)         ;; fresh identifier with unspecified name
(generate-identifier 'temp)   ;; fresh identifier named "temp"
```

The returned identifier is guaranteed unique (not `bound-identifier=?` to any existing identifier).

### Tier 3: Require syntax-case

#### `make-variable-transformer`

Wraps a transformer procedure to also handle `(set! keyword datum)` forms:

```scheme
(define-syntax used-as
  (make-variable-transformer
   (lambda (stx)
     (syntax-case stx (set!)
       (id (identifier? #'id) #'(quote reference))
       ((set! _ value) #'(quote (assignment value)))
       ((_ . operands) #'(quote (combination . operands)))))))
```

**Effort**: Small wrapper — tag the transformer and check in the expander's `set!` handling.

#### `identifier-syntax`

Syntactic sugar for common variable-transformer patterns:

```scheme
;; Read-only identifier macro
(identifier-syntax template)

;; Read-write identifier macro
(identifier-syntax
  (id template1)
  ((set! id pattern) template2))
```

**Effort**: Implementable as a macro over `syntax-rules` + `make-variable-transformer`.

#### `define-syntax-parameter` / `syntax-parameterize`

Syntax parameters allow macros to rebind auxiliary keywords within their expansion:

```scheme
(define-syntax-parameter return
  (erroneous-syntax "return used outside of lambda^"))

(define-syntax lambda^
  (syntax-rules ()
    ((lambda^ formals body ...)
     (lambda formals
       (call-with-current-continuation
        (lambda (escape)
          (syntax-parameterize
              ((return (identifier-syntax escape)))
            body ...)))))))

(lambda^ (x) (if (< x 0) (return 'negative) (* x x)))
```

**Key difference from `let-syntax`**: `syntax-parameterize` adjusts (not shadows) the binding, so uses of `return` within the body see the new transformer even if `return` was used in a macro that expanded before the parameterization.

**Effort**: Medium. Requires a "parameterizable" flag on syntax bindings and a mechanism to dynamically adjust them during expansion.

#### `define-property` / `identifier-property`

Associate metadata with identifiers at expand time:

```scheme
(define-syntax documentation
  (erroneous-syntax "documentation is only a property key"))

(define (my-function x) (* x x))
(define-property my-function documentation "Squares its argument")

;; In a macro transformer:
(identifier-property #'my-function #'documentation)  ;; → "Squares its argument"
```

Properties are lexically scoped, follow imports/exports, and use `free-identifier=?` for key matching.

**Effort**: Medium. Requires a property table on the environment/scope system.

#### `er-macro-transformer` / `ir-macro-transformer`

The fascicle provides implementations of both as macros over `syntax-case`:

```scheme
;; Explicit renaming
(define-syntax er-macro-transformer
  (lambda (stx)
    (syntax-case stx ()
      ((k proc-expr)
       #'(let ((proc proc-expr))
           (lambda (stx)
             (syntax-case stx ()
               ((m . _)
                (rewrap #'m
                        (proc (unwrap stx)
                              (make-rename #'k)
                              (make-compare #'m)))))))))))

;; Implicit renaming — identical but swaps k/m in rewrap and make-rename
(define-syntax ir-macro-transformer
  (lambda (stx)
    (syntax-case stx ()
      ((k proc-expr)
       #'(let ((proc proc-expr))
           (lambda (stx)
             (syntax-case stx ()
               ((m . _)
                (rewrap #'k
                        (proc (unwrap stx)
                              (make-rename #'m)
                              (make-compare #'m)))))))))))
```

**Effort**: Small — just Scheme library code once `syntax-case` + `datum->syntax` exist.

---

## Phasing Requirements

The fascicle specifies **implicit phasing**: transformer expressions evaluate at expand time (phase N+1 for macros defined at phase N). Patina currently doesn't have phased evaluation.

For `syntax-case`, the transformer is an arbitrary Scheme expression:
```scheme
(define-syntax my-macro
  (lambda (stx)           ;; This lambda runs at EXPAND TIME
    (syntax-case stx ()
      ...)))
```

This requires the ability to evaluate Scheme code during macro expansion — which the tree-walker already does (the desugarer calls the evaluator for `syntax-rules`). For `syntax-case`, the same mechanism extends to arbitrary procedures.

The VM backend will need a way to call Scheme closures during compilation, or fall back to the tree-walker for expand-time evaluation.

---

## Dependency Graph

```
                    erroneous-syntax (standalone)
                    custom-ellipsis (standalone)
                    tail patterns (standalone)
                    consecutive ellipses (standalone)
                           │
                    splicing-let/letrec-syntax (standalone)
                           │
              ┌────────────┴────────────┐
              ▼                         ▼
      Syntax Objects              identifier-syntax
     (identifier?,                (read-only form,
      quote-syntax,                standalone)
      unwrap-syntax,                    │
      syntax->datum,                    │
      datum->syntax,                    │
      generate-identifier,             │
      generate-temporaries,            │
      bound-identifier=?,             │
      free-identifier=?,              │
      symbolic-identifier=?,         │
      identifier-defined?)           │
              │                       │
              ▼                       │
         syntax-case ◄───────────────┘
        (+ syntax template,    identifier-syntax
         fenders,               (set! form needs
         with-syntax)           make-variable-transformer)
              │
              ├── quasisyntax / unsyntax / unsyntax-splicing
              │
              ├── make-variable-transformer
              │   └── identifier-syntax (full, with set!)
              │
              ├── define-syntax-parameter / syntax-parameterize
              │
              ├── define-property / identifier-property
              │
              └── er-macro-transformer / ir-macro-transformer
                  (pure Scheme, no Rust changes)
```

---

## References

- **R7RS-large macro fascicle**: https://r7rs.org/large/fascicles/macro/1/macros-and-hygiene.html
  - Ch. 2: Syntax transformation — https://r7rs.org/large/fascicles/macro/1/syntax-transformation.html
  - Ch. 3: Syntax objects — https://r7rs.org/large/fascicles/macro/1/syntax-objects.html
  - Ch. 4: syntax-case — https://r7rs.org/large/fascicles/macro/1/syntax-case-system.html
  - Ch. 5: syntax-rules — https://r7rs.org/large/fascicles/macro/1/syntax-rules-system.html
  - Ch. 6: Other macro systems — https://r7rs.org/large/fascicles/macro/1/other-macro-systems.html
  - Editor's intro — https://r7rs.org/large/fascicles/macro/1/editors-intro.html
- **Chez Scheme** `s/syntax.ss` — reference implementation (10,558 lines)
- **Racket** `src/expander/` — scope-set based expander
- **psyntax** — portable syntax-case: https://www.scheme.com/syntax-case/
- **SRFI-139** — Syntax parameters
- **SRFI-188** — Splicing binding constructs
- **SRFI-213** — Identifier properties
