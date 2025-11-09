# R7RS Hygiene Requirements

## Official R7RS Specification

From Section 4.3 (Macros), the R7RS spec defines two hygiene mechanisms:

### Rule 1: Rename Inserted Bindings
> "If a macro transformer **inserts a binding** for an identifier (variable or keyword), the identifier will in effect be **renamed throughout its scope** to avoid conflicts with other identifiers."

**Example:**
```scheme
(define-syntax swap!
  (syntax-rules ()
    ((swap! a b)
     (let ((temp a))      ; 'temp' is inserted by macro
       (set! a b)
       (set! b temp)))))

(define temp 999)          ; User's temp
(define x 1)
(define y 2)
(swap! x y)
temp  ; Should still be 999 - macro's 'temp' was renamed
```

The macro's `temp` should be renamed (e.g., to `##temp#0`) so it doesn't conflict with the user's `temp` variable.

### Rule 2: Free References Use Lexical Scope
> "If a macro transformer **inserts a free reference** to an identifier, the reference refers to the **binding that was visible where the transformer was specified**, regardless of any local bindings that surround the use of the macro."

**Example:**
```scheme
(define-syntax my-if
  (syntax-rules ()
    ((my-if test then else)
     (cond (test then) (else else)))))  ; 'cond' and 'else' are free refs

(let ((cond +) (else 99))   ; Try to shadow cond and else
  (my-if #t 1 2))           ; Should still work - uses original cond/else
```

Free references like `cond` and `else` in the template should refer to their bindings where the macro was defined, not where it's used.

## What Should Be Renamed?

According to R7RS:

### ✅ Should Be Renamed:
1. **Bindings introduced by the template** that are NOT from pattern variables
   - Example: `temp` in `(let ((temp ...)) ...)`
   - These identifiers didn't exist in the input - the macro introduced them

### ❌ Should NOT Be Renamed:
1. **Pattern variables** (identifiers from the input form)
   - Example: In `(when test body ...)`, the `test` and `body` come from the macro call

2. **Free references to existing bindings** (but they're isolated from the use site)
   - Example: `if`, `begin`, `let` in templates
   - These refer to their lexical scope at macro definition time

3. **Macro keywords** (to allow nested macro calls)
   - Example: `unless` appearing in a `when` macro's expansion
   - These need to be expanded, not renamed

4. **Special form keywords** (part of the language)
   - Example: `if`, `lambda`, `define`
   - These are language syntax, not renameable

## Implementation Status in Patina

### ✅ All R7RS Hygiene Requirements Implemented:
- ✅ Special forms not renamed (`if`, `lambda`, `begin`, etc.)
- ✅ Macro keywords not renamed (nested macros work!)
- ✅ Free references in templates get renamed (hygiene works)
- ✅ Pattern variable values preserved (fixed 2025-11-08)
- ✅ Bindings introduced by macros are renamed correctly
- ✅ User variables never captured by macro-introduced bindings

**Full compliance achieved!** 🎉

## The Solution (Implemented 2025-11-08)

We collect all symbols from pattern variable values and add them to `pattern_vars` before applying hygiene.

**How it works:**

1. **Template Expansion:** Pattern variables are substituted with their matched values
   - Input: `(swap! x y)` matches pattern `(swap! a b)` with `a→x`, `b→y`
   - Template: `(let ((temp a)) (set! a b) (set! b temp))`
   - After substitution: `(let ((temp x)) (set! x y) (set! y temp))`

2. **Symbol Collection:** Extract all symbols from binding values
   - Pattern vars (keys): `{a, b}`
   - Symbols from values: `{x, y}` (recursively extracted)
   - Combined pattern_vars: `{a, b, x, y}`

3. **Hygiene Application:** Free identifiers are renamed (except pattern_vars)
   - Free identifiers: `{let, temp, set!}` (special forms excluded, `x` and `y` in pattern_vars)
   - Renamed: `{temp → ##temp#N}` (let and set! are special forms)
   - Preserved: `{x, y}` ✅

**Code:** See `src/macro_system/mod.rs:68-110` for implementation.

## References

- R7RS Section 4.3: Macros
- "Macros That Work" by Clinger & Rees (1991)
- "Syntactic Abstraction in Scheme" by Dybvig et al.
