# Source Information Tracking - Implementation Plan

**Status:** All Phases Complete ✅ — Archived 2026-03-05
**Priority:** High - Critical for debugging and error reporting
**Created:** 2025-11-19
**Updated:** 2026-03-05

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

## Reference Implementation Survey

Study of four Scheme implementations reveals a spectrum of source tracking designs.
All four preserve source through macro expansion — the mechanism differs in fidelity.

### Comparison Matrix

| | **Chez** | **Chibi** | **Racket** | **Gauche** |
|---|---|---|---|---|
| **Carrier** | `annotation` record wrapping datum | Inline `source` field on pairs | `syntax` struct with `srcloc` field | Extended pair `attributes` alist |
| **Approach** | Wrapping (side-table-ish) | Inline | Wrapping (first-class) | Look-aside (attribute list) |
| **Source data** | `{sfd, bfp, efp}` (file, begin/end pos) | `(filename . line)` | `{source, line, col, pos, span}` | `(filename line)` |
| **Macro strategy** | `source-wrap` creates new annotation with original source | Copy source from input to output top-level pair | `'origin` property chains use-site identifiers | `'original` attribute chains pre-expansion forms |
| **Into compiler** | `preinfo` records on IR nodes | Bytecode source vector | Serialized `srcloc` in bytecode | Attribute chain walked for stack traces |
| **Overhead** | ~32 bytes per annotated form | 1 word per pair | Full struct per syntax object | 1 extra word (extended pair) + alist |

### Chez Scheme — Annotation Wrapping

Chez wraps each parsed datum in an `annotation` record: `{expression, source, stripped, flags}`.
The `source` is `{sfd, bfp, efp}` (source-file-descriptor + begin/end file positions).
The `stripped` field stores the bare datum without annotations for macro expansion.

During macro expansion, `source-wrap` preserves the original source on newly created forms.
The compiler extracts source into `preinfo` records on IR nodes, which carry through to
generated code. Variables (`uvar`) carry their definition-site source through all passes.

**Key insight:** Lazy line computation — `bfp`/`efp` are byte offsets; line/column computed
on demand via `current-locate-source-object-source`.

### Chibi Scheme — Inline Pair Fields

Chibi stores source directly on each cons cell: `sexp_pair_source(res) = (filename . line)`.
The reader captures the opening paren's line number and attaches it to the first pair.

For macro expansion, source is copied from the input form to the top-level output pair:
```c
if (sexp_pairp(res))
  sexp_pair_source(res) = sexp_pair_source(sexp_car(tmp));
```

Simple but lossy — only the outer form gets source, inner generated forms get nothing.
Errors show `"on line N of file F"` but can't trace through expansion chains.

### Racket — Syntax Objects (Gold Standard)

Racket treats syntax objects as first-class values with permanent source:
```racket
(struct syntax (content* scopes shifted-multi-scopes mpi-shifts srcloc props inspector))
```

Every syntax object carries `srcloc` (`{source, line, col, pos, span}`) and a `props` hash.
The `'origin` property chains identifiers that introduced template code during macro expansion.
`syntax-track-origin` merges properties from input to output, building an expansion trace.

Source survives compilation — `srcloc` is serialized into bytecode. `syntax-debug-info`
enables runtime introspection of binding contexts at any source location.

**This is the target for Patina's long-term debugging story.**

### Gauche — Extended Pairs (Closest to Patina's Design)

Gauche uses extended pairs — normal pairs with an extra `attributes` alist slot.
Normal pairs (2 words) pay zero cost; only pairs needing metadata become extended (3 words).
Detection uses pointer alignment (even = normal, odd = extended).

The reader attaches `'source-info → (filename line)` to the first pair of each list.
During macro expansion, `'original → pre-expansion-form` is chained:
```scheme
(pair-attribute-set! p 'original expr)  ; link expanded → original
```

Error reporting walks the chain recursively, collecting all forms:
```
0  (car (cxr a r '(1 2 3 4)))
      at "test.scm":6
      expanded from (cxr a r '(1 2 3 4))
      at "test.scm":6
```

