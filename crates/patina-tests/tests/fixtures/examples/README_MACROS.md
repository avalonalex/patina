# Macro System Examples

This directory contains example files demonstrating the R7RS macro system implementation.

## Files

### `macros.scm`
Comprehensive examples showcasing various macro capabilities:
- **when** - Conditional macro (executes body when test is true)
- **unless** - Inverse conditional (executes body when test is false)  
- **swap!** - Variable swap demonstrating hygiene (prevents variable capture)
- **dup-first** - Simple list construction with pattern matching
- **test-and-double** - Conditional expression builder

All examples use only built-in primitives and work in the REPL.

### `macro_debug.scm`
Demonstrates the debug tracing system for macro expansion:
- How to enable/disable macro expansion tracing with `debug-enable`/`debug-disable`
- Shows the `[MACRO]` debug output format
- Illustrates original form vs expanded form
- Examples of `debug-status` and `debug-mode` commands

## Usage

Since file execution isn't implemented yet, copy and paste examples into the REPL:

```bash
cargo run --release
```

Then paste the examples line by line or section by section.

## Debug Tracing

To see how macros expand:

```scheme
;; Enable macro expansion tracing
(debug-enable 'expand)

;; Define and use a macro
(define-syntax when
  (syntax-rules ()
    ((when test body ...)
     (if test (begin body ...)))))

(when #t 42)
;; Output:
;; [MACRO] Expanding macro 'when': (when #t 42)
;; [MACRO]   Expanded to: (if #t (begin 42))
;; 42
```

## Implementation Status

✅ **Working:**
- Pattern matching with ellipsis (`...`)
- Template expansion with substitution
- Hygienic macro expansion (gensym-based)
- Debug tracing for macro expansion
- `define-syntax` and `syntax-rules`

⚠️ **Known Limitations:**
- Nested macro calls may not work correctly due to hygiene renaming macro keywords
- Limited set of primitives available (no `display`, `newline`, etc. yet)
- Deep recursion (100+ levels) may overflow stack in debug builds (works in release builds)
  - TODO: Implement tail call optimization (TCO) to fix this

## Technical Details

The macro system implements:
1. **Pattern Matching** - R7RS pattern syntax with ellipsis support
2. **Template Expansion** - Variable substitution and ellipsis expansion
3. **Hygiene** - Gensym-based identifier renaming to prevent variable capture
   - Format: `##name#counter`
   - Special forms excluded from renaming
   - Pattern variables never renamed
   - Free identifiers in templates renamed

See `src/macro_system/` for implementation details.
