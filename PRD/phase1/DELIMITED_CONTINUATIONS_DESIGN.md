# Delimited Continuations Design

This document covers the design for implementing full delimited continuation support (shift/reset, prompts).

**Status**: **Done on the VM. Open on the tree-walker — issue #169.**

Written December 2025, when neither backend had prompts. Everything below the
next section is a *design sketch from then*, and the VM did not follow it:
Option A was chosen but implemented in the bytecode VM against
`docs/VM_RUNTIME.md` §5.5–§5.6 rather than in the CPS evaluator this document
assumes. **Read `docs/VM_RUNTIME.md` §5.6 and
`crates/patina-tests/tests/control_flow_matrix.rs` before this file.** The
sections here that were measured and corrected on 2026-09-03 are marked; the
rest is unverified 2025 sketch.

---

## Problem Statement

Current continuation support (**re-measured 2026-09-03**; the 2025 list this
replaces claimed partial prompt support on both backends, which was never
true):

| | VM | tree-walker |
|---|---|---|
| `call/cc`, `dynamic-wind` | ✅ | ✅ |
| `call-with-continuation-prompt` | ✅ | ❌ issue #169 |
| `abort-current-continuation` | ✅ | ❌ issue #169 |
| delimited capture, composable invoke | ✅ (#160, #164, #166) | ❌ issue #169 |
| `call/cc` used as a *value* | ✅ | ❌ Track Q §1.2 |

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

**Corrected 2026-09-03.** The list this replaces said basic prompt
installation, prompt tags and abort-to-prompt all worked. On the tree-walker
none of it did; on the VM it works by a mechanism this document does not
describe.

**The VM**: complete. `call-with-continuation-prompt` and
`abort-current-continuation` are intercepted by name in `vm_control_primitive`
before any registry lookup, an abort travels to a synthesized continuation
whose top frame calls the prompt handler, and a composable invoke appends the
captured region in place. See `docs/VM_RUNTIME.md` §5.5–§5.6. One defect open:
issue #167.

**The tree-walker**: nothing works, and the scaffolding that exists is dead
**but not absent** — an earlier version of this section said `Abort` had no
arm, which is wrong and worth correcting carefully, because "there is nothing
there" is what stops anyone auditing what *is*:

- `CpsExprKind::Prompt` and `CpsExprKind::Abort` are both in the CPS IR, and
  `cps_eval/step.rs` has an arm for **each**. `Prompt` pushes a `PromptFrame`;
  `Abort` finds the prompt by tag, reifies its continuation and jumps to it.
- The evaluator threads a `prompt_stack` through every step and roots it for
  GC. It is always empty, because **nothing emits either form**.
- The `Abort` arm had a real bug the whole time — it truncated the prompt
  stack to the *matched* index, discarding the frame it had just found, and
  then popped the enclosing prompt, panicking outright when the match was the
  only prompt on the stack. Dead code, so nothing caught it; it would have
  fired on the first program a #169 fix made reach there. Corrected
  2026-09-03.

**Which name is claimed where.** This is the part to get right before
starting, and the reason `dynamic-wind` works in value position while
`call/cc` does not:

| | VM | tree-walker |
|---|---|---|
| `call/cc`, `call-with-current-continuation` | `vm_control_primitive` | **syntactically**, `cps_transform.rs`'s `is_callcc_reference` |
| `dynamic-wind`, `apply`, `raise`, `error`, `force`, `call-with-values` | `vm_control_primitive` | at **apply** time, short-name match in `cps_eval/application.rs` |
| the two prompt names | `vm_control_primitive` | nowhere — a registry miss until #170 gave them a deliberate error |

An apply-time match sees the primitive whatever name reached it; a syntactic
one only sees the call it was written in. So the site that decides this is
`application.rs`'s match, not the CPS transform — adding the prompt names
there is the likely shape of the fix, not teaching `cps_transform` a new
special case.

**What implementing it involves**: emit `Prompt`/`Abort` (or handle the names
at apply time), make delimited capture actually delimited, and integrate with
`dynamic-wind` and the exception-handler stack — which is where every one of
the VM's four prompt PRs went wrong. Acceptance criteria already exist: twelve
`UNSUPPORTED` rows in `control_flow_matrix.rs` with values measured against
Guile.

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
