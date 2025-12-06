# R7RS I/O Implementation Guide

**Status:** Phase 1-6 COMPLETE
**Priority:** HIGH (Phases 1-4 done)
**Last Updated:** 2025-12-06

---

## Overview

R7RS requires comprehensive I/O support through the port abstraction. This document tracks what has been implemented and what remains.

**Current Test Results:**
- 1192 tests passing (89.4%)
- 24 tests failing
- 117 errors (mostly missing features: call/cc, guard)

---

## Implementation Status

### Phase 1: Port Infrastructure & String Ports - COMPLETE

**Completed 2025-12-06**

#### Step 1.1: Port Infrastructure
- Created `Port` struct in `patina-core/src/port.rs` with:
  - `PortKind` (Textual/Binary)
  - `PortDirection` (Input/Output)
  - `PortData` enum (String/Bytevector/Stdio/File/Closed)
- `Value::Port(Rc<Port>)` replaces old `InputPort`/`OutputPort` markers

#### Step 1.2: Port Predicates
```scheme
(port? obj)              ; ✅ Implemented
(input-port? obj)        ; ✅ Implemented
(output-port? obj)       ; ✅ Implemented
(textual-port? obj)      ; ✅ Implemented
(binary-port? obj)       ; ✅ Implemented
(input-port-open? port)  ; ✅ Implemented
(output-port-open? port) ; ✅ Implemented
```

#### Step 1.3: String Ports
```scheme
(open-input-string string)    ; ✅ Implemented
(open-output-string)          ; ✅ Implemented
(get-output-string port)      ; ✅ Implemented
```

#### Step 1.4: Current Ports
```scheme
(current-input-port)   ; ✅ Implemented (returns stdin port)
(current-output-port)  ; ✅ Implemented (returns stdout port)
(current-error-port)   ; ✅ Implemented (returns stderr port)
```

Note: These return actual port objects. Full parameterization for dynamic rebinding planned for later.

#### Step 1.5: EOF Handling
```scheme
(eof-object? obj)    ; ✅ Implemented
(eof-object)         ; ✅ Implemented
```

#### Step 1.6: Output Operations with Port Support
```scheme
(display obj)        ; ✅ Write to current-output-port
(display obj port)   ; ✅ Write to specified port
(write obj)          ; ✅ Write to current-output-port (machine-readable)
(write obj port)     ; ✅ Write to specified port
(newline)            ; ✅ Write to current-output-port
(newline port)       ; ✅ Write to specified port
(write-char char)    ; ✅ Write char to current-output-port
(write-char char port) ; ✅ Write char to specified port
(write-string string) ; ✅ Write string to port
(write-string string port start end) ; ✅ With optional bounds
```

**Circular Structure Support:**
- `write` and `display` now handle circular structures using datum labels (`#n=` and `#n#`)
- DFS-based cycle detection distinguishes circular from merely shared structures
- `write` only labels circular structures (not shared)

#### Step 1.7: Read Operations
```scheme
(read-char)          ; ✅ Read from current-input-port
(read-char port)     ; ✅ Read from specified port
(peek-char)          ; ✅ Peek from current-input-port
(peek-char port)     ; ✅ Peek from specified port
(char-ready?)        ; ✅ Check current-input-port
(char-ready? port)   ; ✅ Check specified port
(read-line)          ; ✅ Read line from current-input-port
(read-line port)     ; ✅ Read line from specified port
```

#### Step 1.8: The `read` Procedure
```scheme
(read)               ; ✅ Read from current-input-port
(read port)          ; ✅ Read from specified port
```

Implementation uses existing `patina-frontend` Parser with character source adapter.

#### Step 1.9: Port Operations
```scheme
(close-port port)         ; ✅ Close any port
(close-input-port port)   ; ✅ Close input port (validates direction)
(close-output-port port)  ; ✅ Close output port (validates direction)
(flush-output-port)       ; ✅ Flush current-output-port
(flush-output-port port)  ; ✅ Flush specified port
```

---

### Phase 2: File I/O - COMPLETE

**Completed 2025-12-06**

#### File Port Infrastructure
- Added `PortData::File(FilePortData)` variant
- `FilePortData` contains path and `FileHandle` (BufReader/BufWriter)
- All port methods (`read_char`, `peek_char`, `write_string`, etc.) support file ports
- Proper buffered I/O with `BufReader<File>` and `BufWriter<File>`

