# Tree-walker: call/cc continuations don't deliver multiple values

**Status:** Fixed 2026-08-25
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

## Fix (2026-08-25)

`crates/patina-tree-walker/src/eval/cps_eval/application.rs`, in the branch
that invokes a captured continuation: a call with any argument count but one
now delivers a `#<values>` heap object, exactly as `(values …)` returns one,
instead of raising a wrong-arity error. `call-with-values` unpacks it; a
plain continuation receives the object. The VM had reached the same protocol
from the other direction in #113, when its `value_buffer` side channel was
removed — so the two backends now agree by construction rather than by two
mechanisms.

The prediction below ("the continuation closure should handle multi-value
returns by populating the values continuation") was aimed at the VM's old
buffer design, which no longer exists.

**What this was costing, unrecognised.** SRFI 1's `%cars+cdrs` bails out of
an exhausted list with `(abort '() '())`. Every n-ary procedure that walks
more than one list goes through it, so `zip`, `fold`, `any`, `every` and
`list-index` over two or more lists all failed on the tree-walker — nine
assertions in Larceny's `list` suite, tracked separately as triage family 5
and diagnosed as "undiagnosed beyond" the `apply` shape. It was this bug.

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

Both were `assert_divergence(code, On::Vm, expected, "…")` — asserting the
VM's correct answer *and* that the tree-walker still failed — so the fix made
them fail, as designed, and they are now plain `assert_program_eval_to` calls
holding both backends to the same expectation. The abort-pattern test carries
the SRFI 1 shapes with it, so the connection above cannot be lost again.
