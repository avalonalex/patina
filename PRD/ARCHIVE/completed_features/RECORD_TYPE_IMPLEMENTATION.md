# R7RS Record Types Implementation Plan

This document describes the implementation plan for `define-record-type` as specified in R7RS Section 5.5.

## Overview

Record types are user-defined data types with named fields. R7RS requires:
- **Generative semantics**: Each `define-record-type` creates a new, distinct type
- **Type disjointness**: Record types are disjoint from all other types (including vectors)
- **Field access**: Accessor and optional mutator procedures for each field

## R7RS Specification Summary

From R7RS Section 5.5:

```scheme
(define-record-type <name>
  (<constructor name> <field name> ...)
  <pred>
  (<field name> <accessor name>)
  (<field name> <accessor name> <modifier name>)
  ...)
```

**Bindings created:**
- `<name>` - bound to the record type itself
- `<constructor name>` - procedure to create instances
- `<pred>` - predicate returning `#t` only for instances of this type
- `<accessor name>` - procedures to read field values
- `<modifier name>` - procedures to mutate field values (optional per field)

**Example:**
```scheme
(define-record-type <pare>
  (kons x y)
  pare?
  (x kar set-kar!)
  (y kdr))

(pare? (kons 1 2))     ; => #t
(pare? (cons 1 2))     ; => #f (different from pairs!)
(kar (kons 1 2))       ; => 1
(kdr (kons 1 2))       ; => 2
(let ((k (kons 1 2)))
  (set-kar! k 3)
  (kar k))             ; => 3
```

## Design Decision: Native vs Macro-Based

### Option 1: Macro-based (Vector Representation)
Records implemented as tagged vectors: `#(type-tag field1 field2 ...)`

**Pros:**
- Pure Scheme implementation, no Rust changes
- Self-contained in `base-extras.scm`

**Cons:**
- Violates type disjointness (records would be vectors)
- Less efficient (runtime type-tag checking)
- `vector?` would return `#t` for records (incorrect)

### Option 2: Native Record Type (Chosen)
Add new `Value::Record` and `Value::RecordType` variants.

**Pros:**
- Proper type disjointness
- Efficient predicates (variant matching)
- Clean semantics
- Extensible for SRFI-9, SRFI-99, R7RS-large

**Cons:**
- Requires Rust changes across multiple crates

**Decision**: Native implementation for correctness and future-proofing.

## Implementation Architecture

### 1. Core Types (`patina-core/src/value.rs`)

#### RecordTypeDescriptor
```rust
/// Record type descriptor - represents the type itself
///
/// Each call to define-record-type creates a new descriptor with
/// a unique ID (generative semantics).
#[derive(Debug, Clone)]
pub struct RecordTypeDescriptor {
    /// Unique identifier (generative semantics)
    /// Two record types with same name/fields are still distinct
    pub id: usize,
    /// Name of the record type (for display/debugging)
    pub name: Rc<str>,
    /// Field names in declaration order
    pub fields: Vec<Rc<str>>,
}

impl PartialEq for RecordTypeDescriptor {
    fn eq(&self, other: &Self) -> bool {
        // Identity based on unique ID only (generative)
        self.id == other.id
    }
}
```

#### Value Enum Extensions
```rust
pub enum Value {
    // ... existing variants ...

    /// Record type descriptor (result of define-record-type)
    RecordType(Rc<RecordTypeDescriptor>),

    /// Record instance
    Record {
        /// The record type this instance belongs to
        record_type: Rc<RecordTypeDescriptor>,
        /// Field values (same order as fields in descriptor)
        /// RefCell for mutability via modifier procedures
        fields: Rc<RefCell<Vec<Value>>>,
    },
}
```

#### Display Implementation
```rust
// In impl Display for Value:
Value::RecordType(rtd) => write!(f, "#<record-type {}>", rtd.name),
Value::Record { record_type, .. } => write!(f, "#<record {}>", record_type.name),
```

