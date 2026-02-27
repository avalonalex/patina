# Tagged Pointer Design for Patina

**Status**: Phase 11 complete — Heap numeric operations fully native; `numeric/` Value module deleted (3,139 lines removed)
**Last Updated**: Feb 26, 2026
**Related**: `CLONE_OPTIMIZATION_ANALYSIS.md`, `../phase2/VM_VALUE_ARCHITECTURE.md`, `../phase2/VM_SPECIFICATION.md`

---

## Overview

TaggedValue is the **unified value representation** for all of Patina — tree-walker, VM backend, parser, and primitives. Rather than maintaining two representations (Value + TaggedValue) with conversion at boundaries, we use TaggedValue everywhere. This eliminates conversion overhead, reduces code duplication, and gives 8x memory savings (8 bytes vs 64 bytes per value).

---

## Design: Low-Bit Tagged Pointers

Uses low 3 bits for type tags (same approach as chibi-scheme):

```
Tag    | Type                   | Storage
-------+------------------------+---------
0b000  | Fixnum (61-bit signed) | Immediate — TAG_FIXNUM=0 enables optimal arithmetic
0b001  | Special (#t, #f, (), eof, unspecified) | Immediate
0b010  | Character (Unicode)    | Immediate
0b011  | Pair                   | Heap pointer → (TaggedValue, TaggedValue)
0b100  | Vector                 | Heap pointer → Vec<TaggedValue>
0b101  | String                 | Heap pointer → String
0b110  | Closure                | Heap pointer → code + free vars
0b111  | Object (sub-tagged)    | Heap pointer → HeapObjectData enum
```

Special values: `FALSE=0x01`, `TRUE=0x09`, `NULL=0x11`, `EOF=0x19`, `UNSPECIFIED=0x21`

HeapObjectData sub-types (18): BigInt, Rational, Real, Complex, Symbol, Bytevector, Port, Macro, RecordType, Record, Exception, Identifier, Continuation, Promise, Parameter, Library, Values, PromptTag

### Heap Arena

```rust
pub struct Heap {
    pairs: Vec<(TaggedValue, TaggedValue)>,
    vectors: Vec<HeapVector>,
    strings: Vec<String>,
    closures: Vec<HeapClosure>,
    objects: Vec<HeapObject>,      // Generic sub-tagged objects
    boxed_values: Vec<Box<Value>>, // Legacy escape hatch (shrinking)
    // ... free lists, symbol table, etc.
}
```

---

## Completed Phases (1–6)

1. **Foundation** — TaggedValue + Heap in patina-core, Environment stores TaggedValue, shared heap
2. **Parser & Evaluator** — Parser produces TaggedValue, CPS evaluator native, desugarer has `desugar_tagged()`
3. **Macro System** — Pattern matching on TaggedValue, MatchEnv stores TaggedValue, template expander returns TaggedValue
4. **Heap Types** — HeapString, HeapVector storing `Vec<TaggedValue>`, all 297 primitives use `PrimitiveFn::new_tagged()`
5. **Environment & API** — Environment API uses TaggedValue exclusively, library loading shares global heap
6. **Native Heap Representations** — All primitives, evaluator core, desugarer, and macro compiler operate directly on TaggedValue/Heap

---

## Current State (Feb 26, 2026) — Phases 7–11 Complete

**Test status:** All tests passing (1159/1159 chibi — 100%). All internal tests passing (0 failures).

**Migration progress:** All user-facing crates are Value-free. The Backend trait, interpreter, pipeline, all primitives, macro system, and desugarer all operate on TaggedValue exclusively. All numeric operations now run directly on TaggedValue/HeapObjectData in `heap/numeric.rs` — the entire Value-based `numeric/` module (8 files, 3,139 lines) has been deleted. Remaining Value usage is in the **foundation layer** (patina-core heap internals, BoxedValue bridge) plus BoxedValue fallback paths in records.rs and datum_writer.rs.

### Crate-Level Status

