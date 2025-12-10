# R7RS Module System Design for Patina

**Status:** Design Phase
**Priority:** High (required for chibi test suite and R7RS compliance)
**Estimated Effort:** 6-8 weeks full implementation

## Executive Summary

The R7RS module system provides library organization through `define-library`, `import`, and `export`. Implementation requires adding a module registry and loader between the parser and macro expander, with minimal changes to existing architecture.

**Key Finding:** Module resolution happens **after parsing** but **before macro expansion**.

## Architecture Overview

### Current Pipeline
```
Input → Lexer → Parser → Macro Expander → Desugarer → Evaluator
```

### With Module Support
```
Input → Lexer → Parser → MODULE RESOLVER → Macro Expander → Desugarer → Evaluator
                              ↓
                        Module Registry
                        Library Loader
                        Import Resolution
```

## Core Components

### 1. Module Registry (patina-runtime)

```rust
pub struct Module {
    pub name: Vec<String>,           // e.g., ["scheme", "base"]
    pub exports: ExportSpec,
    pub env: Option<Rc<Environment>>,
    pub declarations: Vec<LibraryDeclaration>,
}

pub struct ModuleRegistry {
    modules: HashMap<Vec<String>, Module>,
    library_paths: Vec<PathBuf>,
}
```

### 2. Import Sets

Support all R7RS import modifiers:
- `(library-name)` - Import all exports
- `(only (lib) id ...)` - Import only specified
- `(except (lib) id ...)` - Import all except specified
- `(prefix (lib) prefix)` - Prefix all imports
- `(rename (lib) (old new) ...)` - Rename imports

These can be **nested**: `(prefix (only (scheme base) + -) math:)`

### 3. Library Files (.sld)

**File Location:** `lib/scheme/base.sld` for `(scheme base)`

```scheme
(define-library (scheme base)
  (export + - * / cons car cdr ...)
  (import (patina primitives))  ; Rust-implemented primitives
  (begin
    (define (caar x) (car (car x)))
    ...))
```

## Implementation Phases

### Phase 1: Foundation (2 weeks) ⭐ START HERE
**Goal:** Can import `(scheme base)` and use primitives

- [ ] Add `Module`/`ModuleRegistry` to patina-runtime
- [ ] Parse `define-library` in patina-frontend
- [ ] Simple library loader (no import modifiers)
- [ ] Create `lib/scheme/base.sld`
- [ ] Auto-import `(scheme base)` in REPL
- [ ] **Test:** `(import (scheme base)) (+ 1 2)` works

### Phase 2: Import Modifiers (1 week)
**Goal:** Full import set support

- [ ] Implement `only`, `except`, `prefix`, `rename`
- [ ] Support nested import sets
- [ ] **Test:** Complex imports work

### Phase 3: Standard Libraries (2-3 weeks)
**Goal:** All R7RS standard libraries

- [ ] `(scheme write)` - display, write
- [ ] `(scheme read)` - read, read-char
- [ ] `(scheme file)` - file I/O
- [ ] `(scheme char)` - char operations
- [ ] `(scheme lazy)` - delay/force
- [ ] `(scheme case-lambda)`
- [ ] **Test:** Chibi r7rs-tests.scm passes

### Phase 4: User Libraries (1 week)
**Goal:** Users can create libraries

- [ ] Library search paths
- [ ] `include`/`include-ci` support
- [ ] `cond-expand` for portability
- [ ] **Test:** User .sld files work

### Phase 5: Advanced (1 week)
- [ ] Circular dependency detection
- [ ] `environment` and `eval` procedures
- [ ] Error messages with library context

## Key Design Decisions

### 1. Library Search Strategy
**Decision:** Environment variable + fixed paths
- Check `PATINA_LIBRARY_PATH`
- Fallback: `./lib`, `<install>/share/patina/lib`
- File mapping: `(scheme base)` → `scheme/base.sld`

### 2. Standard Library Implementation
**Decision:** Hybrid approach (like Chibi)
- Core primitives in Rust: `(patina primitives)`
- `(scheme base)` imports primitives, adds derived procedures in Scheme
- Other libraries import `(scheme base)`

### 3. Environment Layering
**Decision:** Three-layer structure
```
user-definitions → imported-bindings → parent-env
```
This allows local definitions to shadow imports correctly.

### 4. REPL Behavior
**Decision:** Auto-import `(scheme base)` in REPL only
- Programs require explicit `(import ...)`
- REPL starts with `(scheme base)` pre-loaded (per R7RS)

