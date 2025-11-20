# Parameter Bug - Parameterize Not Working

**Date:** 2025-11-19
**Status:** Bug identified, fix needed
**Priority:** HIGH - Blocks parameter functionality completion

## Summary

The `parameterize` special form is not working correctly. When a parameter is rebound inside `parameterize`, the new value is not visible to code in the body. The parameter continues to return its original value.

## Reproduction

```scheme
(define p (make-parameter 10))
(display (p))  ; => 10 ✓

(parameterize ((p 20))
  (display (p)))  ; => 10 ✗ (should be 20)

(display (p))  ; => 10 ✓
```

**Expected:** Inside parameterize body, `(p)` should return 20
**Actual:** Inside parameterize body, `(p)` still returns 10

## Root Cause

The issue is that when `parameterize` evaluates the parameter expression to get the parameter object, it receives a **different instance** with a **different `Rc<RefCell<Value>>` pointer** than the one that's called in the body.

### Debug Evidence

```
[DEBUG] param Rc pointer: 0x16dba10c8           <- Parameter in binding
[DEBUG] Setting parameter value to: 20
[DEBUG] Old value before set: 10
[DEBUG] New value after set: 20                  <- Successfully set to 20
[DEBUG] About to evaluate body
[DEBUG] Calling parameter, Rc pointer: 0x102dbad90, value: 10  <- Different pointer!
```

**Key observation:** The Rc pointers are different:
- Binding evaluation: `0x16dba10c8` (modified to 20)
- Parameter call in body: `0x102dbad90` (still has value 10)

### Why This Happens

1. When `(define p (make-parameter 10))` executes, a `Value::Parameter` with `Rc<RefCell<Value>>` is stored in the environment
2. When `parameterize` evaluates `p` to get the parameter:
   - It calls `evaluator.eval_in_env(&param_expr, env)`
   - This looks up `p` in environment and calls `environment.get()`
   - `environment.get()` returns `Some(value.clone())` (line 52 of environment.rs)
   - **BUT:** Somewhere in the evaluation chain, a NEW parameter is created instead of returning the existing one
3. When `(p)` is called in the body:
   - It looks up `p` in environment again
   - Gets the ORIGINAL parameter (with pointer `0x102dbad90`)
   - Returns its value (still 10)

The modified parameter (pointer `0x16dba10c8`) is a different object that's discarded after parameterize completes.

## Why Rc Clone Should Work (But Doesn't)

In theory, cloning a `Value::Parameter` should preserve the `Rc<RefCell<Value>>`:

```rust
// Value definition
Parameter {
    value: Rc<RefCell<Value>>,  // Rc clone shares same data
    converter: Option<Box<Value>>,
}

// Environment.get() clones the value
pub fn get(&self, name: &str) -> Option<Value> {
    if let Some(value) = self.bindings.borrow().get(name) {
        Some(value.clone())  // Clone should preserve Rc
    }
    // ...
}
```

When `Value::Parameter` is cloned:
- The `Rc<RefCell<Value>>` field is cloned → increments ref count, shares same data ✓
- The `Option<Box<Value>>` field is cloned → creates new Box (OK) ✓

**So why different pointers?** The evaluation must be creating a fresh parameter somewhere instead of just looking it up.

## Investigation Needed

Need to trace through the evaluation of `p` in the parameterize binding to see where the fresh parameter is created:

1. Check if there's macro expansion happening
2. Check if eval_in_env does something unexpected
3. Check if there's any special handling of parameters during evaluation
4. Add more debug output to trace the evaluation path

## Current Implementation

**File:** `crates/patina-tree-walker/src/eval/special_forms/parameterize.rs`

```rust
// Evaluate param expression
let param = evaluator.eval_in_env(&param_expr, env)?;

// Verify it's a parameter and save old value
match &param {
    Value::Parameter { value, .. } => {
        old_values.push(value.borrow().clone());
        let new_val = evaluator.eval_in_env(&value_expr, env)?;
        params_and_values.push((param.clone(), new_val));
    }
    // ...
}

// Set new parameter values
for (param, new_val) in &params_and_values {
    match param {
        Value::Parameter { value, converter } => {
            // Apply converter if needed
            *value.borrow_mut() = converted_val;  // Modifies the WRONG instance
        }
    }
}
```

The bug is that `param` from the binding is a different instance than what `(p)` retrieves in the body.

## Potential Solutions

### Option 1: Modify in Environment (Correct Approach)

Instead of modifying the parameter object, look up the parameter in the environment and modify it there:

```rust
// Instead of storing param copies, store just the names
let mut param_names = Vec::new();

// When processing bindings
let param_name = match &param_expr {
    Value::Symbol(name) => name.clone(),
    _ => return Err(...),
};
param_names.push(param_name);

// Set new values by modifying params in environment
for (name, new_val) in param_bindings {
    if let Some(Value::Parameter { value, .. }) = env.get(&name) {
        *value.borrow_mut() = new_val;  // Modifies the actual param in env
    }
}
```

