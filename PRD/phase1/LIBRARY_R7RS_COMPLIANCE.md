# Library System R7RS Compliance

**Created:** 2025-12-03
**Status:** In Progress (~70% R7RS Compliant)
**Priority:** Medium - Enables portable libraries and SRFIs

---

## Executive Summary

Patina's library system has a solid foundation with all core import/export functionality working. The remaining gaps are four library declaration types (`include`, `include-ci`, `include-library-declarations`, `cond-expand`) plus support for integers in library names.

**Current State:**
- All import modifiers working (only, except, prefix, rename)
- Export with rename working
- Library caching and circular dependency detection working
- `.sld` file loading working

**Gaps:**
- No file inclusion in libraries
- No conditional expansion
- Library names reject integers (breaks SRFI naming)

---

## R7RS Compliance Matrix

### Fully Implemented

| Feature | R7RS Section | Implementation |
|---------|--------------|----------------|
| `define-library` form | §5.6.1 | `library_parser.rs` |
| Library name (identifiers) | §5.6 | Symbols only (integers TODO) |
| `(export identifier)` | §5.6.1 | `ExportSpec::Identifier` |
| `(export (rename old new))` | §5.6.1 | `ExportSpec::Rename` |
| `(import (lib))` | §5.2 | `ImportSet::Library` |
| `(import (only ...))` | §5.2 | `ImportSet::Only` |
| `(import (except ...))` | §5.2 | `ImportSet::Except` |
| `(import (prefix ...))` | §5.2 | `ImportSet::Prefix` |
| `(import (rename ...))` | §5.2 | `ImportSet::Rename` |
| Nested import modifiers | §5.2 | Full recursive composition |
| `(begin ...)` in library | §5.6.1 | Evaluates body expressions |
| Library caching | §5.6.1 | Single load, reused |
| Circular dependency detection | - | `loading_stack` check |

### Not Implemented

| Feature | R7RS Section | Impact | Complexity |
|---------|--------------|--------|------------|
| `(include "file.scm")` | §5.6.1 | Cannot split library across files | Medium |
| `(include-ci "file.scm")` | §5.6.1 | Case-insensitive variant | Medium |
| `(include-library-declarations "f.sld")` | §5.6.1 | Cannot share exports/imports | Medium |
| `(cond-expand ...)` | §5.6.1, §4.2.1 | No conditional/portable code | High |
| Integers in library names | §5.6 | `(srfi 1)` fails | Low |

---

## Architecture Overview

### Current Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│                    (import (scheme base))                    │
└────────────────────────────┬────────────────────────────────┘
                             │
                             ▼
              ┌──────────────────────────────┐
              │ Desugarer: CoreExpr::Import  │
              │ (patina-frontend/desugarer)  │
              └──────────────────┬───────────┘
                                 │
                                 ▼
         ┌───────────────────────────────────────────┐
         │ Evaluator: process_import_for_eval()      │
         │ (patina-tree-walker/eval/mod.rs:1465)     │
         └────────────────────┬──────────────────────┘
                              │
                              ▼
           ┌──────────────────────────────────────┐
           │ LibraryLoaderRegistry (priority)     │
           │ 1. RustLibraryLoader (built-in)      │
           │ 2. SchemeLibraryLoader (.sld files)  │
           └────────────────────┬─────────────────┘
                                │
               ┌────────────────┴────────────────┐
               │                                 │
               ▼                                 ▼
   ┌───────────────────────┐      ┌─────────────────────────────┐
   │ Rust: RustLibraryLoader│      │ Scheme: SchemeLibraryLoader │
   │ - scheme/base          │      │ - Parses .sld files         │
   │ - scheme/char          │      │ - Returns ParsedLibrary     │
   │ - scheme/complex       │      │ - Evaluator runs body       │
   │ - scheme/write         │      └─────────────────────────────┘
   │ - chibi/test           │
   └───────────────────────┘
                                │
                                ▼
                  ┌───────────────────────────┐
                  │ LibraryRegistry (cached)  │
                  │ - Stores loaded libraries │
                  │ - Tracks search paths     │
                  └───────────────────────────┘