### 5. Module Caching
**Decision:** Cache environments in registry
- Parse .sld file once
- Evaluate to environment once
- Cache environment for future imports
- (Disk serialization deferred to later)

## Integration Points

### Changes to Existing Crates

**patina-runtime:**
- ✅ Add `src/module.rs` with module types
- ✅ Extend `Environment` with exports tracking

**patina-frontend:**
- ✅ Add `src/module_loader.rs`
- ✅ Parse `define-library` syntax
- ✅ Parse import sets

**patina-tree-walker:**
- ✅ Add `src/module_eval.rs`
- ✅ Add `import` special form handler
- ✅ Modify initialization to handle program imports

**patina-interpreter:**
- ✅ Add `ModuleRegistry` to `Interpreter`
- ✅ Add `load_library()` method
- ✅ Support top-level imports

## Critical Insights from Chibi

### Module Evaluation Order
1. **Find module:** Check registry, else load .sld file
2. **Parse declarations:** Extract exports, imports, begin blocks
3. **Create environment:** Fresh environment for library
4. **Process imports:** Load imported libraries recursively
5. **Evaluate body:** Run begin blocks and includes
6. **Cache environment:** Store for future imports

### Export Resolution
```rust
// Exports can be:
enum ExportSpec {
    All,  // Export all bindings (default)
    Listed(Vec<(String, String)>),  // (internal_name, external_name)
}
```

### Import Binding
```rust
// When importing:
1. Resolve import set to (library-name, [(external_name, internal_name)])
2. Load library environment
3. For each binding:
   - Look up external_name in library environment
   - Define internal_name in importing environment
```

## Example Usage (Post-Implementation)

### User Library

**File:** `myapp/utils.sld`
```scheme
(define-library (myapp utils)
  (export square sum-of-squares)
  (import (scheme base))

  (begin
    (define (square x) (* x x))
    (define (sum-of-squares x y)
      (+ (square x) (square y)))))
```

**Usage:**
```scheme
(import (scheme base) (myapp utils))
(sum-of-squares 3 4)  ; => 25
```

### Complex Imports

```scheme
(import (scheme base)
        (only (scheme write) display newline)
        (prefix (scheme file) file:))

(display "Hello\n")
(file:call-with-output-file "test.txt"
  (lambda (port) (display "data" port)))
```

## Testing Strategy

### Unit Tests
```rust
#[test]
fn test_module_registration() { ... }

#[test]
fn test_import_resolution() { ... }

#[test]
fn test_library_loading() { ... }
```

### Integration Tests
```scheme
;; Basic import
(import (scheme base))
(+ 1 2)  ; => 3

;; Import modifiers
(import (only (scheme base) + -))
(+ 1 2)  ; => 3
(* 3 4)  ; Error: * not imported

;; Multiple libraries
(import (scheme base) (scheme write))
(display (+ 1 2))  ; => displays "3"
```

### Compliance Tests
- Run chibi r7rs-tests.scm
- Target: Tests requiring standard libraries should pass

## Open Questions

1. **Library versioning:** R7RS supports `(mylib 1)` for versions - implement or defer?
2. **Prelude libraries:** Should some libraries be pre-loaded in binary for performance?
3. **REPL import behavior:** Allow imports anywhere in REPL or require at start?
4. **Error recovery:** How to handle partially-loaded modules on error?

## Success Metrics

### Phase 1 Complete When:
- ✅ Can load .sld files
- ✅ Can import `(scheme base)`
- ✅ Primitives work through library system
- ✅ REPL has `(scheme base)` available

### Full Implementation Complete When:
- ✅ All import modifiers work
- ✅ All R7RS standard libraries implemented
- ✅ Chibi r7rs-tests.scm passes (excluding non-module features)
- ✅ Users can create and use .sld libraries
- ✅ Documentation complete

## References

- **R7RS Spec:** Section 5.6 (Libraries), Section 5.2 (Import declarations)
- **Chibi Implementation:** `~/Project/reference/chibi-scheme/lib/meta-7.scm`
- **Standard Library Example:** `~/Project/reference/chibi-scheme/lib/scheme/base.sld`

## Next Steps

1. **Review this design** with team/users for feedback
2. **Create Phase 1 implementation plan** with detailed tasks
3. **Set up library directory structure** in `lib/`
4. **Begin implementation** with module registry types

---

**Last Updated:** 2025-11-22
**Author:** Claude (via research agent)
