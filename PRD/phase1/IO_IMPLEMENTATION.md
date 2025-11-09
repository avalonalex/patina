# R7RS I/O Implementation Guide

**Status:** Not yet started
**Priority:** MEDIUM-HIGH
**Estimated Effort:** 1-2 weeks
**Last Updated:** 2025-11-09

---

## Overview

R7RS requires comprehensive I/O support through the port abstraction. This document outlines what needs to be implemented for R7RS compliance.

---

## Port Model

### Core Concepts

**Ports** are Scheme objects that represent input/output devices:
- **Input ports** - Deliver data upon command
- **Output ports** - Accept data
- **Textual ports** - Operate on characters (char-based)
- **Binary ports** - Operate on bytes (u8-based)

### Port Types Required

1. **File ports** - Read/write files on filesystem
2. **String ports** - Read/write to strings (textual)
3. **Bytevector ports** - Read/write to bytevectors (binary)
4. **Standard ports** - stdin, stdout, stderr

---

## R7RS Requirements by Category

### 1. Port Type Predicates (6 procedures)

**Status:** ❌ Not implemented
**Difficulty:** Easy (5 minutes each)
**Priority:** HIGH (needed for everything else)

```scheme
(port? obj)              ; Is obj any kind of port?
(input-port? obj)        ; Is obj an input port?
(output-port? obj)       ; Is obj an output port?
(textual-port? obj)      ; Is obj a textual port?
(binary-port? obj)       ; Is obj a binary port?
(input-port-open? port)  ; Is input port still open?
(output-port-open? port) ; Is output port still open?
```

**Implementation:** Add new `Value::Port` variant, add primitive predicates

---

### 2. Current Ports (3 procedures - Parameter Objects)

**Status:** ❌ Not implemented
**Difficulty:** Medium (need parameter object support)
**Priority:** HIGH (needed for basic I/O)

```scheme
(current-input-port)   ; Default input (initially stdin)
(current-output-port)  ; Default output (initially stdout)
(current-error-port)   ; Error output (initially stderr)
```

**Requirements:**
- Must be parameter objects (can override with `parameterize`)
- Initial bindings are implementation-defined textual ports
- Need to implement `parameterize` special form

---

### 3. String Ports (3 procedures)

**Status:** ❌ Not implemented
**Difficulty:** Easy-Medium
**Priority:** HIGH (useful for testing, commonly used)

```scheme
(open-input-string string)    ; Create input port from string
(open-output-string)           ; Create output port accumulating to string
(get-output-string port)       ; Get accumulated string from output port
```

**Why important:** Essential for testing I/O without filesystem

---

### 4. Basic Text I/O (MUST HAVE)

**Status:** ❌ Not implemented
**Difficulty:** Easy-Medium
**Priority:** CRITICAL (most commonly used)

#### Writing (6 procedures)
```scheme
(display obj)              ; Human-readable output (no quotes)
(display obj port)
(write obj)                ; Machine-readable output (with quotes)
(write obj port)
(newline)                  ; Write newline character
(newline port)
(write-char char)          ; Write single character
(write-char char port)
(write-string string)      ; Write string
(write-string string port)
```

**Start here!** These are the most commonly used I/O procedures.

#### Reading (5 procedures)
```scheme
(read)                     ; Read S-expression
(read port)
(read-char)                ; Read single character
(read-char port)
(peek-char)                ; Peek at next character (don't consume)
(peek-char port)
(read-line)                ; Read line as string
(read-line port)
(char-ready?)              ; Is char available without blocking?
(char-ready? port)
```

---

### 5. EOF Handling (2 procedures)

**Status:** ❌ Not implemented
**Difficulty:** Easy
**Priority:** HIGH (needed for reading)

```scheme
(eof-object? obj)    ; Is obj the EOF object?
(eof-object)         ; Return an EOF object
```

**Implementation:** Add `Value::Eof` variant

---

### 6. File I/O (8 procedures - FILE LIBRARY)

**Status:** ❌ Not implemented
**Difficulty:** Medium
**Priority:** MEDIUM (can defer initially)

#### Opening Files
```scheme
(open-input-file string)       ; Open text file for reading
(open-output-file string)      ; Open text file for writing
(open-binary-input-file string)  ; Open binary file for reading
(open-binary-output-file string) ; Open binary file for writing
```

#### Closing Ports
```scheme
(close-port port)              ; Close any port
(close-input-port port)        ; Close input port
(close-output-port port)       ; Close output port
```

**Note:** File operations require error handling (`file-error?`)

---

### 7. Higher-Order File I/O (4 procedures)

**Status:** ❌ Not implemented
**Difficulty:** Medium
**Priority:** LOW (syntactic sugar)

```scheme
(call-with-port port proc)         ; Call proc with port, auto-close
(call-with-input-file string proc) ; Open file, call proc, close
(call-with-output-file string proc)
(with-input-from-file string thunk)  ; Rebind current-input-port
(with-output-to-file string thunk)   ; Rebind current-output-port
```

**Note:** These are convenience wrappers around the basic operations

---

