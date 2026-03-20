# SRFI Porting Issues

**Status:** All 8 issues resolved
**Created:** 2026-03-19
**Context:** Problems discovered while importing commonly used SRFIs in commit `c8dcdb8` (#135).

Seven SRFIs were ported (1, 69, 111, 113, 128, 133, 158). All load and pass basic smoke tests, but several required workarounds that should be properly fixed.

## Issues

### 1. ~~`call/cc` inside library body code is broken on the VM backend~~

**Status:** Fixed — VM library loading redesign (Approach C + B)

Library bodies now execute directly in the main `VmState` with globals swapping. Continuations captured during library loading are valid main-state continuations. Per-closure environment pointers ensure closures use their own library's globals.

**Note:** `patina-patches.scm` removed — the multi-value continuation delivery bug (Issue 8) is also fixed.

### 2. ~~Missing global bindings in temporary VmState~~

**Status:** Fixed — per-closure environment pointer (Approach B)

Each `VmClosure` now carries its own `Rc<Environment>` (the globals it was compiled against). `LoadGlobal`/`StoreGlobal` use the closure's environment, not `state.globals`. No more namespace pollution — internal helpers like `%any-null?` are only visible to their own library's closures.

### 3. ~~Non-R7RS constructs in SRFI reference implementations~~

**Status:** Mostly resolved.

- `receive` (SRFI 8): Implemented as `(srfi 8)` library. SRFI 1 now imports it instead of inlining.
- `check-arg`, `let-optionals`, `:optional`: Remain as per-SRFI shims in SRFI 1's `.sld` file. These are Scheme48-specific and unlikely to be needed by many other SRFIs.

### 4. ~~R5RS vs R7RS naming differences~~

**Status:** Fixed — SRFI 113 now imports `inexact->exact` and `exact->inexact` from `(scheme r5rs)` (which already existed). Removed the `%shim-exact` workaround. Future SRFIs written against R5RS can use the same `(only (scheme r5rs) ...)` import.

### 5. ~~Form-feed characters in source files~~

**Status:** Fixed — lexer now treats `\f` (U+000C) as whitespace.

### 6. Compile-time arity rejection prevents `guard` from catching `apply` errors

**Severity:** Medium — fixed in commit `c8dcdb8`
**Affected:** chibi test framework `test-error` tests for `apply`

The desugarer rejected `(apply + 3)` at compile time with `WrongArgCount` before it reached the runtime. Since the error happened during desugaring (not evaluation), `guard` couldn't catch it.

**Fix applied:** The desugarer now falls through to a regular procedure call for under-arity `apply`, letting the runtime raise the error where `guard` can catch it.

**Status:** Fixed. No further action needed.

### 7. ~~Missing `equal-hash` in SRFI 128~~

**Status:** Fixed — `equal-hash` implemented as a Rust primitive in `patina-primitives` (`equality.rs`), backed by `Heap::tagged_value_hash()` in `patina-core`. Exported via `(patina internal predicates)`. SRFI 128 now imports it directly instead of using a portable Scheme fallback.

### 8. ~~VM continuations don't deliver multiple values~~

**Status:** Fixed — two-part fix:

1. `try_invoke_continuation` now populates `state.value_buffer` when invoked with >1 argument, so `call-with-values` can unpack them.
2. After continuation invocation in Call/TailCall dispatch, if frames dropped to or below `exit_depth` (continuation escaping a synchronous `run_thunk` boundary), the dispatch now returns the primary value immediately instead of continuing to execute unrelated instructions.

SRFI 1 `patina-patches.scm` removed — the original `call/cc`-based `%cars+cdrs` now works correctly.

## Priority Order

1. ~~**Issues 1 & 2** (VM library loading) — Fixed.~~
2. ~~**Issue 5** (form-feed in lexer) — Fixed.~~
3. ~~**Issue 8** (multi-value continuation delivery) — Fixed.~~
4. ~~**Issue 7** (equal-hash primitive) — Fixed.~~
5. ~~**Issues 3 & 4** (shims, renames) — Fixed.~~
6. ~~**Issue 6** — Already fixed.~~