**Most relevant to Patina:** Gauche's extended pairs are conceptually equivalent to our
SourceMap — a look-aside structure where only forms that need source info get entries.
The `'original` chain is the mechanism we should adopt for macro expansion tracking.

### Patina's Progressive Strategy

Based on this survey, Patina will implement source tracking in three fidelity levels:

| Level | Model | What you get | Phase |
|---|---|---|---|
| **L1: Call-site fallback** | Chibi-style | "error at line 5" (the macro call) | Phase 2 ✅ |
| **L2: Expansion chain** | Gauche-style | "error at line 5, macro expansion: let" | Phase 4 ✅ |
| **L3: Full tracking** | Racket-style | Full expansion trace with identifier origins + properties | Phase 4b (deferred) |

The SourceMap design is forward-compatible with all three levels:
- L1: Desugarer captures call-site location as fallback before expanding
- L2: Macro expander records new entries in SourceMap with `original` links
- L3: SourceMap evolves into a richer structure with property chains

### VM Backend Compatibility

The source tracking design naturally extends to the VM backend:

```
Layer 1: SourceMap (parser → desugarer)     — shared by both backends
Layer 2: CoreExpr.source (IR annotations)   — shared by both backends
Layer 3: Bytecode DebugInfo table           — VM-specific, maps instruction offset → SourceLocation
```

