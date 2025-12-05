# Missing Macro Features Analysis

This document analyzes three missing R7RS macro features and how to implement them in Patina.

## Overview

The following features are currently missing or buggy:

1. **Underscore Literal Handling** - When `_` is in the literals list, it should match only literal `_`, not act as wildcard
2. **Ellipsis Escaping** - `(... template)` should produce literal `...` or escape ellipsis in nested macro definitions
3. **Nested Macro Definitions** - Macros that define other macros using ellipsis escaping

## Current Architecture Summary

**Key files:**
- `crates/patina-macros/src/macro_expander/compiler.rs` - Pattern/template compilation
- `crates/patina-macros/src/macro_expander/pattern.rs` - Pattern enum (7 variants)
- `crates/patina-macros/src/macro_expander/template.rs` - Template enum
- `crates/patina-macros/src/macro_expander/matcher.rs` - Pattern matching
- `crates/patina-macros/src/macro_expander/expander.rs` - Template expansion

**Key data structures:**
```rust
// Pattern representation
enum Pattern {
    Wildcard,              // _ (always matches)
    Literal(Value),        // Exact literal match
    Var(PVRef),           // Pattern variable binding
    List(Vec<Pattern>),   // (p1 p2 p3)
    // ... more variants
}

// Compiler state
struct Compiler {
    literals: Vec<Rc<str>>,       // Literals from syntax-rules
    ellipsis: Option<Rc<str>>,    // Usually Some("..."), None when escaped
    // ...
}
```

---

## Issue 1: Underscore Literal Handling

### Problem

When `_` appears in the `syntax-rules` literals list, it should only match the literal symbol `_`, not act as a wildcard pattern.

**Current behavior (WRONG):**
```scheme
(define-syntax count-to-2_
  (syntax-rules (_)       ; _ is a LITERAL
    ((_) 0)
    ((_ _) 1)             ; Should only match (count-to-2_ _)
    ((_ _ _) 2)           ; Should only match (count-to-2_ _ _)
    ((x . y) 'fail)))

(count-to-2_ a b)  ; Expected: fail, Got: 2
```

The pattern `(_ _ _)` is matching `(count-to-2_ a b)` because `_` is still treated as wildcard.

### Root Cause

In `compiler.rs:248`, underscore is checked BEFORE the literals list:

```rust
Value::Symbol(s) if s.as_ref() == "_" => Ok(Pattern::Wildcard)
```

This means `_` is always a wildcard, even when it's in the literals list.

### Fix Location

`crates/patina-macros/src/macro_expander/compiler.rs`, lines 248-259

### Proposed Fix

Check if `_` is in the literals list FIRST:

```rust
Value::Symbol(s) => {
    // Check literals FIRST (including _ if it's there)
    if self.is_literal(s) {
        Ok(Pattern::Literal(form.clone()))
    } else if s.as_ref() == "_" {
        // Underscore is wildcard only if NOT in literals
        Ok(Pattern::Wildcard)
    } else {
        let pvref = self.add_pvar(s.clone(), level)?;
        Ok(Pattern::Var(pvref))
    }
}
```

### Testing

```scheme
; After fix:
(count-to-2_ a b)      ; Should return: fail
(count-to-2_ _ _)      ; Should return: 2
(count-to-2_ _)        ; Should return: 1
```

---

## Issue 2: Ellipsis Escaping

### Problem

R7RS specifies that `(... template)` should:
1. Produce a literal `...` symbol when used as `(... ...)`
2. Escape ellipsis processing in nested templates

**Current behavior (WRONG):**
```scheme
(define-syntax elli-esc-1
  (syntax-rules ()
    ((_) '(... ...))))    ; Should quote to produce symbol ...

(elli-esc-1)  ; Expected: ..., Got: (... ...)
```

### Root Cause

Template ellipsis escaping IS partially implemented in `compiler.rs:494-501`:

```rust
// Check for ellipsis escape: (... template)
if items.len() == 2
    && self.ellipsis.is_some()
    && matches!(&items[0], Value::Symbol(s) if self.ellipsis.as_ref() == Some(s))
{
    return self.compile_with_escaped_ellipsis(&items[1], level);
}
```

However, the issue is that `'(... ...)` is a QUOTED form. Quote handling (`compile_quote_template` at line 475) currently processes quoted data differently and may not be applying ellipsis escaping inside quotes.

### Fix Location

1. `crates/patina-macros/src/macro_expander/compiler.rs` - `compile_quote_template()` around line 475
2. Need to handle `(... ...)` inside quoted forms

### Proposed Fix

**Option A: Handle in quote compilation**

In `compile_quote_template()`, check for ellipsis escape pattern `(... ...)`:

