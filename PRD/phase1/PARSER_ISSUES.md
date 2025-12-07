# Parser Issues - R7RS Compliance Gaps

**Created:** 2025-12-07
**Status:** Active
**Source:** chibi-scheme r7rs-tests.scm compatibility report

This document tracks parser-only issues identified from the R7RS test suite. These are fixes that don't require changes to the evaluator or runtime - purely lexer and parser modifications.

## Summary

| Category | Errors | Failures | Total Issues | Fixed |
|----------|--------|----------|--------------|-------|
| Read Syntax | 8 | 0 | 8 | 3 (datum comments, fold-case, vertical bar escaping) |
| Numeric Syntax | 4 | 0 | 4 | 4 (-.X numbers, exponent markers, case-insensitive inf/nan, radix rationals) |
| **Total** | **12** | **0** | **12** | **7** |

Note: Some errors in the test report are due to missing `test-assert` macro (17 total), not parser issues.

**Test Results After Fixes:**
- Read Syntax: 85/93 passing (was 74/93) - **+11 tests, 0 failures**
- Numeric Syntax: 190/207 passing (was 152/191) - **+38 tests**
- Overall: 1077/1145 passing (94.1%)

---

## Read Syntax Issues

### 1. ~~Datum Comments (`#;`)~~ - ✅ FIXED (2025-12-07)

**Status:** COMPLETED - 7 tests now passing

**Implementation:**
- Added `DatumComment` token to lexer
- Added `skip_datum()` and `skip_list()` methods to parser
- Parser handles `#;` by skipping the next complete datum before parsing
- Works with nested datum comments, quoted expressions, vectors, dotted lists

**Files changed:**
- `crates/patina-frontend/src/lexer/mod.rs` - Added DatumComment token and `;` handling
- `crates/patina-frontend/src/parser/mod.rs` - Added skip_datum(), skip_list(), and datum comment handling in parse_expr() and parse_list()

---

### 2. ~~Reader Directives (`#!fold-case`, `#!no-fold-case`)~~ - ✅ FIXED (2025-12-07)

**Status:** COMPLETED - 2 tests now passing

**Implementation:**
- Added `fold_case: bool` field to Lexer struct
- Added `read_reader_directive()` method to handle `#!` directives
- `#!fold-case` sets `fold_case = true`, `#!no-fold-case` sets `fold_case = false`
- Modified `read_identifier()` to apply `to_lowercase()` when `fold_case` is true
- Directives don't produce tokens - they just modify lexer state and return the next token

**Files changed:**
- `crates/patina-frontend/src/lexer/mod.rs` - Added fold_case state and directive handling

---

### 3. Datum Labels (`#0=`, `#0#`) - 2 errors

**Problem:** R7RS graph notation for cyclic and shared structures.

**Examples that fail:**
```scheme
(cadr (read (open-input-string "#0=(1 . #0#)")))     ; Cyclic list
(cadr (read (open-input-string "(#0=(1 2 3) #0#)"))) ; Shared structure
```

**Current behavior:** Lexer returns `UnexpectedChar('0')` when it sees `#0`.

**Fix location:** Lexer + Parser coordination needed

**Fix approach:**
1. Lexer: Add token types `DatumLabel(usize)` for `#n=` and `DatumRef(usize)` for `#n#`
2. Parser: Track labels in a HashMap during parsing, resolve references

**Effort:** Large - requires parser state and post-processing for cycles

---

### 4. ~~Vertical Bar Identifier Escaping (Write)~~ - ✅ FIXED (2025-12-07)

**Status:** COMPLETED - 2 tests now passing (failures reduced to 0)

