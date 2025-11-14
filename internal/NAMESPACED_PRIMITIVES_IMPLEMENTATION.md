# Namespaced Primitives Implementation

## Summary

We've successfully implemented library-namespaced primitives in the primitive registry. This allows primitives to be explicitly associated with their library of origin, enabling better organization, library-specific implementations, and cleaner integration with the R7RS library system.

## API Design

### Before (Flat Names)
```rust
registry.register(PrimitiveFn::new(
    "+",                    // Just the name
    Arity::Min(0),
    "Returns the sum",
    |eval, args, _tail| add(eval, args).map(EvalResult::Value),
));
```

### After (Namespaced)
```rust
registry.register(PrimitiveFn::new(
    "scheme.base",          // Library namespace
    "+",                    // Function name
    Arity::Min(0),
    "Returns the sum",
    |eval, args, _tail| add(eval, args).map(EvalResult::Value),
));
```

## Implementation Details

### PrimitiveFn Structure
```rust
pub struct PrimitiveFn {
    pub library: &'static str,  // NEW: "scheme.base", "patina.debug", etc.
    pub name: &'static str,     // "+", "car", "debug-enable", etc.
    pub arity: Arity,
    pub help: &'static str,
    pub handler: fn(&Evaluator, Vec<Value>, bool) -> Result<EvalResult, EvalError>,
}

impl PrimitiveFn {
    /// Get fully qualified name: "library/name"
    pub fn qualified_name(&self) -> String {
        format!("{}/{}", self.library, self.name)
    }
}
```

### PrimitiveRegistry Storage

Primitives are stored internally with their fully qualified names:

```rust
pub struct PrimitiveRegistry {
    // Internal storage uses qualified names
    // Example: {"scheme.base/+": PrimitiveFn { ... }, ...}
    primitives: HashMap<String, PrimitiveFn>,
}
```

### Lookup Methods

```rust
// By qualified name
registry.get("scheme.base/+")

// By library + name
registry.get_from_library("scheme.base", "+")

// Get all primitives from a library
registry.get_library_primitives("scheme.base")
```

## Benefits

### 1. **Explicit Library Ownership**
Primitives explicitly belong to libraries:
- `scheme.base/+` - Standard addition
- `scheme.char/char-upcase` - Character library
- `patina.debug/debug-enable` - Patina-specific debug primitives

### 2. **Easy Library Building**
Building `(scheme base)` from the registry:

```rust
fn build_scheme_base(env: Rc<Environment>, registry: &PrimitiveRegistry) -> Vec<String> {
    let mut exports = Vec::new();

    // Get all primitives for scheme.base
    for prim_fn in registry.get_library_primitives("scheme.base") {
        env.define(prim_fn.name.to_string(), prim_fn.to_value());
        exports.push(prim_fn.name.to_string());
    }

    exports
}
```

### 3. **Supports Library-Specific Implementations**
Different libraries could theoretically have different implementations:

```rust
// Standard R7RS
"scheme.base/+": standard_add

// Hypothetical extensions
"scheme.inexact/+": inexact_add       // Always returns inexact
"patina.modular/+": modular_add       // Modular arithmetic
```

### 4. **Better Code Organization**

```rust
// crates/patina-tree-walker/src/eval/primitives/arithmetic.rs

pub(super) fn register(registry: &mut PrimitiveRegistry) {
    // All (scheme base) arithmetic primitives
    registry.register(PrimitiveFn::new(
        "scheme.base", "+",
        Arity::Min(0),
        "Returns the sum of its arguments.",
        |eval, args, _tail| add(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base", "-",
        Arity::Min(1),
        "Subtracts subsequent arguments from the first.",
        |eval, args, _tail| subtract(eval, args).map(EvalResult::Value),
    ));

    // ... more primitives
}

// Could have separate registration for other libraries
pub(super) fn register_scheme_char(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new(
        "scheme.char", "char-upcase",
        Arity::Exact(1),
        "Convert character to uppercase.",
        |eval, args, _tail| char_upcase(eval, args).map(EvalResult::Value),
    ));
}
```

### 5. **Introspection Support**

```rust
// List all primitives in a library
let scheme_base_prims = registry.get_library_primitives("scheme.base");

// Get primitive metadata
if let Some(prim) = registry.get("scheme.base/+") {
    println!("Library: {}", prim.library);
    println!("Name: {}", prim.name);
    println!("Help: {}", prim.help);
}
```

## Current Status

