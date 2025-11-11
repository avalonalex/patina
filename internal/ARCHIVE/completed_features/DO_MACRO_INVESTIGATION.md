# Do Macro Investigation

**Date:** 2025-11-11
**Status:** Special form kept, macro implementation documented

## Summary

We investigated implementing the `do` loop as a macro to avoid nested ellipsis. We found a working solution using `apply`, but it has tail call optimization limitations. The special form remains active for proper TCO support.

## The Challenge

The standard R7RS `do` macro requires nested ellipsis:

```scheme
(define-syntax do
  (syntax-rules ()
    ((do ((var init step ...) ...)
         (test result ...)
       command ...)
     (let loop ((var init) ...)
       (if test
           (begin result ...)
           (begin
             command ...
             (loop step ... ...)))))))  ; Nested ellipsis!
```

The `step ... ...` pattern means "for each binding, expand its step (if present), then expand all bindings." Patina doesn't yet support this.

## Solutions Investigated

### 1. Auxiliary Macro with Normalization ✅ (Partial)

**Approach:** Use helper macro to transform optional steps into explicit steps first.

```scheme
(define-syntax do-normalize
  (syntax-rules ()
    ((do-normalize ((var init step) ...) () test-clause body ...)
     (do ((var init step) ...) test-clause body ...))
    ((do-normalize (done ...) ((var init step) rest ...) test body ...)
     (do-normalize (done ... (var init step)) (rest ...) test body ...))
    ((do-normalize (done ...) ((var init) rest ...) test body ...)
     (do-normalize (done ... (var init var)) (rest ...) test body ...))))
```

**Result:** Successfully handles optional steps, but still need to solve the loop call problem.

### 2. Using Apply with List ✅ (Works, but TCO issue)

**Approach:** Use `(apply loop (list step ...))` instead of `(loop step ... ...)`.

```scheme
(define-syntax do
  (syntax-rules ()
    ((do ((var init step) ...) (test result ...) body ...)
     (letrec ((loop (lambda (var ...)
                      (if test
                          (begin result ...)
                          (begin
                            body ...
                            (apply loop (list step ...)))))))
       (loop init ...)))))
```

**Why it works:**
- `(list step ...)` is single-level ellipsis → creates list of step values
- `apply` unpacks the list and calls `loop` with values as arguments
- Avoids nested ellipsis entirely!

**Problem:**
- `apply` doesn't yet support tail call optimization in Patina
- Causes stack overflow in test: `test_do_exit_clause_tail_recursive`
- Test has 1000 recursive calls in exit clause: `((= i 1) (helper 1000))`

### 3. Direct Loop Call ❌ (Doesn't work)

**Approach:** Try `(loop step ...)` directly without `apply`.

```scheme
((do ((var init step) ...) (test result ...) body ...)
 (letrec ((loop (lambda (var ...)
                  (if test
                      (begin result ...)
                      (begin body ... (loop step ...))))))
   (loop init ...)))
```

**Problem:**
- This SHOULD work according to R7RS spec
- `step ...` should expand to individual step expressions
- But in Patina, it returns incorrect results when body has commands
- Likely a subtle bug in how ellipsis expansion interacts with `begin`
- Needs deeper investigation into macro expander

## Why let-values Worked (But do Doesn't)

**Key difference:** Sequential vs. parallel processing

### let-values (Sequential)
```scheme
(define-syntax let-values
  (syntax-rules ()
    ((let-values ((formals expression) rest ...) body ...)
     (call-with-values (lambda () expression)
                       (lambda formals
                         (let-values (rest ...) body ...))))))
```

Processes **one binding at a time** via recursion. No nested ellipsis needed.

### do (Parallel)
```scheme
;; ALL variables must be in scope together
(do ((i 0 (+ i 1))
     (sum 0 (+ sum i)))  ; sum's step references i!
    ...)
```

All variables exist in the **same environment** because step expressions can reference any variable. Cannot process sequentially.

## Current Status

### Active Implementation
- **Special form** in `src/eval/special_forms.rs:633-820` (~188 lines)
- Provides proper tail call optimization
- All 36 tail recursion tests pass

