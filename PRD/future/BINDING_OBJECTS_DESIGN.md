# Binding Objects Design for Patina

## Status: Research / Proposal

This document explores introducing **binding objects** (similar to Chez Scheme's `prelex` or Gauche's `lvar`) to Patina to solve hygiene problems more systematically.

## Current Problem

Patina's current hygiene implementation uses Racket-style scope sets, where identifiers carry scope annotations that are matched during lookup. This has led to proliferating IR variants:

- `Var(Symbol)` vs `ScopedVar { name, scopes }`
- `Set { var, value }` vs `ScopedSet { var, scopes, value }`
- `ScopedParam` in `Lambda` parameters

The problem: **Hygiene is resolved at evaluation time**, requiring the IR to carry scope information throughout. Every variable-related form needs scoped/non-scoped variants.

## Reference Implementations

### Chez Scheme: `prelex` (Pre-Lexical Objects)

**Source:** `ChezScheme/s/base-lang.ss`, `ChezScheme/s/compile.ss`, `ChezScheme/s/syntax.ss`

Chez uses `prelex` objects - unique binding identifiers created during macro expansion:

```scheme
(define-record-type prelex
  (fields (mutable name)       ; original symbol name (for debugging)
          (mutable flags)      ; binding properties (assigned?, referenced?, etc.)
          source               ; source location
          (mutable operand)    ; for optimization passes
          (mutable $uname))    ; unique gensym (lazily generated)
  ...)

;; Creating a prelex during expansion
(define gen-var (lambda (sym) (make-prelex sym 0 #f #f)))

;; Lambda expansion creates prelex for each parameter
(define chi-lambda-clause
  (lambda (e c r w)
    (let ([new-vars (map gen-var ids)])   ; Create unique binding objects
      (let ([labels (map make-lexical-label new-vars)])
        (let ([body (chi-body ... (make-binding-wrap ids labels w))])
          (values new-vars body))))))
```

**Key insight:** Hygiene is resolved **during expansion**, not evaluation. The expander:
1. Creates fresh `prelex` for each binding (lambda param, let binding, etc.)
2. Builds a "ribcage" mapping identifiers to labels (which point to prelex)
3. Variable references in the body are resolved to their `prelex`
4. The IR contains direct `prelex` references, not names + scopes

The IR after expansion looks like:
```
(lambda (prelex#123 prelex#124)  ; Direct references to binding objects
  (+ prelex#123 prelex#124))     ; No name lookup needed at eval time
```

### Gauche: `lvar` (Local Variables)

**Source:** `Gauche/src/compile.scm`

Gauche uses a simpler vector-based structure:

```scheme
(define-simple-struct lvar 'lvar make-lvar
  (name                ; symbol name (for debugging/error messages)
   (initval #f)        ; initial value (for optimization)
   (ref-count 0)       ; reference count (for dead code elimination)
   (set-count 0)))     ; mutation count (for immutability analysis)

;; Variable references in IForm
(define pass1
  (lambda (program cenv)
    (cond
      [(identifier? program)
       (let1 r (cenv-lookup cenv program)
         (cond [(lvar? r) ($lref r)]        ; Local: direct lvar reference
               [(wrapped-identifier? r) ($gref r)]  ; Global reference
               ...))]
      ...)))
```

**Key insight:** Same approach - bindings are resolved during compilation (Pass 1), the IR uses direct `lvar` references.

## Proposed Design for Patina

### Phase 1: Introduce `BindingId`

Create a unique identifier type for bindings:

```rust
/// Unique identifier for a binding (analogous to Chez's prelex)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(u64);

impl BindingId {
    pub fn fresh() -> Self {
        // Thread-local counter for unique IDs
        thread_local! {
            static NEXT_ID: std::cell::Cell<u64> = std::cell::Cell::new(0);
        }
        NEXT_ID.with(|id| {
            let current = id.get();
            id.set(current + 1);
            BindingId(current)
        })
    }
}
```

### Phase 2: Binding Registry

Store binding metadata separately from the IR:

```rust
/// Metadata about a binding
pub struct BindingInfo {
    pub name: Symbol,              // Original name (for debugging/errors)
    pub source: Option<SourceLoc>, // Source location
    pub flags: BindingFlags,       // assigned?, referenced?, macro-introduced?
}

bitflags! {
    pub struct BindingFlags: u8 {
        const ASSIGNED = 0b0001;       // set! target
        const REFERENCED = 0b0010;     // Used (for dead code)
        const MACRO_INTRODUCED = 0b0100; // Created by macro expansion
    }
}

/// Global registry of binding metadata
pub struct BindingRegistry {
    bindings: HashMap<BindingId, BindingInfo>,
}
```

### Phase 3: Simplify CoreExpr

With binding objects, we can unify variable variants:

```rust
pub enum CoreExpr {
    // Variables: always use BindingId for locals, Symbol for globals
    LocalVar(BindingId),           // Reference to local binding
    GlobalVar(Symbol),             // Reference to global binding

    // No more ScopedVar needed! Hygiene resolved during expansion

    Lambda {
        params: Vec<BindingId>,    // Direct binding references
        variadic: Option<BindingId>,
        body: Vec<CoreExpr>,
    },

    Set {
        target: SetTarget,         // Either local or global
        value: Rc<CoreExpr>,
    },
    // No more ScopedSet needed!

    // ... rest of forms unchanged
}

pub enum SetTarget {
    Local(BindingId),
    Global(Symbol),
}
```

