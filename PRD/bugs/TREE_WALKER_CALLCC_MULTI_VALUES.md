# Tree-walker: call/cc continuations don't deliver multiple values

**Status:** Open
**Severity:** Medium
**Created:** 2026-03-19
**Backend:** Tree-walker only (VM backend fixed)

## Problem

When a `call/cc` continuation is invoked with multiple arguments through `call-with-values`, the tree-walker raises a wrong-arity error instead of delivering all values to the consumer.

## Reproducer

```scheme
(call-with-values
  (lambda ()
    (call-with-current-continuation
      (lambda (k) (k 1 2))))
  (lambda (a b) (list a b)))
;; Expected: (1 2)
;; Actual: error — "Wrong number of arguments: expected 1, got 2"
```

Single-value case also fails:

```scheme
(call-with-values
  (lambda ()
    (call-with-current-continuation
      (lambda (k) (k 42))))
  (lambda (x) x))
;; Expected: 42
;; Actual: error — "Wrong number of arguments: expected 1, got 2"
```

## Impact

- Blocks SRFI 1 `%cars+cdrs` abort pattern on tree-walker (uses `(abort '() '())`)
- Any code using `call/cc` continuation invocation with multiple values through `call-with-values`

## Notes

The VM backend fixed this with two changes:
1. `try_invoke_continuation` populates `value_buffer` with extra args
2. After continuation invocation in Call/TailCall dispatch, exits `run_loop_until` if frames dropped to exit_depth

The tree-walker likely needs an analogous fix in its CPS evaluator — the continuation closure should handle multi-value returns by populating the values continuation.

## Tests

Three tests in `crates/patina-tests/tests/cps_features.rs` are gated to `#[cfg(feature = "vm-backend")]` due to this bug:
- `test_callcc_multi_value_through_call_with_values`
- `test_callcc_single_value_through_call_with_values`
- `test_callcc_abort_pattern_through_call_with_values`
