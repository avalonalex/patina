# Missing R7RS scheme.base Primitives

This document tracks primitives that are required by R7RS scheme.base but not yet implemented in Patina.

**Status Legend:**
- ✅ Implemented (in registry or bootstrap.scm)
- ❌ Missing
- 🚧 Partially implemented

Last updated: 2025-11-13

---

## Summary Statistics

**Total R7RS scheme.base exports:** ~250 procedures/forms
**Currently implemented:** ~150 (60%)
**Missing:** ~100 (40%)

---

## 1. Number Operations (Section 6.2)

### Conversions
- ❌ `number->string` - Convert number to string representation
- ❌ `string->number` - Parse string as number

### Division operations (R7RS added these)
- ❌ `floor/` - Division with floor semantics, returns quotient and remainder
- ❌ `floor-quotient` - Quotient with floor semantics
- ❌ `floor-remainder` - Remainder with floor semantics
- ❌ `truncate/` - Division with truncate semantics, returns quotient and remainder
- ❌ `truncate-quotient` - Quotient with truncate semantics (same as `quotient`)
- ❌ `truncate-remainder` - Remainder with truncate semantics (same as `remainder`)

**Priority:** High (number->string, string->number), Medium (division ops)
**Complexity:** Medium

---

## 2. Symbols (Section 6.5)

All symbol operations except `symbol?` are missing:

- ❌ `symbol=?` - Test symbol equality
- ❌ `symbol->string` - Convert symbol to string
- ❌ `string->symbol` - Intern string as symbol

**Priority:** High (very commonly used)
**Complexity:** Low-Medium

---

## 3. Characters (Section 6.6)

### Predicates
- ✅ `char?` (in registry)
- ❌ `char-alphabetic?` - Is character alphabetic?
- ❌ `char-numeric?` - Is character numeric (0-9)?
- ❌ `char-whitespace?` - Is character whitespace?
- ❌ `char-upper-case?` - Is character uppercase letter?
- ❌ `char-lower-case?` - Is character lowercase letter?

### Comparisons
- ❌ `char=?` - Character equality
- ❌ `char<?` - Character less than
- ❌ `char>?` - Character greater than
- ❌ `char<=?` - Character less than or equal
- ❌ `char>=?` - Character greater than or equal
- ❌ `char-ci=?` - Case-insensitive equality
- ❌ `char-ci<?` - Case-insensitive less than
- ❌ `char-ci>?` - Case-insensitive greater than
- ❌ `char-ci<=?` - Case-insensitive less than or equal
- ❌ `char-ci>=?` - Case-insensitive greater than or equal

### Conversions
- ❌ `char->integer` - Get Unicode codepoint
- ❌ `integer->char` - Create character from codepoint
- ❌ `char-upcase` - Convert to uppercase
- ❌ `char-downcase` - Convert to lowercase
- ❌ `char-foldcase` - Case folding for case-insensitive comparison

### Other
- ❌ `digit-value` - Get numeric value of digit character (0-9 -> 0-9)

**Priority:** High (comparisons and conversions), Medium (predicates)
**Complexity:** Low-Medium

---

## 4. Strings (Section 6.7)

### Higher-order operations
- ❌ `string-map` - Map function over string characters
- ❌ `string-for-each` - Iterate over string characters for side effects

### Mutation
- ❌ `string-fill!` - Fill string with character
- ❌ `string-copy!` - Copy string contents (with ranges)

### Case conversion
- ❌ `string-upcase` - Convert string to uppercase
- ❌ `string-downcase` - Convert string to lowercase
- ❌ `string-foldcase` - Case folding for case-insensitive comparison

**Priority:** Medium-High (string-map, string-for-each), Low (case operations)
**Complexity:** Low-Medium

---

## 5. Pairs and Lists (Section 6.4)

### Mutation
- ❌ `set-car!` - Mutate car of pair
- ❌ `set-cdr!` - Mutate cdr of pair
- ❌ `list-set!` - Set element at index in list

### Operations
- ❌ `list-copy` - Create shallow copy of list
- ✅ `make-list` - Create list of n elements (need to verify if implemented)

**Priority:** Medium (commonly used in some code)
**Complexity:** Low

---

## 6. Control Features (Section 6.10)

### Continuations
- ❌ `call-with-current-continuation` (aka `call/cc`) - Capture continuation
- ❌ `dynamic-wind` - Register before/after thunks for non-local exits

**Priority:** High (call/cc is fundamental, needed for many advanced patterns)
**Complexity:** High (requires major interpreter changes)

**Notes:**
- Continuations require capturing and restoring interpreter state
- May need to implement as special forms with evaluator support
- Dynamic-wind interacts with exception handling

---

## 7. Bytevectors (Section 6.9) ⭐ ENTIRE SECTION MISSING

### Construction and predicates
- ❌ `bytevector?` - Is object a bytevector?
- ❌ `make-bytevector` - Create bytevector of size k
- ❌ `bytevector` - Create bytevector from bytes
- ❌ `bytevector-length` - Get bytevector length

