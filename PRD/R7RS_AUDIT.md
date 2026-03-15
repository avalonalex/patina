# R7RS-Small Compliance Audit

**Date:** 2026-03-15
**Chibi R7RS Tests:** 1163/1163 (both VM and tree-walker backends)

## Executive Summary

Patina is very close to full R7RS-small compliance. All 1163 chibi R7RS tests pass on both backends. However, a detailed audit against the R7RS-small specification (LaTeX source) reveals several gaps — mostly missing libraries, unimplemented expression-level forms, and edge cases not covered by the chibi test suite.

**Status by category:**
- Standard Libraries: 14/16 present (missing `(scheme load)`, `(scheme repl)`)
- `(scheme base)` exports: ~98% complete (missing `include`/`include-ci` at expression level, `syntax-error` as binding)
- `(scheme r5rs)`: Incomplete (missing `exact->inexact`, `inexact->exact`, `load`, `interaction-environment`, several re-exports)
- Lexer/Parser: Datum labels parsed but circular quoted literals hang the writer
- Proper tail calls: Fully implemented

---

## 1. Missing Libraries

### 1.1 `(scheme load)` — NOT IMPLEMENTED

R7RS requires this library to export:
- **`load`** — `(load filename)` and optionally `(load filename environment-specifier)`

**Status:** No `lib/scheme/load.sld` exists. No `load` primitive is implemented. The name appears in the R5RS binding list but has no backing function.

**Effort:** Medium. Needs a Rust primitive that reads a file, parses it, and evaluates each expression in the given (or current) environment.

### 1.2 `(scheme repl)` — NOT IMPLEMENTED

R7RS requires this library to export:
- **`interaction-environment`** — Returns an environment specifier for the REPL

**Status:** No `lib/scheme/repl.sld` exists. The primitive is referenced in code comments but not registered as a callable function.

**Effort:** Low. The REPL already has an interaction environment internally. Just needs a primitive wrapper + `.sld` file.

---

## 2. Missing or Incomplete Exports in `(scheme base)`

### 2.1 `include` / `include-ci` — Library-level only

R7RS specifies `include` and `include-ci` as expression-level syntax in `(scheme base)` (Section 4.1.7). They should splice the contents of a file as if they were written inline.

**Status:** Implemented at library-declaration level (in `.sld` files) but NOT at expression level. Using `(include "file.scm")` in program code gives "unbound variable: include".

**Effort:** Medium. The desugarer needs to handle `include`/`include-ci` as special forms that read and inline file contents during compilation.

### 2.2 `syntax-error` — Not bound

R7RS specifies `syntax-error` as a syntax form in `(scheme base)`. It should signal an error at macro expansion time with the given message and irritants.

**Status:** Used inside macro templates (e.g., in `cond`), but not exported as a binding from `(scheme base)`. Using `(syntax-error "msg")` at top level gives "unbound variable". It appears to work only because `syntax-rules` has hard-coded support for it in template expansion, but it's not available as a standalone form.

**Effort:** Low. Need to handle it in the desugarer/macro expander as a recognized form, and ensure it's exported.

### 2.3 Auxiliary syntax exports: `_`, `...`, `else`, `=>`

R7RS requires these to be exported from `(scheme base)` as auxiliary syntax (so that `(import (only (scheme base) else))` works, and so that `syntax-rules` literal matching works correctly with renamed imports).

**Status:** These work implicitly in syntax-rules but may not be exported as proper bindings. Needs verification that `(import (only (scheme base) else))` works.

**Effort:** Low if already implicit; medium if bindings need to be created.

---

## 3. Incomplete `(scheme r5rs)` Library

The `lib/scheme/r5rs.sld` is self-described as a "stub". Missing exports required by R7RS Section A:

| Missing Export | Notes |
|---|---|
| `exact->inexact` | R5RS name for `inexact` — needs alias |
| `inexact->exact` | R5RS name for `exact` — needs alias |
| `load` | Requires `(scheme load)` implementation first |
| `interaction-environment` | Requires `(scheme repl)` implementation first |
| `delay` / `force` | Commented out in .sld — should import from `(scheme lazy)` |
| `make-rectangular`, `make-polar`, `real-part`, `imag-part`, `magnitude`, `angle` | Complex number ops from `(scheme complex)` |
| `exp`, `log`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `sqrt` | Transcendentals from `(scheme inexact)` |
| `char-alphabetic?`, `char-numeric?`, `char-whitespace?`, `char-upper-case?`, `char-lower-case?`, `char-upcase`, `char-downcase` | From `(scheme char)` |
| `char-ci=?`, `char-ci<?`, `char-ci>?`, `char-ci<=?`, `char-ci>=?` | From `(scheme char)` |
| `string-ci=?`, `string-ci<?`, `string-ci>?`, `string-ci<=?`, `string-ci>=?` | From `(scheme char)` |
| `make-string` | Missing from export list |
| `string-copy` | Missing from export list |
| `string-fill!` | Missing from export list |
| `make-vector` | Missing from export list |
| `vector-fill!` | Missing from export list |
| `positive?`, `negative?`, `odd?`, `even?` | Missing from export list |
| `abs` | Present |
| `expt` | Missing from export list |
| `numerator`, `denominator` | Missing from export list |
| `rationalize` | Missing from export list |
| `exact?`, `inexact?` | Present |
| `write`, `display`, `newline`, `read`, `read-char`, `peek-char`, `write-char` | I/O — need to import from `(scheme read)` and `(scheme write)` |
| `open-input-file`, `open-output-file`, `close-input-port`, `close-output-port` | File I/O — need import from `(scheme file)` |
| `current-input-port`, `current-output-port` | Present via `(scheme base)` |
| `call-with-input-file`, `call-with-output-file`, `with-input-from-file`, `with-output-to-file` | From `(scheme file)` |
| `eof-object?` | Missing from export list |
| `dynamic-wind` | Missing from export list |

