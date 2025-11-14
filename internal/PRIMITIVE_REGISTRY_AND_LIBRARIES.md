# Primitive Registry and Library System Integration

## Current Architecture

### Library System (R7RS)
The library system has two parallel tracks:

1. **Rust Libraries** (`RustLibraryLoader`)
   - Used for performance-critical primitives
   - Example: `(scheme base)` in `stdlib/scheme_base.rs`
   - Builder function populates an `Environment` with `Procedure::Primitive` values
   - Returns list of exported identifiers

2. **Scheme Libraries** (`SchemeLibraryLoader`)
   - Loads `.sld` files
   - Parses `define-library` forms
   - Evaluates library body in fresh environment
   - Collects exports

### Primitive Dispatch (Current)
- Global `install_primitives()` populates global environment
- Hardcoded 530-line match statement in `apply_primitive()`
- Primitives available globally, not scoped to libraries

### Primitive Registry (New)
- Runtime-extensible primitive registration
- Metadata: name, arity, help text, handler
- Introspection support
- Currently populated but not used for dispatch

## Design Questions

### Question 1: Where should primitives live?

**Option A: Global Primitives (Current Model)**
```
Global Environment
├── + (primitive)
├── - (primitive)
├── cons (primitive)
└── ...

(scheme base) Library
└── exports: ["+", "-", "cons", ...] (references to global primitives)
```

**Pros:**
- Simple implementation
- Fast lookup (single environment)
- Matches current behavior

**Cons:**
- Not true R7RS library isolation
- Can't have library-specific primitive implementations
- Global namespace pollution

**Option B: Library-Scoped Primitives**
```
(scheme base) Library
├── Environment
│   ├── + (primitive)
│   ├── - (primitive)
│   └── ...
└── exports: ["+", "-", ...]

User Environment (after import)
├── + → (scheme base).+
├── - → (scheme base).-
└── (user definitions)
```

**Pros:**
- True library isolation
- Can have different primitive implementations per library
- Matches R7RS semantics

**Cons:**
- More complex implementation
- Need to copy/reference primitives into library environments

### Question 2: How should the registry relate to libraries?

**Option A: Global Registry Only**
```rust
// Single global primitive registry
PrimitiveRegistry {
    primitives: {
        "+": PrimitiveFn { ... },
        "-": PrimitiveFn { ... },
        ...
    }
}

// Libraries reference primitives by name
impl RustLibraryBuilder {
    fn build_scheme_base(env: Rc<Environment>) {
        // Look up primitives from global registry
        for name in ["+", "-", "*", ...] {
            let prim = GLOBAL_REGISTRY.get(name)?;
            env.define(name, prim.to_value());
        }
    }
}
```

**Pros:**
- Single source of truth
- Easy primitive sharing across libraries
- Simple registry management

**Cons:**
- Couples libraries to global registry
- Hard to have library-specific overrides
- Not extensible for plugins

**Option B: Library-Specific Registries**
```rust
// Each library can have its own registry
Library {
    name: Vec<String>,
    exports: HashMap<String, Value>,
    env: Rc<Environment>,
    primitive_registry: Option<PrimitiveRegistry>,  // NEW
}

// Libraries populate their own registries
impl RustLibraryBuilder {
    fn build_scheme_base(lib: &mut Library) {
        lib.primitive_registry = Some(PrimitiveRegistry::new());
        arithmetic::register(&mut lib.primitive_registry);
        lists::register(&mut lib.primitive_registry);

        // Convert registry entries to environment bindings
        for (name, prim_fn) in lib.primitive_registry.primitives() {
            lib.env.define(name, prim_fn.to_value());
        }
    }
}
```

**Pros:**
- Library isolation
- Can override primitives per library
- Plugin system: libraries can provide new primitives

**Cons:**
- More complex
- Potential duplication of primitive data
- Harder to share primitives

**Option C: Hybrid - Global Registry + Library Projections**
```rust
// Global registry as canonical source
GlobalPrimitiveRegistry {
    primitives: HashMap<String, PrimitiveFn>
}

// Libraries project subsets of the global registry
Library {
    name: Vec<String>,
    exports: HashMap<String, Value>,
    env: Rc<Environment>,
    primitive_set: HashSet<String>,  // Which primitives this library uses
}

impl RustLibraryBuilder {
    fn build_scheme_base(env: Rc<Environment>) -> (Vec<String>, HashSet<String>) {
        let primitive_names = vec!["+", "-", "*", ...];

        // Populate environment from global registry
        for name in &primitive_names {
            let prim = GLOBAL_REGISTRY.get(name)?;
            env.define(name, prim.to_value());
        }

        (exports, primitive_set)
    }
}
```