### Phase 4: Resolve Hygiene During Expansion

Move hygiene resolution from evaluation to macro expansion:

```rust
/// Resolution context during expansion
struct ExpansionContext {
    /// Maps (name, scopes) -> BindingId for local bindings
    local_bindings: HashMap<(Symbol, ScopeSet), BindingId>,

    /// Parent context for lexical scoping
    parent: Option<Box<ExpansionContext>>,
}

impl ExpansionContext {
    /// Resolve an identifier to its binding
    fn resolve(&self, name: &Symbol, scopes: &ScopeSet) -> Resolution {
        // Find binding where binding.scopes ⊆ reference.scopes
        // This is the existing subset matching logic
        if let Some(binding_id) = self.find_local(name, scopes) {
            Resolution::Local(binding_id)
        } else {
            Resolution::Global(name.clone())
        }
    }

    /// Create new binding (for lambda params, let bindings, etc.)
    fn bind(&mut self, name: Symbol, scopes: ScopeSet) -> BindingId {
        let id = BindingId::fresh();
        self.local_bindings.insert((name, scopes), id);
        id
    }
}

enum Resolution {
    Local(BindingId),
    Global(Symbol),
}
```

### Phase 5: Simplify Evaluation

With hygiene resolved at expansion time, evaluation becomes simpler:

```rust
fn eval(expr: &CoreExpr, env: &Environment) -> Result<Value, Error> {
    match expr {
        CoreExpr::LocalVar(id) => {
            // Direct lookup by BindingId - no scope matching needed
            env.get_local(*id)
        }
        CoreExpr::GlobalVar(name) => {
            env.get_global(name)
        }
        CoreExpr::Lambda { params, body, .. } => {
            // Capture current environment for closure
            Ok(Value::Closure { params: params.clone(), body: body.clone(), env: env.clone() })
        }
        // ... etc
    }
}

/// Simplified environment
struct Environment {
    locals: HashMap<BindingId, Value>,
    parent: Option<Rc<Environment>>,
    globals: Rc<RefCell<HashMap<Symbol, Value>>>,
}
```

## Migration Path

### Step 1: Add BindingId alongside existing system
- Create `BindingId` and `BindingRegistry`
- Add `LocalVar(BindingId)` variant to `CoreExpr` (don't remove existing)
- Update desugarer to optionally emit `LocalVar` when binding info available

### Step 2: Resolve hygiene during expansion
- Modify macro expander to create `BindingId` for introduced bindings
- Track resolution context through expansion
- Emit `LocalVar` for resolved local references

### Step 3: Update evaluation
- Add `BindingId`-based lookup to Environment
- Evaluator handles both old (scoped) and new (binding ID) variants

### Step 4: Remove legacy variants
- Once all tests pass with new system
- Remove `ScopedVar`, `ScopedSet`, scope-based lookup
- Simplify `Formals` to use `Vec<BindingId>`

## Benefits

1. **Cleaner IR**: No proliferating Scoped* variants
2. **Faster evaluation**: Direct ID lookup vs scope subset matching
3. **Better optimization**: BindingId enables precise dataflow analysis
4. **VM-ready**: Direct references compile naturally to stack slots/registers
5. **Debugging info**: Original names preserved in registry, not polluting IR

## Drawbacks / Considerations

1. **Memory**: BindingRegistry adds global state
2. **Serialization**: IR now depends on registry; need to serialize together
3. **REPL**: Incremental expansion needs careful registry management
4. **Complexity**: Two-phase resolution (expansion + eval) instead of one

## Comparison: Current vs Proposed

| Aspect | Current (Scope Sets at Eval) | Proposed (Binding Objects) |
|--------|------------------------------|---------------------------|
| IR Size | Larger (Scoped* variants) | Smaller (unified forms) |
| Hygiene Resolution | Evaluation time | Expansion time |
| Lookup Performance | O(n) scope matching | O(1) hash lookup |
| Variable Representation | Symbol + optional ScopeSet | BindingId (8 bytes) |
| Environment | Complex (scoped bindings) | Simple (ID → Value) |
| Implementation Effort | Current state | Significant refactoring |

## Recommendation

**Short-term**: Keep current implementation. The `ScopedVar`/`ScopedSet` approach works and hygiene tests pass. The proliferation is manageable.

**Medium-term (VM implementation)**: Introduce `BindingId` as part of VM work. The VM will need direct references anyway for efficient code generation. This is a natural point to unify the hygiene system.

**Long-term**: Full binding objects with resolution during expansion. This is the industry-standard approach (Chez, Gauche, Racket) and scales better.

## References

1. Chez Scheme source: `ChezScheme/s/syntax.ss`, `ChezScheme/s/base-lang.ss`
2. Gauche source: `Gauche/src/compile.scm`, `Gauche/src/compile-1.scm`
3. "Binding as Sets of Scopes" - Matthew Flatt (POPL 2016)
4. R. Kent Dybvig - "Writing Hygienic Macros in Scheme with Syntax-Case"