#### Textual File Operations
```scheme
(open-input-file filename)   ; ✅ Open file for reading (textual)
(open-output-file filename)  ; ✅ Open file for writing (textual, creates/truncates)
```

#### Binary File Operations
```scheme
(open-binary-input-file filename)   ; ✅ Open file for binary reading
(open-binary-output-file filename)  ; ✅ Open file for binary writing
```

#### File Utilities
```scheme
(file-exists? filename)      ; ✅ Returns #t if file exists
(delete-file filename)       ; ✅ Deletes the file
```

#### Higher-Order File I/O
```scheme
(call-with-input-file filename proc)  ; ✅ Open, call proc with port, close
(call-with-output-file filename proc) ; ✅ Open, call proc with port, close
(with-input-from-file filename thunk) ; ✅ Dynamic rebinding of current-input-port
(with-output-to-file filename thunk)  ; ✅ Dynamic rebinding of current-output-port
```

#### Library Organization
- Created `patina-runtime/src/stdlib/scheme_file.rs` for `(scheme file)` library
- All file operations properly registered with correct arities

---

### Phase 3: Extended Text I/O - COMPLETE

**Completed 2025-12-06**

```scheme
(read-string k)              ; ✅ Read k characters from current-input-port
(read-string k port)         ; ✅ Read k characters from specified port
```

**Implementation details:**
- Returns string of up to k characters
- Returns EOF if at end of file before reading any characters
- Returns empty string for k=0
- Returns partial string if fewer than k characters available

**Unit tests added:**
- `test_read_string_basic` - Reading k chars
- `test_read_string_eof` - EOF handling
- `test_read_string_partial` - Partial read when fewer chars available
- `test_read_string_zero` - Edge case: k=0 returns empty string
- `test_read_string_with_newline` - Newlines included in read

Note: `read-line` and `write-string` were already implemented in Phase 1.

---

### Phase 4: Binary I/O - COMPLETE

**Completed 2025-12-06**

#### Bytevector Port Infrastructure
- Added `PortData::Bytevector(BytevectorPortData)` variant
- `BytevectorPortData` contains `Vec<u8>` content and position
- All binary operations supported on bytevector ports
- Proper separation of textual/binary operations (errors when mixed)

#### Bytevector Ports
```scheme
(open-input-bytevector bytevector)  ; ✅ Create input port from bytevector
(open-output-bytevector)            ; ✅ Create output port accumulating to bytevector
(get-output-bytevector port)        ; ✅ Get accumulated bytevector
```

#### Byte Read/Write
```scheme
(read-u8)                           ; ✅ Read from binary input port
(read-u8 port)                      ; ✅ Read from specified port
(peek-u8)                           ; ✅ Peek from binary input port
(peek-u8 port)                      ; ✅ Peek from specified port
(write-u8 byte)                     ; ✅ Write to binary output port
(write-u8 byte port)                ; ✅ Write to specified port
(u8-ready?)                         ; ✅ Check binary input port
(u8-ready? port)                    ; ✅ Check specified port
```

#### Bulk Byte Operations
```scheme
(read-bytevector k)                              ; ✅ Read k bytes
(read-bytevector k port)                         ; ✅ Read k bytes from port
(read-bytevector! bytevector port start end)     ; ✅ Read into existing bytevector
(write-bytevector bytevector)                    ; ✅ Write bytevector
(write-bytevector bytevector port start end)     ; ✅ Write with optional bounds
```

**Unit tests added:**
- `test_input_bytevector_port` - Basic input port operations
- `test_output_bytevector_port` - Basic output port operations
- `test_peek_u8` - Peek without consuming
- `test_u8_ready` - Check for available data
- `test_binary_port_display` - Port display formatting
- `test_textual_operations_on_binary_port_fail` - Error handling
- `test_binary_operations_on_textual_port_fail` - Error handling
- `test_read_bytevector` - Bulk read operations
- `test_write_bytevector` - Bulk write operations
- `test_read_bytevector_into` - Read into existing buffer

---

### Phase 5: Additional Higher-Order I/O - COMPLETE

**Completed 2025-12-06**

```scheme
(call-with-port port proc)          ; ✅ Implemented
```

**Implementation details:**
- Calls `proc` with `port` as argument
- Closes the port after `proc` returns (whether normally or exceptionally)
- Returns the value returned by `proc`

Note: `with-input-from-file` and `with-output-to-file` require proper dynamic parameter support for current ports (planned for future).

---

### Phase 6: Advanced Write - COMPLETE

**Completed 2025-12-06**