| Crate | Status | Notes |
|-------|--------|-------|
| **patina-macros** | ✅ Value-free | All 14 conversion calls replaced with `format_tagged()` |
| **patina-pipeline** | ✅ Value-free | |
| **patina-interpreter** | ✅ Value-free | Backend trait + API fully TaggedValue |
| **patina-frontend** | ✅ Value-free (tests) | Production: 1 `get_boxed_value` for datum label placeholder (irreducible). All number parsing returns TaggedValue directly. Tests fully native. |
| **patina-tree-walker** | 🔶 2 files remain | records.rs (BoxedValue fallbacks, 12 sites), datum_writer.rs (8 `get_boxed_value`) |
| **patina-core** | 🔶 Bridge layer | heap/mod.rs (50 — BoxedValue bridge), environment.rs (7 — define + debug), debug_format.rs (3 fallback). **heap/numeric.rs fully native (0 sites).** |
| **patina-runtime** | 🔴 Type definitions | Value enum, Arity, Procedure, RecordTypeDescriptor |
| **patina-tests** | ✅ Value-free | All test files migrated to TaggedValue-native assertions |

### Remaining Conversion Sites by File (sorted by count)

| File | `tagged_to_value` | `value_to_tagged` | `get_boxed_value` | Total |
|------|-------------------|--------------------|--------------------|-------|
| `patina-core/value_convert.rs` | 28 | 25 | 0 | **53** |
| `patina-core/heap/mod.rs` | 17 | 4 | 29 | **50** |
| `patina-tree-walker/records.rs` | — | — | 12 | **12** |
| `patina-tree-walker/datum_writer.rs` | 0 | 0 | 8 | **8** |
| `patina-core/environment.rs` | 5 | 2 | 0 | **7** |
| `patina-core/debug_format.rs` | 3 | 0 | 0 | **3** |
| `patina-frontend/parser/mod.rs` | 0 | 0 | 1 | **1** |
| **Total** | **53** | **31** | **50** | **~134** |

Note: `value_convert.rs` (53 sites) is infrastructure — deleted last when all consumers are gone. Total down from ~243 to ~134 (45% reduction from Phase 11).

### Non-Value Types from `patina_runtime::value` Module

Many tree-walker files import helper types that are **not** the Value enum but live in the same module:

| Type | Used by | Notes |
|------|---------|-------|
| `Arity` | 10 files (debug, lazy, process_context, time, system, continuations, records, eval, test, mod.rs) | Procedure arity — trivial to move to patina-core |
| `Procedure` | application.rs, mod.rs | Procedure enum (Primitive, CpsLambda) — needs redesign |
| `RecordTypeDescriptor` | records.rs | Record type metadata — stored in HeapObjectData::Record |
| `PromiseState` | lazy.rs | Promise state tracking — stored in HeapObjectData::Promise |

---

## Roadmap to Complete Value Elimination

### Phase 10: Frontend Boundary Migration (✅ Complete)

~~25. **Parser number assembly**~~ ✅ — See completed phases below.

### Phase 11: Foundation Layer (✅ Complete)

See completed phases below.

### Phase 12: Extract Value Module Types

28. **Move `Arity` to patina-core** — Trivial move, update 10 import sites
29. **Move `Procedure` enum to patina-core** — Larger change, used by application.rs and HeapObjectData::Procedure
30. **Move `RecordTypeDescriptor` to patina-core** — Used by HeapObjectData::Record
31. **Move `PromiseState` to patina-core** — Used by HeapObjectData::Promise

### Phase 13: Delete Value

32. **Delete BoxedValue** — Remove `HeapObjectData::BoxedValue` variant; all values stored natively
33. **Delete `value_convert.rs`** — No more consumers (53 sites become dead code)
34. **Delete `Value` enum** — Remove from patina-runtime and patina-core
35. **Delete datum_writer Value paths** — With no BoxedValue, all formatting is TaggedValue-native

**Result:** TaggedValue is the **only** value representation. 8 bytes per value, zero conversion overhead.

### Phase 14: VM Integration (Separate project)

VM uses same TaggedValue and Heap — no conversion between tree-walker and VM.

---

## Completed Phases

### Phase 7: Eliminate Value from Internal Paths (✅ Complete)

