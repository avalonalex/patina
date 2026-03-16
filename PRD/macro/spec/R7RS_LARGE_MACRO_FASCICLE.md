# The Macrological Fascicle — R7RS Large

**Source**: https://r7rs.org/large/fascicles/macro/1/
**Retrieved**: 2026-03-15
**Library**: `(r7rs-drafts macro-fascicle)` (temporary, for experimentation)

> This is a local reference copy of the R7RS-large macro fascicle specification.
> For the authoritative version, see the URL above.

---

## Table of Contents

1. [Editor's Introduction](#1-editors-introduction)
2. [Editorial Conventions](#2-editorial-conventions)
3. [Macros and Hygiene](#3-macros-and-hygiene)
4. [Syntax Transformation](#4-syntax-transformation)
5. [Syntax Objects](#5-syntax-objects)
6. [The syntax-case System](#6-the-syntax-case-system)
7. [The syntax-rules System](#7-the-syntax-rules-system)
8. [Other Macro Systems](#8-other-macro-systems)
9. [Acknowledgements](#9-acknowledgements)
10. [References](#10-references)

---

## 1. Editor's Introduction

This fascicle explains the entire macro system of R7RS Large, building on six decades of research. It progresses from theoretical hygiene models through syntax object implementation to high-level systems.

### Key Refinements Since R4RS

- **Multi-phase evaluation**: Libraries enable helper procedures in macro transformers
- **Unified systems**: `syntax-case` merges high-level and low-level syntax manipulation
- **Flexible macro uses**: Identifiers can appear outside operator positions
- **Extended patterns**: Both `syntax-rules` and `syntax-case` match additional pattern types
- **New features**: Identifier properties and syntax parameters (R7RS Large innovations)

### What Implementations Need to Do

**`syntax-case` adoption**: Implementations which do not support `syntax-case` as specified by the R6RS will need to adopt it, either by replacing expanders completely or adapting existing ones. Complete replacement is "generally the easier approach in practice."

**Phasing changes**: Switch from explicit to implicit phasing. Ignore phase declarations on imports and allow uses of syntax defined in previous phases.

**Pattern matcher extensions**: Extend to support renaming the ellipsis and using ellipsis and underscore as pattern literals.

**New feature support**: Add expander support for lexically-scoped identifier properties and syntax parameters.

**R4RS low-level procedures**: Adds R4RS procedures lacking R6RS equivalents, including `quote-syntax`. Some are implementable portably in terms of R6RS higher-level constructs.

**Temporary library**: `(r7rs-drafts macro-fascicle)` contains all fascicle bindings for experimentation only. Production code should not depend on this library.

### Changes from Source Texts

- R6RS hygiene model expressed in different terms and in more detail
- Low-level R4RS procedures restored, enabling `syntax-case` implementation as derived syntax
- Identifier property behavior nearly completely respecified to match existing implementations
- New `identifier-defined?` procedure for detecting identifier bindings
- Explicit phasing provisions dropped; implicit phasing behavior specified
- Expansion process adapted for new macro features
- Clarification that `syntax-case` and `syntax-rules` use `free-identifier=?` for input form literals but `bound-identifier=?` for pattern literals
- Updated rules for syntax object wrapping/unwrapping from `syntax` expressions
- New `custom-ellipsis` form for ellipsis renaming in `syntax-case`
- `syntax-rules` now specified via `syntax-case` semantics, allowing multiple ellipses after single pattern variables
- New `erroneous-syntax` declarative form for error-signaling macros

---

## 2. Editorial Conventions

The phrase "it is an error" has been retired. Three-way distinction:

- **Undefined behaviour**: Equivalent to "it is an error" in R7RS-small and R6RS. Implementation may behave in any way.
- **Unspecified behaviour**: Report allows implementations to choose one of several explicitly allowed behaviours.
- **Implementation-specified behaviour**: Implementations may choose but must document their choice.

"An error is signalled" means an exception is required to be raised.

"It is a syntax violation" means an exception is raised with condition type `&syntax` as in R6RS.

**Variable naming conventions**:

| Variable | Type |
|---|---|
| *id* | identifier |
| *proc* | procedure |
| *stx* | syntax object |
| *symbol* | symbol |

The key words "must", "must not", "required", "shall", "shall not", "should", "should not", "recommended", "may", and "optional" are to be interpreted as described in RFC 2119.

---

## 3. Macros and Hygiene

Scheme programs and libraries can define and use new derived expression types called macros. A macro definition binds an identifier — the keyword or syntax keyword — to a transformer. The transformer analyzes the macro use form and transcribes it into a more primitive expression.

This report defines two related high-level systems for writing transformers: `syntax-case` and `syntax-rules`. Within these systems, user-defined macros are "by default 'hygienic' and 'referentially transparent' and thus preserve Scheme's lexical scoping."

### 3.1 Defining Hygiene

Two key properties:

1. If a macro transformer inserts a binding for an identifier, the identifier is effectively renamed throughout its scope to avoid conflicts with other identifiers.

2. If a macro transformer inserts a free reference to an identifier, that reference refers to the binding visible where the transformer was specified, regardless of local bindings surrounding the macro use.

Kohlbecker et al. (1986) propose the hygiene condition for macro expansion: "Generated identifiers that become binding instances in the completely expanded program must only bind variables that are generated at the same transcription step."

The hygiene condition restates as:

> A binding for an identifier introduced into the output of a transformer call from the expander must capture only references to the identifier introduced into the output of the same transformer call. A reference to an identifier introduced into the output of a transformer refers to the closest enclosing binding for the introduced identifier or, if it appears outside of any enclosing binding for the introduced identifier, the closest enclosing lexical binding where the identifier appears inside the transformer body or one of the helpers it calls.

### 3.2 Modelling Hygiene

*Note: This section describes a possible operational semantics. Implementations need not adopt this exact model, provided their model respects the hygiene condition.*

During macro expansion, Scheme code is represented as datums wrapped in hygienic context. This context propagates to each individual symbol in the datum tree, ultimately wrapping each. A single symbol with context is an identifier.

The hygienic context in a wrap consists of two components:

- **History**: a set of time-stamps tracking when identifiers were introduced during expansion
- **Lexical environment**: tracking identifiers' bindings and lexical addresses

Time-stamps are of two types: those recording transformer entry and exit during single macro transcription steps.

When the expander encounters a macro use, it:
1. Adds a new time-stamp to the macro use wrap's history (recording step beginning)
2. Calls the transformer
3. Adds a time-stamp recording step end to every wrap in the returned syntax object

Identifiers present in the macro use input form carry both beginning and end time-stamps. Transformer-introduced identifiers carry only the end time-stamp. When comparing histories, both beginning and end time-stamps for the same step are ignored and may be discarded.

Identifiers have two name kinds:
- **Symbolic name**: the symbol datum wrapped by the identifier
- **Time-stamped name**: the symbolic name plus the history

In most contexts, Scheme identifiers are treated as if their name were the time-stamped name.

---

## 4. Syntax Transformation

### 4.1 Transformers

Syntax keywords are bound by user code to transformers. Most transformers are ordinary Scheme procedures receiving exactly one syntax object argument and returning one syntax object representing the expansion result.

It is undefined behaviour to re-enter the dynamic extent of a call to a transformer by the expander after it has returned once.

Variable transformers represent another transformer category (section 4.4).

### 4.2 Syntax Definition and Binding Forms

#### `define-syntax`

```
(define-syntax keyword transformer-expression)
```

Binds syntax keywords similarly to how `define` binds variables. The transformer expression evaluates at expand time to produce a transformer. The keyword becomes visible throughout the defining body unless shadowed.

**Example 1**:
```scheme
(let ()
  (define even?
    (lambda (x)
      (or (= x 0) (odd? (- x 1)))))
  (define-syntax odd?
    (syntax-rules ()
      ((odd? x) (not (even? x)))))
  (even? 10))
⇒ #t
```

**Example 2** (left-to-right processing):
```scheme
(let ()
  (define-syntax bind-to-zero
    (syntax-rules ()
      ((bind-to-zero id) (define id 0))))
  (bind-to-zero x)
  x)
⇒ 0
```

#### `splicing-let-syntax`

```
(splicing-let-syntax ((keyword transformer-expression) ...)
  definition-or-expression ...)
```

Bindings have form `((keyword transformer-expression) ...)`. It is a violation if the same identifier appears as keyword more than once.

Forms expand in a syntactic environment containing the base environment plus new bindings. Definition and expression forms are treated as wrapped in implicit `begin`, so expanded definitions have the same extent as a definition in the form's place — they **splice into the enclosing scope**.

**Example**:
```scheme
(let ((x 21))
  (splicing-let-syntax
      ((def (syntax-rules ()
              ((def stuff ...) (define stuff ...)))))
    (def foo 42))
  foo)
⇒ 42
```

#### `splicing-letrec-syntax`

```
(splicing-letrec-syntax ((keyword transformer-expression) ...)
  definition-or-expression ...)
```

Same as `splicing-let-syntax` but transformer expressions evaluate in an environment containing the keywords themselves, enabling transformers to transcribe forms into macro uses. It is undefined behaviour if the evaluation of any transformer expression requires knowledge of the actual transformer bound to one of the keywords.

#### `let-syntax`

```
(let-syntax ((keyword transformer-expression) ...) body)
```

Extends the syntactic environment like `splicing-let-syntax`, but creates a new lexical body not spliced into surrounding context — definitions within remain internal.

**Example** (contrast with splicing version):
```scheme
(let ((x 21))
  (let-syntax
      ((def (syntax-rules ()
              ((def stuff ...) (define stuff ...)))))
    (def foo 42))
  foo)
⇒ 21
```

**Implementation** (derivable from splicing variant):
```scheme
(define-syntax let-syntax
  (syntax-rules ()
    ((_ bindings body_0 body_1 ...)
     (splicing-let-syntax bindings
       (let () body_0 body_1 ...)))))
```

#### `letrec-syntax`

```
(letrec-syntax ((keyword transformer-expression) ...) body)
```

Extends the environment like `splicing-letrec-syntax`, but creates a new lexical body (not spliced).

**Example**:
```scheme
(letrec-syntax
    ((xor
      (syntax-rules ()
        ((_) #f)
        ((_ e)
         (if e #t #f))
        ((_ e_1 e_2 ...)
         (let ((temp e_1))
           (if temp
               (not (or e_2 ...))
               (xor e_2 ...)))))))
  (values (xor #t #f #f)
          (xor #t #t #f)))
⇒ #t #f
```

**Implementation**:
```scheme
(define-syntax letrec-syntax
  (syntax-rules ()
    ((_ bindings body_0 body_1 ...)
     (splicing-letrec-syntax bindings
       (let () body_0 body_1 ...)))))
```

### 4.3 Syntax Parameters

Syntax parameters enable rebinding macro definitions within a macro expansion's dynamic extent, providing solutions for unhygienic bindings by adjusting existing keywords rather than introducing new ones.

#### `define-syntax-parameter`

```
(define-syntax-parameter keyword transformer-expression)
```

Binds keyword as a parameterizable syntax keyword using the transformer from evaluating the expression at expand time as the default. Outside `syntax-parameterize` context, behaves identically to `define-syntax`, but the binding is marked as parameterizable.

#### `syntax-parameterize`

```
(syntax-parameterize ((keyword transformer-expression) ...) body)
```

Adjusts keywords to use transformers from evaluating corresponding expressions during body expansion. A violation occurs if any keyword refers to non-parameterizable bindings. Unlike `let-syntax`, the binding is adjusted (not shadowed), so uses within body expansion employ new transformers.

**Example** (early-return mechanism):
```scheme
(define-syntax-parameter return
  (erroneous-syntax "return used outside of lambda^"))

(define-syntax lambda^
  (syntax-rules ()
    ((lambda^ formals body_0 body_1 ...)
     (lambda formals
       (call-with-current-continuation
        (lambda (escape)
          (syntax-parameterize
              ((return (identifier-syntax escape)))
            body_0 body_1 ...)))))))
```

### 4.4 Variable Transformers

Variable transformers declare that a procedure processes macro uses of the form `(set! keyword datum)`. An attempt to expand such a form whose transformer is not a variable transformer is a syntax violation.

#### `make-variable-transformer`

```
(make-variable-transformer proc)
```

Wraps procedure `proc` in a variable transformer container. When a syntax keyword binds to the result, that transformer procedure processes all macro uses including `set!` forms where the keyword is the left-hand side.

**Example**:
```scheme
(define-syntax used-as
  (make-variable-transformer
   (lambda (stx)
     (cond ((identifier? stx)
            (quote-syntax (quote reference)))
           ((free-identifier=? (car (unwrap-syntax stx)) #'set!)
            `(,(quote-syntax cons)
              ,(quote-syntax (quote assignment))
              (,(quote-syntax quote)
               ,(cdr (unwrap-syntax
                      (cdr (unwrap-syntax stx)))))))
           (else
            `(,(quote-syntax cons) ,(quote-syntax (quote combination))
                                   (,(quote-syntax quote)
                                   ,(cdr (unwrap-syntax stx)))))))))
```

Usage:
- `used-as` → `reference`
- `(set! used-as x)` → `(assignment x)`
- `(used-as y)` → `(combination y)`

### 4.5 Identifier Properties

During expansion, properties associate with each identifier, allowing arbitrary information binding useful for macro transformers. Properties form key-value pairs where keys are identifiers.

**Scope rules**: When identifier bindings form, they connect to an empty property set. Properties belong only to their defining lexical scope. Properties shadow prior ones on the same identifier with identical keys in containing contexts. Imported identifiers carry property copies from their origin library.

Re-exported identifiers carry properties as (re-)defined in the re-exporting library. If the same binding imports from multiple libraries with differing property values for the same key, an import error occurs.

#### `define-property`

```
(define-property identifier key expression)
```

Both identifier and key must be bound identifiers. The expression evaluates at expand time, producing a value. An identifier property associates on the identifier linking the key to this value.

#### `identifier-property`

```
(identifier-property id key)
(identifier-property id key default)
```

Returns the identifier property on `id` whose key binds identically to `key`. Returns `default` (or `#f` if absent) if no property exists. Signals a violation if `id` or `key` is unbound.

Callable only within a transformer call's dynamic extent by the expander. Behavior outside this context remains unspecified.

Note: Two identifiers which share the same binding will not necessarily have the same identifier properties: `free-identifier=?` is used to match identifier keys but not the identifiers themselves.

**Example**:
```scheme
(import (scheme base)
        (rename (only (scheme base) cons) (cons make-pair)))

(define-syntax renamed?
  (erroneous-syntax "only an identifier property key"))
(define-property make-pair renamed? #t)

(define-syntax both-renamed?
  (lambda (stx)
    (and (identifier-property #'cons #'renamed?)
         (identifier-property #'make-pair #'renamed?))))

(values (free-identifier=? #'cons #'make-pair)
        (both-renamed?))
⇒ #t #f
```

### 4.6 Expansion Process

The expander processes body forms left-to-right. Processing depends on form type:

- **macro use**: Invokes the transformer, recursively processes the result.
- **`define-syntax` or `define-syntax-parameter`**: Expands and evaluates the right-hand expression, binds the keyword to the transformer.
- **`define`**: Records the identifier as a variable, defers right-hand-side expansion until after all body forms process.
- **`define-property`**: Expands and evaluates the value expression, creates or replaces a property for the key on the identifier.
- **`begin`**: Splices subforms into the body form list.
- **`splicing-let-syntax` or `splicing-letrec-syntax`**: Splices inner body forms into the outer form list, scoping keywords to inner forms.
- **expression** (nondefinition): Defers expansion until after all body forms process.

After processing the rightmost form, the expander makes a second pass over deferred right-hand sides and nondefinitions.

Note: This algorithm does not directly reprocess any form. It requires a single left-to-right pass over the definitions followed by a single pass (in any order) over the body expressions and deferred right-hand sides.

**Undefined behavior examples**:
```scheme
(define define 3)
(begin (define begin list))
(display (+ x 16))
(define x 32)
```

**Valid example** (not undefined):
```scheme
(define (make-counter)
  (define (increase)
    (set! value (+ value 1))
    value)
  (define value 0)
  increase)
```

### 4.7 Phases of Evaluation and Macro Expansion

Macro transformer evaluation occurs in distinct phases identified by non-negative integers. If a macro definition appears in phase n, its right-hand expression evaluates in phase n + 1. Program evaluation and expansion after all syntax keywords are defined occurs in phase 0; thus top-level transformer expressions evaluate in phase 1.

The earliest phase contains all imported bindings, available across all phases. Syntactic bindings created during expansion remain available at all phases within their visible scopes. Variable bindings exist only in their creation phase. It is undefined behaviour to attempt to access or rebind an identifier which is a variable defined in a different phase.

The R6RS provision for explicit control of imported binding availability at particular phases has been removed.

---

## 5. Syntax Objects

Syntax objects are the means by which the hygiene model is implemented in the Scheme language. They enable macros to obtain information about invocation forms and produce output while maintaining referential transparency.

A syntax object may be:
- A pair of syntax objects
- A vector of syntax objects
- A datum (neither symbol, pair, nor vector)
- A wrapped syntax object

The distinction between "syntax object" and "wrapped syntax object" is important.

### 5.1 Identifiers

#### `identifier?`

```
(identifier? obj) → boolean
```

Returns `#t` if obj represents an identifier (wrapped syntax object), `#f` otherwise.

```scheme
(identifier? #'x)    ⇒ #t
(identifier? 'x)     ⇒ #f
(identifier? #'(x))  ⇒ #f
```

Syntax objects representing identifiers are always wrapped. A symbol which is not wrapped is never a valid syntax object.

#### `identifier-defined?`

```
(identifier-defined? id) → boolean
```

Returns `#t` if the given identifier has a binding, `#f` otherwise.

Operationally, `identifier-defined?` returns `#t` if the given identifier has a lexical address associated with it within its lexical environment.

```scheme
(identifier-defined? #'identifier-defined?)  ⇒ #t
(identifier-defined? #'x)                    ⇒ #f  ; assuming x undefined
(let ((x 1)) (identifier-defined? #'x))     ⇒ #t
```

#### `generate-identifier`

```
(generate-identifier) → identifier
(generate-identifier symbol) → identifier
```

Returns a new identifier. The optional symbol argument specifies the symbolic name. The returned identifier is guaranteed not to be `bound-identifier=?` to any existing identifier.

Operationally, `(generate-identifier symbol)` returns a new wrapped syntax object wrapping the symbol with a history containing a time-stamp.

#### `generate-temporaries`

```
(generate-temporaries list-stx) → list of identifiers
```

`list-stx` must represent a list-structured form. Returns a list of generated identifiers with the same length as the input list. Each generated identifier is subject to the same requirements as `generate-identifier` when called without an argument.

#### `bound-identifier=?`

```
(bound-identifier=? id1 id2) → boolean
```

Returns `#t` if a binding for one would capture a reference to the other in transformer output (assuming reference appears within binding scope), `#f` otherwise.

In general, two identifiers are `bound-identifier=?` only if both are present in the original program or both are introduced by the same transformer application.

```scheme
(bound-identifier=? #'x #'x)  ⇒ #t
(bound-identifier=? #'x #'y)  ⇒ #f
(bound-identifier=? (generate-identifier 'x) (generate-identifier 'x))  ⇒ #f
```

#### `symbolic-identifier=?`

```
(symbolic-identifier=? id1 id2) → boolean
```

Returns `#t` if both identifiers have the same symbolic name, `#f` otherwise.

```scheme
(symbolic-identifier=? (generate-identifier 'x) (generate-identifier 'x))  ⇒ #t
(symbolic-identifier=? #'x #'y)  ⇒ #f
```

**Implementation**:
```scheme
(define (symbolic-identifier=? id_1 id_2)
  (symbol=? (syntax->datum id_1)
            (syntax->datum id_2)))
```

#### `free-identifier=?`

```
(free-identifier=? id1 id2) → boolean
```

Returns `#t` if both identifiers would refer to the same lexical binding when inserted as free identifiers in transformer output. If either id is not lexically bound, returns `#t` if they are `symbolic-identifier=?`.

Operationally, `free-identifier=?` returns `#t` if id1 and id2 map to the same lexical address within their respective lexical environments, or if neither maps to any lexical address and their symbolic names are the same.

```scheme
(import (scheme base)
        (rename (scheme base) (else otherwise)))
(free-identifier=? #'else #'otherwise)  ⇒ #t

(free-identifier=? #'else #'=>)  ⇒ #f
(free-identifier=? (generate-identifier 'x) (generate-identifier 'x))  ⇒ #t
(free-identifier=? #'x (generate-identifier 'x))  ⇒ #t
(let ((x 1)) (free-identifier=? #'x (generate-identifier 'x)))  ⇒ #f
```

### 5.2 Wrapped Syntax Objects

#### `quote-syntax`

```
(quote-syntax syntactic-datum)
```

`quote-syntax` is the syntactic analogue of `quote`. It evaluates to a syntax object representation of the syntactic datum which retains hygiene information for identifiers.

The result is suitable for inclusion in the expansion of a macro use.

```scheme
(symbol? (quote-syntax x))  ⇒ #f
(identifier? (unwrap-syntax (quote-syntax x)))  ⇒ #t

(let-syntax ((car (lambda (x) (quote-syntax car))))
  ((car) '(0)))
⇒ 0

(let-syntax
    ((quote-foo
      (lambda (stx)
        (quote-syntax (quote foo)))))
  (let ((quote (lambda (x) 'bar)))
    (quote-foo)))
⇒ foo
```

#### `unwrap-syntax`

```
(unwrap-syntax stx) → syntax-object
```

Unwraps the immediate datum structure from the syntax object `stx`, leaving nested syntax structure in place, without stripping any syntactic information from identifiers.

Operationally, if `stx` is an identifier or is not a wrapped syntax object, it is returned unchanged. Otherwise converts the outermost structure of `stx` into a data object.

```scheme
(identifier? (unwrap-syntax (quote-syntax x)))  ⇒ #t
(identifier? (cdr (unwrap-syntax (quote-syntax (x . y)))))  ⇒ #t
(identifier? (cdr (unwrap-syntax (quote-syntax (x y z)))))  ⇒ #f
```

#### `syntax->datum`

```
(syntax->datum stx) → datum
```

Strips all syntactic information from the syntax object `stx` and returns the corresponding Scheme datum. Identifiers stripped in this manner are converted to their symbolic names.

The result must not be and must not contain any wrapped syntax objects.

This procedure irrevocably deletes hygiene information: `syntax->datum` and `datum->syntax` cannot, in general, round-trip cleanly.

```scheme
(symbol? (syntax->datum #'x))  ⇒ #t
(syntax->datum (quote-syntax (quote #1=(a . #1#))))  ⇒ (quote #1=(a . #1#))
```

#### `datum->syntax`

```
(datum->syntax context-id datum) → syntax-object
```

`context-id` must be an identifier; `datum` should be a datum value. Returns a syntax object representation of datum containing the same contextual information as context-id. The syntax object behaves as if it were introduced into the code when `context-id` was introduced.

Operationally, creates a new wrapped syntax object which wraps `datum` and copies its hygiene information from `context-id`.

**Example** (macro providing early return under name `return`):
```scheme
(define-syntax with-return
  (lambda (stx)
    (let ((return-id
           (datum->syntax (car (unwrap-syntax stx)) 'return))
          (body (cdr (unwrap-syntax stx))))
      `(,(quote-syntax call-with-current-continuation)
         (,(quote-syntax lambda) (,return-id) . ,body)))))

(define (find-odd ls)
  (with-return
    (for-each
     (lambda (n) (if (odd? n) (return n)))
     ls)))

(find-odd '(6 2 8 3 1 8))
⇒ 3
```

Demonstrating hygiene preservation:
```scheme
(define-syntax suppress-exceptions
  (syntax-rules ()
    ((_ body_0 body_1 ...)
     (with-return
       (with-exception-handler
           (lambda (e) (return #f))
         (lambda () body_0 body_1 ...))))))

(suppress-exceptions (raise 'oops))
⇒ #f

(let ((return (lambda (ignored) #t)))
  (suppress-exceptions
    (return #f)))
⇒ #t
```

---

## 6. The syntax-case System

The `syntax-case` system enables writing low-level macros in a high-level style.

### 6.1 Pattern Variables

Pattern variables serve as the unifying concept of both `syntax-case` and `syntax-rules`. They occupy the same namespace as regular variables and syntax keywords, with identical shadowing rules. The value of pattern variables cannot be changed after they have been bound.

Unlike standard variables, pattern variables may bind to a sequence of multiple values, or any nesting of sequences of multiple values. Nesting depth is statically determined by the pattern structure, though at runtime each sequence may be empty or contain a single value.

### 6.2 Parsing Input: `syntax-case`

```
(syntax-case expression (pattern-literal ...) syntax-case-clause ...)
(syntax-case custom-ellipsis-clause expression (pattern-literal ...) syntax-case-clause ...)
```

Clauses take two forms:
- `(pattern output-expression)`
- `(pattern fender output-expression)`

Patterns include identifiers, constants, lists, improper lists, vectors, and combinations with ellipsis. The underscore (`_`) matches without binding; ellipsis (`...`) matches repeated elements.

**Semantics**: The expression evaluates to a syntax object, which is matched against patterns left-to-right. Pattern variables match arbitrary input. Duplicate pattern variables constitute syntax violations. If underscore appears in the literals list, it becomes literal.

Identifiers in `(pattern-literal ...)` are matched literally using `free-identifier=?`.

Fender expressions provide additional constraints — returning `#f` rejects the clause. When patterns match with no fender (or true fender), the output expression evaluates and returns.

**Formal matching rules** for input E against pattern P:
- P is underscore → matches
- P is non-literal identifier → matches, binds
- P is literal identifier and E is `free-identifier=?` to it → matches
- P is list `(P₁...Pₙ)` and E matches all elements → matches
- P is improper list `(P₁...Pₙ.Pₙ₊₁)` → matches n or more elements with tail
- P is `(P₁...Pₖ Pₑ ... Pₘ₊₁...Pₙ)` → proper list with m−k middle elements matching Pₑ
- P is `(P₁...Pₖ Pₑ ... Pₘ₊₁...Pₙ.Pₓ)` → improper list with ellipsis + tail
- P is vector `#(P₁...Pₙ)` → vector matching all elements
- P is vector `#(P₁...Pₖ Pₑ ... Pₘ₊₁...Pₙ)` → vector with ellipsis
- P is constant and E equals P via `equal?` → matches

Pattern variables in subpatterns followed by ellipsis bind to sequences.

### 6.3 Generating Expansions: `syntax`

```
(syntax template)
#'template
(syntax custom-ellipsis-clause template)
```

Abbreviated form: `#'template` equals `(syntax template)`.

Templates consist of identifiers, pattern datums, lists, improper lists, vectors, or ellipsis forms. Subtemplates are templates with zero or more ellipsis instances.

**Semantics**: A `syntax` expression resembles `quote-syntax` but substitutes pattern variable values. Pattern variables in subpatterns followed by ellipsis may appear only in subtemplates with matching or greater ellipsis counts. When subtemplates have more ellipses than their subpatterns, input is replicated for excess outer ellipses.

The subtemplate must contain at least one pattern variable from an ellipsis subpattern, and at least one such variable must be followed by exactly as many ellipses as in its subpattern pattern.

Template form `(ellipsis template)` suppresses ellipsis effects, making it ordinary. Thus `(ellipsis ellipsis)` produces a single ellipsis.

**Wrapping rules for the result**:
- List templates: pairs are unwrapped through the rightmost subtemplate containing pattern variables
- Vector templates: unwrapped if any subtemplate contains pattern variables
- Other templates: may be wrapped

#### `quasisyntax`

```
(quasisyntax quasi-template)
#`quasi-template
(unsyntax expression ...)
#,expression
(unsyntax-splicing expression ...)
#,@expression
```

`quasisyntax` parallels `syntax` but allows evaluation via `unsyntax` and `unsyntax-splicing`, analogous to `unquote` and `unquote-splicing` in quasiquotes.

Within quasi-templates, unsyntax/unsyntax-splicing expressions evaluate; everything else behaves like ordinary template material. Each `unsyntax` value replaces its form; each `unsyntax-splicing` value splices into surrounding structure.

Nesting rules: Each `quasisyntax` introduces a quotation level; each `unsyntax` or `unsyntax-splicing` removes one.

Uses of `unsyntax-splicing` and multi-subform `unsyntax`/`unsyntax-splicing` are valid only in lists or vectors. Zero-subform instances insert nothing.

### 6.4 Binding Pattern Variables: `with-syntax`

```
(with-syntax ((pattern expression) ...) body)
(with-syntax custom-ellipsis-clause ((pattern expression) ...) body)
```

Each expression evaluates and destructures by its pattern; pattern variables bind within body as if by `syntax-case`. It's a syntax violation if evaluated expressions don't match patterns.

**Implementation**:
```scheme
(define-syntax with-syntax
  (lambda (stx)
    (syntax-case stx ()
      ((_ ((pattern expression) ...) body_0 body_1 ...)
       #'(syntax-case (list expression ...) ()
           ((pattern ...) (let () body_0 body_1 ...)))))))
```

### 6.5 Writing Macros Which Generate Other Macros: `custom-ellipsis`

```
(custom-ellipsis custom-ellipsis-id)
```

When `custom-ellipsis` is the first subform of `syntax-case`, `syntax`, `quasisyntax`, or `with-syntax`, ellipsis within patterns/templates refers to the custom identifier (via `bound-identifier=?`) instead of the standard `...` keyword.

### 6.6 Examples

**`swap!` using syntax-case**:
```scheme
(define-syntax swap!
  (lambda (stx)
    (syntax-case stx ()
      ((_ a b)
       #'(let ((temp a))
           (set! a b)
           (set! b temp))))))
```

**With fender for error checking**:
```scheme
(define-syntax swap!
  (lambda (stx)
    (syntax-case stx ()
      ((_ a b)
       (and (identifier? #'a)
            (identifier? #'b))
       #'(let ((temp a))
           (set! a b)
           (set! b temp))))))
```

**`my-case` with type checking**:
```scheme
(define-syntax my-case
  (let ((eqv-undefined?
         (lambda (x-stx)
           (let ((x (syntax->datum x-stx)))
             (not (or (boolean? x) (symbol? x) (number? x)
                      (char? x) (null? x)))))))
    (lambda (stx)
      (syntax-case stx ()
        ((_ key ((datum ...) expr_0 expr_1 ...) ...)
         (cond ((find eqv-undefined? #'(datum ... ...))
                => (lambda (bad-datum)
                     (syntax-violation
                      'my-case
                      "use of datum in my-case is not portable"
                      stx bad-datum)))
               (else
                #'(case key
                    ((datum ...) expr_0 expr_1 ...) ...
                    (else
                     (error "key did not match any my-case datum"
                            key))))))))))
```

**`with-return` using quasisyntax**:
```scheme
(define-syntax with-return
  (syntax-case stx ()
    ((k body_0 body_1 ...)
     (let ((return-id (datum->syntax #'k 'return)))
       #`(call-with-current-continuation
          (lambda (#,return-id)
            body_0 body_1 ...))))))
```

**Identifier macro `used-as`**:
```scheme
(define-syntax used-as
  (make-variable-transformer
   (lambda (stx)
     (syntax-case stx (set!)
       (id
        (identifier? #'id)
        #'(quote reference))
       ((set! _ value)
        #'(quote (assignment value)))
       ((_ . operands)
        #'(quote (combination . operands)))))))
```

**`fast-concatenate` optimization**:
```scheme
(define-syntax fast-concatenate
  (lambda (stx)
    (syntax-case stx (map)
      ((_ (map f ls_0 ls_1 ...))
       #'(append-map f ls_0 ls_1 ...))
      ((_ ls)
       #'(concatenate ls))
      (id
       (identifier? #'id)
       #'concatenate))))

(fast-concatenate (map make-list '(1 2 3) '(a b c)))  ⇒ (a b b c c c)
(fast-concatenate (list '(bh b p) '(dh d t)))         ⇒ (bh b p dh d t)
(apply fast-concatenate '(((gh g k) (g*h g* k*))))    ⇒ (gh g k g*h g* k*)
```

---

## 7. The syntax-rules System

### 7.1 `syntax-rules`

```
(syntax-rules (pattern-literal ...) syntax-rule ...)
(syntax-rules custom-ellipsis (pattern-literal ...) syntax-rule ...)
```

A macro implemented in the `syntax-rules` system cannot perform arbitrary Scheme evaluation during its expansion.

Rule patterns must follow one of four forms:
- `(identifier pattern ...)`
- `(identifier pattern ... . pattern)`
- `(identifier pattern ... pattern ellipsis pattern ...)`
- `(identifier pattern ... pattern ellipsis pattern ... . pattern)`

The initial identifier is not involved in matching and is considered neither a pattern variable nor a literal identifier.

**Auxiliary syntax**:
- `_` — matches any input without creating pattern variables
- `...` — ellipsis for repetition patterns

**Example** (`swap!` macro):
```scheme
(define-syntax swap!
  (syntax-rules ()
    ((_ a b)
     (let ((temp a))
       (set! a b)
       (set! b temp)))))
```

**Example** (`call*` — ensure left-to-right evaluation):
```scheme
(define-syntax call*
  (syntax-rules ()
    ((_ args ...)
     (call*-aux (args ...) ()))))

(define-syntax call*-aux
  (syntax-rules ()
    ((_ (expr . more-exprs) (exprs-w/gen-ids ...))
     (call*-aux more-exprs (exprs-w/gen-ids ... (gen-id expr))))
    ((_ () ((gen-id expr) ...))
     (let* ((gen-id expr) ...)
       (gen-id ...)))))
```

### 7.2 `identifier-syntax`

First form (read-only):
```
(identifier-syntax template)
```

Second form (with `set!` support):
```
(identifier-syntax
  (identifier₁ template₁)
  ((set! identifier₂ pattern) template₂))
```

**Example** (`define-constant`):
```scheme
(define-syntax define-constant
  (syntax-rules ()
    ((_ name value)
     (begin
       (define constant-value value)
       (define-syntax name (identifier-syntax constant-value))))))
```

**Example** (`magic-variable` — mutable-appearing export):
```scheme
(define-syntax magic-variable
  (identifier-syntax
    (_ (car magic-variable-contents))
    ((set! _ val) (set-car! magic-variable-contents val))))
```

### 7.3 Indicating Erroneous Macro Uses

#### `syntax-error`

```
(syntax-error message irritant ...)
```

Any attempt to expand a `syntax-error` form results in a syntax violation being signalled.

**Example**:
```scheme
(define-syntax simple-let
  (syntax-rules ()
    ((_ ((x . y) val) body1 body2 ...)
     (syntax-error "expected an identifier" (x . y)))
    ((_ (name val) body1 body2 ...)
     ((lambda (name) body1 body2 ...) val))))
```

#### `erroneous-syntax`

```
(erroneous-syntax)
(erroneous-syntax message)
```

An instance of `erroneous-syntax` evaluates to a macro transformer which always signals a syntax violation when invoked.

```scheme
(define-syntax => (erroneous-syntax))
(define-syntax else (erroneous-syntax))

(define-syntax documentation
  (erroneous-syntax "The documentation keyword is used only as an identifier property key"))
```

---

## 8. Other Macro Systems

This chapter demonstrates how macros from alternative Scheme systems can be accommodated within a syntax-object-based macro framework.

### 8.1 Unhygienic Macros

Traditional Lisp macros that lack protection against identifier capture.

**Implementation**:
```scheme
(define (lisp-transformer transformer)
  (lambda (stx)
    (syntax-case stx ()
      ((use-ctx . rest)
       (datum->syntax #'use-ctx
                      (transformer
                       (syntax->datum stx)))))))
```

### 8.2 Explicit Renaming Macros

Introduced by Clinger (1991) as an alternative to `syntax-rules`.

**Key limitations**: Relative verbosity for fully hygienic implementations. Cannot control identifier capture context per the hygiene condition requirements. Original definition uses symbols; this version allows `datum->syntax` for selective capture.

**Helper procedures**:
```scheme
(define (unwrap stx)
  (syntax-case stx ()
    ((a . b) (cons (unwrap #'a) (unwrap #'b)))
    (#(a ...) (vector-map unwrap #'#(a ...)))
    (id (identifier? #'id) #'id)
    (_ (syntax->datum stx))))

(define (rewrap ctx expr)
  (let rewrap* ((e expr))
    (cond
     ((pair? e) (cons (rewrap* (car e)) (rewrap* (cdr e))))
     ((vector? e) (vector-map rewrap* e))
     ((identifier? e) e)
     (else (datum->syntax ctx e)))))

(define (make-compare ctx)
  (lambda (x y)
    (free-identifier=? (rewrap ctx x) (rewrap ctx y))))

(define (make-rename ctx)
  (lambda (x)
    (datum->syntax ctx x)))
```

**Main form**:
```scheme
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
```

### 8.3 Implicit Renaming Macros

Variant of explicit renaming with inverted identifier handling. Bare symbols in output are renamed hygienically; an `inject` procedure captures identifiers in keyword context.

**Main form**:
```scheme
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

Uses identical `unwrap`, `rewrap`, `make-compare`, and `make-rename` procedures as explicit renaming.

---

## 9. Acknowledgements

The following people responded to the Yellow Ballot: Alex Shinn, Amirouche Boubekki, Artem Chernyak, Arthur A. Gleckler, Chris Vine, Daphne Preston-Kendal, Dimitris Vyzovitis, Dmitry Moskowski, Duy Nguyen, Emmanuel Medernach, Gabriel B. Sant'Anna, Graham Watt, Jaime Fournier, Jani Juhani Sinervo, Jeremy Steward, John Cowan, Justin Ethier, Linas Vepstas, 'Lulu', Marc Nieper-Wißkirchen, Marc-André Bélanger, Mark Hughes, Martin Rodgers, Nicholas Carlson, Ondřej Majerech, Ross Angle, Roy Mu, Sam Phillips, Shiro Kawai, Takashi Kato, Taylan Kammer, Tim Van den Langenbergh, Vijay Marupudi, Vincent Manis, Vladimir Nikishkin, Wolfgang Corcoran-Mathe.

Syntax parameters were introduced by Barzilay, Culpepper and Flatt (2011).

Marc Nieper-Wißkirchen contributed the sample implementation of explicit renaming in terms of syntax objects.

Sources also used: The Revised4, Revised6, and Revised7 (small language) Reports on Scheme; The Guile manual; The Racket Reference; The Chez Scheme 9 User Guide.

---

## 10. References

1. Barendregt, Henk P. "Introduction to the Lambda Calculus", *Nieuw Archief Voor Wiskunde*, 4/2 (1984), 337–72.

2. Barzilay, Eli; Culpepper, Ryan; and Flatt, Matthew. "Keeping It Clean with Syntax Parameters", 2011. http://www.schemeworkshop.org/2011/papers/Barzilay2011.pdf

3. Bawden, Alan and Rees, Jonathan. "Syntactic Closures", in *Proceedings of the 1988 ACM Conference on LISP and Functional Programming*, LFP '88 (1988), 86–95. https://doi.org/10.1145/62678.62687

4. Bradner, Scott O. "Key words for use in RFCs to Indicate Requirement Levels", RFC 2119 (1997). https://www.rfc-editor.org/info/rfc2119

5. Clinger, William. "Hygienic Macros Through Explicit Renaming", *SIGPLAN Lisp Pointers*, 4/4 (1991), 25–28. https://doi.org/10.1145/1317265.1317269

6. Clinger, William D. and Wand, Mitchell. "Hygienic Macro Technology", *Proceedings of the ACM on Programming Languages*, 4/HOPL (2020). https://doi.org/10.1145/3386330

7. Clinger, William and Rees, Jonathan. "Macros That Work", in *Proceedings of the 18th ACM SIGPLAN-SIGACT Symposium on Principles of Programming Languages*, POPL '91 (1991), 155–62. https://doi.org/10.1145/99583.99607

8. Dybvig, R. Kent; Hieb, Robert; and Bruggeman, Carl. "Syntactic Abstraction in Scheme", *Lisp and Symbolic Computation*, 5/4 (1992), 295–326. https://doi.org/10.1007/BF01806308

9. Flatt, Matthew. "Binding as Sets of Scopes", in *Proceedings of the 43rd Annual ACM SIGPLAN-SIGACT Symposium on Principles of Programming Languages*, POPL '16 (2016), 705–17. https://doi.org/10.1145/2837614.2837620

10. Hanson, Chris. "A Syntactic Closures Macro Facility", *SIGPLAN Lisp Pointers*, 4/4 (1991), 9–16. https://doi.org/10.1145/1317265.1317267

11. Kohlbecker, Eugene E., Jr. "Syntactic Extensions in the Programming Language LISP" (PhD thesis, Indiana University, 1986).

12. Kohlbecker, Eugene E., Jr.; Friedman, Daniel P.; Felleisen, Matthias; and Duba, Bruce. "Hygienic Macro Expansion", in *Proceedings of the 1986 ACM Conference on LISP and Functional Programming*, LFP '86 (1986), 151–61. https://doi.org/10.1145/319838.319859

13. van Tonder, André. "R6RS Libraries and Macros", 2006. http://www.het.brown.edu/people/andre/macros/index.html