```rust
fn compile_quote_template(&mut self, form: &Value, level: usize) -> Result<Template, MacroError> {
    match form {
        Value::Pair(_) => {
            let items = value_list_to_vec(form)?;

            // Check for (... ...) escape producing literal ...
            if items.len() == 2 {
                if let (Value::Symbol(s1), Value::Symbol(s2)) = (&items[0], &items[1]) {
                    if self.ellipsis.as_ref() == Some(s1) && self.ellipsis.as_ref() == Some(s2) {
                        // Produce literal ellipsis symbol
                        return Ok(Template::Literal(Value::Symbol(s2.clone())));
                    }
                }
            }

            // ... rest of existing logic
        }
        // ...
    }
}
```

**Option B: Handle in expander**

Alternatively, handle `(... ...)` during template expansion in `expander.rs`.

### Testing

```scheme
; After fix:
(elli-esc-1)           ; Should return: ...
(elli-esc-1 100)       ; Should return: (100 ...)
(elli-esc-1 100 200)   ; Should return: (... 100 200)
```

---

## Issue 3: Nested Macro Definitions (Macro-Generating Macros)

### Problem

Macros that generate other macros need to use ellipsis escaping to prevent the outer macro from consuming the inner macro's ellipsis:

```scheme
(define-syntax be-like-begin1
  (syntax-rules ()
    ((be-like-begin1 name)
     (define-syntax name
       (... (syntax-rules ()        ; <-- Escape outer ellipsis
              ((name expr ...)      ; This ... belongs to inner macro
               (begin expr ...))))))))

(be-like-begin1 sequence1)
(sequence1 1 2 3 4)  ; Expected: 4
```

**Current behavior:**
```
Error: No matching pattern for macro sequence1
```

### Root Cause

The ellipsis escape `(... (syntax-rules () ...))` should:
1. Prevent the outer macro from treating `...` as ellipsis
2. Pass through the `(syntax-rules () ...)` form literally
3. Let the inner macro definition keep its `...` intact

Currently, `compile_with_escaped_ellipsis()` exists (line 606-621) but may not be working correctly for complex nested cases.

### Fix Location

1. `crates/patina-macros/src/macro_expander/compiler.rs` - `compile_with_escaped_ellipsis()`
2. `crates/patina-macros/src/macro_expander/expander.rs` - Template expansion

### Proposed Fix

Ensure `compile_with_escaped_ellipsis()` properly handles all template forms:

```rust
fn compile_with_escaped_ellipsis(&mut self, form: &Value, level: usize) -> Result<Template, MacroError> {
    // Save current ellipsis setting
    let saved_ellipsis = self.ellipsis.take();

    // Compile with ellipsis disabled - this means:
    // 1. ... is treated as a regular symbol, not special
    // 2. Pattern variables are still substituted
    // 3. The resulting template preserves ... literally
    let result = self.compile_template(form, level);

    // Restore ellipsis setting
    self.ellipsis = saved_ellipsis;

    result
}
```

The key insight is that when ellipsis is `None`:
- `is_ellipsis()` returns `false` for all symbols
- `...` compiles as `Template::Symbol` (regular identifier)
- Pattern variables inside ARE still substituted

### Testing

```scheme
; After fix:
(be-like-begin1 sequence1)
(sequence1 1 2 3 4)    ; Should return: 4

(be-like-begin2 sequence2)
(sequence2 1 2 3 4)    ; Should return: 4
```

---

## Implementation Priority

1. **Underscore Literal** - Simple fix, high impact (affects pattern matching correctness)
2. **Ellipsis Escaping for `(... ...)`** - Medium complexity, needed for R7RS compliance
3. **Nested Macro Definitions** - May work once #2 is fixed, needs verification

## R7RS Specification References

From R7RS Section 4.3.2 (syntax-rules):

> "An identifier appearing within a pattern can be an underscore (_), a literal identifier listed in the list of literals, or the ellipsis. All other identifiers appearing within a pattern are pattern variables."

> "If the identifier ... appears after a ⟨pattern⟩ in a ⟨pattern datum⟩, it indicates that the preceding ⟨pattern⟩ may match zero or more elements of the input."

> "It is an error to use a macro keyword, within the scope of its binding, in an expression that does not match any of the patterns."

The spec also mentions that `(... ⟨template⟩)` is an escape:

> "A subtemplate followed by ... expands into zero or more occurrences of the subtemplate. [...] As a special case, when the subtemplate consists of a single ⟨pattern identifier⟩ followed by ..., at least one ... is required; this is to allow the ... to be used in the ⟨template⟩ to stand for a literal ellipsis."