1. ~~EvalResult::Value variant~~ ✅ Removed; TailCall { expr: Value } also deleted
2. ~~Native HeapObjectData::Identifier~~ ✅
3. ~~Native Parameter, Promise, Values, Continuation~~ ✅
4. ~~Macro patterns/templates store TaggedValue~~ ✅ (Pattern::Literal, Template::Literal)
5. ~~Macro compiler takes TaggedValue rules~~ ✅
6. ~~Desugarer fully Value-free~~ ✅
7. ~~cond-expand evaluates TaggedValue directly~~ ✅
8. ~~Macro test infrastructure~~ ✅
9. ~~patina-macros production code fully Value-free~~ ✅ (Feb 24)
10. ~~BodyElement::Begin stores Vec<TaggedValue>~~ ✅ (Feb 24)
11. ~~Library parser migration~~ ✅ (Feb 24)
12. ~~Delete `parse_to_value_deep()` / `parse_all_to_values_deep()`~~ ✅ (Feb 24)
13. ~~Delete Value-based cond_expand helpers~~ ✅ (Feb 24)
14. ~~CPS evaluator cleanup~~ ✅ (Feb 24)
15. ~~Primitive-by-primitive migration~~ ✅ (Feb 25) — IO (8 files), arithmetic (helpers, number_theory, mod tests), test.rs, datum_writer.rs
16. ~~Numeric comparison native path~~ ✅ (Feb 25) — Heap comparison methods return `Result<bool, NumericError>`, comparison.rs fully Value-free
17. ~~Record storage migration~~ ✅ (Feb 25) — `HeapObjectData::Record` fields now `Vec<TaggedValue>`, record_ref/record_set! work natively
18. ~~Complex number display fix~~ ✅ (Feb 25) — R7RS-correct formatting in datum_writer.rs. Fixed 9 complex_numbers test failures + 2 chibi numeric syntax failures.
19. ~~Symbol write escaping~~ ✅ (Feb 25) — Symbols wrapped in `|...|` in write mode when they could be misread. Fixed 17 chibi Read syntax failures.

### Phase 8: Backend API Migration (✅ Complete)

20. ~~Backend trait~~ ✅ (Feb 25) — `Backend::eval()` now takes/returns `TaggedValue` directly. Deleted 4 intermediate methods (`eval_tagged`, `eval_global_tagged`, `eval_tagged_internal`, `eval_global_tagged_internal`). TreeWalker collapsed from 3 impl methods to 1. Interpreter uses `eval_global()` directly. Zero Value↔TaggedValue conversions at API boundary.
21. ~~Records.rs cleanup~~ ✅ (Feb 25) — `extract_symbol_list_tagged` now uses `get_pair_as_tagged()` instead of manual BoxedValue::Pair handling. Eliminated `value_to_tagged` from field list extraction.
22. ~~Values display fix~~ ✅ (Feb 25) — `display_tagged()` in Interpreter and SimpleInterpreter now unpacks `Values` objects, displaying each value on its own line. Fixed `test_callcc_with_values`.

### Phase 9: Test & Peripheral Cleanup (✅ Complete)

23. ~~pvref.rs compatibility methods~~ ✅ (Feb 26) — Deleted 3 unused methods (`leaf_from_value`, `insert_value`, `get_value`) and `Value` import. Zero external callers.
24. ~~Test file migration~~ ✅ (Feb 26) — `common/mod.rs`: deleted unused `assert_eval_type` + `Value` import. `verify_bigint_promotion.rs`: replaced `to_value()` helper with `tv.as_fixnum()`/`heap.is_bigint()`/`heap.get_bigint()`. `sld_file_loading.rs`: replaced `env_get_value()` helper with `tv.as_fixnum()`/`heap.get_symbol_name()`. All 3 test files now Value-free.
25. ~~Environment.rs test migration~~ ✅ (Feb 26) — Tests use `heap.borrow_mut().intern_symbol()` instead of `value_to_tagged(&Value::Symbol(...))`. Assertions use `heap.get_symbol_name()` instead of `tagged_to_value` + `Value::Symbol` pattern matching. Production debug logging (6 sites behind `macro_debug::is_enabled()`) retained — irreducible until Value deleted.
26. ~~Procedure display in datum_writer~~ ✅ (Feb 26) — Added `heap.get_procedure(tv)` check for `HeapObjectData::Procedure` (CPS lambdas, primitives). Previously only TAG_CLOSURE was handled, causing `#<unknown>` for procedures stored in record fields. Fixed pre-existing `test_record_field_holds_any_value` failure.

