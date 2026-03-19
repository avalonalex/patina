# SRFI Porting Issues

**Status:** Open
**Created:** 2026-03-19
**Context:** Problems discovered while importing commonly used SRFIs in commit `c8dcdb8` (#135).

Seven SRFIs were ported (1, 69, 111, 113, 128, 133, 158). All load and pass basic smoke tests, but several required workarounds that should be properly fixed.

## Issues

### 1. ~~`call/cc` inside library body code is broken on the VM backend~~

**Status:** Fixed — VM library loading redesign (Approach C + B)

Library bodies now execute directly in the main `VmState` with globals swapping. Continuations captured during library loading are valid main-state continuations. Per-closure environment pointers ensure closures use their own library's globals.

**Verify:** Remove `patina-patches.scm` from `lib/srfi/1.sld`, run SRFI 1 test suite.

### 2. ~~Missing global bindings in temporary VmState~~

**Status:** Fixed — per-closure environment pointer (Approach B)

Each `VmClosure` now carries its own `Rc<Environment>` (the globals it was compiled against). `LoadGlobal`/`StoreGlobal` use the closure's environment, not `state.globals`. No more namespace pollution — internal helpers like `%any-null?` are only visible to their own library's closures.

### 3. Non-R7RS constructs in SRFI reference implementations

**Severity:** Low — one-time porting effort per SRFI
**Affected:** SRFI 1 (and likely many future SRFIs)

The SRFI 1 reference implementation uses `check-arg`, `let-optionals`, `:optional` (from Scheme48), and `receive` (SRFI 8) — none of which are R7RS-small.

**Current workaround:** `lib/srfi/1/r7rs-shim.scm` provides macro definitions for `receive`, `let-optionals`, `:optional`, and a `check-arg` procedure.

**Long-term options:**
- Keep per-SRFI shims (current approach, adequate)
- Implement SRFI 8 (`receive`) as a standalone library — it's trivial and widely used
- Consider a shared `(srfi shim)` library for common non-R7RS constructs

### 4. R5RS vs R7RS naming differences

**Severity:** Low — one-time patch per SRFI
**Affected:** SRFI 113 comparators shim

The SRFI 113 reference implementation uses `inexact->exact` (R5RS name). R7RS renamed it to `exact`.

**Current workaround:** `lib/srfi/113/comparators-shim.scm` defines `(define %shim-exact exact)`.

**Note:** This will recur for any SRFI written against R5RS. Common renames: `inexact->exact` → `exact`, `exact->inexact` → `inexact`.

### 5. Form-feed characters in source files

**Severity:** Low — trivial to fix
**Affected:** SRFI 133

Patina's lexer doesn't accept form-feed characters (`\f`, U+000C), which appear in some reference implementations as page separators.

**Current workaround:** Removed form-feed characters from `lib/srfi/133/vectors-impl.scm`.

**Proper fix:** Extend the lexer to treat `\f` as whitespace (R7RS §7.1.1 lists it as `<intraline whitespace>`). This is a one-line fix in the lexer.

### 6. Compile-time arity rejection prevents `guard` from catching `apply` errors

**Severity:** Medium — fixed in commit `c8dcdb8`
**Affected:** chibi test framework `test-error` tests for `apply`

The desugarer rejected `(apply + 3)` at compile time with `WrongArgCount` before it reached the runtime. Since the error happened during desugaring (not evaluation), `guard` couldn't catch it.

**Fix applied:** The desugarer now falls through to a regular procedure call for under-arity `apply`, letting the runtime raise the error where `guard` can catch it.

**Status:** Fixed. No further action needed.

### 7. Missing `equal-hash` in SRFI 128

**Severity:** Low — one-time addition
**Affected:** SRFI 128 (Comparators)

SRFI 128 expects an `equal-hash` procedure that isn't part of R7RS-small. Most Scheme implementations provide it as a built-in, but Patina doesn't have a native hash function for arbitrary Scheme values.

**Current workaround:** Added a portable Scheme implementation of `equal-hash` to `lib/srfi/128/128.body2.scm`.

**Long-term option:** Implement `equal-hash` as a Rust primitive in `patina-primitives` for better performance. The portable Scheme version works but is slow for large structures.

## Priority Order

1. ~~**Issues 1 & 2** (VM library loading) — Fixed.~~
2. **Issue 5** (form-feed in lexer) — Trivial one-line fix, prevents future surprises.
3. **Issue 7** (equal-hash primitive) — Performance improvement, not blocking.
4. **Issues 3 & 4** (shims, renames) — Per-SRFI porting work, handle as needed.
5. ~~**Issue 6** — Already fixed.~~