**Effort:** Medium. Mostly adding imports from other scheme libraries and aliases.

---

## 4. Circular/Shared Data (Datum Labels)

### 4.1 Reader datum labels — IMPLEMENTED

`#n=` (datum label definition) and `#n#` (datum label reference) are tokenized and parsed. `(read)` correctly handles circular structures like `#0=(a . #0#)`.

### 4.2 `write-shared` — IMPLEMENTED

`write-shared` correctly outputs datum labels for circular and shared structure.

### 4.3 Quoted circular literals — HANGS

`'#0=(a . #0#)` as a quoted literal in source code causes the writer to loop infinitely when printing the result. The parser creates the circular structure, but the default `display`/`write` (non-shared mode) doesn't detect cycles.

**Effort:** Low-medium. The `write` procedure should detect cycles and either error or fall back to `write-shared` behavior. R7RS says `write` should handle shared structure by using datum labels.

---

## 5. Tail Call Compliance

R7RS requires proper tail calls in specific positions for all syntax forms. This is fully implemented:

- **VM backend:** Explicit `TailCall` instruction in tail position
- **Tree-walker:** Trampoline-based CPS evaluation
- **Derived forms:** All macro-expand to core forms preserving tail position (`cond` → nested `if`, `let` → `lambda` app, etc.)

**Status:** COMPLETE

---

## 6. Feature Identifiers

R7RS requires `cond-expand` to recognize certain feature identifiers. Current `(features)` returns:
```
(aarch64 darwin exact-closed full-unicode ieee-float little-endian macosx patina posix r7rs ratios unix)
```

This covers the required features well. Note: `exact-complex` is not listed (correct, since complex numbers use inexact representation).

**Status:** COMPLETE

---

## 7. Minor Spec Compliance Items

### 7.1 `#!fold-case` / `#!no-fold-case` — IMPLEMENTED
Lexer supports these reader directives.

### 7.2 `cond-expand` at expression level — IMPLEMENTED
Works correctly at top level and in expression context.

### 7.3 `let-syntax` / `letrec-syntax` — IMPLEMENTED
Handled by the macro expander with proper scoping.

### 7.4 `define-values` / `let-values` / `let*-values` — IMPLEMENTED
Implemented as macros in `lib/scheme/base/binding.scm`.

### 7.5 `guard` — IMPLEMENTED
Implemented as a macro in `lib/scheme/base/exceptions.scm`.

### 7.6 `parameterize` — IMPLEMENTED
Implemented as a macro using `dynamic-wind`.

### 7.7 `define-record-type` — IMPLEMENTED
Implemented as a macro in `lib/scheme/base/records.scm`.

### 7.8 `case-lambda` — IMPLEMENTED
Available via `(scheme case-lambda)`.

### 7.9 `string->vector` / `vector->string` — IMPLEMENTED
In `(scheme base)` with optional start/end arguments.

---

## 8. Summary: Action Items by Priority

### Priority 1 — Missing Libraries (blocks full R7RS claim)

| Item | Effort | Description |
|------|--------|-------------|
| `(scheme repl)` library | Low | Create `.sld`, implement `interaction-environment` primitive |
| `(scheme load)` library | Medium | Implement `load` primitive + create `.sld` |

### Priority 2 — Missing Expression-Level Syntax

| Item | Effort | Description |
|------|--------|-------------|
| `include` / `include-ci` expressions | Medium | Handle in desugarer as special forms |
| `syntax-error` as exported binding | Low | Handle in macro expander, export from base |

### Priority 3 — Incomplete Libraries

| Item | Effort | Description |
|------|--------|-------------|
| `(scheme r5rs)` completion | Medium | Add missing re-exports, `exact->inexact`/`inexact->exact` aliases |

### Priority 4 — Edge Cases

| Item | Effort | Description |
|------|--------|-------------|
| `write` cycle detection | Low-Med | Non-shared `write` should detect and handle cycles |
| Auxiliary syntax exports (`_`, `...`, `else`, `=>`) | Low | Verify/fix export as proper bindings |

---

## 9. What's NOT Missing

For context, here is what is fully implemented and tested:

- All 148+ `(scheme base)` procedures
- All `(scheme char)` procedures (26 total)
- All `(scheme complex)` procedures (6 total)
- All `(scheme cxr)` procedures (24 total)
- All `(scheme eval)` procedures (`eval`, `environment`)
- All `(scheme file)` procedures (10 total)
- All `(scheme inexact)` procedures (12 total)
- All `(scheme lazy)` procedures and syntax (5 total)
- All `(scheme process-context)` procedures (5 total)
- All `(scheme read)` procedures (`read`)
- All `(scheme time)` procedures (3 total)
- All `(scheme write)` procedures (4 total)
- `(scheme case-lambda)` syntax
- Full exception handling (`with-exception-handler`, `guard`, `raise`, `raise-continuable`)
- Full continuations (`call/cc`, `dynamic-wind`, `values`, `call-with-values`)
- Full numeric tower (fixnum, rationals, floats, complex)
- Full Unicode support
- Hygienic macros with `syntax-rules` and scope sets
- Proper tail calls in all required positions
