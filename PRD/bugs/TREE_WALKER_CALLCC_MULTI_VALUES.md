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

**Two** tests in `crates/patina-tests/tests/backend_divergence.rs` are
quarantined to the VM by this bug:
- `callcc_multi_value_through_call_with_values`
- `callcc_abort_pattern_through_call_with_values`

They were among three `#[cfg(feature = "vm-backend")]` tests in
`cps_features.rs` until the `vm-backend` feature was removed on 2026-08-10.
The third — `test_callcc_single_value_through_call_with_values` — turned out
**not to be affected by this bug at all**: the tree-walker returns `42`
correctly. It had been swept into the gate along with its two neighbours, and
was only caught when the quarantine was changed to assert that the tree-walker
actually fails. It is now a plain both-backends test back in `cps_features.rs`.
The scope of this bug is *multi-value* continuation returns, as the title says.

Each is written as `assert_divergence(code, On::Vm, expected, "…")`, which
asserts the VM's correct answer **and** that the tree-walker still fails. So
**fixing this bug will make those three tests fail** — that is intentional. The
panic message tells you to replace each `assert_divergence` call with a plain
`assert_program_eval_to`, which puts both backends back under the same
expectation. Delete this section at the same time.
