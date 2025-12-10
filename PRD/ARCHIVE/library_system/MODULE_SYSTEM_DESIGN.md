# R7RS Module System Implementation Plan for Patina

**Goal:** Implement R7RS-compliant library/module system to enable running chibi's test suite

**Reference:** R7RS spec sections 5.6 (Libraries) and 5.2 (Import declarations)

**Chibi reference:** `~/Project/reference/chibi-scheme/lib/scheme/`

---

## R7RS Module System Overview

### Core Concepts

**Libraries** are the fundamental unit of code organization in R7RS. Each library:
- Has a unique **name** (hierarchical list of identifiers)
- **Exports** specific bindings (procedures, syntax, variables)
- **Imports** bindings from other libraries
- Contains **definitions and expressions** in its body

**Import sets** provide powerful ways to control what gets imported:
- `(only lib id ...)` - Import only specific identifiers
- `(except lib id ...)` - Import all except specific identifiers
- `(prefix lib prefix)` - Add prefix to all imported identifiers
- `(rename lib (old new) ...)` - Rename specific imports

---

## Syntax Overview

### 1. Library Definition (`define-library`)

```scheme
(define-library (library name parts...)
  (export identifier ...)
  (import import-set ...)
  (begin
    ;; definitions and expressions
    ))
```

**Library names:**
- List of identifiers: `(scheme base)`, `(example grid)`
- Can include version numbers: `(srfi 1)`, `(my lib 1 0)`
- Reserved prefixes: `scheme`, `srfi`

