# Source Information Tracking - Implementation Plan

**Status:** Planning (Updated for TaggedValue architecture)
**Priority:** High - Critical for debugging and error reporting
**Created:** 2025-11-19
**Updated:** 2026-02-27

## Problem Statement

Patina loses all source location information when parsing Scheme code. Error messages can't point to source locations:

**Current:**
```
Error: Unbound variable: x
```

**Desired:**
```
Error: Unbound variable: x
  at example.scm:15:8
     (define (foo) x)
                   ^
```

This makes debugging difficult:
1. Error messages can't point to exact source locations
2. Stack traces don't show where code was defined
3. Macro expansion errors are hard to diagnose
4. IDE integration (future) requires source mapping

## Existing Infrastructure

The following pieces already exist and should be built upon:

### `SourceLocation` (patina-core/src/error.rs)
```rust
pub struct SourceLocation {
    pub source: Rc<str>,    // File path, "<repl>", or "<string>"
    pub line: u32,          // 1-indexed
    pub column: u32,        // 1-indexed
    pub length: Option<u32>,
}
```
Includes constructors: `new()`, `with_length()`, `repl()`, `string()`.
Display: `"source:line:column"`.

### `ErrorDetail` (patina-core/src/error.rs)
```rust
pub struct ErrorDetail {
    pub kind: ErrorKind,
    pub message: String,
    pub irritants: Vec<TaggedValue>,
    pub location: Option<SourceLocation>,  // Already has this field!
}
```
Builder: `.with_location()`, `.with_opt_location()`.

### Comments pointing to this plan
- `crates/patina-frontend/src/error.rs`: "Note: Source location will be None until SOURCE_INFO_PLAN.md is implemented."
- `crates/patina-frontend/src/desugarer/error.rs`: Same comment.

## Architecture Constraints

### TaggedValue Architecture
Since the Value enum was removed, all runtime values are **TaggedValue** — compact 8-byte tagged pointers. Key implications:

1. **TaggedValue can't hold source info** — 8 bytes, no room for metadata
2. **AST is heap-allocated pair trees** — Parser writes TaggedValue pairs to the heap
3. **Source info must live in a side structure** — either a source map table or on IR nodes
4. **Source info is compile-time only** — stripped before evaluation (TaggedValue has no room anyway)

### Current Pipeline
```
Source String
    ↓
Lexer → Token (no source info currently)
    ↓
Parser → TaggedValue (AST as heap pair tree, no source info)
    ↓
Macro Expander → TaggedValue (transformed AST, no source info)
    ↓
Desugarer → CoreExpr (IR, no source info)
    ↓
CPS Transformer → CpsExpr (CPS IR, no source info)
    ↓
Evaluator → TaggedValue (runtime value)
```

### Target Pipeline
```
Source String + File Path
    ↓
Lexer → Token { kind, location }  ← tracks line/column
    ↓
Parser → TaggedValue + SourceMap  ← side table: pair addr → SourceLocation
    ↓
Macro Expander → TaggedValue + SourceMap (preserves/extends source map)
    ↓
Desugarer → CoreExpr { ..., source: Option<SourceLocation> }
    ↓
CPS Transformer → CpsExpr { ..., source: Option<SourceLocation> }
    ↓
Evaluator → errors carry SourceLocation from current IR node
```

## Design: IR-Level Source Annotations + Parser Source Map

### Why Not Wrap TaggedValue?

A `Syntax<TaggedValue>` wrapper doesn't work well because:
- TaggedValue is Copy (8 bytes); wrapping it in a 40+ byte struct defeats the purpose
- The parser allocates pairs directly on the heap — can't wrap individual heap cells
- Macro expansion works on TaggedValue pair trees, not wrapper types
- Adding wrappers would require changing every function that touches AST

### Chosen Approach: Two-Layer Strategy

**Layer 1 — Parser Source Map (temporary):**
A side table mapping heap pair addresses to source locations. Lives only during parsing → desugaring. The parser records each list/vector's start position; the desugarer looks it up when building CoreExpr.