**Problem:** This only works if param expression is a simple symbol, not `(car params)` etc.

### Option 2: Use Parameter Stack (R7RS Approach)

Parameters should maintain their own stack of values. When `parameterize` rebinds a parameter, it pushes a new value onto the stack. When parameterize exits, it pops the stack.

```rust
// In Value::Parameter
Parameter {
    values: Rc<RefCell<Vec<Value>>>,  // Stack of values
    converter: Option<Box<Value>>,
}

// Parameter call returns top of stack
fn call_parameter() {
    values.borrow().last().cloned()
}

// Parameterize pushes/pops
fn parameterize() {
    for (param, new_val) in bindings {
        param.values.borrow_mut().push(new_val);
    }
    // evaluate body
    for (param, _) in bindings {
        param.values.borrow_mut().pop();
    }
}
```

**Advantage:** Works with any parameter expression, properly handles errors
**Disadvantage:** Requires changing Parameter representation

### Option 3: Environment Extension (Simplest Fix)

Create a new environment that shadows the parameter bindings:

```rust
// Create new environment with shadowed parameters
let new_env = Rc::new(Environment::with_parent(env.clone()));

for (param_expr, value_expr) in bindings {
    // Param expr must be a symbol for now
    let name = extract_symbol(&param_expr)?;
    let new_val = evaluator.eval_in_env(&value_expr, env)?;

    // Get the original parameter
    if let Some(Value::Parameter { value, converter }) = env.get(&name) {
        // Create a NEW parameter with the new value but same converter
        let new_param = Value::Parameter {
            value: Rc::new(RefCell::new(new_val)),
            converter: converter.clone(),
        };
        new_env.define(name, new_param);
    }
}

// Evaluate body in new environment
```

**Advantage:** Simple, uses existing environment mechanism
**Disadvantage:** Creates new parameter objects, doesn't preserve identity

## Recommended Fix

**Option 2 (Parameter Stack)** is the most correct approach and matches how parameters are typically implemented in Scheme. It handles all edge cases properly and supports error handling with automatic cleanup.

## Test Status

**Working:**
- ✅ `make-parameter` creates parameters
- ✅ `(p)` gets value
- ✅ `(p new-val)` sets value
- ✅ Parameter with converter

