# CPS Continuation Escape - Technical Debt

## Current State

When `call/cc` captures a continuation and that continuation is invoked inside a Scheme procedure like `for-each` or `map`, we need a mechanism to escape back to the main CPS evaluation loop.

### The Problem

1. Library code (e.g., `for-each` in `lib/scheme/base/higher_order.scm`) is loaded in **direct mode**
2. This creates `Procedure::Lambda` instead of `Procedure::CpsLambda`
3. When user code runs in CPS mode and passes a CPS lambda to `for-each`:
   - `for-each` calls the CPS lambda via `apply_from_direct`
   - If the CPS lambda invokes a captured continuation, it needs to abort `for-each`
   - But `for-each` doesn't know about CPS continuations

### Current Solution: Thread-Local Escape

We use thread-local storage to pass continuation escape data:

```rust
// crates/patina-tree-walker/src/eval/cps_eval.rs
thread_local! {
    static PENDING_ESCAPE: RefCell<Option<(Value, Rc<CpsContinuation>)>> = const { RefCell::new(None) };
}
```

When `Value::Continuation` is invoked:
1. Store `(value, continuation)` in thread-local
2. Return `EvalError::ContinuationEscape` (a marker error, no data)
3. Error propagates through direct evaluator's call stack
4. Main `eval` loop catches the error, retrieves data from thread-local, resumes

### Why Thread-Local?

`EvalError` must be `Send + Sync` (required by `Backend::Error` trait), but `Rc<CpsContinuation>` is not thread-safe. Thread-local avoids putting non-`Send` data in the error.

## Future Work: Remove Thread-Local Hack

Once we remove the `--cps` flag and make CPS the **only** evaluation mode:

### Option 1: Load Libraries in CPS Mode

**Approach**: When libraries are loaded, transform their code with CPS transformation.

**Benefits**:
- `for-each`, `map`, etc. become `Procedure::CpsLambda`
- Continuation invocation works naturally within CPS evaluation
- No escape mechanism needed

**Challenges**:
- Library loading happens at startup before mode is known
- Need to ensure all library code CPS-transforms correctly
- May impact startup time slightly

**Implementation**:
1. Remove `EvalMode` enum from `TreeWalker`
2. Change `Evaluator::evaluate_parsed_library` to use `eval_cps` instead of `eval_in_env`
3. Change `Evaluator::load_library_extras` to use CPS evaluation
4. Remove `apply_from_direct` and thread-local escape mechanism
5. Remove `EvalError::ContinuationEscape`

### Option 2: Keep Hybrid but Improve

If hybrid mode is still needed for some reason:

**Approach**: Make escape mechanism cleaner.

**Ideas**:
- Use a dedicated escape channel instead of thread-local
- Make `EvalError` generic over backend to allow non-`Send` data
- Use `Arc` instead of `Rc` for continuation data

## Files to Modify

When implementing the cleanup:

- `crates/patina-tree-walker/src/backend.rs` - Remove `EvalMode`, make CPS default
- `crates/patina-tree-walker/src/eval/cps_eval.rs` - Remove thread-local, `PENDING_ESCAPE`, escape functions
- `crates/patina-tree-walker/src/eval/error.rs` - Remove `ContinuationEscape` variant
- `crates/patina-tree-walker/src/eval/mod.rs` - Change library loading to use CPS
- `crates/patina-tree-walker/src/eval/application.rs` - Remove `Procedure::CpsLambda` special case (all lambdas will be CPS)
- `crates/patina-repl/src/main.rs` - Remove `--cps` flag

## Priority

Low - The current solution works correctly. This is technical debt cleanup to be addressed when:
1. CPS mode is stable and well-tested
2. We're ready to remove the `--cps` flag
3. All tests pass in CPS-only mode

## Related

- `PRD/phase1/IMPLEMENTATION_STATUS.md` - Overall roadmap
- `crates/patina-tree-walker/src/eval/cps_eval.rs` - Current implementation