#### type_name Extension
```rust
// In Value::type_name():
Value::RecordType(_) => "record-type",
Value::Record { record_type, .. } => {
    // Could return the specific record type name
    // For now, just "record"
    "record"
},
```

### 2. Unique ID Generation

Thread-safe atomic counter for generative semantics:

```rust
// In patina-core or patina-tree-walker
use std::sync::atomic::{AtomicUsize, Ordering};

static RECORD_TYPE_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn next_record_type_id() -> usize {
    RECORD_TYPE_COUNTER.fetch_add(1, Ordering::SeqCst)
}
```

### 3. Low-Level Primitives (`patina-tree-walker/src/eval/primitives/`)

Create new file `records.rs` with internal primitives:

| Primitive | Signature | Description |
|-----------|-----------|-------------|
| `%make-record-type` | `(name fields) -> rtd` | Create new RecordTypeDescriptor |
| `%record-type?` | `(obj) -> bool` | Check if obj is a RecordTypeDescriptor |
| `%record?` | `(obj) -> bool` | Check if obj is a Record |
| `%record-type-of` | `(record) -> rtd` | Get descriptor from Record |
| `%make-record` | `(rtd values) -> record` | Create Record instance |
| `%record-ref` | `(record index) -> value` | Read field by index |
| `%record-set!` | `(record index value) -> unspecified` | Write field by index |
| `%record-type-name` | `(rtd) -> symbol` | Get type name |
| `%record-type-fields` | `(rtd) -> list` | Get field names |

**Implementation pattern:**
```rust
fn make_record_type(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    let name = extract_symbol(&args[0])?;
    let fields = extract_list(&args[1])?
        .iter()
        .map(|v| extract_symbol(v))
        .collect::<Result<Vec<_>, _>>()?;

    let rtd = RecordTypeDescriptor {
        id: next_record_type_id(),
        name: Rc::from(name),
        fields: fields.into_iter().map(Rc::from).collect(),
    };

    Ok(Value::RecordType(Rc::new(rtd)))
}
```

### 4. Macro Implementation (`lib/scheme/base-extras.scm`)

The `define-record-type` macro expands to primitive calls:

```scheme
(define-syntax define-record-type
  (syntax-rules ()
    ;; Main pattern with fields
    ((_ name (constructor-name constructor-field ...)
        predicate
        (field-name accessor . maybe-modifier) ...)
     (begin
       ;; Create the record type descriptor
       (define name
         (%make-record-type 'name '(field-name ...)))

       ;; Create the predicate
       (define (predicate obj)
         (and (%record? obj)
              (eq? (%record-type-of obj) name)))

       ;; Create the constructor
       ;; Note: Must handle field ordering - constructor fields may be
       ;; in different order than field declarations
       (define constructor-name
         (%make-record-constructor name
                                   '(constructor-field ...)
                                   '(field-name ...)))

       ;; Create accessors and mutators
       (define-record-field name field-name accessor . maybe-modifier)
       ...))))

;; Helper macro for individual field definitions
(define-syntax define-record-field
  (syntax-rules ()
    ;; Field with accessor only
    ((_ rtd field-name accessor)
     (define (accessor record)
       (%record-ref record (%field-index rtd 'field-name))))
    ;; Field with accessor and mutator
    ((_ rtd field-name accessor mutator)
     (begin
       (define (accessor record)
         (%record-ref record (%field-index rtd 'field-name)))
       (define (mutator record value)
         (%record-set! record (%field-index rtd 'field-name) value))))))
```

**Note:** The macro needs careful handling of:
1. Constructor field ordering vs declaration field ordering
2. Fields in constructor that initialize to unspecified if not listed
3. Error checking (field names must match, no duplicates)

### 5. Constructor Complexity

The constructor is non-trivial because:
- Constructor fields may be a subset of all fields
- Constructor field order may differ from declaration order
- Non-constructor fields initialize to `unspecified`

**Option A: Macro generates inline constructor**
```scheme
(define (kons x y)
  (%make-record <pare> (vector x y)))
```

**Option B: Runtime constructor builder primitive**
```scheme
(define kons
  (%make-record-constructor <pare> '(x y) '(x y)))
```