**Failing:**
- ❌ `parameterize` rebinding (returns old value)
- ❌ `parameterize` restoration (N/A - rebinding doesn't work)
- ❌ Nested `parameterize`
- ❌ Multiple parameters in `parameterize`

**Test file:** `crates/patina-tests/tests/parameters.rs` (12 tests, 5 failing)

## Impact on r7rs-tests.scm

Parameters are used in r7rs-tests.scm for the `radix` parameter test. This test will fail until parameterize is fixed.

```scheme
(define radix (make-parameter 10 ...))
(test "1100" (parameterize ((radix 2))
  (f 12)))  ; Currently fails - returns "12" instead of "1100"
```

## Files Modified

1. `crates/patina-runtime/src/value.rs` - Added `Parameter` variant
2. `crates/patina-tree-walker/src/eval/primitives/parameters.rs` - `make-parameter` implementation
3. `crates/patina-tree-walker/src/eval/application.rs` - Parameter call handling
4. `crates/patina-tree-walker/src/eval/special_forms/parameterize.rs` - `parameterize` (buggy)
5. `crates/patina-runtime/src/stdlib/scheme_base.rs` - Export `make-parameter`
6. `crates/patina-tests/tests/parameters.rs` - Integration tests

## Debug Output Location

Temporary debug output added to:
- `parameterize.rs` lines 98, 128-130, 150, 155
- `application.rs` line 191

**Remove before committing the fix!**

## Resolution (2025-11-19)

**FIXED** ✅ All 12 tests passing!

### The Real Bug: Tail Call Optimization Interaction

After extensive debugging, the root cause was discovered to be a **subtle interaction between tail call optimization and parameter stack management**, NOT an Rc cloning issue as initially suspected.

### What We Initially Thought

The debug output showed different Rc pointer addresses when cloning parameters, leading to the hypothesis that `Rc::clone()` wasn't working properly:
```
[ENV-DEBUG] get(p) Rc pointer: 0x87310b3c8  (environment)
[DEBUG] After eval Rc pointer: 0x16bece828  (cloned parameter)
```

However, testing revealed that `{:p}` format prints the address of the Rc **smart pointer itself**, not the data it points to. Multiple Rc smartpointers can point to the same data with different addresses - this is expected behavior!

### The Actual Problem

The bug occurred because of **premature stack popping in tail call optimization**:

1. `parameterize` evaluates its body in tail position
2. When the last expression is detected, the tail call optimization path is taken
3. **BUG:** The code popped the parameter stack BEFORE returning the `TailCall` result
4. By the time the tail-called expression was evaluated, the stack had already been reverted
5. Result: parameter calls saw the old value instead of the new value

Debug trace showing the bug:
```
[DEBUG] param[0] stack before body: [Integer(10), Integer(20)], strong_count: 2
[DEBUG] POPPING for tail call              <-- BUG: Popping too early!
[DEBUG] Before pop: [Integer(10), Integer(20)]
[DEBUG] After pop: [Integer(10)]
[DEBUG] Calling parameter: stack: [Integer(10)]  <-- Wrong value!
```

### The Fix

**Disable tail call optimization for `parameterize` bodies** (lines 175-184 in `parameterize.rs`):

```rust
// TODO: Bad interaction between tail call optimization and parameterize
// We don't use tail call optimization for parameterize because we need to
// ensure the parameter stack is popped AFTER the body completes, not before.
// If we did tail call optimization, we would pop the stack before returning
// the TailCall result, causing the called procedure to see the old parameter value.
//
// The proper solution would be to implement dynamic-wind or use a similar
// mechanism that ensures cleanup happens after tail calls complete.
// For now, we sacrifice tail call optimization in parameterize bodies.
result = evaluator.eval_in_env(&pair.0, env)?;
```

### Implementation: Option 2 (Parameter Stack)

We implemented **Option 2** from the analysis - using a stack of values:

1. **Changed `Value::Parameter`** to use `Rc<RefCell<Vec<Value>>>` (stack) instead of `Rc<RefCell<Value>>` (single value)
2. **Updated `make-parameter`** to initialize with `vec![initial_value]`
3. **Updated parameter calls** to use `.last()` (get) and `.last_mut()` (set)
4. **Updated `parameterize`** to `push()` new values and `pop()` after body completes

This approach correctly shares state via Rc - all clones of a parameter share the same stack.

### Testing Verification

```bash
cargo test --package patina-tests --test parameters
```

Results: **12/12 tests passing** ✅
- `test_make_parameter_basic` ✅
- `test_parameter_set` ✅
- `test_parameterize_simple` ✅
- `test_parameterize_restores` ✅
- `test_parameterize_multiple_params` ✅
- `test_parameterize_nested` ✅
- `test_parameterize_nested_restores` ✅
- `test_parameter_with_converter` ✅
- `test_parameter_converter_on_set` ✅
- `test_parameterize_empty_body_error` ✅
- `test_parameterize_non_parameter_error` ✅
- `test_parameterize_body_sequence` ✅

### Lessons Learned

1. **`{:p}` format can be misleading** - Different Rc smartpointer addresses doesn't mean different data
2. **Tail call optimization has hidden interactions** with cleanup code (parameters, exception handlers, etc.)
3. **R7RS `dynamic-wind`** is designed specifically to solve this class of problems
4. **Extensive debug output was essential** - The bug was found by tracing execution line-by-line
5. **Simple tests confirmed theory** - Creating minimal repro cases (HashMap clone test) validated assumptions

### Future Work / TODOs

**TODO: Implement `dynamic-wind` for proper tail call support in `parameterize`**

Currently, `parameterize` disables tail call optimization to ensure parameters are restored after the body completes. This is correct but not optimal.

The proper R7RS solution is to implement `dynamic-wind` (R7RS section 6.10), which allows:
- Cleanup code to run even when tail calls occur
- Proper interaction with continuations (`call/cc`)
- Correct exception handling with cleanup guarantees

Implementation steps:
1. **Add winding stack to evaluator** - Track (before, after) thunk pairs
2. **Implement `dynamic-wind` primitive** - Push/pop winding stack, call thunks
3. **Integrate with tail call optimization** - Ensure "after" thunks run before tail calls
4. **Reimplement `parameterize` as macro** - Expand to `dynamic-wind` (see `CHIBI_PARAMETERIZE_ANALYSIS.md`)
5. **Simplify `Value::Parameter`** - Can remove stack, use single value with mutation

**References:**
- See `internal/CHIBI_PARAMETERIZE_ANALYSIS.md` for how chibi-scheme does it
- Chibi implements `parameterize` as a macro that expands to `dynamic-wind`
- The VM tracks a "winding stack" that ensures cleanup happens even with tail calls

**Other Future Work:**
- Consider if other special forms have similar tail call/cleanup interactions
- Profile performance impact of disabled tail call optimization
- Implement `call/cc` (also requires winding stack integration)

### Files Modified

1. `crates/patina-runtime/src/value.rs` - Changed `Parameter` to use stack
2. `crates/patina-tree-walker/src/eval/primitives/parameters.rs` - Initialize stack
3. `crates/patina-tree-walker/src/eval/application.rs` - Use stack top for get/set
4. `crates/patina-tree-walker/src/eval/special_forms/parameterize.rs` - Push/pop + disable TCO

---

**Related:**
- R7RS spec section 4.2.6 (Dynamic bindings)
- R7RS spec section 6.10 (`dynamic-wind`)
- Reference implementation in `spec/r7rs-small-spec/derive.tex`
