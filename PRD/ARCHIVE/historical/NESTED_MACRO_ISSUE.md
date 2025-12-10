# Nested Macro Issue in Patina

## Status: ✅ FIXED (2025-11-08)

This issue has been resolved by passing the environment to the hygiene system so it can check if an identifier is bound to a macro.

## The Problem (Historical)

When a macro call appeared inside another macro's expansion, the inner macro got renamed by the hygiene system and became uncallable.

## Example

```scheme
(define-syntax when
  (syntax-rules ()
    ((when test body ...)
     (if test (begin body ...)))))

(define-syntax unless
  (syntax-rules ()
    ((unless test body ...)
     (if (not test) (begin body ...)))))

;; This fails:
(when #t
  (unless #f
    42))
```

**Error:** `Not a procedure: #<macro:unless>`

## What's Happening (Step by Step)

### Input
```scheme
(when #t (unless #f 42))
```

### Step 1: Macro Expansion
The `when` macro matches and expands to:
```scheme
(if #t (begin (unless #f 42)))
```

### Step 2: Hygiene Application
The hygiene system analyzes the expanded code:
- Pattern variables: `{test, body}` (matched to `#t` and `(unless #f 42)`)
- Free identifiers: `{if, begin, unless}` (introduced by the template)

Special forms like `if` and `begin` are excluded from renaming, but `unless` is NOT a special form - it's a user-defined macro!

So hygiene renames it:
```scheme
(if #t (begin (##unless#0 #f 42)))
```

### Step 3: Evaluation Fails
When evaluating `(##unless#0 #f 42)`:
1. Evaluator looks up `##unless#0` as a variable
2. Finds nothing (the macro is bound to `unless`, not `##unless#0`)
3. Error: "Not a procedure"

## Root Cause

**The hygiene system cannot distinguish between:**
1. Regular identifiers (should be renamed for hygiene)
2. Macro keywords (should NOT be renamed - they need to be expanded)

The system treats `unless` as a free identifier from the template and renames it, breaking the macro call.

## Why This Happens

In our current implementation (`src/macro_system/hygiene.rs`):

```rust
fn collect_free_identifiers(
    expr: &Value,
    pattern_vars: &HashSet<Rc<str>>,
    free_ids: &mut HashSet<Rc<str>>,
) {
    match expr {
        Value::Symbol(name) => {
            // Don't rename if:
            // 1. It's a pattern variable
            // 2. It's already a gensym
            // 3. It's a special form keyword
            if !pattern_vars.contains(name)
                && !is_gensym(name.as_ref())
                && !is_special_form(name.as_ref())  // <-- Problem!
            {
                free_ids.insert(name.clone());
            }
        }
        // ...
    }
}
```

The `is_special_form()` check only excludes built-in special forms like `if`, `let`, etc. User-defined macros like `unless` are not in that list, so they get renamed.

## The Fix (Not Implemented Yet)

To fix this properly, we need to:

1. **Pass environment to `apply_hygiene()`:**
   ```rust
   pub fn apply_hygiene(
       expr: &Value, 
       pattern_vars: &HashSet<Rc<str>>,
       env: &Rc<Environment>  // <-- Add this
   ) -> Value
   ```

2. **Check if identifier is bound to a macro:**
   ```rust
   fn collect_free_identifiers(
       expr: &Value,
       pattern_vars: &HashSet<Rc<str>>,
       env: &Rc<Environment>,  // <-- Add this
       free_ids: &mut HashSet<Rc<str>>,
   ) {
       match expr {
           Value::Symbol(name) => {
               if !pattern_vars.contains(name)
                   && !is_gensym(name.as_ref())
                   && !is_special_form(name.as_ref())
                   && !is_macro(name, env)  // <-- Add this check!
               {
                   free_ids.insert(name.clone());
               }
           }
           // ...
       }
   }
   
   fn is_macro(name: &Rc<str>, env: &Rc<Environment>) -> bool {
       if let Some(value) = env.get(name) {
           matches!(value, Value::Macro(_))
       } else {
           false
       }
   }
   ```

## The Fix (Implemented 2025-11-08)

The fix involved:
1. ✅ Threading the environment through the hygiene system
2. ✅ Modifying the hygiene module API to accept `&Rc<Environment>`
3. ✅ Adding `is_macro()` helper function to check if identifier is bound to a macro
4. ✅ Updating `collect_free_identifiers()` to exclude macro keywords
5. ✅ Updating all call sites to pass environment

**Changes made:**
- `src/macro_system/hygiene.rs`: Added environment parameter, `is_macro()` check
- `src/macro_system/mod.rs`: Updated `expand_macro()` signature
- `src/eval/special_forms.rs`: Pass environment through
- `src/eval/mod.rs`: Pass environment at call site
- `tests/compliance/derived.rs`: Added tests for nested macros

**Tests added:**
- `test_nested_macros` - Basic nested macro call
- `test_nested_macros_complex` - Multiple levels of nesting

## Workaround (No Longer Needed)

~~For now, avoid nesting macro calls.~~

**This issue is now fixed!** Nested macro calls work correctly:

```scheme
(when #t
  (unless #f
    42))  ; Now works! Returns 42
```

## Related Files

- `src/macro_system/hygiene.rs` - Hygiene implementation
- `src/macro_system/mod.rs:27-51` - `expand_macro()` calls `apply_hygiene()`
- `tests/fixtures/examples/README_MACROS.md:65` - Documented limitation