The VM compiler reads `source: Option<SourceLocation>` from CoreExpr nodes during
compilation and emits a debug info table alongside bytecode (analogous to DWARF debug info
or Chez's `preinfo` records). This is the standard approach confirmed by both Chez and Racket.

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
CPS Transformer → CpsExpr { ..., source: Option<SourceLocation> }  ← propagates from CoreExpr
    ↓
CPS Evaluator → errors carry SourceLocation from current CpsExpr node
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
`Option<SourceLocation>` on CoreExpr and CpsExpr wrapper structs. This is the primary carrier of source info through evaluation. Once populated by the desugarer, the source map can be dropped.

```rust
// Wrapper struct pattern (patina-core/src/core_expr.rs)
pub struct CoreExpr {
    pub kind: CoreExprKind,              // Literal, Var, App, If, Lambda, etc.
    pub source: Option<SourceLocation>,  // Populated by desugarer from SourceMap
}

// Same pattern for CPS IR (patina-core/src/cps_expr.rs)
pub struct CpsExpr {
    pub kind: CpsExprKind,
    pub source: Option<SourceLocation>,  // Propagated from CoreExpr during CPS transform
}
```

## Implementation Phases

### Phase 1: Foundation (Non-Breaking) — COMPLETE (2026-03-03)

**Goal:** Add source fields to IR, make lexer track positions. Zero behavioral change.

**Design Decision: Wrapper Struct**

Instead of adding `source: Option<SourceLocation>` to every enum variant (which would
require converting tuple variants to struct variants and touching every pattern match),
we used a **wrapper struct** pattern:

```rust
// CoreExpr enum renamed to CoreExprKind, then wrapped:
pub struct CoreExpr {
    pub kind: CoreExprKind,              // the old enum, renamed
    pub source: Option<SourceLocation>,  // new field
}
```

Benefits: uniform `.source` access, tuple variants unchanged, future metadata
(expansion_origin) trivially added. Same pattern applied to CpsExpr/CpsExprKind.

Helper methods: `CoreExpr::new(kind)` (source=None), `CoreExpr::with_source(kind, src)`,
`CoreExpr::with_opt_source(kind, src)`, `CoreExpr::rc(kind)` (Rc::new shorthand).
`kind()` method renamed to `expr_kind()` to avoid conflict with the `kind` field.

**Completed Steps:**

1. **CoreExpr wrapper struct** (`patina-core/src/core_expr.rs`)
   - Renamed `CoreExpr` enum → `CoreExprKind`, created `CoreExpr` wrapper struct
   - Updated all construction sites (~85 in desugarer, ~38 in cps_transform, tests)
   - Updated all pattern matches to use `expr.kind` / `&expr.kind`
   - `map_children()` preserves source on rebuilt nodes
   - Re-exported `CoreExprKind` from `patina-core` and `patina-ir`

2. **CpsExpr wrapper struct** (`patina-core/src/cps_expr.rs`)
   - Same pattern: renamed `CpsExpr` enum → `CpsExprKind`, created wrapper struct
   - Updated ~75 construction sites in `cps_transform.rs`
   - Updated ~20 match sites in `step.rs`, ~14 in `continuation.rs`, ~9 in `mod.rs`,
     ~4 in `environment.rs`, ~1 in `wind.rs`

3. **Lexer line/column tracking** (`patina-frontend/src/lexer/mod.rs`)
   - Added `line: u32`, `column: u32` fields to Lexer (both start at 1)
   - `advance()` increments line on `\n`, resets column; else increments column
   - Added `current_line()` and `current_column()` accessors

4. **Token location info** (`patina-frontend/src/lexer/mod.rs`)
   - Added `Spanned { token: Token, line: u32, column: u32 }` wrapper
   - `next_token()` returns `Result<Spanned, LexError>` — captures position before lexing
   - Added `next_token_kind()` convenience returning `Result<Token, LexError>`
   - Internal lexing extracted to private `lex_token()` method
   - Parser stores `current_token_line` and `current_token_column` alongside `current_token`
   - Exported `Spanned` from `patina-frontend`

5. **SourceMap struct** (`patina-frontend/src/source_map.rs`)
   - `HashMap<u64, SourceLocation>` keyed by `TaggedValue::raw_bits()`
   - Methods: `record()`, `get()`, `len()`, `is_empty()`
   - Added `TaggedValue::raw_bits() -> u64` accessor

**Verification:**
- `cargo clippy --all-targets --all-features -- -D warnings` — clean
- `cargo test --all --lib --tests` — all ~1400 tests pass, 0 failures
- `cargo build --release && ./scripts/run_chibi_tests.sh` — 1163/1163 (100%)
- All source fields are `None` — zero behavioral change
- SourceMap exists but is empty — no population until Phase 2

### Phase 2: Parser → Desugarer Source Threading — COMPLETE (2026-03-03)

**Goal:** Parser records positions, desugarer populates CoreExpr source fields.
Macro-expanded code gets call-site fallback locations (Chibi-style, L1 fidelity).

**Design Decisions:**

- **SourceMap stays in patina-frontend** — only needed by Parser (writes) and Desugarer (reads).
  The Backend trait doesn't need to know about it. Threading happens within TreeWalker's eval
  path, which already imports patina-frontend.
- **What gets recorded:** List forms (position of `(`), vectors (position of `#(`), and
  quote abbreviations (position of `'`, `` ` ``, `,`, `,@`). NOT symbols/identifiers
  (symbol interning makes per-occurrence tracking unreliable) or literals (self-evaluating,
  rarely in error messages).
- **Desugarer uses `Rc<RefCell<SourceMap>>`** — shared with parser, avoids copying.
  The desugarer calls `.borrow()` when looking up positions.
- **`eval_with_source_map` is TreeWalker-specific** — not on the Backend trait, since
  patina-runtime can't depend on patina-frontend (circular dep). Other future backends
  can add their own source-map-aware methods.
- **Tracked variants** — `eval_str_tracked()`, `eval_program_tracked()`, and
  `eval_program_resilient_tracked()` on `Interpreter<TreeWalker>` create SourceMaps
  and thread them through. The generic `eval_str()`/`eval_program()` remain unchanged
  for backward compatibility.

**Completed Steps:**

1. **Parser records source positions** (`patina-frontend/src/parser/mod.rs`)
   - Added `source_map: Option<Rc<RefCell<SourceMap>>>` and `source_name: Rc<str>` fields
   - Added `new_with_source_map(input, heap, source_name, source_map)` constructor
   - Added `record_source()` helper that writes to source map if present
   - Records positions for: list forms (`(`), vectors (`#(`), quote abbreviations
   - Existing constructors pass `source_map: None` — fully backward compatible

2. **Desugarer consumes SourceMap** (`patina-frontend/src/desugarer/mod.rs`)
   - Added `source_map: Option<Rc<RefCell<SourceMap>>>` field
   - Added `with_env_and_source_map()` constructor
   - Added `lookup_source()` helper
   - In `desugar_tagged()`: looks up source for pair forms, attaches to resulting CoreExpr
   - Macro expansion: saves call-site source before expansion, uses as fallback for
     expanded form (L1 fidelity — errors point to macro call site, not internal code)
   - All child desugarers (`with_fresh_scope`, `with_shadowed_names`, `with_new_env`)
     propagate `source_map`

3. **TreeWalker eval_with_source_map** (`patina-tree-walker/src/backend.rs`)
   - Added `eval_with_source_map(expr, env, source_map)` method on TreeWalker
   - Creates `Desugarer::with_env_and_source_map()` instead of `Desugarer::with_env()`
   - Not on Backend trait — TreeWalker-specific

4. **Interpreter tracked methods** (`patina-interpreter/src/lib.rs`)
   - Added `eval_str_tracked()`, `eval_program_tracked()`, `eval_program_resilient_tracked()`
     on `Interpreter<TreeWalker>`
   - These create SourceMap, pass to `Parser::new_with_source_map()`, then call
     `backend.eval_with_source_map()` for each expression
   - Generic methods unchanged (backward compatible)

5. **Tests** — 10 new tests:
   - Parser: list, nested lists, quote abbreviation, vector, multiline, backward compat
   - Interpreter: tracked eval_str, tracked eval_program, tracked resilient, desugarer integration

**Verification:**
- `cargo clippy --all-targets --all-features -- -D warnings` — clean
- `cargo test --all --lib --tests` — all tests pass
- `cargo build --release && ./scripts/run_chibi_tests.sh` — 1163/1163 (100%)
- CoreExpr.source populated for forms parsed with source map
- CoreExpr.source remains None for forms without source map (backward compatible)
- Zero behavioral change — source info is structural only, not yet used in error messages

### Phase 3: Error Reporting Integration — COMPLETE (2026-03-05)

**Goal:** Errors include source locations from IR nodes.

**Completed Steps:**

1. **`SourceLocation::source` changed to `Arc<str>`** (`patina-core/src/error.rs`)
   - Required by `Backend::Error: Send + Sync + 'static` constraint
   - Constructors use `impl AsRef<str>` → `Arc::from(source.as_ref())`

2. **`EvalError::WithLocation` variant** (`patina-tree-walker/src/eval/error.rs`)
   - Wraps any `EvalError` with a `SourceLocation` — no changes to existing error construction sites
   - `.at(loc)` / `.at_opt(loc)` methods to attach source
   - `is_catchable()`, `to_error_kind()`, `to_error_detail()` delegate through WithLocation

3. **CPS transform stamps source onto `LetVal` wrappers** (`patina-ir/src/cps_transform.rs`)
   - `transform_args_then` propagates `final_expr.source` to wrapping `LetVal` nodes
   - `transform_app` and `transform_apply` already stamp source onto `App`/`Apply` nodes
   - This ensures arg-evaluation errors carry the call-site source location

4. **`eval_one_step` attaches source at key sites** (`patina-tree-walker/src/eval/cps_eval/step.rs`)
   - `Var` arm: `err.at_opt(current_expr.source.clone())` before routing through exception handlers
   - `LetVal` arm: `.map_err(|e| e.at_opt(current_expr.source.clone()))` on `eval_trivial_tagged`
   - `App` arm: `call_source` attached to func-lookup and arg-evaluation errors

5. **SourceMap stores source text** (`patina-frontend/src/source_map.rs`)
   - `source_text: Option<String>` field, `set_source_text()` method
   - `get_line(line)` and `format_context(loc)` for caret-style display
   - Parser calls `source_map.set_source_text(input.to_string())` in `new_with_source_map()`

6. **Pretty error formatting** (`patina-interpreter/src/lib.rs`)
   - `format_eval_error_with_source(error, source_map)` produces:
     ```
     error: undefined variable: x
       at test.scm:15:8
          15 | (define (foo) x)
                             ^
     ```

**Verification:**
- All ~1400 tests pass, 0 failures
- `test_eval_error_with_location_tracked`, `test_format_eval_error_with_source`, `test_source_map_format_context` all pass

### Phase 4: Macro Expansion Chain Tracking (Gauche-style, L2) — COMPLETE (2026-03-05)

**Goal:** Source info survives macro expansion with expansion chain tracing.
Upgrades from L1 (call-site fallback) to L2 (expansion chain).

**Architecture constraint:** `patina-macros` cannot depend on `patina-frontend` (circular:
`patina-frontend` depends on `patina-macros`). All Phase 4 work stays in `patina-frontend`
(SourceMap + Desugarer) and `patina-interpreter` (error formatting). The macro expander
is untouched.

**Key insight:** `CompiledMacro { pub name: Rc<str>, ... }` — the desugarer already has the
macro name at the expansion call site without needing to parse the list head.

**Completed Steps:**

1. **`SourceMap::expansion_records`** (`patina-frontend/src/source_map.rs`)
   - Added `expansion_records: HashMap<(u32, u32), Vec<String>>` keyed by `(line, col)` of
     the macro call site. Each entry is an ordered list of macro names (outermost first).
   - `record_expansion(loc, macro_name)` — appends a name to the list at that location
   - `get_expansions(loc)` — returns the chain as `&[String]`

2. **`stamp_expansion_source` free function** (`patina-frontend/src/desugarer/mod.rs`)
   - Recursively walks a freshly-expanded pair tree and stamps each unrecorded pair with
     the call-site source location. Bounded to depth 64.
   - Only stamps pairs not already in the SourceMap (parser-recorded pairs are preserved).
   - Borrow discipline: `source_map.borrow_mut()` is dropped before `heap.borrow()`.

3. **Desugarer records expansions at call sites** (`patina-frontend/src/desugarer/mod.rs`)
   - In the macro expansion path of `desugar_tagged()`, after calling
     `expand_macro_with_shadowed_tagged`, Phase 4 inserts:
     ```rust
     if let (Some(src), Some(sm)) = (&call_site_source, &self.source_map) {
         stamp_expansion_source(expanded_tagged, src, sm, shared_heap, 0);
         sm.borrow_mut().record_expansion(src, compiled_macro.name.to_string());
     }
     ```
   - No changes to `patina-macros` or the `expand_macro_with_shadowed_tagged` signature.

4. **Error formatter shows macro chain** (`patina-interpreter/src/lib.rs`)
   - `format_eval_error_with_source()` now appends after caret context:
     - Single macro: `"  macro expansion: let"`
     - Multiple macros: `"  macro expansion chain: cond → if"`
   - Example output:
     ```
     Undefined variable: y
       at test.scm:1:1
        1 | (let ((x 1)) (+ x y))
          ^^^^^^^^^^^^^^^^^^^^^
       macro expansion: let
     ```

**Tests added:** `test_expansion_chain_single_macro`, `test_expansion_chain_nested_macros`,
`test_source_stamp_inner_forms`, `test_no_regression_unexpanded_errors`

**Note:** Bare variable references (not in call position) don't carry source via the CPS
mechanism — the error must come from inside a call form `(+ 0 y)` for the expansion chain to
appear. This is a known limitation of the L1/L2 hybrid approach.

**Verification:**
- `cargo test --all --lib --tests` — all tests pass, 0 failures

### Phase 4b: Full Source Tracking (Racket-style, L3) — Deferred

**Deferred because:** L2 (Gauche-style) gives the majority of the user-visible improvement.
L3 requires identifier-level source tagging which intersects deeply with the scope-set hygiene
system and is best done after Phase 2 (VM backend) gives us a more stable IR pipeline.
Design is preserved here for when we revisit.

**Why L3 matters:** Gauche-style only chains form-level origins. Racket-style tracks
_which macro introduced each identifier_ — enabling "go to macro definition" and
step-by-step macro expansion debugging. This is the foundation for the future macro stepper.

**Goal:** Racket-level debugging fidelity with identifier origin tracking and properties.

This phase moves beyond form-level tracking to identifier-level granularity.

**Tasks:**

1. **Identifier-level source tracking**
   - Each identifier in expanded code tracks which macro introduced it
   - Analogous to Racket's `'origin` property on syntax objects
   - Enables: "this `lambda` was introduced by the `let` macro defined at lib/scheme/base/binding.scm:3"

2. **Property system on SourceMap entries**
   - Extend SourceMap entries with a property bag (like Racket's `syntax-property`)
   - Properties: `origin` (introducing identifier), `original` (pre-expansion form),
     `paren-shape`, custom user properties
   - Foundation for IDE features: "show macro expansion", "go to macro definition"

3. **Source-preserving macro debugging**
   - `(expand expr)` shows expansion with full source annotations
   - Step-by-step macro expansion viewer showing source at each step
   - Analogous to Racket's macro stepper

4. **`syntax-debug-info` equivalent**
   - Runtime-queryable binding context at any source location
   - "At line 42, these identifiers are in scope: {x from local, cons from (scheme base), ...}"
   - Foundation for IDE autocomplete and hover info

### Phase 5: REPL Integration — COMPLETE (2026-03-05)

**Goal:** REPL and script mode emit rich caret-style error messages.

**Completed Steps:**

1. **`format_interpreter_error` helper** (`patina-interpreter/src/lib.rs`)
   - Dispatches to `format_eval_error_with_source` for `EvalError`; falls back to `Display` for parse/lex/desugar errors

2. **Three new `Interpreter<TreeWalker>` methods** (`patina-interpreter/src/lib.rs`)
   - `eval_str_with_source_name(input, name)` → `(Result, Rc<RefCell<SourceMap>>)`
   - `eval_program_with_source_name(input, name)` → `(Result, Rc<RefCell<SourceMap>>)`
   - `eval_program_resilient_with_source_name(input, name)` — prints rich errors inline

3. **REPL** (`patina-repl/src/repl/mod.rs`)
   - Added `expr_counter: u32` field; each eval uses `<repl-N>` as source name
   - Switched to `eval_str_with_source_name`; errors formatted via `format_interpreter_error`

4. **Script mode** (`patina-repl/src/main.rs`)
   - Strict mode uses `eval_program_with_source_name(filename)`
   - Test/resilient mode uses `eval_program_resilient_with_source_name(filename)`

**Example output:**
```
Error: Undefined variable: y
  at <repl-1>:1:15
   1 | (let ((x 1)) (+ x y))
     ^^^^^^^^^^^^^^^^^^^^^
  macro expansion: let
```

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

## Key Files Modified/To Modify

| Phase | File | Change | Status |
|-------|------|--------|--------|
| 1 | `patina-core/src/core_expr.rs` | Wrapper struct: CoreExpr{kind,source} | Done |
| 1 | `patina-core/src/cps_expr.rs` | Wrapper struct: CpsExpr{kind,source} | Done |
| 1 | `patina-core/src/tagged_value.rs` | Added `raw_bits()` accessor | Done |
| 1 | `patina-core/src/lib.rs` | Re-export CoreExprKind, CpsExprKind | Done |
| 1 | `patina-ir/src/lib.rs` | Re-export CoreExprKind, CpsExprKind | Done |
| 1 | `patina-ir/src/cps_transform.rs` | Updated ~75 CpsExpr + ~38 CoreExpr sites | Done |
| 1 | `patina-ir/src/visitor.rs` | Match on expr.kind | Done |
| 1 | `patina-frontend/src/lexer/mod.rs` | Line/column tracking + Spanned wrapper | Done |
| 1 | `patina-frontend/src/parser/mod.rs` | Store token position in parser | Done |
| 1 | `patina-frontend/src/source_map.rs` | New: SourceMap struct | Done |
| 1 | `patina-frontend/src/desugarer/mod.rs` | Updated ~85 CoreExpr construction/match sites | Done |
| 1 | `patina-tree-walker/src/eval/cps_eval/*.rs` | Updated CpsExpr match/construction sites | Done |
| 2 | `patina-frontend/src/parser/mod.rs` | Build SourceMap during parsing | Done |
| 2 | `patina-frontend/src/desugarer/mod.rs` | Look up SourceMap, populate CoreExpr.source | Done |
| 2 | `patina-tree-walker/src/backend.rs` | eval_with_source_map method | Done |
| 2 | `patina-interpreter/src/lib.rs` | Tracked eval methods (eval_str_tracked, etc.) | Done |
| 3 | `patina-core/src/error.rs` | SourceLocation::source → Arc<str> for Send+Sync | Done |
| 3 | `patina-tree-walker/src/eval/error.rs` | EvalError::WithLocation + .at()/.at_opt() | Done |
| 3 | `patina-ir/src/cps_transform.rs` | Stamp source on App/Apply/LetVal nodes | Done |
| 3 | `patina-tree-walker/src/eval/cps_eval/step.rs` | Attach source in Var/LetVal/App arms | Done |
| 3 | `patina-frontend/src/source_map.rs` | source_text + format_context() | Done |
| 3 | `patina-interpreter/src/lib.rs` | format_eval_error_with_source() | Done |
| 4 | `patina-frontend/src/source_map.rs` | expansion_records + record_expansion() + get_expansions() | Done |
| 4 | `patina-frontend/src/desugarer/mod.rs` | stamp_expansion_source() free fn + call in macro path | Done |
| 4 | `patina-interpreter/src/lib.rs` | Expansion chain display in format_eval_error_with_source() | Done |
| 5 | `patina-interpreter/src/lib.rs` | `eval_str/program_with_source_name()` + `format_interpreter_error()` | Done |
| 5 | `patina-repl/src/repl/mod.rs` | `expr_counter` + `eval_str_with_source_name` + rich errors | Done |
| 5 | `patina-repl/src/main.rs` | Script mode uses `*_with_source_name` + `format_interpreter_error` | Done |

## Success Criteria

1. Error messages show file:line:col for parse errors
2. Error messages show source location for eval errors
3. Macro expansion errors show both macro definition and use site
4. Source tracking adds <10% overhead to parse time
5. Existing tests pass with minimal changes
6. Zero runtime overhead (TaggedValue evaluation unaffected)

## References

- **Existing infrastructure**: `patina-core/src/error.rs` (SourceLocation, ErrorDetail)
- **Chez Scheme**: `annotation` wrapping + `preinfo` IR records + lazy line computation (`/reference/ChezScheme/s/`)
  - `types.ss` — annotation/source record types
  - `syntax.ss` — `source-wrap` for macro expansion
  - `base-lang.ss` — `preinfo` records on compiler IR
- **Chibi Scheme**: Inline pair source fields (`/reference/chibi-scheme/`)
  - `include/chibi/sexp.h` — `sexp_pair_source` field
  - `sexp.c` — reader attaches source
  - `eval.c` — macro expansion preserves source
- **Racket**: First-class syntax objects with `srcloc` + `'origin` properties (`/reference/racket/`)
  - `src/expander/syntax/syntax.rkt` — syntax struct with srcloc
  - `src/expander/syntax/track.rkt` — `syntax-track-origin`
  - `src/expander/compile/serialize.rkt` — srcloc serialization to bytecode
- **Gauche**: Extended pairs with `'source-info` + `'original` chains (`/reference/Gauche/`)
  - `src/gauche/priv/pairP.h` — extended pair structure
  - `src/read.c` — reader attaches source-info attribute
  - `src/libmacbase.scm` — macro expansion chains `'original`
  - `src/libeval.scm` — recursive chain walking for error display
- **Rust compiler**: `rustc_span` crate
