# Delimited Continuations Design

This document covers the design for implementing full delimited continuation support (shift/reset, prompts).

**Status**: **Done on both backends.** VM: PRs #158–#172 (2026-09-02/03).
Tree-walker: issue #169, closed 2026-09-04.

Written December 2025, when neither backend had prompts. Everything below the
next section is a *design sketch from then*, and neither implementation
followed it: Option A was chosen, but the VM implemented it against
`docs/VM_RUNTIME.md` §5.5–§5.6, and the tree-walker as a boundary value in
the CPS continuation (`crates/patina-tree-walker/src/eval/cps_eval/prompts.rs`,
whose header is the design). **Read `docs/VM_RUNTIME.md` §5.6,
`crates/patina-tests/tests/control_flow_matrix.rs` and that header before
this file.** The sections here that were measured and corrected on
2026-09-03/04 are marked; the rest is unverified 2025 sketch.

---

## Problem Statement

Current continuation support (**re-measured 2026-09-04**; the 2025 list this
replaces claimed partial prompt support on both backends, which was never
true, and the 2026-09-03 table recorded the tree-walker as lacking all of it):

| | VM | tree-walker |
|---|---|---|
| `call/cc`, `dynamic-wind` | ✅ | ✅ |
| `call-with-continuation-prompt` | ✅ | ✅ (2026-09-04, #169) |
| `abort-current-continuation` | ✅ | ✅ (2026-09-04, #169) |
| delimited capture, composable invoke | ✅ (#160, #164, #166) | ✅ (2026-09-04, #169) |
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

**Corrected 2026-09-03, and again 2026-09-04.** The list this replaces said
basic prompt installation, prompt tags and abort-to-prompt all worked. On the
tree-walker none of it did until 2026-09-04; on both backends it now works by
a mechanism this document does not describe.

**The VM**: complete. `call-with-continuation-prompt` and
`abort-current-continuation` are intercepted by name in `vm_control_primitive`
before any registry lookup, an abort travels to a synthesized continuation
whose top frame calls the prompt handler, and a composable invoke appends the
captured region in place. See `docs/VM_RUNTIME.md` §5.5–§5.6. The last defect against it, #167, was closed
by PR #172; `control_flow_matrix.rs` is where the next one shows up.

**The tree-walker**: complete as of 2026-09-04 (issue #169), by the
apply-time route the 2026-09-03 version of this section predicted. The two
names are claimed in `cps_eval/application.rs`'s short-name match beside
`dynamic-wind`, so they work as values too, and the implementation is
`cps_eval/prompts.rs`, whose module header is the design: a prompt is a
boundary *value* in the continuation (`ContValue::PromptBoundary`) with the
caller's continuation waiting in a `PromptFrame`; a delimited continuation is
the aborting call's own continuation chain plus the dynamic state above the
frame; an abort *jumps* to a landing whose stacks are cut back to the frame;
a composable invoke re-enters the captured extents one step each and returns
through its boundary. `CpsContinuation` carries the prompt stack now, which
it always should have.

What went with it: the `Prompt`/`Control`/`Abort` CPS IR nodes and their
evaluator arms, which nothing ever emitted (the `Abort` arm had panicked on
its only-prompt case since it was written; corrected 2026-09-03 and deleted
2026-09-04); the placeholder delimited capture in `wind.rs`; and the #170
"not implemented" registrations. Acceptance was the twelve
`control_flow_matrix.rs` rows and the seven prompt tests in
`cps_features.rs`, all of which now run on both backends and agree.

**Which name is claimed where**, for the next control primitive:

| | VM | tree-walker |
|---|---|---|
| `call/cc`, `call-with-current-continuation` | `vm_control_primitive` | **syntactically**, `cps_transform.rs`'s `is_callcc_reference` |
| `dynamic-wind`, `apply`, `raise`, `error`, `force`, `call-with-values`, the two prompt names | `vm_control_primitive` | at **apply** time, short-name match in `cps_eval/application.rs` |

An apply-time match sees the primitive whatever name reached it; a syntactic
one only sees the call it was written in — which is why `call/cc` still does
not work as a value (Track Q §1.2).

**What is still shared with `call/cc`**: a nested trampoline
(`apply_from_direct_tagged`, which Rust primitives call back through) starts
every stack empty, so an abort from inside such a callback to a prompt outside
it reports no matching prompt. Winds and handlers have had that gap since
before prompts (the "primitive's callback" entry in the triage doc).

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

- [x] `call-with-continuation-prompt` captures up to prompt — both backends
- [x] `abort-current-continuation` works correctly — both backends
- [ ] `shift`/`reset` can be implemented as library — nothing stops it now;
      not done, not tested
- [x] The seven prompt tests in `cps_features.rs` and the twelve prompt rows of
      `control_flow_matrix.rs` pass on both backends (the "7 ignored tests"
      this line used to name no longer exist)
- [x] `dynamic-wind` interacts correctly with delimited escapes — both
      backends, measured against Guile and Racket

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
