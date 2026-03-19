# SRFI Porting Issues

**Status:** Open
**Created:** 2026-03-19
**Context:** Problems discovered while importing commonly used SRFIs in commit `c8dcdb8` (#135).

Seven SRFIs were ported (1, 69, 111, 113, 128, 133, 158). All load and pass basic smoke tests, but several required workarounds that should be properly fixed.

## Issues

### 1. `call/cc` inside library body code is broken on the VM backend

**Severity:** High — blocks any SRFI that uses `call/cc` during library initialization
**Affected:** SRFI 1 (`%cars+cdrs`, `%cdrs`, `%cars+cdrs+`), potentially SRFI 158 (coroutine generators)

The VM evaluates library bodies in a temporary `VmState` that is discarded after loading. When `call/cc` captures a continuation during library loading, the snapshot contains the temp state's frames, registers, and wind records. When the resulting closure is later called from user code in the main `VmState`, the captured continuation is invalid — frames don't match, deliver registers are wrong, and the VM crashes or produces garbage.

**Current workaround:** `lib/srfi/1/patina-patches.scm` replaces `%cars+cdrs` and friends with versions that avoid `call/cc`.

**Proper fix:** See `PRD/phase2/VM_LIBRARY_LOADING_REDESIGN.md`. Recommended approach: execute library bodies in the main VmState (Approach C: globals swapping), or add per-closure environment pointers (Approach B).

**How to verify fix:** Remove `patina-patches.scm` from `lib/srfi/1.sld`, run SRFI 1 test suite.

### 2. Missing global bindings in temporary VmState

**Severity:** High — affects any library whose closures reference internal helpers
**Affected:** Any SRFI importing `(scheme base)` whose closures use internal names like `%any-null?`, `%map-cars`

Library closures compiled with `LoadGlobal` reference helpers defined in `lib_env`. When the closure runs in the temp `VmState` (or later in the main state), `LoadGlobal` fails unless those bindings are present.

**Current workaround:** Copy ALL `global_env` bindings into `lib_env` before creating the temp state, plus copy the primitive registry and existing code objects into it (`backend.rs` lines 432–452).

**Downside:** Pollutes every library's namespace with all global bindings. Not correct R7RS library encapsulation.

**Proper fix:** Same as Issue 1 — `VM_LIBRARY_LOADING_REDESIGN.md` Approach B (per-closure environment pointer) eliminates the need for merging entirely.

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

1. **Issues 1 & 2** (VM library loading) — These are the same root cause and block correct behavior for any SRFI using `call/cc` or internal helpers during library loading. Fix via `VM_LIBRARY_LOADING_REDESIGN.md`.
2. **Issue 5** (form-feed in lexer) — Trivial one-line fix, prevents future surprises.
3. **Issue 7** (equal-hash primitive) — Performance improvement, not blocking.
4. **Issues 3 & 4** (shims, renames) — Per-SRFI porting work, handle as needed.
5. **Issue 6** — Already fixed.