**Library declarations** (order doesn't matter except semantically):
- `(export spec ...)` - What to make visible
- `(import set ...)` - What to bring in
- `(begin body ...)` - Code to execute
- `(include "file.scm" ...)` - Include file contents
- `(include-ci "file.scm" ...)` - Case-insensitive include
- `(include-library-declarations "file.scm")` - Include declarations only
- `(cond-expand clause ...)` - Conditional compilation

### 2. Export Specifications

```scheme
(export
  identifier              ; Export as-is
  (rename old new))       ; Export with different name
```

### 3. Import Declarations

```scheme
(import
  (library name)                    ; Import all exports
  (only (lib) id ...)               ; Import specific
  (except (lib) id ...)             ; Import all except
  (prefix (lib) prefix)             ; Add prefix to all
  (rename (lib) (old new) ...))     ; Rename imports
```

**Import sets can be nested:**
```scheme
(import (only (except (scheme base) error) + - * /))
```

---

## Implementation Architecture

### Phase 1: Data Structures

**1. Library Definition**
```rust
#[derive(Debug, Clone)]
pub struct Library {
    pub name: LibraryName,
    pub exports: HashMap<String, Binding>,  // exported-name -> binding
    pub env: Rc<Environment>,                // library's private environment
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LibraryName(Vec<LibraryNamePart>);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LibraryNamePart {
    Identifier(String),
    Version(i64),  // for version numbers
}
```

**2. Binding Information**
```rust
#[derive(Debug, Clone)]
pub struct Binding {
    pub name: String,         // internal name in library
    pub value: Value,         // the actual binding
    pub kind: BindingKind,
}

#[derive(Debug, Clone)]
pub enum BindingKind {
    Variable,
    Syntax,      // For macros
    Procedure,
}
```

**3. Library Registry**
```rust
pub struct LibraryRegistry {
    libraries: HashMap<LibraryName, Library>,
    search_paths: Vec<PathBuf>,  // For finding library files
}
```

**4. Import Set AST**
```rust
#[derive(Debug, Clone)]
pub enum ImportSet {
    Library(LibraryName),
    Only(Box<ImportSet>, Vec<String>),
    Except(Box<ImportSet>, Vec<String>),
    Prefix(Box<ImportSet>, String),
    Rename(Box<ImportSet>, Vec<(String, String)>),
}
```

---

### Phase 2: Parsing

**1. Parse `define-library` form**

Add to `eval/special_forms.rs`:
```rust
pub(super) fn eval_define_library(
    &self,
    args: &Value,
    env: &Rc<Environment>,
) -> Result<Value, EvalError> {
    // Parse: (define-library (name ...) declaration ...)
    let (name_expr, declarations) = self.extract_pair(args)?;

    let library_name = parse_library_name(&name_expr)?;
    let parsed_decls = parse_library_declarations(&declarations)?;

    // Create and register library
    let library = self.create_library(library_name, parsed_decls, env)?;
    self.library_registry.register(library);

    Ok(Value::Unspecified)
}
```

**2. Parse library declarations**

```rust
struct LibraryDeclarations {
    exports: Vec<ExportSpec>,
    imports: Vec<ImportSet>,
    body: Vec<Value>,          // From begin/include
}

enum ExportSpec {
    Direct(String),            // identifier
    Renamed(String, String),   // (rename old new)
}
```

**3. Parse import sets**

Recursive parser for nested import sets:
```rust
fn parse_import_set(expr: &Value) -> Result<ImportSet, EvalError> {
    match expr {
        Value::Pair(pair) if is_symbol(&pair.0, "only") => {
            // (only import-set id ...)
            let (_, rest) = extract_pair(expr)?;
            let (import_set_expr, ids) = extract_pair(&rest)?;
            let import_set = parse_import_set(&import_set_expr)?;
            let identifiers = collect_identifiers(&ids)?;
            Ok(ImportSet::Only(Box::new(import_set), identifiers))
        }
        // ... except, prefix, rename similarly
        _ => {
            // Plain library name
            let name = parse_library_name(expr)?;
            Ok(ImportSet::Library(name))
        }
    }
}
```

---

### Phase 3: Evaluation & Instantiation

**Library Loading Phases:**

1. **Parse** - Read library definition syntax
2. **Expand** - Process cond-expand, resolve imports
3. **Compile** - Expand macros in body
4. **Execute** - Run library body, populate environment
5. **Register** - Make exports available

**Dependency Resolution:**

```rust
impl LibraryRegistry {
    pub fn load_library(&mut self, name: &LibraryName) -> Result<&Library, EvalError> {
        // Check if already loaded
        if let Some(lib) = self.libraries.get(name) {
            return Ok(lib);
        }

        // Find library file
        let path = self.find_library_file(name)?;

        // Parse library definition
        let lib_def = parse_library_file(&path)?;

        // Recursively load dependencies (imports)
        for import_set in &lib_def.imports {
            self.resolve_import_set(import_set)?;
        }

        // Instantiate library
        let library = self.instantiate_library(lib_def)?;

        // Register and return
        self.libraries.insert(name.clone(), library);
        self.libraries.get(name).ok_or(...)
    }
}
```

**Import Set Resolution:**

```rust
fn resolve_import_set(&self, import_set: &ImportSet) -> Result<HashMap<String, Value>, EvalError> {
    match import_set {
        ImportSet::Library(name) => {
            let lib = self.load_library(name)?;
            Ok(lib.exports.clone())
        }
        ImportSet::Only(set, ids) => {
            let bindings = self.resolve_import_set(set)?;
            let mut result = HashMap::new();
            for id in ids {
                if let Some(binding) = bindings.get(id) {
                    result.insert(id.clone(), binding.clone());
                } else {
                    return Err(EvalError::ImportError(
                        format!("Identifier {} not found in import set", id)
                    ));
                }
            }
            Ok(result)
        }
        ImportSet::Except(set, ids) => {
            let mut bindings = self.resolve_import_set(set)?;
            for id in ids {
                if bindings.remove(id).is_none() {
                    return Err(EvalError::ImportError(...));
                }
            }
            Ok(bindings)
        }
        ImportSet::Prefix(set, prefix) => {
            let bindings = self.resolve_import_set(set)?;
            Ok(bindings.into_iter()
                .map(|(k, v)| (format!("{}{}", prefix, k), v))
                .collect())
        }
        ImportSet::Rename(set, renames) => {
            let mut bindings = self.resolve_import_set(set)?;
            for (old, new) in renames {
                if let Some(binding) = bindings.remove(old) {
                    bindings.insert(new.clone(), binding);
                } else {
                    return Err(EvalError::ImportError(...));
                }
            }
            Ok(bindings)
        }
    }
}
```

**Library Instantiation:**

```rust
fn instantiate_library(&self, lib_def: LibraryDefinition) -> Result<Library, EvalError> {
    // 1. Create new environment extending base environment
    let lib_env = Rc::new(Environment::with_parent(self.base_env.clone()));

    // 2. Import all dependencies into library environment
    for import_set in &lib_def.imports {
        let bindings = self.resolve_import_set(import_set)?;
        for (name, value) in bindings {
            lib_env.define(name, value);
        }
    }

    // 3. Evaluate library body in this environment
    for expr in &lib_def.body {
        self.evaluator.eval_in_env(expr, &lib_env)?;
    }

    // 4. Create export map
    let mut exports = HashMap::new();
    for export_spec in &lib_def.exports {
        match export_spec {
            ExportSpec::Direct(name) => {
                let value = lib_env.get(name).ok_or(...)?;
                exports.insert(name.clone(), Binding {
                    name: name.clone(),
                    value,
                    kind: infer_binding_kind(&value),
                });
            }
            ExportSpec::Renamed(internal, external) => {
                let value = lib_env.get(internal).ok_or(...)?;
                exports.insert(external.clone(), Binding {
                    name: internal.clone(),
                    value,
                    kind: infer_binding_kind(&value),
                });
            }
        }
    }

    Ok(Library {
        name: lib_def.name,
        exports,
        env: lib_env,
    })
}
```

---

### Phase 4: Top-level `import` Statement

For REPL and program-level imports:

```rust
pub(super) fn eval_import(
    &self,
    args: &Value,
    env: &Rc<Environment>,
) -> Result<Value, EvalError> {
    // Parse: (import import-set ...)
    let import_sets = collect_list_items(args)?;

    for import_set_expr in import_sets {
        let import_set = parse_import_set(&import_set_expr)?;
        let bindings = self.library_registry.resolve_import_set(&import_set)?;

        // Add to current environment
        for (name, binding) in bindings {
            // Check for conflicts (error in library, warning in REPL)
            if env.get(&name).is_some() {
                if self.in_repl {
                    eprintln!("Warning: redefining {}", name);
                } else {
                    return Err(EvalError::ImportError(
                        format!("Conflicting import: {}", name)
                    ));
                }
            }
            env.define(name, binding.value);
        }
    }

    Ok(Value::Unspecified)
}
```

---

### Phase 5: File Loading & Search Paths

**Library File Naming Convention:**

Library name `(foo bar baz)` maps to file paths:
- `foo/bar/baz.sld` (preferred, R7RS standard)
- `foo/bar/baz.scm` (fallback)

**Library Search:**

```rust
impl LibraryRegistry {
    fn find_library_file(&self, name: &LibraryName) -> Result<PathBuf, EvalError> {
        let rel_path = library_name_to_path(name);

        for search_path in &self.search_paths {
            // Try .sld first
            let sld_path = search_path.join(&rel_path).with_extension("sld");
            if sld_path.exists() {
                return Ok(sld_path);
            }

            // Try .scm
            let scm_path = search_path.join(&rel_path).with_extension("scm");
            if scm_path.exists() {
                return Ok(scm_path);
            }
        }

        Err(EvalError::LibraryNotFound(name.clone()))
    }
}

fn library_name_to_path(name: &LibraryName) -> PathBuf {
    let mut path = PathBuf::new();
    for part in &name.0 {
        match part {
            LibraryNamePart::Identifier(s) => path.push(s),
            LibraryNamePart::Version(v) => path.push(v.to_string()),
        }
    }
    path
}
```

**Search Path Configuration:**

```rust
impl LibraryRegistry {
    pub fn new() -> Self {
        let mut search_paths = vec![
            PathBuf::from("lib"),           // Local lib directory
            PathBuf::from("~/.scheme/lib"), // User libraries
        ];

        // Add from SCHEME_LIB_PATH environment variable
        if let Ok(paths) = env::var("SCHEME_LIB_PATH") {
            for path in paths.split(':') {
                search_paths.push(PathBuf::from(path));
            }
        }

        Self {
            libraries: HashMap::new(),
            search_paths,
        }
    }
}
```

---

## Implementation Plan

### Phase 1: Core Data Structures (2-3 days)

**Files to create:**
- `src/library/mod.rs` - Library module
- `src/library/registry.rs` - Library registry
- `src/library/types.rs` - Library, ImportSet, etc.
- `src/library/parse.rs` - Parsing utilities

**Tasks:**
1. Define Library, LibraryName, Binding structs
2. Define ImportSet enum
3. Create LibraryRegistry with basic HashMap storage
4. Add to src/lib.rs and wire into Evaluator

**Estimated:** 2-3 days

---

### Phase 2: Parsing (2-3 days)

**Files to modify:**
- `src/library/parse.rs` - Implement parsing
- `src/eval/mod.rs` - Add define-library, import dispatching

**Tasks:**
1. Parse library names from Value
2. Parse export specifications
3. Parse import sets (recursive)
4. Parse library declarations

**Estimated:** 2-3 days

---

### Phase 3: Library Loading & Resolution (3-4 days)

**Files to modify:**
- `src/library/registry.rs` - Loading logic
- `src/library/resolve.rs` - Import resolution

**Tasks:**
1. Implement find_library_file with search paths
2. Implement load_library with dependency resolution
3. Implement resolve_import_set for all variants
4. Handle circular dependency detection
5. Proper error messages

**Estimated:** 3-4 days

---

### Phase 4: Library Instantiation (2-3 days)

**Files to modify:**
- `src/library/instantiate.rs` - Instantiation logic
- `src/eval/special_forms.rs` - eval_define_library

**Tasks:**
1. Create library environment
2. Import dependencies into environment
3. Evaluate library body
4. Extract exports
5. Register in registry

**Estimated:** 2-3 days

---

### Phase 5: Top-level Import (1-2 days)

**Files to modify:**
- `src/eval/special_forms.rs` - eval_import

**Tasks:**
1. Parse import declaration
2. Resolve and add bindings to current environment
3. Conflict detection (error vs warning for REPL)

**Estimated:** 1-2 days

---

### Phase 6: Standard Library Organization (3-4 days)

**Files to create:**
- `lib/scheme/base.sld` - (scheme base)
- `lib/scheme/complex.sld` - (scheme complex)
- `lib/scheme/write.sld` - (scheme write)
- etc.

**Tasks:**
1. Reorganize existing primitives into libraries
2. Create minimal (scheme base) exporting current features
3. Create (scheme complex) for complex numbers
4. Test with simple imports

**Estimated:** 3-4 days

---

### Phase 7: Testing & Integration (2-3 days)

**Tasks:**
1. Unit tests for parsing
2. Unit tests for import resolution
3. Integration tests for library loading
4. Test with chibi examples
5. Fix bugs and edge cases

**Estimated:** 2-3 days

---

## Total Estimated Timeline

**Minimum:** 15 days (3 weeks)
**Expected:** 18 days (3.5 weeks)
**Maximum:** 22 days (4.5 weeks)

**Conservative estimate:** 3-4 weeks

---

## Challenges & Considerations

### 1. Macro Hygiene Across Libraries

**Challenge:** Macros defined in one library must maintain hygiene when used in another.

**Solution:** Our existing syntax-rules implementation already handles hygiene. Just need to ensure macro definitions are properly exported/imported.

### 2. Circular Dependencies

**Challenge:** Library A imports B, B imports A

**Solution:**
```rust
fn load_library_with_cycle_detection(
    &mut self,
    name: &LibraryName,
    loading_stack: &mut Vec<LibraryName>,
) -> Result<(), EvalError> {
    if loading_stack.contains(name) {
        return Err(EvalError::CircularDependency(loading_stack.clone()));
    }
    loading_stack.push(name.clone());
    // ... load library ...
    loading_stack.pop();
    Ok(())
}
```

### 3. Load Once Semantics

**Challenge:** R7RS requires libraries loaded once, shared across all importers

**Solution:** Registry caches loaded libraries. Multiple imports get same instance.

### 4. REPL vs Program Semantics

**Challenge:** REPL allows redefinition, programs don't

**Solution:**
```rust
struct EvaluatorContext {
    is_repl: bool,
}

// In import handling:
if env.get(&name).is_some() {
    if self.context.is_repl {
        eprintln!("Warning: redefining {}", name);
    } else {
        return Err(EvalError::ConflictingImport(name));
    }
}
```

### 5. Performance

**Challenge:** Loading many libraries could be slow

**Solutions:**
- Lazy loading (don't load until needed)
- Caching parsed libraries
- Consider bytecode compilation later

---

## Minimal Viable Implementation

To run chibi's test suite, we need at minimum:

**Required Libraries:**
- `(scheme base)` - Core language ✅ (we have most of this)
- `(scheme write)` - display, write, newline
- `(chibi test)` - Test framework (can implement ourselves)

**Optional but helpful:**
- `(scheme char)` - Character operations
- `(scheme complex)` - Complex numbers ✅ (we have this)
- `(scheme file)` - File I/O

**MVP Timeline:** 2-3 weeks

1. Week 1: Core module system (parse, load, resolve)
2. Week 2: Import/export, basic standard libraries
3. Week 3: Testing, bug fixes, (chibi test) library

---

## Example Usage After Implementation

```scheme
;; Define a library
(define-library (example math)
  (export square cube)
  (import (scheme base))
  (begin
    (define (square x) (* x x))
    (define (cube x) (* x x x))))

;; Use it in REPL
(import (example math))
(square 5)  ; => 25

;; Use in another library
(define-library (example geometry)
  (export circle-area)
  (import (scheme base)
          (only (example math) square))
  (begin
    (define pi 3.14159)
    (define (circle-area r)
      (* pi (square r)))))
```

---

## Next Steps

If you want to proceed with implementation:

1. **Start with Phase 1** - Data structures (2-3 days)
2. **Then Phase 2** - Parsing (2-3 days)
3. **Then Phase 3** - Loading/resolution (3-4 days)

After ~1 week, you'll have basic library system working.
After ~3 weeks, you'll be able to run chibi's test suite!

Would you like me to start implementing Phase 1 (data structures)?