```

### Key Files

| Component | Location | Purpose |
|-----------|----------|---------|
| Library Parser | `patina-frontend/src/library_parser.rs` | Parses `define-library` |
| Import/Export Types | `patina-runtime/src/library_loader.rs` | `ImportSet`, `ExportSpec` |
| Library Registry | `patina-runtime/src/library_registry.rs` | Caching, search paths |
| Scheme Loader | `patina-tree-walker/src/library_support.rs` | .sld file loading |
| Import Desugaring | `patina-frontend/src/desugarer/mod.rs:378` | `CoreExpr::Import` |
| Import Processing | `patina-tree-walker/src/eval/mod.rs:1465-1664` | Modifier application |

---

## Implementation Plan

### 1. Integer Library Names (Priority: High, Effort: Low)

**Goal:** Support `(srfi 1)`, `(srfi 69)` etc.

**R7RS Spec (§5.6):**
> `<library name>` is a list whose members are identifiers and exact non-negative integers.

**Current Behavior:**
```scheme
(import (srfi 1))  ; ERROR: Library name parts must be symbols
```

**Implementation:**

Modify `parse_library_name()` in `library_parser.rs:80-102`:

```rust
fn parse_library_name(value: &Value) -> Result<Vec<String>, ParseError> {
    let list = Self::expect_list(value)?;

    if list.is_empty() {
        return Err(ParseError::InvalidSyntax(
            "Library name cannot be empty".to_string(),
        ));
    }

    let mut name = Vec::new();
    for part in list {
        match part {
            Value::Symbol(s) => name.push(s.to_string()),
            Value::Integer(n) if n >= 0 => name.push(n.to_string()),
            _ => return Err(ParseError::InvalidSyntax(format!(
                "Library name parts must be symbols or non-negative integers, got: {}",
                part
            ))),
        }
    }

    Ok(name)
}
```

**Testing:**
```scheme
(define-library (srfi 1)
  (export xcons)
  (import (scheme base))
  (begin (define (xcons d a) (cons a d))))

(import (srfi 1))
(xcons 1 2)  ; => (2 . 1)
```

---

### 2. Include Declaration (Priority: High, Effort: Medium)

**Goal:** Allow splitting library code across files.

**R7RS Spec (§5.6.1):**
> `(include <filename1> <filename2> ...)` is used to specify the body of the library. The contents of the files are read and their expressions are spliced in.

**Example:**
```scheme
;; mylib.sld
(define-library (mylib)
  (export foo bar)
  (import (scheme base))
  (include "helpers.scm")
  (include "main.scm"))

;; helpers.scm
(define (helper x) (* x 2))

;; main.scm
(define (foo x) (helper x))
(define (bar x) (+ (helper x) 1))
```

**Implementation:**

#### Step 1: Extend LibraryDefinition

In `library_parser.rs`, add include tracking:

```rust
pub struct LibraryDefinition {
    pub name: Vec<String>,
    pub exports: Vec<ExportSpec>,
    pub imports: Vec<ImportSet>,
    pub body: Vec<Value>,
    pub includes: Vec<IncludeDecl>,  // NEW
}

pub struct IncludeDecl {
    pub filenames: Vec<String>,
    pub case_insensitive: bool,  // for include-ci
}
```

#### Step 2: Parse include declarations

In `parse_declaration()`:

```rust
"include" => {
    let mut filenames = Vec::new();
    for filename_value in &list[1..] {
        if let Value::String(s) = filename_value {
            filenames.push(s.borrow().clone());
        } else {
            return Err(ParseError::InvalidSyntax(
                "include filenames must be strings".to_string(),
            ));
        }
    }
    includes.push(IncludeDecl { filenames, case_insensitive: false });
    Ok(())
}
"include-ci" => {
    // Same but case_insensitive: true
}
```

#### Step 3: Process includes during library evaluation

In `SchemeLibraryLoader` or evaluator:

```rust
fn resolve_includes(&self, lib_def: &LibraryDefinition, lib_path: &Path)
    -> Result<Vec<Value>, LoaderError>
{
    let mut all_body = Vec::new();
    let lib_dir = lib_path.parent().unwrap_or(Path::new("."));

    for include in &lib_def.includes {
        for filename in &include.filenames {
            let include_path = lib_dir.join(filename);
            let contents = std::fs::read_to_string(&include_path)
                .map_err(|e| LoaderError::FileNotFound(include_path.clone()))?;

            // Parse the file
            let lexer = Lexer::new(&contents, include.case_insensitive);
            let parser = Parser::new(lexer);
            let exprs = parser.parse_program()?;

            all_body.extend(exprs);
        }
    }

    // Add inline body expressions
    all_body.extend(lib_def.body.clone());

    Ok(all_body)
}
```

**Testing:**
```scheme
;; Create test library with include
;; lib/test/split.sld
(define-library (test split)
  (export greet)
  (import (scheme base))
  (include "greet.scm"))

