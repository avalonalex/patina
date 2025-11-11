# General Tail Call Optimization for All Procedures

**Status:** Future Enhancement (Post Phase 1)
**Priority:** Medium
**Complexity:** High
**Inspiration:** chibi-scheme's approach

## Current State (Phase 1)

### How Tail Calls Work Today

Patina currently implements **selective tail call optimization**:

1. **Special forms** explicitly return `EvalResult::TailCall` for tail positions
2. **Lambda procedures** in tail position are optimized (see `eval/mod.rs:260-320`)
3. **Primitive procedures** are NOT tail-optimized - they always use the stack

### The Dual Nature Problem

Some procedures need to be **both** a special form and a primitive:

- **`call-with-values`**: Special form for tail calls + primitive for environment
- **`apply`**: Currently only handles as special form (TODO for tail optimization)

This dual nature is necessary because:
1. Special forms can return `TailCall` for optimization
2. Primitives exist in the environment as first-class values
3. Without the primitive, macros can't reference the symbol

## Chibi-Scheme's Approach

### How Chibi Does It

Chibi-scheme uses **automatic tail call optimization for ALL procedures**:

```c
// From chibi eval.c (simplified)
sexp_context_tailp(ctx) = 1;  // Mark context as tail position
result = sexp_apply(ctx, proc, args);  // ANY procedure application
```

Key insights:
1. **No special forms for `call-with-values`** - it's just a regular Scheme procedure defined in `lib/init-7.scm`
2. **Context tracking** - the evaluator tracks whether it's in tail position via `tailp` flag
3. **Automatic optimization** - ANY procedure call in tail position gets optimized, whether it's:
   - A lambda
   - A primitive
   - A call-with-values
   - Anything else!

### Benefits

✅ **Simpler design** - No dual-nature procedures
✅ **More general** - Works for user-defined tail-recursive procedures too
✅ **First-class procedures** - All procedures work the same way
✅ **Cleaner code** - Fewer special cases

### Trade-offs

❌ **More complex evaluator** - Needs sophisticated context tracking
❌ **Requires primitive refactor** - Primitives must support tail calls
❌ **Potential performance impact** - More bookkeeping overhead

## Proposed Design

### Phase 1: Enhance EvalResult for Primitives

**Current:**
```rust
pub(crate) enum EvalResult {
    Value(Value),
    TailCall { expr: Value, env: Rc<Environment> },
}
```

**Proposed:**
```rust
pub(crate) enum EvalResult {
    Value(Value),
    TailCall { expr: Value, env: Rc<Environment> },
    // New: Primitives can request tail call optimization
    TailCallPrimitive {
        proc: Value,      // The procedure to call
        args: Vec<Value>, // Already-evaluated arguments
    },
}
```

### Phase 2: Update Primitive Signature

**Current:**
```rust
pub(super) fn apply_primitive(
    &self,
    name: &str,
    args: Vec<Value>,
) -> Result<Value, EvalError>
```

**Proposed:**
```rust
pub(super) fn apply_primitive(
    &self,
    name: &str,
    args: Vec<Value>,
    in_tail_position: bool,  // NEW: context from evaluator
) -> Result<EvalResult, EvalError>  // NEW: can return TailCall
```

### Phase 3: Update call-with-values Implementation

**Current (Special Form + Primitive):**
```rust
// In eval/mod.rs
"call-with-values" => {
    return self.eval_call_with_values_impl(&cdr, env, in_tail_position)
}

// In primitives/values.rs
pub(super) fn call_with_values(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    // Can't do tail calls!
}
```

**Proposed (Just Primitive):**
```rust
// No special form needed!

// In primitives/values.rs
pub(super) fn call_with_values(
    evaluator: &Evaluator,
    args: Vec<Value>,
    in_tail_position: bool,  // NEW
) -> Result<EvalResult, EvalError> {  // NEW
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    }

    let producer = &args[0];
    let consumer = &args[1];

    // Call producer with no arguments
    let produced = evaluator.apply(producer.clone(), vec![])?;

    // Unpack multiple values
    let consumer_args = match produced {
        Value::Values(vals) => vals,
        other => vec![other],
    };

    // If in tail position, request tail call optimization
    if in_tail_position {
        Ok(EvalResult::TailCallPrimitive {
            proc: consumer.clone(),
            args: consumer_args,
        })
    } else {
        Ok(EvalResult::Value(
            evaluator.apply(consumer.clone(), consumer_args)?
        ))
    }
}
```

