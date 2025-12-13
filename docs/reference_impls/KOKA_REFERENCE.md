# Koka Reference: Effect System and Compilation

This document studies Koka's effect system and compilation strategy, focusing on lessons
applicable to Patina's future effect system and bytecode VM phases.

## Overview

Koka is a research language from Microsoft Research that demonstrates production-quality
implementation of:

1. **Row-polymorphic effect types** - Effects tracked in function types
2. **Algebraic effect handlers** - User-defined control abstractions
3. **Evidence-based compilation** - No runtime type information needed
4. **Perceus reference counting** - GC-free memory management with reuse analysis

**Key Papers:**
- "Generalized Evidence Passing for Effect Handlers" (ICFP'21)
- "Effect Handlers, Evidently" (ICFP'20)
- "Type Directed Compilation of Row-Typed Algebraic Effects" (POPL'17)
- "Perceus: Garbage Free Reference Counting with Reuse" (PLDI'21)

## 1. Effect System Architecture

### 1.1 Kind System for Effects

Koka extends the standard kind system with effect-specific kinds:

```
Kind ::= V           (* values/types *)
       | E           (* effect rows *)
       | X           (* atomic effect labels *)
       | (E,V) -> V  (* handler types *)
       | H           (* heap types *)
       | S           (* scope types *)
```

**Key insight:** Effects are first-class types with their own kind. This allows:
- Effect polymorphism (functions generic over effects)
- Row polymorphism (extensible effect rows)
- Effect inference (minimal effect annotation)

### 1.2 Effect Row Representation

Effects are represented as rows (extensible records of effects):

```koka
// Function type: (args) -> <effects> result
fun divide(x: int, y: int) : <exn> int

// Effect row with multiple effects
fun risky() : <exn,io,state<int>> int

// Effect polymorphism - e is an effect row variable
fun map(f: a -> e b, xs: list<a>) : e list<b>
```

**Row structure:**
```
Effect Row = label1 | label2 | ... | tail
           where tail is either:
           - Empty effect () aka "total"
           - Effect type variable (for polymorphism)
```

### 1.3 Type-Level Effect Representation

From `src/Type/Type.hs`:

```haskell
data Type
  = TForall  [TypeVar] Rho           -- forall a. rho
  | TFun     [(Name,Type)] Effect Type  -- (x:a, y:b) -> e c
  | TCon     TypeCon                  -- type constant
  | TVar     TypeVar                  -- type variable
  | TApp     Type [Type]              -- application
  | TSyn     TypeSyn [Type] Type      -- type synonym

type Effect = Tau  -- Effects are just types of kind E
```

**Effect operations:**
- `effectEmpty` - The empty/total effect
- `effectExtend label eff` - Add an effect label to a row
- `extractEffectExtend eff` - Decompose an effect row

### 1.4 Built-in Effect Types

From `lib/std/core/types.kk`:

```koka
type total :: E              // Empty effect (pure computation)
type div :: X                // Divergence (may not terminate)
type exn :: X                // Exceptions
type read :: H -> X          // Heap read
type write :: H -> X         // Heap write
type alloc :: H -> X         // Heap allocation
type handled :: HX -> X      // User-defined effects (lifted)
type handled1 :: HX -> X     // Linear user-defined effects
```

**Lesson for Patina:** Distinguish between built-in primitive effects (like `exn`)
and user-defined handler effects (`handled`). Built-in effects can be compiled
more efficiently.

## 2. Effect Handler Implementation

### 2.1 User-Defined Effects

Effects are declared as abstract types with operations:

```koka
// Declare an effect with operations
effect exc {
  ctl raise(s: string) : a    // Control operation (captures continuation)
}

effect state<s> {
  fun get() : s               // Function operation (no capture)
  fun put(x: s) : ()
}

effect yield<a> {
  ctl yield(item: a) : ()     // Control with resumption
}
```

**Two kinds of operations:**
1. `fun` - Tail-resumptive, runs in-place (no continuation capture)
2. `ctl` - Control operation, may capture/discard continuation

### 2.2 Handler Syntax

```koka
// Basic handler
fun catch(action, h) {
  handle(action) {
    ctl raise(s) { h(s) }     // Handle raise by calling h
  }
}

// State handler using local mutable variable
fun state(init, action) {
  var s := init               // Local mutable state
  handle(action) {
    return(x) { (x, s) }      // Return result with final state
    fun get()  { s }          // In-place operation
    fun put(x) { s := x }
  }
}

// Amb handler - resumes multiple times
val amb = handler {
  return(x)    { [x] }
  ctl flip()   { resume(False) ++ resume(True) }  // Non-determinism
}
```

### 2.3 Evidence-Based Compilation

**The key insight:** Instead of searching the stack at runtime for handlers,
compile effects to pass "evidence" (handler pointers) explicitly.

From `lib/std/core/hnd.kk`:

```koka
// Evidence for a handler h in context
abstract type ev<h>
  con Ev<e,r>(htag: htag<h>,      // Runtime tag for dynamic lookup
              marker: marker<e,r>, // Unique marker for this handler instance
              hnd: h<e,r>,         // The actual handler (operations)
              hevv: evv<e>)        // Evidence vector at handler definition

// Evidence vector - array of all handlers in scope
type evv<e::E>

// Perform an operation given evidence
pub fun @perform1(ev: ev<h>, op: (forall<e1,r> h<e1,r> -> clause1<a,b,h,e1,r>), x: a) : e b
  match ev
    Ev(_tag, m, h, _w) -> match h.op
      Clause1(f) -> cast-clause1(f)(m, ev, x)
```

**Compilation strategy:**
1. Each handler defines a record of operation implementations
2. Evidence vectors track all handlers in the current scope
3. Operations are compiled to function calls through evidence
4. No runtime stack walking needed

### 2.4 Clause Types

Operations are compiled to clauses with different behaviors:

```koka
// Clause for 1-argument operation
abstract value type clause1<a,b,h,e,r>
  Clause1(clause: (marker<e,r>, ev<h>, a) -> e b)

// Tail-resumptive: runs in-place, no continuation capture
pub fun clause-tail1(op: a -> e b) : clause1<a,b,h,e,r>
  Clause1(fn(_m, ev, x) { under1(ev, op, x) })

// Control: yields to handler with continuation
pub fun clause-control1(clause: (x:a, k: b -> e r) -> e r) : clause1<a,b,h,e,r>
  Clause1(fn(m, _ev, x) { yield-to(m, fn(k) { protect(x, clause, k) }) })

// Never resumes: for exceptions
pub fun clause-never1(op: a -> e r) : clause1<a,b,h,e,r>
  Clause1(fn(m, _ev, x) { yield-to-final(m, fn(_k) { op(x) }) })
```

**Lesson for Patina:** Optimize for common cases:
- Tail-resumptive operations need no continuation capture
- Exception-style handlers need no resume support
- Only general control operations need full continuation machinery

## 3. Compilation Pipeline

### 3.1 Core IR

From `src/Core/Core.hs`, Koka uses a System F-like IR:

```haskell
data Expr =
    Lam [TName] Effect Expr          -- Lambda with effect annotation
  | Var TName VarInfo
  | App Expr [Expr]
  | TypeLam [TypeVar] Expr           -- Type abstraction
  | TypeApp Expr [Type]              -- Type application
  | Con TName ConRepr                -- Constructor
  | Lit Lit
  | Let DefGroups Expr
  | Case [Expr] [Branch]
```

**Key feature:** Lambda and function types always carry effect annotations.
This allows effect-aware optimizations.

### 3.2 Monadic Translation

From `src/Core/Monadic.hs`, effects are compiled via monadic bind:

```haskell
-- Transform effectful applications to monadic binds
-- f(x) where f : a -> e b becomes:
--   bind(f(x), fn(y) { ... })

monMakeBind :: Type -> Effect -> Type -> Expr -> Expr -> Expr
monMakeBind tpArg tpEff tpRes arg next
  = App (TypeApp (Var (TName nameBind typeBind) info) [tpArg, tpRes, tpEff]) [arg, next]
```

**Optimization:** Only effectful calls need monadic translation:
- Total functions: direct calls
- Tail-resumptive operations: in-place execution
- Control operations: yield and bind

### 3.3 Evidence Translation

The evidence translation transforms effect-polymorphic code:

1. **Evidence parameters:** Add evidence vector parameter to functions
2. **Evidence lookup:** Replace operation calls with evidence-based dispatch
3. **Handler installation:** Compile handlers to evidence vector updates

```
Before: fun foo() : <state<int>> int = get()
After:  fun foo(evv: evv<state<int>>) : int = perform(evv-at(0), get-clause)
```

### 3.4 Compilation Phases

```
Source Code
    ↓
[Parser] → Syntax Tree
    ↓
[Kind Inference] → Kinds assigned
    ↓
[Type Inference] → Types and effects inferred
    ↓
[Core IR] → System F with effects
    ↓
[Simplify] → Inlining, constant folding
    ↓
[Monadic Translation] → Bind insertion for effects
    ↓
[MonadicLift] → Lift binds out of lambdas
    ↓
[CTail] → Tail call optimization
    ↓
[Inline/Specialize] → Specialization
    ↓
[PARC] → Reference counting analysis
    ↓
[ParcReuse] → Reuse analysis
    ↓
[C Backend] → Generate C code
```

## 4. Perceus Reference Counting

### 4.1 Core Principles

From `src/Backend/C/Parc.hs`:

1. **Precise:** Knows exact ownership at every point
2. **Automatic:** Compiler inserts all dup/drop
3. **Reuse:** In-place updates when reference is unique
4. **No GC:** Deterministic, immediate deallocation

### 4.2 Owned vs Borrowed

```haskell
-- Track owned and borrowed variables
data ParamInfo = Own | Borrow

-- Owned: caller gives up ownership, callee must drop
-- Borrowed: caller keeps ownership, callee must not drop
```

**Borrowing rules:**
- Primitive operations can borrow (no dup needed)
- Pattern matching borrows scrutinee
- Last use transfers ownership (no drop needed)

### 4.3 Reference Count Operations

From `kklib/include/kklib.h`:

```c
// Reference count in header (32-bit)
typedef int32_t kk_refcount_t;

// Unique reference has refcount 0 (not 1!)
// Allows faster uniqueness test
static inline bool kk_refcount_is_unique_or_thread_shared(kk_refcount_t rc) {
  return (rc <= 0);  // 0 = unique, negative = thread-shared
}
```

**Optimization:** Refcount 0 means unique. This makes the common case
(unique reference, can update in-place) a single comparison with 0.

### 4.4 Reuse Analysis

The PARC pass identifies opportunities for in-place updates:

```koka
// Without reuse: allocate new Cons
fun map(f, xs) {
  match xs {
    Nil -> Nil
    Cons(x, xx) -> Cons(f(x), map(f, xx))  // New allocation
  }
}

// With reuse analysis: if xs is unique, reuse Cons cell
fun map(f, xs) {
  match xs {
    Nil -> Nil
    Cons(x, xx) ->
      if is-unique(xs) then
        set-fields(xs, f(x), map(f, xx))  // Reuse allocation
      else
        Cons(f(x), map(f, xx))            // New allocation
  }
}
```

## 5. Lessons for Patina

### 5.1 Effect System Design

1. **Use kinds for effects:** Distinguish effect types from value types at the
   kind level. This provides better type errors and enables effect-aware
   optimizations.

2. **Row polymorphism:** Effect rows should be extensible. A function with
   effect `<exn|e>` works in any context that has at least `exn`.

3. **Inference first:** Design the effect system so most effects can be
   inferred. Only require annotations at module boundaries.

4. **Separate effect categories:**
   - Built-in effects (exn, io, div) - hardcoded semantics
   - User-defined effects - compiled via evidence passing

### 5.2 Compilation Strategy

1. **Evidence passing:** Compile effect handlers to evidence vectors passed
   as function arguments. This avoids runtime stack walking.

2. **Optimize common cases:**
   - Tail-resumptive operations: no continuation capture
   - Exception handlers: no resume support needed
   - Pure code: no monadic overhead

3. **Clause specialization:** Different clause types for different handler
   behaviors enables targeted optimization.

4. **Two-phase compilation:**
   - Phase 1: Type/effect inference, evidence translation
   - Phase 2: Optimize effectful code (inline evidence, specialize clauses)

### 5.3 Memory Management

1. **Precise reference counting:** Track exact ownership at compile time.
   This is more predictable than GC and enables functional in-place updates.

2. **Borrowing:** Mark parameters as borrowed when they don't need ownership
   transfer. Reduces dup/drop pairs.

3. **Reuse analysis:** When a data structure is uniquely owned before
   pattern matching, its memory can be reused for the result.

4. **Refcount optimization:** Use 0 for unique references, negative for
   thread-shared. This makes uniqueness checks very fast.

### 5.4 Integration with VM

For Patina's future bytecode VM:

1. **Evidence as stack slots:** Evidence vectors can be passed on the VM stack
   just like other values.

2. **Specialized opcodes:**
   - `PERFORM_TAIL` - tail-resumptive operation (no capture)
   - `PERFORM_CTL` - control operation (capture continuation)
   - `YIELD` - yield to handler with value
   - `RESUME` - resume captured continuation

3. **Continuation representation:** Capture continuation as a list of
   stack frames. Resume by pushing frames back.

4. **Handler frames:** Mark handler installation on the stack. Yield walks
   back to the marker.

### 5.5 Syntax Considerations

Koka's effect syntax is explicit but lightweight:

```koka
// Effect in function type
fun foo() : <exn,state<int>> int

// Handler using 'with' syntax
fun bar() {
  with handler { ... }
  action()
}

// Effect masking
fun pure-wrapper() : total int {
  mask<exn> { may-throw() }
}
```

For Patina/Scheme:
```scheme
;; Effect annotation via special form or pragma
(define (foo) : (-> <exn state<int>> int)
  ...)

;; Handler via macro
(with-handler ((raise (lambda (s) 42)))
  (safe-div 10 0))
```

## 6. Implementation Roadmap for Patina

### Phase 1: Basic Effect Tracking (Type-Level Only)
- Add effect kinds to the type system
- Track effects in function types
- Implement basic effect inference
- No runtime support yet (effects are documentation only)

### Phase 2: Built-in Effects
- Implement `exn` (exceptions) as first effect
- Add `io` effect for I/O operations
- Effects affect type checking but not runtime

### Phase 3: User-Defined Effects
- `define-effect` form for declaring effects
- `handle` form for installing handlers
- Evidence translation in compiler
- Basic continuation capture for `ctl` operations

### Phase 4: Optimization
- Clause specialization (tail, control, never)
- Evidence inlining for known handlers
- Borrowing analysis for reference counting

## 7. File Locations in Koka

Key files for further study:

- **Effect types:** `src/Type/Type.hs` (lines 1-1000+)
- **Kind system:** `src/Kind/Kind.hs`
- **Type inference:** `src/Type/Infer.hs`
- **Core IR:** `src/Core/Core.hs`
- **Monadic translation:** `src/Core/Monadic.hs`
- **Perceus:** `src/Backend/C/Parc.hs`
- **C backend:** `src/Backend/C/FromCore.hs`
- **Runtime handlers:** `lib/std/core/hnd.kk`
- **Effect examples:** `test/algeff/common.kk`
- **Runtime library:** `kklib/include/kklib.h`

## 8. References

1. Daan Leijen. "Koka: Programming with Row Polymorphic Effect Types" (2014)
2. Daan Leijen. "Type Directed Compilation of Row-Typed Algebraic Effects" (POPL'17)
3. Ningning Xie, Daan Leijen. "Effect Handlers, Evidently" (ICFP'20)
4. Ningning Xie, Daan Leijen. "Generalized Evidence Passing for Effect Handlers" (ICFP'21)
5. Alex Reinking et al. "Perceus: Garbage Free Reference Counting with Reuse" (PLDI'21)
6. Anton Lorenzen et al. "Reference Counting with Frame-Limited Reuse" (MSR-TR-2021-30)
7. Anton Lorenzen, Daan Leijen. "FP2: Fully in-Place Functional Programming" (ICFP'23)