Option B is cleaner - the primitive handles field mapping internally.

## Files to Modify

### Core Changes
1. **`crates/patina-core/src/value.rs`**
   - Add `RecordTypeDescriptor` struct
   - Add `Value::RecordType` variant
   - Add `Value::Record` variant
   - Update `Display` impl
   - Update `type_name()`
   - Add `next_record_type_id()` function

2. **`crates/patina-core/src/lib.rs`**
   - Export new types

### Primitives
3. **`crates/patina-tree-walker/src/eval/primitives/mod.rs`**
   - Add `mod records;`
   - Register record primitives

4. **`crates/patina-tree-walker/src/eval/primitives/records.rs`** (new file)
   - Implement all `%record-*` primitives

5. **`crates/patina-tree-walker/src/eval/primitives/predicates.rs`**
   - Add `record?` and `record-type?` public predicates (if desired)

### Macro
6. **`lib/scheme/base-extras.scm`**
   - Add `define-record-type` macro
   - Add helper macros if needed

### Tests
7. **`crates/patina-tests/tests/scheme_base.rs`**
   - Add comprehensive record type tests

## Test Cases

```scheme
;; Basic record type
(define-record-type <point>
  (make-point x y)
  point?
  (x point-x)
  (y point-y set-point-y!))

;; Construction and predicates
(test #t (point? (make-point 1 2)))
(test #f (point? (cons 1 2)))
(test #f (point? (vector 1 2)))

;; Accessors
(test 1 (point-x (make-point 1 2)))
(test 2 (point-y (make-point 1 2)))

;; Mutators
(test 3 (let ((p (make-point 1 2)))
          (set-point-y! p 3)
          (point-y p)))

;; Generative semantics - two definitions create distinct types
(define-record-type <point2>
  (make-point2 x y)
  point2?
  (x point2-x)
  (y point2-y))

(test #f (point? (make-point2 1 2)))
(test #f (point2? (make-point 1 2)))

;; Partial constructor (not all fields in constructor)
(define-record-type <person>
  (make-person name)
  person?
  (name person-name)
  (age person-age set-person-age!))

(test "Alice" (person-name (make-person "Alice")))
;; person-age returns unspecified for new person

;; Type disjointness
(test #f (vector? (make-point 1 2)))
(test #f (pair? (make-point 1 2)))
(test #f (null? (make-point 1 2)))
```

## Implementation Order

1. **Phase 1: Core types**
   - Add `RecordTypeDescriptor` and Value variants
   - Implement Display
   - Add ID generation

2. **Phase 2: Basic primitives**
   - `%make-record-type`
   - `%record?`, `%record-type?`
   - `%make-record`
   - `%record-ref`, `%record-set!`
   - `%record-type-of`

3. **Phase 3: Constructor primitive**
   - `%make-record-constructor` (handles field ordering)

4. **Phase 4: Macro**
   - Basic `define-record-type` macro
   - Handle accessor/mutator generation

5. **Phase 5: Tests**
   - Unit tests for primitives
   - Integration tests for macro
   - R7RS compliance tests

## Edge Cases and Gotchas

1. **Field ordering**: Constructor fields may differ from declaration order
2. **Uninitialized fields**: Fields not in constructor are `unspecified`
3. **Duplicate field names**: Should error at macro expansion time
4. **Same accessor/mutator name**: Should error
5. **eq? semantics**: Records use reference equality (same instance)
6. **eqv? semantics**: Same as eq? for records
7. **equal? semantics**: Could compare field-by-field (implementation choice)

## Future Extensions

This design supports future enhancements:
- **SRFI-9**: Already compatible (R7RS is based on SRFI-9)
- **SRFI-99**: ERR5RS records with inheritance
- **R7RS-large**: Extended record facilities
- **Record inheritance**: Could extend RecordTypeDescriptor with parent field

## References

- R7RS Section 5.5 "Record-type definitions"
- SRFI-9 "Defining Record Types"
- Chibi-scheme `lib/srfi/9.scm`