```rust
/// Side table mapping AST pair addresses to source locations.
/// Lives during parsing → desugaring, then discarded.
pub struct SourceMap {
    /// Map from pair's raw TaggedValue bits to source location.
    /// Uses raw bits because TaggedValue is Copy and each fresh
    /// pair allocation has a unique address.
    locations: HashMap<u64, SourceLocation>,
}

impl SourceMap {
    pub fn record(&mut self, tv: TaggedValue, loc: SourceLocation) {
        self.locations.insert(tv.raw_bits(), loc);
    }

    pub fn get(&self, tv: TaggedValue) -> Option<&SourceLocation> {
        self.locations.get(&tv.raw_bits())
    }
}
```

**Layer 2 — IR Source Annotations (permanent):**
`Option<SourceLocation>` on CoreExpr and CpsExpr nodes. This is the primary carrier of source info through evaluation. Once populated by the desugarer, the source map can be dropped.

```rust
// In CoreExpr (patina-ir)
pub enum CoreExpr {
    Var { name: String, source: Option<SourceLocation> },
    App { operator: Box<CoreExpr>, operands: Vec<CoreExpr>, source: Option<SourceLocation> },
    If { test: ..., source: Option<SourceLocation> },
    // etc.
}
```

## Implementation Phases

### Phase 1: Foundation (Non-Breaking)

**Goal:** Add source fields to IR, make lexer track positions. Zero behavioral change.

**Tasks:**

1. **Add `Option<SourceLocation>` to CoreExpr variants** (`patina-ir/src/lib.rs`)
   - Add `source: Option<SourceLocation>` to each variant
   - All existing construction sites pass `source: None`
   - Display/Debug impls skip None source

2. **Add `Option<SourceLocation>` to CpsExpr variants** (`patina-core/src/cps_expr.rs`)
   - Same pattern as CoreExpr
   - CPS transformer propagates source from CoreExpr where available

3. **Make Lexer track line/column** (`patina-frontend/src/lexer/mod.rs`)
   - Add `line: u32`, `column: u32` fields to Lexer
   - Update `advance()` to track newlines
   - Add `SourceLocation` to Token struct (alongside existing data)
   - All tokens now carry their position

4. **Add SourceMap struct** (`patina-frontend/src/source_map.rs`)
   - Simple `HashMap<u64, SourceLocation>` keyed by TaggedValue raw bits
   - `record()` and `get()` methods

**Compatibility:** 100% backward compatible. All source fields are None.

### Phase 2: Parser → Desugarer Source Threading

**Goal:** Parser records positions, desugarer populates CoreExpr source fields.

**Tasks:**

1. **Parser builds SourceMap** (`patina-frontend/src/parser/mod.rs`)
   - Add `source_map: SourceMap` and `source_name: Option<Rc<str>>` to Parser
   - `new_with_source(input, source_name)` constructor
   - When parsing a list `(...)`, record the outer pair's SourceLocation
   - When parsing atoms (symbols, numbers), record their positions
   - Return `(TaggedValue, SourceMap)` from top-level parse

2. **Thread SourceMap to Desugarer** (`patina-frontend/src/desugarer/mod.rs`)
   - Add `source_map: Option<&SourceMap>` parameter to desugar functions
   - When building CoreExpr nodes, look up the current AST node in the source map
   - If found, attach the SourceLocation to the CoreExpr

3. **Update Backend to pass SourceMap through**
   - Parser → Backend → Desugarer chain threads the SourceMap

### Phase 3: Error Reporting Integration

**Goal:** Errors include source locations from IR nodes.

**Tasks:**

1. **Wire source info into EvalError** (`patina-tree-walker/src/eval/error.rs`)
   - Add `source: Option<SourceLocation>` to relevant EvalError variants
   - Or: convert EvalError to use ErrorDetail (which already has location)