### Access and mutation
- ❌ `bytevector-u8-ref` - Get byte at index
- ❌ `bytevector-u8-set!` - Set byte at index

### Operations
- ❌ `bytevector-copy` - Copy bytevector (with optional range)
- ❌ `bytevector-copy!` - Copy bytevector contents to another
- ❌ `bytevector-append` - Concatenate bytevectors

### UTF-8 conversions
- ❌ `utf8->string` - Decode UTF-8 bytevector to string
- ❌ `string->utf8` - Encode string to UTF-8 bytevector

**Priority:** Medium (less commonly used, but important for binary I/O)
**Complexity:** Low-Medium (need to add Bytevector to Value enum)

**Implementation plan:**
1. Add `Bytevector(Rc<RefCell<Vec<u8>>>)` variant to Value enum
2. Implement basic operations in primitives/bytevectors.rs
3. UTF-8 conversion uses Rust's string encoding

---

## 8. Exceptions (Section 6.11) ⭐ ENTIRE SECTION MISSING ⭐

### Basic exception handling
- ❌ `with-exception-handler` - Install exception handler
- ❌ `raise` - Raise exception (non-continuable)
- ❌ `raise-continuable` - Raise exception (continuable)

### Error reporting
- ❌ `error` - Signal error with message and irritants
- ❌ `error-object?` - Is object an error object?
- ❌ `error-object-message` - Get error message
- ❌ `error-object-irritants` - Get error irritants (extra info)

### Specific error predicates
- ❌ `read-error?` - Is error a read error?
- ❌ `file-error?` - Is error a file error?

### R7RS guard macro (usually in scheme.base)
- ❌ `guard` - Exception handling with pattern matching

**Priority:** HIGH (fundamental for robust error handling)
**Complexity:** HIGH (requires exception system architecture)

**Implementation plan:**
1. Add exception types to Value enum or EvalError
2. Implement exception handler stack in Evaluator
3. Add raise/raise-continuable that unwinds to handler
4. Implement with-exception-handler to install handlers
5. Add guard macro to bootstrap.scm
6. Update error reporting to use error objects

**This is a major feature and very interesting!**

---

## 9. Input and Output (Section 6.13) ⭐ MAJOR SECTION ⭐

### Port types and predicates
- ❌ `port?` - Is object a port?
- ❌ `input-port?` - Is object an input port?
- ❌ `output-port?` - Is object an output port?
- ❌ `textual-port?` - Is port textual (character-based)?
- ❌ `binary-port?` - Is port binary (byte-based)?
- ❌ `input-port-open?` - Is input port open?
- ❌ `output-port-open?` - Is output port open?

### Current ports
- ❌ `current-input-port` - Get current input port (stdin)
- ❌ `current-output-port` - Get current output port (stdout)
- ❌ `current-error-port` - Get error port (stderr)

### Port operations
- ❌ `close-port` - Close port
- ❌ `close-input-port` - Close input port
- ❌ `close-output-port` - Close output port

### File I/O
- ❌ `call-with-port` - Call procedure with port, ensuring it's closed
- ❌ `call-with-input-file` - Open file for input, call proc, close
- ❌ `call-with-output-file` - Open file for output, call proc, close
- ❌ `open-input-file` - Open file for reading
- ❌ `open-output-file` - Open file for writing
- ❌ `open-binary-input-file` - Open binary file for reading
- ❌ `open-binary-output-file` - Open binary file for writing

### String ports
- ❌ `open-input-string` - Create input port from string
- ❌ `open-output-string` - Create output string port
- ❌ `get-output-string` - Get accumulated output from string port

### Bytevector ports
- ❌ `open-input-bytevector` - Create input port from bytevector
- ❌ `open-output-bytevector` - Create output bytevector port
- ❌ `get-output-bytevector` - Get accumulated output from bytevector port

### Reading
- ❌ `read` - Read S-expression from port
- ❌ `read-char` - Read character from port
- ❌ `peek-char` - Peek at next character without consuming
- ❌ `read-line` - Read line as string
- ❌ `eof-object?` - Is object the EOF object?
- ❌ `eof-object` - Return the EOF object
- ❌ `char-ready?` - Is character available for reading?
- ❌ `read-string` - Read k characters as string
- ❌ `read-u8` - Read byte from binary port
- ❌ `peek-u8` - Peek at next byte without consuming
- ❌ `u8-ready?` - Is byte available for reading?

### Writing
- ✅ `write` - Write object in machine-readable form (in registry)
- ✅ `display` - Write object in human-readable form (in registry)
- ✅ `newline` - Write newline (in registry)
- ❌ `write-char` - Write character to port
- ❌ `write-string` - Write string to port (with optional range)
- ❌ `write-u8` - Write byte to binary port
- ❌ `flush-output-port` - Flush output buffer