### 8. Binary I/O (7 procedures)

**Status:** ❌ Not implemented
**Difficulty:** Medium
**Priority:** LOW (less commonly used)

#### Bytevector Ports
```scheme
(open-input-bytevector bytevector)
(open-output-bytevector)
(get-output-bytevector port)
```

#### Binary Reading/Writing
```scheme
(read-u8)                ; Read single byte
(read-u8 port)
(peek-u8)                ; Peek at next byte
(peek-u8 port)
(write-u8 byte)          ; Write single byte
(write-u8 byte port)
(u8-ready?)              ; Is byte available?
(u8-ready? port)
```

---

### 9. Bulk I/O (5 procedures)

**Status:** ❌ Not implemented
**Difficulty:** Medium
**Priority:** MEDIUM

```scheme
(read-string k)              ; Read k characters
(read-string k port)
(read-bytevector k)          ; Read k bytes
(read-bytevector k port)
(read-bytevector! bytevector port start end)  ; Read into bytevector
(write-bytevector bytevector)
(write-bytevector bytevector port start end)
```

---

### 10. Advanced Write Procedures (4 procedures - WRITE LIBRARY)

**Status:** ❌ Not implemented
**Difficulty:** Medium
**Priority:** LOW (less commonly used)

```scheme
(write-shared obj)         ; Write with shared structure notation
(write-shared obj port)
(write-simple obj)         ; Simple write (no shared structure)
(write-simple obj port)
```

---

## Implementation Phases

### Phase 1: Minimal I/O (Most Important - Start Here!)

**Goal:** Basic display/write/read for debugging and REPL
**Effort:** 2-3 days
**Priority:** CRITICAL

```scheme
; Port types
(port? obj)
(input-port? obj)
(output-port? obj)
(textual-port? obj)

; Current ports (use stdout/stdin initially, no parameterize)
(current-input-port)
(current-output-port)
(current-error-port)

; String ports (for testing)
(open-input-string str)
(open-output-string)
(get-output-string port)

; Basic output (MOST IMPORTANT!)
(display obj)
(display obj port)
(write obj)
(write obj port)
(newline)
(newline port)
(write-char char)
(write-char char port)

; Basic input
(read)
(read port)
(read-char)
(read-char port)
(eof-object? obj)
(eof-object)
```

**Test Plan:**
```scheme
; Should work after Phase 1
(display "Hello, world!")
(newline)
(write '(1 2 3))
(define out (open-output-string))
(display "test" out)
(get-output-string out)  ; => "test"
```

---

### Phase 2: File I/O

**Goal:** Read/write files
**Effort:** 2-3 days
**Priority:** HIGH

```scheme
(open-input-file filename)
(open-output-file filename)
(close-port port)
(close-input-port port)
(close-output-port port)
```

**Requirements:**
- Error handling (file not found, permissions, etc.)
- `file-error?` predicate
- Proper resource cleanup

---

### Phase 3: Parameter Objects & Dynamic Rebinding

**Goal:** Make current ports overridable
**Effort:** 1-2 days
**Priority:** MEDIUM

```scheme
(parameterize ((current-output-port new-port))
  (display "goes to new-port"))

(with-output-to-file "output.txt"
  (lambda () (display "to file")))
```

**Requirements:**
- Implement `parameterize` special form
- Make current-{input,output,error}-port parameter objects

---

### Phase 4: Advanced I/O

**Goal:** Complete R7RS compliance
**Effort:** 3-4 days
**Priority:** LOW-MEDIUM

```scheme
; Binary I/O
(read-u8)
(write-u8 byte)
(open-input-bytevector bv)
(open-output-bytevector)

; Bulk operations
(read-string k)
(read-bytevector k)

; Higher-order
(call-with-port port proc)
(call-with-input-file filename proc)
```

---

## Implementation Strategy

### Value Type Extension

```rust
// src/value/mod.rs
pub enum Value {
    // ... existing variants ...

    Port(Rc<RefCell<Port>>),
    Eof,
}

pub struct Port {
    kind: PortKind,
    direction: PortDirection,
    open: bool,
    // Implementation-specific data
    data: PortData,
}

pub enum PortKind {
    Textual,
    Binary,
}

pub enum PortDirection {
    Input,
    Output,
    InputOutput,  // For sockets, etc.
}

pub enum PortData {
    String {
        content: String,
        position: usize,
    },
    File {
        file: File,
    },
    Bytevector {
        content: Vec<u8>,
        position: usize,
    },
    Stdio {
        handle: StdioHandle,
    },
}

pub enum StdioHandle {
    Stdin,
    Stdout,
    Stderr,
}
```

---

### Primitive Implementation