**Implementation:**
- Added `escape_for_vertical_bar()` helper function in Value's Display impl
- Escapes `|` as `\|` and `\` as `\\` when writing symbols inside vertical bar notation
- Applied to both Symbol and Identifier display

**Files changed:**
- `crates/patina-core/src/value.rs` - Added escape_for_vertical_bar() and used it in Display impl

---

## Numeric Syntax Issues

### 1. ~~Numbers Starting with `-.`~~ - ✅ FIXED (2025-12-07)

**Status:** COMPLETED - 4 more tests passing, failures reduced from 10 to 6

**Implementation:**
- Added `peek_is_decimal_start()` helper that checks for `.` followed by a digit
- Added this check to the number detection condition in `next_token()`
- Now `-.1`, `+.5`, etc. correctly parse as numbers instead of symbols

**Files changed:**
- `crates/patina-frontend/src/lexer/mod.rs` - Added peek_is_decimal_start() and updated number detection

---

### 2. ~~Alternate Exponent Markers (`s`, `f`, `d`, `l`)~~ - ✅ FIXED (2025-12-07)

**Status:** COMPLETED - 16 more tests passing

**Implementation:**
- Added `normalize_exponent_markers()` helper that replaces `s/S/f/F/d/D/l/L` with `e`
- Uses `Cow<str>` for zero-allocation fast path when no markers present
- Applied before `f64` parsing in `parse_number()`

**Note:** Per R7RS 7.1.1, these markers are optional ("implementations may accept") and the spec
defines no specific precision requirements - only that `s < f < d < l` and default (`e`) is at
least double precision. Since we use `f64` for all inexact numbers, our implementation is
spec-compliant.

**Files changed:**
- `crates/patina-frontend/src/parser/mod.rs` - Added normalize_exponent_markers() and used in parse_number()

---

### 3. ~~Case-Insensitive `inf.0` and `nan.0`~~ - ✅ FIXED (2025-12-07)

**Status:** COMPLETED - 6 more tests passing, failures reduced from 6 to 4

**Implementation:**
- Updated `is_special_float_literal()` in lexer to use case-insensitive comparison
- Updated `parse_number()`, `parse_real_component()`, and `parse_real_component_as_value()` to use `.to_lowercase()` matching
- Also added support for `-nan.0` which R7RS allows

**Files changed:**
- `crates/patina-frontend/src/lexer/mod.rs` - Case-insensitive is_special_float_literal()
- `crates/patina-frontend/src/parser/mod.rs` - Case-insensitive matching in 3 locations

---

### 4. ~~Rationals with Non-Decimal Radix~~ - ✅ FIXED (2025-12-07)

**Status:** COMPLETED - 12 more tests passing

**Implementation:**
- Extended `parse_number_with_prefix()` to detect `/` in non-decimal radix numbers
- Parse numerator and denominator separately using `BigInt::parse_bytes()` with the given radix
- Handles hex (`#x1/10`), octal (`#o11/2`), and binary (`#b11/10`) rationals
- Simplifies to integer when denominator divides evenly
- Validates zero denominator

**Examples now working:**
```scheme
#x1/10   ; => 1/16 (hex 1 / hex 10)
#x10/2   ; => 8 (hex 16 / hex 2 = 8)
#x11/2   ; => 17/2
#o11/2   ; => 9/2 (octal 11 / octal 2)
#b11/10  ; => 3/2 (binary 11 / binary 10)
#xa/b    ; => 10/11 (hex a / hex b)
```

**Files changed:**
- `crates/patina-frontend/src/parser/mod.rs` - Extended parse_number_with_prefix() for rational support

---

## Prioritized Implementation Plan

### Phase 1: Easy Wins (Small Changes)

These can be done together and would fix ~12 issues:

1. **`-.1` parsing** (2 failures fixed)
   - Add `peek_is_decimal_point()` helper
   - Add condition in number detection

2. **Case-insensitive inf/nan** (2 errors fixed)
   - Use `.to_lowercase()` in 3 locations

3. **Alternate exponent markers** (8 errors fixed)
   - Normalize `s/S/f/F/d/D/l/L` to `e` before parsing

**Estimated effort:** 1-2 hours

### Phase 2: Medium Effort

4. **`#;` datum comments** (7 errors fixed)
   - Requires lexer to skip complete datum
   - May need to refactor to allow parser callback

5. **`#!fold-case`** (2 errors fixed)
   - Add lexer state flag
   - Apply case folding during identifier reading

6. **Radix rationals** (6 errors fixed)
   - Extend `parse_number_with_prefix` for `/`

**Estimated effort:** 3-4 hours

### Phase 3: Larger Effort

7. **Datum labels `#n=`/`#n#`** (2 errors fixed)
   - New token types
   - Parser state for label tracking
   - Post-processing for cycle creation

**Estimated effort:** 4-6 hours

---

## Non-Parser Issues (For Reference)

These errors appeared in the report but are not parser issues:

| Issue | Count | Actual Cause |
|-------|-------|--------------|
| `test-assert` undefined | 17 | Missing test macro |
| Vertical bar escaping | 2 | Writer/display issue |

---

## Success Metrics

**Current Status (after 7 fixes):**
- Read Syntax: 85/93 passing (91.4%)
- Numeric Syntax: 190/207 passing (91.8%)
- Overall: 1077/1145 passing (94.1%)

**Remaining issues:**
- Datum labels (`#n=`/`#n#`) - 2 errors (complex, requires parser state for cycles)
- Various non-parser errors (missing exceptions, eval, etc.)

Total parser-related improvements: **+49 tests** from initial state