;; lib/test/greet.scm
(define (greet name)
  (string-append "Hello, " name "!"))

;; Test
(import (test split))
(greet "World")  ; => "Hello, World!"
```

---

### 3. Include-library-declarations (Priority: Medium, Effort: Medium)

**Goal:** Share library declarations (exports, imports) across libraries.

**R7RS Spec (§5.6.1):**
> `(include-library-declarations <filename> ...)` is similar to include except that the contents of the file are spliced directly into the current library definition.

**Example:**
```scheme
;; common-exports.scm
(export make-widget widget? widget-name)

;; widget.sld
(define-library (widget)
  (include-library-declarations "common-exports.scm")
  (import (scheme base))
  (begin ...))
```

**Implementation:**

Similar to `include`, but splices at declaration level rather than body level:

```rust
"include-library-declarations" => {
    for filename_value in &list[1..] {
        if let Value::String(s) = filename_value {
            let contents = read_file(s.borrow())?;
            let decls = parse_library_declarations(&contents)?;

            for decl in decls {
                Self::parse_declaration(&decl, exports, imports, body)?;
            }
        }
    }
    Ok(())
}
```

---

### 4. Cond-expand (Priority: High, Effort: High)

**Goal:** Conditional code based on features/libraries.

**R7RS Spec (§4.2.1, §5.6.1):**
> `cond-expand` provides a way to conditionally expand different code based on implementation features.

**Syntax:**
```scheme
(cond-expand
  (<feature-requirement> <library-declaration>*)
  ...
  (else <library-declaration>*))

<feature-requirement> ::= <feature-identifier>
                       |  (library <library-name>)
                       |  (and <feature-requirement>*)
                       |  (or <feature-requirement>*)
                       |  (not <feature-requirement>)
```

**Example:**
```scheme
(define-library (mylib)
  (cond-expand
    ((library (srfi 1))
     (import (srfi 1)))
    (else
     (import (scheme base))
     (begin
       (define (first lst) (car lst))
       (define (second lst) (cadr lst)))))
  (export first second))
```

**Implementation:**

#### Step 1: Define Feature Registry

Create `patina-runtime/src/features.rs`:

```rust
use std::collections::HashSet;

pub struct FeatureRegistry {
    features: HashSet<String>,
}

impl FeatureRegistry {
    pub fn new() -> Self {
        let mut features = HashSet::new();

        // R7RS standard features
        features.insert("r7rs".to_string());
        features.insert("exact-closed".to_string());
        features.insert("exact-complex".to_string());
        features.insert("ieee-float".to_string());
        features.insert("ratios".to_string());
        features.insert("full-unicode".to_string());

        // Platform features
        #[cfg(unix)]
        features.insert("posix".to_string());
        #[cfg(target_os = "macos")]
        features.insert("darwin".to_string());
        #[cfg(target_os = "linux")]
        features.insert("gnu-linux".to_string());
        #[cfg(windows)]
        features.insert("windows".to_string());

        // Architecture
        #[cfg(target_arch = "x86_64")]
        features.insert("x86-64".to_string());
        #[cfg(target_arch = "aarch64")]
        features.insert("aarch64".to_string());

        // Endianness
        #[cfg(target_endian = "little")]
        features.insert("little-endian".to_string());
        #[cfg(target_endian = "big")]
        features.insert("big-endian".to_string());

        // Implementation name
        features.insert("patina".to_string());

        Self { features }
    }

    pub fn has_feature(&self, name: &str) -> bool {
        self.features.contains(name)
    }

    pub fn all_features(&self) -> Vec<String> {
        self.features.iter().cloned().collect()
    }
}
```

#### Step 2: Add cond-expand parsing

In `library_parser.rs`:

```rust
pub enum CondExpandClause {
    Feature {
        requirement: FeatureRequirement,
        declarations: Vec<Value>,  // Raw declarations to parse if matched
    },
    Else {
        declarations: Vec<Value>,
    },
}

pub enum FeatureRequirement {
    Identifier(String),
    Library(Vec<String>),
    And(Vec<FeatureRequirement>),
    Or(Vec<FeatureRequirement>),
    Not(Box<FeatureRequirement>),
}
```

#### Step 3: Evaluate cond-expand during library loading

```rust
fn evaluate_cond_expand(
    &self,
    clauses: &[CondExpandClause],
    features: &FeatureRegistry,
    library_registry: &LibraryRegistry,
) -> Option<Vec<Value>> {
    for clause in clauses {
        match clause {
            CondExpandClause::Feature { requirement, declarations } => {
                if self.check_requirement(requirement, features, library_registry) {
                    return Some(declarations.clone());
                }
            }
            CondExpandClause::Else { declarations } => {
                return Some(declarations.clone());
            }
        }
    }
    None
}