### Documented Macro Implementation
- In `lib/bootstrap.scm:248-314` (commented out)
- Uses auxiliary macro `do-normalize` for optional steps
- Uses `apply` to avoid nested ellipsis
- Works correctly for non-tail-recursive cases
- **Not active** due to TCO limitations

## Future Paths

### Path 1: Implement Nested Ellipsis (Proper Solution)
**Effort:** ~2-3 days, 500-800 lines
**Benefit:** Full R7RS macro compliance
**Impact:** Enables proper `do` macro and other advanced macros

**Steps:**
1. Add `depth` field to `Pattern::Ellipsis`
2. Change bindings to multi-dimensional: `Vec<Vec<Value>>`
3. Update template expansion to handle nested iteration
4. Comprehensive testing

**See:** `internal/NESTED_ELLIPSIS_LIMITATION.md` for details

### Path 2: Add TCO to Apply (Workaround)
**Effort:** ~1-2 days
**Benefit:** Enables `do` macro with `apply`
**Impact:** Limited - only helps specific macro patterns

**Problem:** Still doesn't demonstrate true macro power since we're working around the limitation rather than solving it.

### Path 3: Fix Direct Loop Call Bug (Investigation needed)
**Effort:** Unknown - depends on root cause
**Benefit:** Would allow direct `(loop step ...)` without `apply`
**Impact:** May uncover deeper macro expansion issues

**Issue:** `(loop step ...)` in `begin` context returns incorrect results when `body ...` has commands. Needs debugging in macro expander.

### Path 4: Keep Special Form (Current)
**Effort:** 0 (done)
**Benefit:** Works perfectly, proper TCO
**Impact:** Demonstrates evaluator design, maintains performance

## Lessons Learned

1. **Nested ellipsis is rare but impactful**
   - Only affects a handful of macros (`do`, complex pattern matchers)
   - But those macros are important for R7RS compliance

2. **Clever workarounds exist**
   - `apply` + `list` avoids nested ellipsis
   - Auxiliary macros can normalize patterns
   - But workarounds have trade-offs (TCO, complexity)

3. **Special forms have their place**
   - When performance matters (TCO critical for loops)
   - When macro system has limitations
   - For core language constructs

4. **Sequential vs. parallel matters**
   - `let-values` works as macro because it's sequential
   - `do` needs special form because it's parallel
   - Understanding this distinction guides design decisions

## Recommendation

**Keep `do` as a special form** for Phase 1 (R7RS compliance).

Consider implementing nested ellipsis in Phase 2 (gradual typing) when:
- More complex macros become useful
- Educational value of complete macro system increases
- Time permits deeper macro system work

The documented macro implementation serves as:
- Proof of concept for nested ellipsis workarounds
- Educational material for macro design
- Future migration path when TCO in `apply` is ready

## Related Files

- `lib/bootstrap.scm:248-314` - Commented macro implementation
- `src/eval/special_forms.rs:633-820` - Active special form
- `internal/NESTED_ELLIPSIS_LIMITATION.md` - Nested ellipsis analysis
- `internal/ARCHIVE/completed_features/DO_LOOP_IMPLEMENTATION.md` - Original implementation
- `tests/tail_recursion.rs:633-645` - Failing test with macro (TCO issue)

## Test Results

**With special form (current):**
```
cargo test --quiet --test tail_recursion
running 36 tests
....................................
test result: ok. 36 passed; 0 failed; 0 ignored
```

**With macro (commented out):**
```
thread 'test_do_exit_clause_tail_recursive' has overflowed its stack
fatal runtime error: stack overflow
```

## Conclusion

While we successfully implemented a `do` macro that avoids nested ellipsis using the `apply` technique, the lack of tail call optimization in `apply` makes it unsuitable for production use. The special form remains the best choice until either:

1. Nested ellipsis is implemented (proper solution)
2. TCO is added to `apply` (enables workaround)
3. The direct `(loop step ...)` bug is fixed (mysterious issue)

This investigation demonstrates both the power and limitations of macro-based language design, and shows that special forms serve an important role when performance or language limitations require them.
