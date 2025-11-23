# Critical Architecture Review: Patina Macro System
## From a Seasoned Scheme Compiler Engineer's Perspective

**Date**: 2025-01-23
**Context**: Following lexical hygiene implementation that required fixing multiple edge cases

---

## Executive Summary

The current macro system has **fundamental architectural problems** that caused us to encounter numerous edge cases and breaking tests during the lexical hygiene fix. The root cause is **mixing compilation-time and expansion-time concerns** with **three different environment representations**. This creates a fragile system where changes in one place break seemingly unrelated parts.

**Severity**: 🔴 **HIGH** - Current architecture will make future macro features (syntax-case, modules) very difficult.

---

## Critical Issues

### Issue 1: **Environment Schizophrenia** 🔴

**Problem**: Three different representations of "what's in scope":

```rust
// In Compiler (compile time)
env: Option<Rc<dyn Any>>  // Full Environment, type-erased

// In Expander (expansion time)
expansion_env: MacroEnv    // Only macros, rebuilt fresh

// In Identifier (carried around)
env: Option<Rc<dyn Any>>  // Full Environment, type-erased
```

**Why This is Bad**:
1. **Temporal coupling**: `MacroEnv` is built at expansion time, but `Identifier.env` was captured at definition time
2. **Data duplication**: We have the same environment in two forms
3. **Downcasting hell**: Type erasure with `dyn Any` everywhere
4. **Race conditions**: Environment can change between capture and check (we fixed this with compile-time checking, but it's fragile)

**Evidence from implementation session**:
- `temp` was found as "bound" when it shouldn't be
- We had to add compile-time binding check in `compiler.rs:406-416` to fix it
- Had to handle `Identifier` in macro lookup (`mod.rs:517-566`)

**Root Cause**: Trying to avoid circular dependency between `patina-macros` and `patina-runtime` by using `dyn Any`, but this creates type safety holes.

---

### Issue 2: **Value::Identifier is a Runtime Leak** 🔴

**Problem**: Macro system internals (identifiers with captured environments) leak into the runtime Value type.

```rust
// In patina-runtime/src/value.rs:38-43
Value::Identifier {
    name: Rc<str>,
    env: Rc<dyn std::any::Any>,  // Macro system detail in runtime!
}
```

**Why This is Bad**:
1. **Violation of separation**: Runtime shouldn't know about macro hygiene details
2. **Pervasive changes needed**: Every place that handles `Value` needs to handle `Identifier`
   - Lambda parameter parsing (`mod.rs:1218-1242`)
   - Macro call detection (`mod.rs:521-530`)
   - Desugarer (`desugarer/mod.rs:109-111`)
   - Everywhere `Symbol` is matched
3. **No clear lifetime**: When does an `Identifier` become a `Symbol`? (Answer: never, it stays in the AST forever)

**Evidence from implementation session**:
- Had to add `Identifier` handling in 6 different places
- Tests broke because lambda didn't accept `Identifier` parameters
- `let*-values` broke because macro names were `Identifier` in car position

**What Scheme Compilers Do Instead**:
- **Chez**: Uses `syntax objects` that are separate from runtime values
- **Racket**: `syntax` type with source location + scope sets
- **Gauche**: Identifiers exist during macro expansion, converted to symbols before evaluation

---

### Issue 3: **Compile-Time vs Expansion-Time Confusion** 🟡

**Problem**: We check if symbols are bound at **compile time** (when `define-syntax` runs), but this check uses the environment which can **mutate later**.

```rust
// In compiler.rs:406-416 - AT COMPILE TIME
let should_capture = if let Some(env_any) = &self.env {
    if let Some(env) = env_any.downcast_ref::<Environment>() {
        env.get(&s).is_some()  // ← Environment can change!
    }
}
```

**Why This is Bad**:
1. **Shared mutable state**: `Environment` uses `Rc<RefCell<HashMap>>` for bindings
2. **Time-of-check/time-of-use**: We check at compile time, but binding might be added/removed later
3. **Only works by accident**: We're cloning `Rc`, not the contents, so we share the same HashMap

**Wait, how does it work then?**
- When we call `env.clone()`, we clone the `Rc`, **not** the `RefCell` or `HashMap`
- But when we do `env.get()`, we check the **current** state of the HashMap
- This works because Scheme semantics prevent changing bindings in outer scopes
- **BUT**: It's relying on runtime behavior, not compiler guarantees!

**What Should Happen**:
- Snapshot the environment at definition time
- Or use immutable environment chains (like most Scheme compilers)

---

### Issue 4: **MacroEnv is Redundant** 🟡

**Problem**: `MacroEnv` duplicates information that's already in `Environment`.

```rust
fn build_macro_env(&self, env: &Rc<Environment>) -> patina_macros::MacroEnv {
    let mut macro_env = patina_macros::MacroEnv::new();
    // Walk through environment and extract just macros
    self.collect_macros_from_env(env, &mut macro_env);
    macro_env
}
```

Called on **every macro expansion** (`mod.rs:1569`).

**Why This is Bad**:
1. **O(n) overhead**: Traverse entire environment for every expansion
2. **Data duplication**: Same information exists in `Environment`
3. **Synchronization burden**: Must rebuild when environment changes

**Defense**: "It's for separation of concerns!"
- **Counter**: We already pass `Environment` as `Rc<dyn Any>` anyway
- **Counter**: We could just check `matches!(env.get(name), Some(Value::Macro{..}))`

**Verdict**: Premature optimization that adds complexity without clear benefit.

---

### Issue 5: **Hygiene is Too Late** 🟡

**Problem**: Hygiene renaming happens **after** template expansion, which means intermediate AST contains unrenamed symbols.

```rust
// In expander.rs:575-622
fn rename_identifier(&self, id: &Identifier) -> Value {
    // This happens DURING expansion, for each identifier
    // But intermediate template values already have symbols
}
```

**Why This is Bad**:
1. **Multiple passes**: Need to touch every identifier in the output
2. **Stateful**: Uses `RefCell<HashMap>` for renamings within `Expander`
3. **Fragile**: Relies on correct ordering of checks (special forms, macros, free vars, introduced)

**What Real Compilers Do**:
- **Chez/Racket**: Hygiene is built into the syntax object representation
- **Scope Sets** (Racket): Each identifier has a set of scopes, renamed only when resolving
- **Marks** (Chez): Each macro application adds a mark, used for conflict detection

**Better Approach**: Alpha-renaming with De Bruijn indices or scope sets.

---

## Debuggability Issues

### Issue 6: **Opaque Errors** 🔴

Current error messages:
```
Error: Backend error: Undefined variable: c
Error: Invalid syntax: lambda parameters must be symbols
```

**Missing**:
- Which macro expanded to this?
- What was the original source location?
- What was the expansion trace?

**What We Need**:
```
Error: Undefined variable: c
  in expansion of (let*-values (((c d) (values a b))) ...)
  from line 5: (let*-values ...)
  expansion trace:
    let*-values → let-values → call-with-values → lambda
```

### Issue 7: **No Expansion Tracing** 🟡

We have `MACRO_DEBUG`, but it's all-or-nothing. Can't trace a specific macro.

**What We Need**:
- Expansion steps with source location
- Intermediate forms
- Which rule matched
- Pattern variable bindings

---

## Structural Problems

### Issue 8: **Circular Dependency Workaround** 🟡

Using `Rc<dyn Any>` to avoid `patina-macros` depending on `patina-runtime` is a code smell.

**Why Circular Dependency Exists**:
```
patina-macros needs:  Environment (for captured env)
patina-runtime needs: CompiledMacro (for Value::Macro)
```

**Better Solutions**:
1. **Three-crate split**:
   - `patina-types`: Value, Environment (no logic)
   - `patina-macros`: Uses types, implements macros
   - `patina-runtime`: Uses both

2. **Trait-based**:
   - `Environment` is a trait in `patina-macros`
   - `patina-runtime` implements it

3. **Event-sourced**:
   - `Compiler` emits "need to check binding" events
   - `Evaluator` provides binding oracle

---

## Recommendations

### Priority 1: Fix Environment Representation 🔴

**Proposal**: Use immutable environment chains, not `RefCell<HashMap>`.

```rust
pub struct Environment {
    bindings: HashMap<Rc<str>, Value>,  // Immutable!
    parent: Option<Rc<Environment>>,
}

impl Environment {
    fn extend(&self, name: Rc<str>, value: Value) -> Environment {
        // Returns NEW environment, doesn't mutate
        let mut bindings = HashMap::new();
        bindings.insert(name, value);
        Environment {
            bindings,
            parent: Some(Rc::new(self.clone())),
        }
    }
}
```

**Benefits**:
- Snapshot is free (just clone the `Rc`)
- No temporal coupling issues
- Thread-safe (if we ever want parallelism)
- Functional style (easier to reason about)

**Tradeoffs**:
- Requires changing `set!` implementation
- Slightly more memory (but Rc-sharing helps)

---

### Priority 2: Eliminate Value::Identifier 🔴

**Proposal**: Keep identifiers internal to macro system.

```rust
// In expander, return a "marked" AST
enum HygienicValue {
    Renamed(Rc<str>),  // Was renamed
    Captured { name: Rc<str>, env: Rc<Environment> },  // Free variable
    Normal(Value),  // Regular value
}

fn expand(...) -> HygienicValue {
    // Process hygiene
}

// Then RESOLVE before returning to evaluator
fn resolve_hygiene(hygienic: HygienicValue) -> Value {
    match hygienic {
        Renamed(new_name) => Value::Symbol(new_name),
        Captured { name, env } => {
            // Look up NOW, return the value
            env.get(&name).unwrap_or_else(||
                panic!("Free variable {} not found", name))
        }
        Normal(v) => v,
    }
}
```

**Benefits**:
- Runtime `Value` stays clean
- Hygiene is resolved before evaluation
- Easier to debug (can inspect hygienic values)

**Alternative**: Use Syntax Objects (more comprehensive but bigger change).

---

### Priority 3: Unify Environment Handling 🟡

**Proposal**: Eliminate `MacroEnv`, use `Environment` directly everywhere.

Change expander signature:
```rust
impl Expander {
    pub fn new(expansion_env: Rc<Environment>) -> Self { ... }

    fn is_macro(&self, name: &Rc<str>) -> bool {
        matches!(self.expansion_env.get(name), Some(Value::Macro{..}))
    }
}
```

**Benefits**:
- One source of truth
- No rebuilding overhead
- Simpler mental model

---

### Priority 4: Add Proper Error Context 🔴

```rust
pub struct MacroError {
    message: String,
    macro_name: Option<Rc<str>>,
    source_location: Option<SourceLocation>,
    expansion_trace: Vec<ExpansionStep>,
}

struct ExpansionStep {
    macro_name: Rc<str>,
    rule_index: usize,
    pattern: String,
    location: SourceLocation,
}
```

This requires propagating source locations through the entire pipeline.

---

### Priority 5: Structured Tracing 🟡

```rust
pub struct MacroTracer {
    enabled_macros: HashSet<Rc<str>>,  // Trace only these
    max_depth: usize,
}

impl MacroTracer {
    fn trace_expansion(&self, step: ExpansionStep) {
        // Log to structured format (JSON?)
    }
}
```

---

## Test Architecture Issues

### Issue 9: Integration Tests Too Coarse

We discovered bugs through integration tests (`let*-values`), not unit tests.

**Missing Unit Tests**:
- `Identifier` handling in various contexts
- Environment snapshot behavior
- Macro name recognition with different environment states

**Recommendation**: Property-based testing for hygiene.

```rust
#[quickcheck]
fn hygiene_preserves_semantics(program: ValidProgram) {
    let without_macros = eval_without_macros(program);
    let with_macros = eval_with_macro_expansion(program);
    assert_eq!(without_macros, with_macros);
}
```

---

## Long-Term Architectural Vision

### Phase 1: Stabilize (Do Now)
1. Make Environment immutable
2. Remove `Value::Identifier`, resolve hygiene before eval
3. Add error context

### Phase 2: Modernize (Next Quarter)
1. Implement syntax objects (Racket-style)
2. Add scope sets for proper hygiene
3. Support `syntax-case`

### Phase 3: Optimize (Future)
1. Macro compilation cache
2. Incremental expansion
3. Parallel macro expansion (if immutable)

---

## Comparison with Reference Implementations

### Gauche (our reference)
- **Identifiers**: Separate type, converted to symbols
- **Environment**: Immutable frames
- **Hygiene**: Mark-based, not gensym
- **Complexity**: ~3000 LOC for macro system

### Racket
- **Syntax Objects**: First-class with source location
- **Scope Sets**: Precise hygiene tracking
- **Expander**: Separate phase from evaluator
- **Complexity**: ~10,000 LOC (but very feature-rich)

### Chez Scheme
- **psyntax**: Portable syntax-case expander
- **Marks and Ribs**: For hygiene
- **Separate Expansion Pass**: Fully expands before eval
- **Complexity**: ~5000 LOC

### Our Current System
- **Complexity**: 5826 LOC
- **Features**: Basic syntax-rules
- **Hygiene**: Gensym + capture environment
- **Problem**: Complex as Chez, but less capable

**Verdict**: We're paying the cost of a sophisticated system without getting the benefits.

---

## Immediate Action Items

1. ✅ **Document the hybrid approach** - This review serves as documentation
2. **Add integration test for environment mutation** - Ensure our assumptions hold
3. **Create macro debugging guide** - How to use `MACRO_DEBUG` effectively
4. **Refactor Identifier handling** - Make it a clear, documented pattern

---

## Conclusion

The current architecture works but is **fragile** and **hard to extend**. The core issues are:

1. **Three environment representations** (confusing)
2. **Value::Identifier leaks hygiene into runtime** (violation of abstraction)
3. **Mutable environment with immutable semantics** (works by accident)
4. **Poor debuggability** (hard to trace errors)

**Recommendation**: Plan a systematic refactor (Priority 1 & 2 above) before adding new macro features. The current architecture will make `syntax-case` or modules extremely difficult.

**Estimated Effort**: 2-3 weeks for Priority 1 & 2, but will prevent months of pain later.

---

*Review conducted 2025-01-23 based on actual implementation issues encountered during lexical hygiene fix session.*
