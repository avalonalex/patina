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

---

## Datum Labels Research

### R7RS Specification (Section 2.4)

The R7RS spec defines datum labels as follows:

**Syntax:**
- `#<n>=<datum>` - Labels `<datum>` with label `<n>` (where `<n>` is a sequence of digits)
- `#<n>#` - References the object labelled by `#<n>=`

**Semantics:**
- `#<n>=<datum>` reads the same as `<datum>`, but also labels it with `<n>`
- `#<n>#` evaluates to the **same object** (in the `eqv?` sense) as the labelled datum
- Enables notation of **shared** and **circular** structures

**Scope Rules:**
- Label scope is the portion of the **outermost datum** to the **right** of the label
- References can only appear **after** the label definition (no forward references)
- Self-reference `#n= #n#` is an error (object not well-defined)

**Example from spec:**
```scheme
(let ((x (list 'a 'b 'c)))
  (set-cdr! (cddr x) x)
  x)                       ⇒ #0=(a b c . #0#)
```

### Reference Implementation Analysis (chibi-scheme)

chibi-scheme implements datum labels using a **two-phase approach**:

**Phase 1: Parsing with Placeholder Labels**

During parsing (`sexp_read_raw`):
1. When `#<n>=` is encountered:
   - Create a placeholder `reader_label(n)` object
   - Store it in a `shares` vector at index `n`
   - Continue reading the datum
   - Replace the placeholder with the actual parsed value

2. When `#<n>#` is encountered:
   - Look up label `n` in the `shares` vector
   - Return either:
     - The actual value (if already resolved)
     - The placeholder (if inside the labelled datum - creates cycles)

**Phase 2: Post-Processing (`sexp_fill_reader_labels`)**

After parsing completes:
1. Walk the entire parsed structure
2. Replace any remaining placeholder labels with actual values
3. Use mark bits to handle cycles (avoid infinite loops)
4. Run twice with different mark states to handle all cases

**Key Data Structures:**
```c
// Placeholder for unresolved labels
sexp_make_reader_label(n)  // Creates placeholder for label n
sexp_reader_labelp(x)      // Checks if x is a placeholder
sexp_unbox_reader_label(x) // Gets label number from placeholder

// Storage for labels
shares: Vector[Value]      // Index = label number, Value = labelled datum
```

### Implementation Plan for Patina

#### Phase 1: Lexer Changes

Add two new token types:
```rust
pub enum Token {
    // ... existing tokens ...
    DatumLabel(usize),  // #n=
    DatumRef(usize),    // #n#
}
```

In `read_hash_dispatch()`:
```rust
// After '#', check if next char is a digit
if ch.is_ascii_digit() {
    let n = self.read_label_number(ch);
    match self.peek_char() {
        Some('=') => { self.advance(); Ok(Token::DatumLabel(n)) }
        Some('#') => { self.advance(); Ok(Token::DatumRef(n)) }
        _ => Err(LexError::InvalidSyntax("expected = or # after #<n>"))
    }
}
```

#### Phase 2: Parser Changes

Add parser state for tracking labels:
```rust
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    labels: HashMap<usize, Value>,           // Resolved labels
    pending_refs: Vec<(usize, *mut Value)>,  // References to fix up
}
```

Parsing `#n=<datum>`:
```rust
Token::DatumLabel(n) => {
    // Check for forward reference / duplicate
    if self.labels.contains_key(&n) {
        return Err(ParseError::DuplicateLabel(n));
    }
    // Parse the datum
    let datum = self.parse_expr()?;
    // Store in labels map
    self.labels.insert(n, datum.clone());
    Ok(datum)
}
```

Parsing `#n#`:
```rust
Token::DatumRef(n) => {
    match self.labels.get(&n) {
        Some(value) => Ok(value.clone()),
        None => Err(ParseError::UndefinedLabel(n)),
    }
}
```

#### Phase 3: Cycle Handling (The Hard Part)

For cyclic structures like `#0=(1 . #0#)`:

**Option A: Placeholder + Post-Processing (chibi-style)**
1. Create a special `Value::LabelPlaceholder(usize)` variant
2. During parsing, insert placeholder for forward refs within same datum
3. After parsing, walk structure and replace placeholders with actual values
4. Use `Rc<RefCell<...>>` to enable mutation for cycles

**Option B: Lazy Resolution**
1. Store references as `Value::LabelRef(usize)` permanently
2. Resolve lazily when the value is accessed
3. Requires changes throughout the evaluator

**Option C: Immediate Resolution with Mutation**
1. Parse `#0=` but don't know the full value yet
2. Create an empty placeholder Pair
3. Parse the contained datum, which may reference `#0#`
4. Fill in the placeholder using `Rc<RefCell<...>>` mutation

**Recommended: Option A (Placeholder + Post-Processing)**

This matches chibi-scheme's approach and is cleanest:

```rust
// New Value variant (temporary, only during parsing)
enum Value {
    // ... existing variants ...
    LabelPlaceholder(usize),  // Only exists during read
}

// Post-processing function
fn resolve_labels(value: Value, labels: &HashMap<usize, Value>) -> Value {
    match value {
        Value::LabelPlaceholder(n) => labels[&n].clone(),
        Value::Pair(cell) => {
            let (car, cdr) = &*cell.borrow();
            let new_car = resolve_labels(car.clone(), labels);
            let new_cdr = resolve_labels(cdr.clone(), labels);
            // Mutate in place to preserve identity for cycles
            let mut inner = cell.borrow_mut();
            inner.0 = new_car;
            inner.1 = new_cdr;
            Value::Pair(cell.clone())
        }
        Value::Vector(vec) => { /* similar */ }
        other => other,
    }
}
```

#### Challenges and Edge Cases

1. **Cycle Detection**: Need to track visited nodes to avoid infinite loops
2. **Identity Preservation**: `#0#` must return the **same object** as `#0=`, not a copy
3. **Nested Labels**: `#0=(#1=(a b) #1#)` - multiple labels in same datum
4. **Error Handling**:
   - Forward references: `#0# ... #0=...` is an error
   - Self-reference: `#0= #0#` is an error
   - Undefined label: `#0#` without `#0=` is an error
   - Duplicate label: Two `#0=` in same datum

#### Testing

Test cases needed:
```scheme
;; Basic shared structure
(let ((x (read (open-input-string "(#0=(1 2 3) #0#)"))))
  (eq? (car x) (cadr x)))  ; => #t (same object)

;; Cyclic list
(let ((x (read (open-input-string "#0=(a b c . #0#)"))))
  (eq? x (cdddr x)))  ; => #t

;; Nested labels
(read (open-input-string "#0=(#1=(a) #1# #0#)"))

;; Error cases
(read (open-input-string "#0#"))         ; Error: undefined label
(read (open-input-string "#0= #0#"))     ; Error: self-reference
(read (open-input-string "(#0# #0=x)"))  ; Error: forward reference
```

#### Effort Estimate

- **Lexer changes**: Small (add two token types, digit parsing)
- **Parser changes**: Medium (label tracking, placeholder handling)
- **Post-processing**: Medium-Large (cycle handling, identity preservation)
- **Testing**: Medium (many edge cases)

**Total: Large effort** - Estimated 4-6 hours for full implementation

#### Dependencies

- Requires `Rc<RefCell<...>>` for pairs/vectors (already in place)
- May need changes to `write` procedure to output `#n=` / `#n#` notation
- May need `Value::LabelPlaceholder` or similar temporary variant

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