**Pros:**
- Balance between simplicity and flexibility
- Easy primitive sharing
- Libraries declare dependencies on primitives
- Could support primitive versioning later

**Cons:**
- Still coupled to global registry
- Extra bookkeeping (primitive_set)

### Question 3: How should primitives be dispatched?

**Current (Match Statement)**
```rust
pub fn apply_primitive(&self, name: &str, args: Vec<Value>, in_tail: bool)
    -> Result<EvalResult, EvalError>
{
    match name {
        "+" => arithmetic::add(self, args).map(EvalResult::Value),
        "-" => arithmetic::subtract(self, args).map(EvalResult::Value),
        // ... 500+ more lines
    }
}
```

**Option A: Registry-First Dispatch**
```rust
pub fn apply_primitive(&self, name: &str, args: Vec<Value>, in_tail: bool)
    -> Result<EvalResult, EvalError>
{
    // Try registry first
    if let Some(prim_fn) = self.primitive_registry.get(name) {
        return prim_fn.call(self, args, in_tail);
    }

    // Fall back to match statement for unconverted primitives
    match name {
        "call-with-values" => values::call_with_values(self, args, in_tail),
        // Only special cases that need custom handling
        _ => Err(EvalError::UndefinedVariable(name.to_string()))
    }
}
```

**Option B: Registry-Only Dispatch**
```rust
pub fn apply_primitive(&self, name: &str, args: Vec<Value>, in_tail: bool)
    -> Result<EvalResult, EvalError>
{
    self.primitive_registry.apply(name, args, self, in_tail)
}
```

## Recommended Design: Hybrid Approach

### Short Term (Current Work)
1. **Keep global primitives model** - Don't change library semantics yet
2. **Global registry** - Single source of primitive metadata
3. **Gradual migration** - Convert categories one at a time
4. **Registry-first dispatch** - Check registry, fall back to match

### Medium Term (Next Phase)
1. **Library-scoped primitives** - Migrate to proper R7RS library isolation
2. **Registry as builder input** - Libraries specify which primitives they need
3. **Eliminate match statement** - Once all primitives converted

### Long Term (Future)
1. **Plugin system** - Libraries can extend primitive set
2. **Primitive versioning** - Support different primitive implementations
3. **Optimization** - JIT-friendly primitive dispatch

## Implementation Plan

### Phase 1: Make Registry Live (Current)
```rust
// In primitives/mod.rs
impl Evaluator {
    pub(super) fn apply_primitive(&self, name: &str, args: Vec<Value>, in_tail: bool)
        -> Result<EvalResult, EvalError>
    {
        // Try registry first
        if let Ok(result) = self.primitive_registry.apply(name, args.clone(), self, in_tail) {
            return Ok(result);
        }

        // Fall back to existing match statement
        match name {
            "+" => arithmetic::add(self, args).map(EvalResult::Value),
            // ... existing code
        }
    }
}
```

### Phase 2: Convert All Primitives
- Convert each category module to register its primitives
- As primitives are registered, remove from match statement
- Eventually match statement only has special cases

### Phase 3: Library Integration
- Modify `build_scheme_base()` to use registry
- Instead of listing primitives, just call `registry.list_names()`
- Libraries become projections of the registry

## Key Insight: Separation of Concerns

The registry serves **two different purposes**:

1. **Implementation/Dispatch** - How primitives are called (registry vs match)
2. **Scoping/Export** - Which primitives are available in which libraries

These can be decoupled:
- Registry handles dispatch uniformly
- Library system handles scoping/export
- Both can evolve independently

## Questions for Discussion

1. **Should we support library-specific primitive implementations?**
   - e.g., Could a user library override `+` with custom behavior?
   - R7RS says no, but useful for teaching/experimentation

2. **How to handle primitive metadata in libraries?**
   - Currently `(scheme base)` just lists names and arities
   - Registry has help text too - should libraries expose this?

3. **Migration strategy for existing code?**
   - Global environment still has primitives installed
   - Libraries export references to them
   - When/how to migrate to pure library-scoped model?

4. **Performance implications?**
   - Registry lookup vs match statement
   - Environment lookup for library-scoped primitives
   - Caching strategies?