2. **Evaluator captures source from current expression**
   - `eval_core()` extracts source from the CoreExpr being evaluated
   - `eval_one_step()` extracts source from the CpsExpr being evaluated
   - Errors created during evaluation carry the source location

3. **Pretty error formatting**
   - Store source text alongside SourceMap for context display
   - Format errors with source line + caret pointer:
     ```
     error: undefined variable: x
       at test.scm:15:8
        15 | (define (foo) x)
                          ^
     ```

### Phase 4: Macro Expansion Source Tracking

**Goal:** Source info survives macro expansion.

**Tasks:**

1. **Macro expander updates SourceMap**
   - When expanding a macro, new pairs get the macro use-site's location
   - Template-inserted identifiers get the macro definition-site's location
   - Add "in expansion of" notes for chained expansions

2. **Hygiene integration**
   - Scope sets already track macro expansion context
   - Source info augments this with file/line information

### Phase 5: REPL Integration

**Goal:** REPL errors show source positions.

**Tasks:**

1. **REPL passes source name to parser**
   - Each expression gets `"<repl-N>"` as source name
   - Counter increments per expression

2. **Interpreter API**
   - `eval_str_with_source(input, source_name)` passes through to parser
   - Existing `eval_str()` uses `"<string>"` as default

3. **Optional: source text cache**
   - Store `HashMap<Rc<str>, String>` mapping source name → source text
   - Enables showing source context in error messages

### Phase 6: Stack Traces (Future)

**Goal:** Show call stack with source locations on errors.

**Tasks:**

1. **Track call stack** — Vec<(proc_name, SourceLocation)> during evaluation
2. **Lambda source tracking** — CpsLambda stores definition-site location
3. **Display stack trace** — On error, show call chain with locations

## Performance Considerations

- **SourceLocation**: 16 bytes (Rc<str> + 2×u32 + Option<u32>), small
- **CoreExpr source field**: Option<SourceLocation> = 24 bytes per node; only during compilation
- **SourceMap**: Lives only during parse → desugar; dropped before evaluation
- **Runtime impact**: Zero — TaggedValue evaluation path unchanged
- **Parsing overhead**: O(1) per token for position tracking

## Key Files to Modify

| Phase | File | Change |
|-------|------|--------|
| 1 | `patina-ir/src/lib.rs` | Add `source` to CoreExpr |
| 1 | `patina-core/src/cps_expr.rs` | Add `source` to CpsExpr |
| 1 | `patina-frontend/src/lexer/mod.rs` | Track line/column in tokens |
| 1 | `patina-frontend/src/source_map.rs` | New: SourceMap struct |
| 2 | `patina-frontend/src/parser/mod.rs` | Build SourceMap during parsing |
| 2 | `patina-frontend/src/desugarer/mod.rs` | Look up SourceMap, populate CoreExpr source |
| 3 | `patina-tree-walker/src/eval/error.rs` | Wire SourceLocation into errors |
| 3 | `patina-tree-walker/src/eval/core_eval.rs` | Extract source from CoreExpr for errors |
| 3 | `patina-tree-walker/src/eval/cps_eval/step.rs` | Extract source from CpsExpr for errors |
| 5 | `patina-repl/src/repl/mod.rs` | Pass `<repl-N>` source names |
| 5 | `patina-interpreter/src/lib.rs` | `eval_str_with_source()` API |

## Success Criteria

1. Error messages show file:line:col for parse errors
2. Error messages show source location for eval errors
3. Macro expansion errors show both macro definition and use site
4. Source tracking adds <10% overhead to parse time
5. Existing tests pass with minimal changes
6. Zero runtime overhead (TaggedValue evaluation unaffected)

## References

- **Existing infrastructure**: `patina-core/src/error.rs` (SourceLocation, ErrorDetail)
- **Racket**: Syntax objects for source tracking and hygiene
- **Chibi Scheme**: Source annotations in AST
- **Rust compiler**: `rustc_span` crate
