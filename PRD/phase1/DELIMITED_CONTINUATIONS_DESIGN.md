# Delimited Continuations Design

This document covers the design for implementing full delimited continuation support (shift/reset, prompts).

**Status**: Deferred from Phase 1 tech debt (POST_CPS_TECH_DEBT.md Item 3)

---

## Problem Statement

Current continuation support includes:
- ✅ `call/cc` - Full undelimited continuations
- ✅ `dynamic-wind` - Wind protection
- ⚠️ `call-with-continuation-prompt` - Partial (stubs exist)
- ⚠️ `abort-current-continuation` - Partial (stubs exist)
- ❌ Proper delimited capture - Not implemented

The TODO comments in the codebase identify:
```rust
// TODO: Implement proper capture for these special continuations
// TODO: Implement proper delimited continuation capture
```

---

## Background: Delimited vs Undelimited Continuations

**Undelimited (`call/cc`)**:
- Captures the entire continuation to program exit
- Can only be invoked once meaningfully (unless explicitly copied)
- Sufficient for most control flow (exceptions, coroutines)

**Delimited (`shift`/`reset`, `call-with-continuation-prompt`)**:
- Captures continuation up to a prompt/delimiter
- Returns a value when the delimited continuation completes
- Enables more compositional control operators
- Required for some advanced macro patterns

---

## Current Implementation Status

**Files involved**:
- `cps_eval/continuation.rs` - Continuation capture/restore
- `cps_eval/wind.rs` - Dynamic-wind integration
- `primitives/continuations.rs` - Primitive stubs

**What works**:
- Basic prompt installation (`call-with-continuation-prompt`)
- Prompt tags (unique identifiers)
- Abort to prompt (`abort-current-continuation`)

**What's missing**:
- Proper delimited capture (capturing up to prompt, not entire continuation)
- Composable continuations (re-invoking delimited continuations)
- Integration with `dynamic-wind` for delimited escapes

---

## Design Considerations

### Option A: Racket-style Prompts
- `call-with-continuation-prompt` installs a prompt
- `call-with-composable-continuation` captures up to prompt
- `abort-current-continuation` aborts to prompt with values
- Most flexible, matches Racket semantics

### Option B: Felleisen-style shift/reset
- `reset` installs a delimiter
- `shift` captures and removes delimiter
- Simpler model, well-understood theory
- Can be implemented on top of prompts

**Recommendation**: Implement Racket-style prompts (Option A) as the primitive, then optionally provide shift/reset as derived forms.

---

## Implementation Plan

### Phase 1: Audit Current State
1. Document exactly what `call-with-continuation-prompt` does now
2. Identify where capture truncation should happen
3. Map out continuation data structures

### Phase 2: Implement Delimited Capture
1. Modify `capture_continuation` to accept an optional prompt tag
2. Stop capture at matching prompt frame
3. Store captured segment as composable continuation

### Phase 3: Implement Composable Invocation
1. When invoking delimited continuation, splice it into current continuation
2. Handle prompt re-installation correctly
3. Ensure proper `dynamic-wind` interaction

### Phase 4: Testing
1. Port Racket's continuation test suite
2. Test `shift`/`reset` derived forms
3. Test interaction with exceptions and `dynamic-wind`

---

## Known Edge Cases

From `cps_features.rs` ignored tests:
1. `dynamic-wind` after thunk not run on exception propagation
2. `dynamic-wind` after thunk not run on `call/cc` escape
3. Before/after thunks not run on continuation re-entry

These bugs may be related to delimited continuation issues and should be addressed together.

---

## Related TODOs

From POST_CPS_TECH_DEBT.md Item 3:

1. **Delimited continuation capture** (`cps_eval/continuation.rs`, `cps_eval/wind.rs`)
   - Primary focus of this design doc

2. **Default prompt tag singleton** (`primitives/continuations.rs`, line 209)
   ```rust
   // TODO: Use a thread-local singleton for the default tag
   ```
   - Each call creates new tag instead of singleton
   - Should be addressed as part of this work

---

## Effort Estimate

- **Total**: 5-7 days
- **Risk**: Medium (complex semantics, edge cases)

---

## Success Criteria

- [ ] `call-with-continuation-prompt` captures up to prompt
- [ ] `abort-current-continuation` works correctly
- [ ] `shift`/`reset` can be implemented as library
- [ ] All 7 ignored `cps_features.rs` tests pass
- [ ] `dynamic-wind` interacts correctly with delimited escapes

---

## References

### Academic Papers
- [Abstracting Control](https://legacy.cs.indiana.edu/~dfried/appcont.pdf) - Danvy & Filinski 1990 (original shift/reset)
- [Composable and Compilable Macros](https://www.cs.utah.edu/plt/publications/macromod.pdf) - Flatt 2002

### SRFIs
- [SRFI-226: Control Features](https://srfi.schemers.org/srfi-226/) - Comprehensive delimited continuations for Scheme
  - Defines `call-with-continuation-prompt`, `call-with-composable-continuation`, `abort-current-continuation`
  - Includes prompt tags, continuation barriers, and continuation marks
  - **Primary specification to follow for R7RS compatibility**

### Implementation References
- [Racket Continuation Documentation](https://docs.racket-lang.org/reference/cont.html)
  - Mature implementation of prompts and composable continuations
  - Defines semantics for `call/cc`, `call/ec`, prompts, barriers, and marks
  - Good test cases and edge case documentation

- [Gauche Scheme - Continuations](https://practical-scheme.net/gauche/man/gauche-refe/Continuations.html)
  - `partial-continuation` and `reset`/`shift` support
  - Shows how to implement shift/reset on top of prompts
  - Useful for understanding CPS-based implementation

- [Chibi Scheme - (chibi continuations)](https://synthcode.com/scheme/chibi/)
  - Lightweight implementation
  - Good reference for minimal delimited continuation support

---

## Related Documents

- `POST_CPS_TECH_DEBT.md` - Item 3 deferred to this doc
- `PRD/ARCHIVE/cps_continuation_2025_12/` - CPS implementation history