```scheme
(write-shared obj)         ; ✅ Implemented in (scheme write)
(write-shared obj port)    ; ✅ Implemented
(write-simple obj)         ; ✅ Implemented in (scheme write)
(write-simple obj port)    ; ✅ Implemented
```

**Implementation details:**
- `write-shared` labels all shared (multiply-referenced) structures with datum labels (`#n=` and `#n#`)
- `write-simple` outputs without any datum labels (may loop on circular structures)
- Both procedures support optional port argument (defaults to current-output-port)
- Created new `(scheme write)` library with `display`, `write`, `write-shared`, `write-simple`

---

## File Organization

### Port Infrastructure
- `crates/patina-core/src/port.rs` - Port types and operations (17 unit tests)

### Primitives
- `crates/patina-tree-walker/src/eval/primitives/io.rs` - All I/O primitives
  - Port predicates
  - String port operations
  - Bytevector port operations
  - File port operations
  - Read/write operations
  - Circular structure handling (DatumLabelWriter)

### Library Definitions
- `crates/patina-runtime/src/stdlib/scheme_base.rs` - Base I/O exports
- `crates/patina-runtime/src/stdlib/scheme_file.rs` - File I/O exports
- `crates/patina-runtime/src/stdlib/scheme_write.rs` - Write library exports (write-shared, write-simple)

---

## Summary: Priority Order

| Phase | Features | Status | Impact |
|-------|----------|--------|--------|
| **1** | Port infrastructure, string ports, read | ✅ COMPLETE | ~200 tests |
| **2** | File I/O | ✅ COMPLETE | +4 tests |
| **3** | Extended text I/O (read-string) | ✅ COMPLETE | Minor |
| **4** | Binary I/O | ✅ COMPLETE | ~20 tests |
| **5** | Higher-order I/O (call-with-port) | ✅ COMPLETE | Minor |
| **6** | Advanced write (write-shared, write-simple) | ✅ COMPLETE | Minor |

---

## Remaining Work for Full R7RS I/O Compliance

1. ~~**Dynamic current-port rebinding** - For `with-input-from-file`/`with-output-to-file`~~ ✅ COMPLETE
2. **Error predicates** - `file-error?`, `read-error?` - Depends on [EXCEPTION_HANDLING.md](./EXCEPTION_HANDLING.md)

---

## R7RS I/O Library Organization

The I/O functionality is organized into R7RS-compliant libraries:

### (scheme base)
Core I/O in the base library:
- Port predicates: `port?`, `input-port?`, `output-port?`, `textual-port?`, `binary-port?`, etc.
- Current ports: `current-input-port`, `current-output-port`, `current-error-port`
- String ports: `open-input-string`, `open-output-string`, `get-output-string`
- Bytevector ports: `open-input-bytevector`, `open-output-bytevector`, `get-output-bytevector`
- Text I/O: `read-char`, `peek-char`, `read-line`, `read-string`, `write-char`, `write-string`, `newline`
- Binary I/O: `read-u8`, `peek-u8`, `write-u8`, `read-bytevector`, `write-bytevector`
- Output: `display`, `write`
- Input: `read`
- Port ops: `close-port`, `close-input-port`, `close-output-port`, `flush-output-port`, `call-with-port`
- EOF: `eof-object?`, `eof-object`

### (scheme read)
- `read` - Parse S-expressions (also in scheme base)

### (scheme write)
- `display`, `write` - Also in scheme base
- `write-shared` - Labels all shared structures
- `write-simple` - No datum labels

### (scheme file)
- Textual: `open-input-file`, `open-output-file`
- Binary: `open-binary-input-file`, `open-binary-output-file`
- Higher-order: `call-with-input-file`, `call-with-output-file`, `with-input-from-file`, `with-output-to-file`
- Utilities: `file-exists?`, `delete-file`

---

## References

- **R7RS Spec:** Section 6.13 (Input and output)
- **Chibi Tests:** `scheme_tests/chibi/r7rs-tests.scm`
- **Port Implementation:** `crates/patina-core/src/port.rs`
- **I/O Primitives:** `crates/patina-tree-walker/src/eval/primitives/io.rs`
- **Library Definitions:**
  - `crates/patina-runtime/src/stdlib/scheme_base.rs` - Base I/O exports
  - `crates/patina-runtime/src/stdlib/scheme_read.rs` - Read library
  - `crates/patina-runtime/src/stdlib/scheme_write.rs` - Write library
  - `crates/patina-runtime/src/stdlib/scheme_file.rs` - File library
