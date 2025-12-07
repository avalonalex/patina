# Parser Issues - R7RS Compliance Gaps

**Created:** 2025-12-07
**Status:** Active
**Source:** chibi-scheme r7rs-tests.scm compatibility report

This document tracks parser-only issues identified from the R7RS test suite. These are fixes that don't require changes to the evaluator or runtime - purely lexer and parser modifications.

## Summary

| Category | Errors | Failures | Total Issues | Fixed |
|----------|--------|----------|--------------|-------|
| Read Syntax | 8 | 0 | 8 | 3 (datum comments, fold-case, vertical bar escaping) |
| Numeric Syntax | 0 | 9 | 9 | 8 (-.X numbers, exponent markers, case-insensitive inf/nan, radix rationals, complex exponent markers, complex inf display, decimal prefix complex, number->string precision) |
| **Total** | **8** | **9** | **17** | **11** |

**Test Results (Latest):**
- Read Syntax: 85/93 passing (91.4%)
- Numeric Syntax: 220/220 passing (100%) ✅
- Overall: 1111/1158 passing (95.9%)

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

## Remaining Numeric Syntax Issues

### 5. ~~Complex Numbers with Alternate Exponent Markers~~ - ✅ FIXED (2025-12-07)

**Status:** COMPLETED - 6 more tests passing (2 errors fixed + 4 additional passing)

**Implementation:**
- Updated `parse_real_component_as_value()` to detect alternate exponent markers (`s`, `f`, `d`, `l`)
- Added call to `normalize_exponent_markers()` before parsing floats
- Also fixed `parse_real_component()` for polar notation
- Fixed complex number display: inexact `1.0` now displays as `1.0i` not `i` (preserves exactness)

**Examples now working:**
```scheme
(read (open-input-string "1s2+1.0i"))   ; => 100.0+1.0i
(read (open-input-string "1.0+1s2i"))   ; => 1.0+100.0i
(read (open-input-string "1d2+1.0i"))   ; => 100.0+1.0i (double marker)
(read (open-input-string "1l2+1.0i"))   ; => 100.0+1.0i (long marker)
```

**Files changed:**
- `crates/patina-frontend/src/parser/mod.rs` - Updated `parse_real_component_as_value()` and `parse_real_component()`
- `crates/patina-core/src/value.rs` - Fixed complex display to preserve inexact `1.0` as `1.0i`

---

### 6. ~~Complex Number Display with `+inf.0`~~ - ✅ FIXED (2025-12-07)

**Status:** COMPLETED - 2 more tests passing

**Implementation:**
- Updated Complex number Display impl to check if imaginary part already has a sign
- When imaginary part displays with `+` or `-` prefix (like `+inf.0`, `+nan.0`), don't add another `+`
- Fixed both the pure imaginary case (real=0) and the general case (real≠0)

**Examples now working:**
```scheme
(make-rectangular +inf.0 +inf.0)  ; => +inf.0+inf.0i (was +inf.0++inf.0i)
(make-rectangular -inf.0 +inf.0)  ; => -inf.0+inf.0i (was -inf.0++inf.0i)
(make-rectangular 1.0 +inf.0)     ; => 1.0+inf.0i (was 1.0++inf.0i)
(make-rectangular 0 +inf.0)       ; => +inf.0i (was ++inf.0i)
```

**Files changed:**
- `crates/patina-core/src/value.rs` - Updated Complex Display impl to check for existing signs

---

### 7. ~~Decimal Prefix with Complex Numbers~~ - ✅ FIXED (2025-12-07)

**Status:** COMPLETED - Issue was fixed by previous changes to `parse_real_component_as_value()`

**Implementation:**
- The fix for Issue #5 (Complex Numbers with Alternate Exponent Markers) also fixed this issue
- `parse_real_component_as_value()` now properly preserves inexact `1.0` in the imaginary part
- The `is_exact_one()` helper (renamed from `is_one()`) only matches exact integers, not inexact 1.0

**Examples now working:**
```scheme
(read (open-input-string "#d1.0+1.0i"))  ; => 1.0+1.0i (was 1.0+i)
(read (open-input-string "#d2.5+3.5i"))  ; => 2.5+3.5i
```

**Files changed:**
- No additional changes needed - fix was part of Issue #5

---

### 8. ~~`number->string` Precision (Write Issues)~~ - ✅ FIXED (2025-12-07)

**Status:** COMPLETED - 9 failures fixed, Numeric syntax now 220/220 (100%)

**Implementation:**
- Updated `real_to_string()` to use scientific notation for extreme values
- Added `format_scientific()` helper for R7RS-compatible formatting
- Uses scientific notation for:
  - Numbers >= 1e15 (too many digits for decimal)
  - Numbers < 1e-4 (too many leading zeros)
- Ensures mantissa has decimal point (e.g., "5.0e-324" not "5e-324")
- Ensures exponent has explicit sign (e.g., "1e+15" not "1e15")

**R7RS Compliance:**
- `number->string` must produce output that round-trips correctly via `string->number`
- Uses minimum digits needed for round-trip (per R7RS 6.2.6)
- Matches chibi-scheme behavior

**Examples now working:**
```scheme
(number->string 1.7976931348623157e308)  ; => "1.7976931348623157e+308"
(number->string 4.940656458412465e-324)  ; => "5.0e-324"
(number->string 1e15)                    ; => "1.0e+15"
(number->string 1e-5)                    ; => "1.0e-5"
```

**Files changed:**
- `crates/patina-tree-walker/src/eval/primitives/conversion.rs` - Updated `real_to_string()`, added `format_scientific()`

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

**Current Status (after 11 parser fixes + test-assert):**
- Read Syntax: 85/93 passing (91.4%)
- Numeric Syntax: 220/220 passing (100%) ✅
- Overall: 1111/1158 passing (95.9%)

**Remaining Parser Issues (1 total):**

| Issue | Type | Count | Effort | Category |
|-------|------|-------|--------|----------|
| Datum labels `#n=`/`#n#` | Error | 2 | Large | Parser |

**Non-Parser Issues (not tracked here):**
- Exception handling tests (guard, raise) - ~23 errors
- Environments and evaluation (scheme eval) - 4 errors
- System interface (file-error?) - 1 error
- Read syntax datum labels (#0=, #0#) - 2 errors

Total parser-related improvements: **+74 tests** from initial state (1037 → 1111)