### Phase 4: Update Trampoline

```rust
// In eval/mod.rs
pub fn eval(&self, expr: &Value) -> Result<Value, EvalError> {
    let mut current_expr = expr.clone();
    let mut current_env = self.global_env.clone();

    loop {
        match self.eval_step(&current_expr, &current_env)? {
            EvalResult::Value(v) => return Ok(v),

            EvalResult::TailCall { expr, env } => {
                current_expr = expr;
                current_env = env;
            }

            // NEW: Handle primitive tail calls
            EvalResult::TailCallPrimitive { proc, args } => {
                // Construct application and continue trampolining
                let mut app_list = vec![proc];
                app_list.extend(args);
                current_expr = self.list_from_vec(app_list);
                // Keep same environment
            }
        }
    }
}
```

## Migration Path

### Step 1: Add TailCallPrimitive variant
- Add to `EvalResult` enum
- Update trampoline to handle it
- **No breaking changes** - old code still works

### Step 2: Update primitive system
- Add `in_tail_position` parameter to `apply_primitive`
- Change return type to `Result<EvalResult, _>`
- Update all primitives to return `EvalResult::Value(...)` (mechanical change)

### Step 3: Migrate special forms to primitives
- Convert `call-with-values` from dual-nature to pure primitive
- Remove special form dispatch
- Remove from hygiene special forms list

### Step 4: Test thoroughly
- Ensure all tail recursion tests still pass
- Benchmark performance impact
- Test with deep recursion (10,000+ iterations)

## Testing Strategy

```rust
#[test]
fn test_primitive_tail_recursion() {
    // call-with-values in deep tail recursion
    assert_program_eval_to(
        r#"
        (define (countdown n acc)
          (call-with-values
            (lambda () (values n acc))
            (lambda (a b)
              (if (= a 0)
                  b
                  (countdown (- a 1) (+ b 1))))))

        (countdown 10000 0)
        "#,
        "10000",
    );
}

#[test]
fn test_user_defined_tail_recursive_combinator() {
    // User defines their own call-with-values-like combinator
    assert_program_eval_to(
        r#"
        (define (my-with-values producer consumer)
          (let ((result (producer)))
            (consumer result)))

        (define (countdown n)
          (my-with-values
            (lambda () n)
            (lambda (x)
              (if (= x 0)
                  'done
                  (countdown (- x 1))))))

        (countdown 1000)
        "#,
        "done",
    );
}
```

## Performance Considerations

### Overhead
- Extra `in_tail_position` parameter passed to all primitives
- Pattern matching on `EvalResult` in trampoline
- Potentially more heap allocations for `TailCallPrimitive`

### Mitigation
- Use `#[inline]` hints for hot paths
- Benchmark before/after
- Consider making tail call optimization opt-in per primitive

## Open Questions

1. **Should ALL primitives support tail calls?**
   - Option A: Yes, for consistency
   - Option B: Only specific ones (call-with-values, apply, user combinators)

2. **How to handle primitives that can't meaningfully be tail calls?**
   - e.g., `+`, `cons`, `car` - they don't call other procedures
   - Just always return `Value`?

3. **What about apply?**
   - Should also benefit from this system
   - Currently marked TODO in code

4. **Performance impact acceptable?**
   - Need to benchmark
   - Tail calls are less common than regular calls

## References

- **chibi-scheme eval.c**: Context-based tail call tracking
- **R7RS Section 3.5**: Proper tail recursion requirements
- **Patina eval/mod.rs:260-320**: Current lambda tail call optimization
- **This session's investigation**: Why dual-nature is currently needed

## Conclusion

This enhancement would make Patina's design cleaner and more general, following chibi-scheme's elegant approach. However, it's a significant refactor that should wait until after Phase 1 is complete and the core evaluator is stable.

The current dual-nature approach (special form + primitive) is a reasonable pragmatic solution that meets R7RS requirements while keeping the implementation tractable.