### Phase 10: Frontend Boundary Migration (✅ Complete)

27. ~~Macro system~~ ✅ (Feb 26) — Added `format_tagged(tv, heap)` to patina-core as a native TaggedValue display formatter. Replaced all 14 `tagged_to_value`/`tagged_to_value_deep` calls across 5 patina-macros files (debug.rs, list_match.rs, matcher/mod.rs, interface.rs, expander/tests.rs). patina-macros is now fully Value-free.
28. ~~Environment display in datum_writer~~ ✅ (Feb 26) — Added `heap.is_environment(tv)` check so environment specifiers display as `#<environment>` instead of `#<unknown>`. Fixed 4 pre-existing `scheme_eval` test failures.
29. ~~Parser number assembly~~ ✅ (Feb 26) — All number parsing methods (`parse_number`, `parse_number_with_prefix`, `parse_rectangular`, `parse_polar`, `parse_real_component_as_tagged`) now return TaggedValue directly via heap allocation (`alloc_bigint`, `alloc_rational`, `alloc_real`, `alloc_complex`). Deleted `parse_number_as_value()`, `value_to_tagged()`, and `make_complex()`. Added `tagged_integer()`, `tagged_rational()`, `apply_exactness()`, `alloc_complex()` helpers. All 30+ test `tagged_to_value`/`parse_to_value` calls replaced with native heap queries (`get_real`, `get_rational`, `get_complex`, `get_symbol_name`, `vector_ref`). Only 1 `get_boxed_value` remains for datum label placeholder resolution (irreducible until Phase 13). patina-frontend tests are fully Value-free.

### Phase 11: Foundation Layer — Heap Numeric Operations (✅ Complete)

30. ~~Heap numeric operations~~ ✅ (Feb 26) — Replaced all 112 `tagged_to_value`/`value_to_tagged` conversion sites in `heap/numeric.rs` with direct TaggedValue/HeapObjectData operations. New implementation uses `NumData` private enum for type dispatch, `_impl` pattern (public methods handle NaN + contagion; private `_impl` methods handle type dispatch + complex recursion), `simplify_complex_tagged` for complex result normalization, and `is_object()` guards for non-object TaggedValues. Comparisons borrow heap data immutably (`&self`); arithmetic uses extract-then-compute pattern (`&mut self`). Zero `tagged_to_value` or `value_to_tagged` calls remain. Zero `use crate::value::Value` imports.
31. ~~Dead code cleanup~~ ✅ (Feb 26) — Deleted 8 files from `numeric/` module (arithmetic.rs, comparison.rs, complex.rs, exactness.rs, integer_div.rs, predicates.rs, rounding.rs, transcendental.rs) — **3,139 lines removed**. All Value-based numeric methods had zero external callers after step 30. Only `numeric/error.rs` (NumericError enum, 61 lines) retained. Remaining `heap/mod.rs` conversion sites (50) are BoxedValue bridge code — irreducible until Phase 13.

### Optional Future Enhancements

- **NaN-boxing for floats** — Unbox f64 in TaggedValue (eliminates heap/numeric.rs conversions)
- **Generational GC** — Replace reference counting with tracing GC
- **Inline small strings** — Store short strings (≤7 chars) directly in TaggedValue

---

## References

- [Chibi-Scheme sexp.h](https://github.com/ashinn/chibi-scheme/blob/master/include/chibi/sexp.h) — Real-world tagged pointer implementation
- [NaN Boxing](https://piotrduperas.com/posts/nan-boxing/)
- [Value representation in JS](https://wingolog.org/archives/2011/05/18/value-representation-in-javascript-implementations)
- [Tagged pointers in Clasp CL](https://drmeister.wordpress.com/2015/05/16/tagged-pointers-and-immediate-fixnums-characters-and-single-floats-in-clasp/)
- [Float Self-Tagging (2024)](https://arxiv.org/html/2411.16544) — Academic research on Scheme value representations