### ✅ Completed
- [x] Updated `PrimitiveFn` to include `library` field
- [x] Added `qualified_name()` method
- [x] Registry stores primitives with qualified names
- [x] Added `get_from_library()` lookup method
- [x] Added `get_library_primitives()` iterator
- [x] Updated all tests to use new API
- [x] Converted 4 arithmetic primitives to use namespacing
- [x] All tests passing (6 registry tests, primitives still work)

### 🚧 TODO (Next Steps)

#### Step 1: Make Registry Live for Namespaced Lookups
Currently, primitives still work because dispatch uses the old match statement. Need to:

1. Update `apply_primitive()` to check registry with qualified names
2. Add fallback logic for backward compatibility

```rust
pub fn apply_primitive(&self, name: &str, args: Vec<Value>, in_tail: bool)
    -> Result<EvalResult, EvalError>
{
    // Try registry with qualified name (scheme.base/name)
    let qualified = format!("scheme.base/{}", name);
    if let Some(prim_fn) = self.primitive_registry.get(&qualified) {
        return prim_fn.call(self, args, in_tail);
    }

    // Fall back to match statement for unconverted primitives
    match name {
        "+" => arithmetic::add(self, args).map(EvalResult::Value),
        // ... existing code
    }
}
```

#### Step 2: Convert More Primitives
- Convert all arithmetic primitives (~40 total)
- Convert list primitives
- Convert string primitives
- Convert predicates

#### Step 3: Library Integration
- Update `build_scheme_base()` to use `get_library_primitives()`
- Remove manual primitive lists
- Libraries become projections of the registry

#### Step 4: Remove Match Statement
- Once all primitives converted, remove the fallback match
- Pure registry-based dispatch

## Design Patterns

### Namespace Convention
```
{library-name}/{primitive-name}

Examples:
- scheme.base/+
- scheme.base/cons
- scheme.char/char-upcase
- scheme.inexact/inexact
- patina.debug/debug-enable
```

### Library Naming
- Standard R7RS libraries: `scheme.{library}` (e.g., `scheme.base`, `scheme.char`)
- Patina-specific: `patina.{category}` (e.g., `patina.debug`, `patina.test`)
- User libraries: `{library-path}` (follows R7RS naming)

## Testing

### Registry Tests
All tests updated and passing:
- `test_primitive_fn_creation` - Validates namespaced creation
- `test_arity_checking` - Arity validation still works
- `test_registry_registration` - Registration with qualified names
- `test_registry_lookup` - Both qualified and library-specific lookup
- `test_registry_list_names` - Lists qualified names sorted
- `test_get_library_primitives` - Filter by library namespace

### Integration Tests
Primitives still work through existing match statement:
```scheme
(+ 1 2 3)     ; => 6
(- 10 3)      ; => 7
(* 2 3 4)     ; => 24
(floor 3.7)   ; => 3.0
```

## Future Enhancements

### 1. Context-Aware Dispatch
```rust
impl Evaluator {
    // Track current library context
    current_library: Option<String>,

    pub fn apply_primitive(&self, name: &str, ...) {
        // Try current library first
        if let Some(lib) = &self.current_library {
            let qualified = format!("{}/{}", lib, name);
            if let Some(prim) = self.primitive_registry.get(&qualified) {
                return prim.call(...);
            }
        }

        // Fall back to scheme.base
        // ...
    }
}
```

### 2. Library-Specific Overrides
Allow user libraries to override primitives:
```scheme
(define-library (mylib custom-math)
  (import (scheme base))
  (export +)  ; Override +

  (begin
    ;; Custom + implementation
    (define (+ . args)
      (display "Custom addition!")
      (apply scheme:+ args))))  ; Call original
```

### 3. Primitive Versioning
Support different versions of the same primitive:
```rust
"scheme.base.r7rs/+"
"scheme.base.r6rs/+"
"scheme.base.patina/+"  // Enhanced version
```

### 4. Help System Integration
```scheme
(help 'scheme.base/+)
; => Library: scheme.base
;    Name: +
;    Arity: 0 or more arguments
;    Description: Returns the sum of its arguments. With no arguments, returns 0.

(library-exports 'scheme.base)
; => (+ - * / cons car cdr ...)
```

## Migration Path

### Phase 1: ✅ Infrastructure (Complete)
- Namespace support in registry
- Dual lookup (qualified + library-specific)
- Tests updated

### Phase 2: Make Registry Live (Next)
- Update dispatch to use registry
- Maintain backward compatibility
- Validate with integration tests

### Phase 3: Convert All Primitives
- Systematic conversion by category
- Remove from match statement as converted
- Track progress

### Phase 4: Library Integration
- Update library builders to use registry
- Remove manual primitive lists
- Pure registry-based library building

### Phase 5: Cleanup
- Remove match statement entirely
- Registry-only dispatch
- Final optimization