**Priority:** VERY HIGH (I/O is fundamental, needed for file operations)
**Complexity:** HIGH (requires port system architecture)

**Implementation plan:**
1. Design Port abstraction (trait with implementations for file, string, bytevector)
2. Add Port variant to Value enum
3. Implement port registry/current ports in Evaluator
4. Add file I/O primitives in primitives/io.rs
5. Implement string/bytevector ports
6. Add read/write primitives
7. Ensure ports are properly closed (RAII with Drop)

**This is a VERY interesting and large project!**

---

## 10. System Interface (Section 6.14)

### File system
- ❌ `file-exists?` - Does file exist?
- ❌ `delete-file` - Delete file

### Program execution
- ❌ `load` - Load and evaluate Scheme file
- ❌ `exit` - Exit program with optional exit code
- ❌ `emergency-exit` - Exit immediately without cleanup
- ❌ `command-line` - Get command-line arguments

### Environment
- ❌ `get-environment-variable` - Get environment variable value
- ❌ `get-environment-variables` - Get all environment variables

### Time
- ❌ `current-second` - Get current time in seconds (TAI)
- ❌ `current-jiffy` - Get high-resolution time counter
- ❌ `jiffies-per-second` - Get jiffy resolution

### Features
- ❌ `features` - Get list of implementation features

**Priority:** Medium (system interface is less critical for basic programs)
**Complexity:** Low-Medium (mostly FFI to Rust std library)

---

## Implementation Roadmap

### Phase 1: Quick Wins (Low complexity, high value)
- [ ] Symbol operations: `symbol=?`, `symbol->string`, `string->symbol`
- [ ] Number conversions: `number->string`, `string->number`
- [ ] Character conversions: `char->integer`, `integer->char`
- [ ] Character comparisons: `char=?`, `char<?`, `char>?`, etc.
- [ ] List operations: `set-car!`, `set-cdr!`, `list-copy`, `list-set!`
- [ ] String operations: `string-fill!`, `string-copy!`

**Estimated effort:** 1-2 sessions

### Phase 2: Higher-order operations
- [ ] `string-map`, `string-for-each`
- [ ] Character predicates: `char-alphabetic?`, `char-numeric?`, etc.
- [ ] Character case: `char-upcase`, `char-downcase`, `char-foldcase`
- [ ] String case: `string-upcase`, `string-downcase`, `string-foldcase`
- [ ] Division ops: `floor/`, `floor-quotient`, etc.

**Estimated effort:** 1-2 sessions

### Phase 3: Bytevectors
- [ ] Design bytevector representation
- [ ] Add Bytevector variant to Value enum
- [ ] Implement all bytevector operations
- [ ] UTF-8 conversion primitives

**Estimated effort:** 1-2 sessions

### Phase 4: Exception System ⭐ INTERESTING PROJECT
- [ ] Design exception handling architecture
- [ ] Add exception handler stack to Evaluator
- [ ] Implement `raise`, `raise-continuable`
- [ ] Implement `with-exception-handler`
- [ ] Implement `error` and error object operations
- [ ] Add `guard` macro to bootstrap.scm
- [ ] Update all error paths to use exception system

**Estimated effort:** 3-5 sessions (major architectural change)

### Phase 5: Port System and I/O ⭐ VERY INTERESTING PROJECT
- [ ] Design Port trait and implementations
- [ ] Add Port variant to Value enum
- [ ] Implement current ports (stdin, stdout, stderr)
- [ ] Implement file I/O (open, close, call-with-*)
- [ ] Implement string ports
- [ ] Implement bytevector ports
- [ ] Implement read primitives (read, read-char, read-line, etc.)
- [ ] Implement write primitives (write-char, write-string, etc.)
- [ ] Implement flush and port state queries

**Estimated effort:** 5-10 sessions (major feature)

### Phase 6: Continuations ⭐ VERY CHALLENGING
- [ ] Design continuation capture mechanism
- [ ] Implement call/cc
- [ ] Implement dynamic-wind
- [ ] Ensure TCO works with continuations
- [ ] Test with complex control flow patterns

**Estimated effort:** 5-10 sessions (very complex)

### Phase 7: System Interface
- [ ] File system operations
- [ ] Program execution (load, exit)
- [ ] Environment variables
- [ ] Time functions
- [ ] Features list

**Estimated effort:** 2-3 sessions

---

## Notes

**Most interesting for next steps:**
1. **Exception System (Phase 4)** - Fundamental for error handling, clean architecture
2. **Port System and I/O (Phase 5)** - Enables file operations, REPLs, string manipulation
3. **Continuations (Phase 6)** - Most challenging, enables advanced control flow

**Quick wins to improve compatibility:**
- Phases 1-2 would add ~40+ commonly-used primitives with relatively low effort
- Bytevectors (Phase 3) are straightforward and enable binary I/O

**Current status:**
- We have strong coverage of numbers, lists, strings, vectors
- Main gaps are I/O, exceptions, continuations, and characters
