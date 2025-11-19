# 🔍 Macro Debug Mode - Currently ENABLED

## Status: GLOBALLY ENABLED

Macro debug mode is currently enabled globally to help debug macro expansion issues in r7rs-tests.scm.

**Location:** `crates/patina-repl/src/main.rs:11`

```rust
// TODO: Disable macro debug mode once all macro-related tests in r7rs-tests.scm pass
// Currently enabled globally to help debug macro expansion issues
// See: PRD/phase1/IMPLEMENTATION_STATUS.md for macro test status
patina_runtime::macro_debug::enable();
```

## What This Means

When running Patina (REPL or scripts), you'll see detailed macro expansion traces:

```
[MACRO] Expanding macro: <name>
[MACRO]   === Trying rule 1 ===
[MACRO]   Pattern: <pattern>
[MACRO]   ✓ Match successful!
[MACRO]   Bindings: <variables>
[MACRO]   === Expanding template ===
[MACRO]   Template: <template>
[MACRO]   Expanded: <result>
[MACRO]   === Applying hygiene ===
[MACRO]   Final: <final>
```

## When to Disable

**Disable when:** All macro-related tests in r7rs-tests.scm pass

To disable, simply remove or comment out the `macro_debug::enable()` line in `main.rs`.

## Current r7rs-tests.scm Status

Track progress in:
- `PRD/phase1/IMPLEMENTATION_STATUS.md` - Overall roadmap
- `docs/FEATURE_STATUS.md` - Detailed test-by-test status
- `scheme_tests/reports/compatibility.md` - Latest test results

## Benefits While Enabled

✅ **See exactly what macros are doing**
- Which rules match/fail
- Variable bindings
- Template expansion steps
- Final expanded code

✅ **Catch errors early**
- Validation happens at compile time
- Clear error messages with hints
- See why patterns don't match

✅ **Faster debugging**
- No guessing about macro behavior
- Step-by-step visibility
- Immediate feedback

## Performance Note

⚠️ Debug output adds overhead. Once macro tests pass, disable for production use.

The debug output is printed to stdout/stderr, which may slow down execution slightly,
especially for programs that heavily use macros.

## Quick Test

Verify debug mode is enabled:

```bash
echo '(define-syntax test (syntax-rules () ((test x) x)))
(test 42)' | ./target/release/patina

# Should show [MACRO] debug output
```

---

**Last Updated:** 2025-11-18
**Reason:** Debugging r7rs-tests.scm macro issues
**TODO:** Disable once macro tests pass