fn check_requirement(
    &self,
    req: &FeatureRequirement,
    features: &FeatureRegistry,
    lib_registry: &LibraryRegistry,
) -> bool {
    match req {
        FeatureRequirement::Identifier(name) => features.has_feature(name),
        FeatureRequirement::Library(name) => lib_registry.library_exists(name),
        FeatureRequirement::And(reqs) => reqs.iter().all(|r| self.check_requirement(r, features, lib_registry)),
        FeatureRequirement::Or(reqs) => reqs.iter().any(|r| self.check_requirement(r, features, lib_registry)),
        FeatureRequirement::Not(req) => !self.check_requirement(req, features, lib_registry),
    }
}
```

#### Step 4: Add `features` procedure

R7RS requires `(features)` to return the list of features:

```scheme
(features)  ; => (r7rs exact-closed ieee-float ratios patina darwin x86-64 little-endian)
```

**Testing:**
```scheme
;; Test feature detection
(cond-expand
  (patina (display "Running on Patina!"))
  (else (display "Unknown implementation")))

;; Test library detection
(cond-expand
  ((library (srfi 1)) (import (srfi 1)))
  (else (begin)))

;; Test compound requirements
(cond-expand
  ((and r7rs ratios) (display "Has ratios"))
  (else (display "No ratios")))
```

---

## Testing Strategy

### Unit Tests

Add to `crates/patina-tests/tests/`:

```rust
// library_compliance_test.rs

#[test]
fn test_integer_library_name() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    // Create temp .sld file with integer in name
    // Test import works
}

#[test]
fn test_include_declaration() {
    // Create library with include
    // Verify included code is available
}

#[test]
fn test_cond_expand_feature() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp.eval_str("(cond-expand (r7rs 'yes) (else 'no))");
    assert_eq!(result.unwrap(), symbol("yes"));
}

#[test]
fn test_cond_expand_library() {
    // Test (library (scheme base)) requirement
}

#[test]
fn test_features_procedure() {
    let interp = TreeWalkInterpreter::new_tree_walker();
    let result = interp.eval_str("(features)");
    // Verify returns list containing 'r7rs
}
```

### Integration Tests

1. Port chibi-scheme's library tests
2. Test SRFI libraries that use `cond-expand`
3. Test multi-file library loading

---

## Success Criteria

### Must Have (Full R7RS Compliance)
- [ ] `(srfi 1)` library name works
- [ ] `(include "file.scm")` loads and splices code
- [ ] `(include-ci "file.scm")` works case-insensitively
- [ ] `(include-library-declarations "file.scm")` splices declarations
- [ ] `(cond-expand (r7rs ...) (else ...))` works
- [ ] `(cond-expand ((library (scheme base)) ...) ...)` works
- [ ] `(features)` returns implementation features
- [ ] Compound requirements: `and`, `or`, `not`

### Nice to Have
- [ ] `cond-expand` in expression context (not just libraries)
- [ ] Custom feature registration API
- [ ] `define-library` as top-level form in REPL

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| File path resolution complexity | Medium | Use library file's directory as base |
| Circular includes | High | Track included files, error on cycle |
| Feature detection for libraries | Medium | Query registry before loading |
| Case-insensitive lexing | Low | Add flag to lexer, test thoroughly |

---

## Timeline Recommendation

| Phase | Features | Notes |
|-------|----------|-------|
| 1 | Integer library names | Quick win, enables SRFIs |
| 2 | `include`, `include-ci` | Enables standard patterns |
| 3 | `include-library-declarations` | Less commonly used |
| 4 | `cond-expand` + features | Most complex, highest value |

---

## References

- R7RS Specification §5.2 (Import declarations)
- R7RS Specification §5.6 (Libraries)
- R7RS Specification §4.2.1 (Conditionals - cond-expand)
- R7RS Specification Appendix B (Standard Feature Identifiers)
- Chibi-scheme library implementation: `lib/init-7.scm`
- Current implementation: `PRD/ARCHIVE/phase1_completed/LIBRARY_SYSTEM_STATUS.md`