```rust
// src/eval/primitives/io.rs (NEW FILE)

pub fn install_io_primitives(env: &Rc<Environment>) {
    // Port predicates
    env.define("port?", Value::Primitive(Primitive::new("port?", port_p)));
    env.define("input-port?", Value::Primitive(Primitive::new("input-port?", input_port_p)));
    // ...

    // Current ports
    env.define("current-output-port",
               Value::Primitive(Primitive::new("current-output-port", current_output_port)));
    // ...

    // String ports
    env.define("open-input-string",
               Value::Primitive(Primitive::new("open-input-string", open_input_string)));
    // ...

    // Output
    env.define("display", Value::Primitive(Primitive::new("display", display)));
    env.define("write", Value::Primitive(Primitive::new("write", write)));
    env.define("newline", Value::Primitive(Primitive::new("newline", newline)));
    // ...

    // Input
    env.define("read", Value::Primitive(Primitive::new("read", read)));
    env.define("read-char", Value::Primitive(Primitive::new("read-char", read_char)));
    // ...
}

fn display(args: &[Value]) -> Result<Value, EvalError> {
    match args {
        [obj] => {
            // Use current-output-port
            print!("{}", obj.display_format());
            Ok(Value::Unspecified)
        }
        [obj, Value::Port(port)] => {
            // Write to specified port
            write_to_port(port, &obj.display_format())?;
            Ok(Value::Unspecified)
        }
        _ => Err(EvalError::WrongArity { ... })
    }
}

fn write(args: &[Value]) -> Result<Value, EvalError> {
    match args {
        [obj] => {
            print!("{}", obj);  // Uses Display trait (includes quotes)
            Ok(Value::Unspecified)
        }
        [obj, Value::Port(port)] => {
            write_to_port(port, &format!("{}", obj))?;
            Ok(Value::Unspecified)
        }
        _ => Err(EvalError::WrongArity { ... })
    }
}
```

---

### Display vs Write

**Critical distinction:**

```scheme
(display "hello")    ; prints: hello
(write "hello")      ; prints: "hello"

(display 'foo)       ; prints: foo
(write 'foo)         ; prints: foo

(display '(a b c))   ; prints: (a b c)
(write '(a b c))     ; prints: (a b c)
```

**Implementation:**
- `display` - Use a "display format" (no quotes for strings)
- `write` - Use standard `Display` trait (machine-readable)

---

## Testing Strategy

### Unit Tests

```rust
// tests/compliance/io.rs (NEW)

#[test]
fn test_display_string() {
    // Should print without quotes
    assert_output_is("(display \"hello\")", "hello");
}

#[test]
fn test_write_string() {
    // Should print with quotes
    assert_output_is("(write \"hello\")", "\"hello\"");
}

#[test]
fn test_string_ports() {
    assert_program_eval_to(
        r#"
        (define out (open-output-string))
        (display "hello" out)
        (display " world" out)
        (get-output-string out)
        "#,
        "\"hello world\"",
    );
}

#[test]
fn test_read_from_string() {
    assert_eval_to(
        r#"(read (open-input-string "(+ 1 2)"))"#,
        "(+ 1 2)",
    );
}
```

---

## R7RS Test Suite Coverage

From `chibi-scheme/tests/r7rs-tests.scm`, I/O is used extensively:

```scheme
; Common patterns in tests:
(display "FAIL: ") (write 'expr) (newline)
(read (open-input-string "..."))
(get-output-string (open-output-string))
```

**Implication:** Once basic I/O works, we can use chibi's test suite directly!

---

## Dependencies

### Required First
- ✅ Strings (already implemented)
- ✅ Error handling (already have EvalError)
- ❌ Parameter objects (`parameterize`) - for current ports

### Nice to Have
- Bytevectors (already implemented for binary I/O)
- Exception system (`file-error?`)

---

## Summary: What's Actually Needed?

### Absolute Minimum (Phase 1 - 2-3 days)
✅ Start here for immediate value:

1. **Port type** (`Value::Port`)
2. **String ports** (easy, no filesystem)
3. **display/write/newline** (most used)
4. **read** (parse S-expressions)
5. **eof handling**

### High Priority (Phase 2 - 2-3 days)
📁 File operations:

6. **open-input-file/open-output-file**
7. **close-port**
8. **Error handling**

### Medium Priority (Phase 3+ - 1 week)
🔧 Complete the system:

9. **Parameter objects** (`parameterize`)
10. **Binary I/O**
11. **Bulk operations**
12. **Higher-order wrappers**

---

## Estimated Timeline

**Week 1:**
- Day 1-2: Port infrastructure, string ports
- Day 3: display/write/newline
- Day 4-5: read, read-char, eof

**Week 2:**
- Day 1-2: File I/O
- Day 3: Parameter objects
- Day 4-5: Binary I/O, polishing

**Total: 10 days for full compliance**

---

## References

- **R7RS Spec:** Section 6.13 (Input and output)
- **Chibi Tests:** `~/Project/reference/chibi-scheme/tests/r7rs-tests.scm`
- **Chibi Implementation:** `lib/init-7.scm` for Scheme-level I/O wrappers

---

## Decision

**Recommended approach:** Implement in phases, starting with Phase 1 (minimal I/O).

**Rationale:**
1. Provides immediate value (debugging, REPL improvements)
2. Enables running chibi test suite
3. Can defer file I/O until after TCO
4. String ports are easy and very useful

**Next step:** After TCO and advanced math, implement Phase 1 I/O.
