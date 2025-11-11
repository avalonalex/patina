# Gauche Scheme Reference

This document provides guidance on when and how to use Gauche Scheme as a reference implementation for Patina development. Gauche is a practical, production-grade R7RS Scheme implementation with excellent C integration and battle-tested implementations of complex features.

## Location

**Repository:** `~/Project/reference/Gauche`

## Overview

Gauche is a practical R7RS Scheme implementation written in C, designed for:
- **Scripting and system programming** - Excellent Unix integration
- **Production use** - Used in real-world applications for 20+ years
- **C integration** - Clean FFI and extension mechanisms
- **R7RS compliance** - Full R7RS-small support

**Key characteristics:**
- Written in C (unlike Chibi which is also C, but Gauche is more mature)
- Implements R7RS-small + many SRFIs
- Production-tested implementations of complex features
- Well-commented, readable C code

## When to Reference Gauche

### ✅ Primary Use Cases

#### 1. **Macro System Implementation** (HIGH PRIORITY)

**Why Gauche?**
- Gauche implements a **sophisticated pattern matching engine** for macros
- Handles **arbitrary nested ellipsis** patterns (`((x ...) ...)`)
- Production-tested with complex real-world macros
- Clean separation between pattern matching and template expansion

**Key files:**
```
src/macro.c          - Main macro expansion engine
src/compile.c        - Compiler integration
lib/gauche/macro.scm - Macro utilities
```

**When to use:**
- Implementing nested ellipsis support (see `internal/NESTED_ELLIPSIS_ROADMAP.md`)
- Understanding how to handle auxiliary keywords in patterns
- Learning pattern matching algorithms for `syntax-rules`
- Debugging complex macro expansion issues

**Specific scenarios:**
- ❌ **let-values/let*-values macros failing** → Study Gauche's pattern matcher
- ❌ **Recursive macro expansion issues** → Reference Gauche's expansion algorithm
- ❌ **Dotted pair patterns not working** → Check Gauche's pattern handling

**Referenced in:**
- `internal/NESTED_ELLIPSIS_ROADMAP.md` - Migration plan based on Gauche's approach
- `internal/MACRO_ARCHITECTURE_DECISIONS.md` - Comparison with Gauche's strategy

#### 2. **Complex Standard Library Implementations**

**Why Gauche?**
- Battle-tested implementations of tricky R7RS procedures
- Handles edge cases that simpler implementations miss
- Good balance of clarity and correctness

**Examples:**
```scheme
; lib/srfi-1.scm - Comprehensive list library
; lib/gauche/partcont.scm - Partial continuations
; lib/gauche/parameter.scm - Parameters (dynamic variables)
```

**When to use:**
- Implementing complex list procedures (fold, unfold, etc.)
- Understanding parameter objects (dynamic bindings)
- Reference implementations for SRFI compliance

#### 3. **C Implementation Patterns**

**Why Gauche?**
- Clean, readable C code
- Good separation of concerns
- Modern C practices (unlike some older Schemes)

**Key files:**
```
src/eval.c      - Evaluator implementation
src/port.c      - I/O ports
src/read.c      - Reader/parser
src/number.c    - Numeric tower
src/error.c     - Exception handling
```

**When to use:**
- Understanding how to integrate C primitives
- Learning memory management patterns for Scheme objects
- Reference for implementing I/O, exceptions, or numerics in Rust

#### 4. **R7RS Library System**

**Why Gauche?**
- Comprehensive R7RS library implementation
- Module system with good separation
- Examples of how libraries should work

**Key files:**
```
lib/scheme/base.scm      - R7RS base library
lib/scheme/read.scm      - Read library
lib/scheme/write.scm     - Write library
lib/scheme/file.scm      - File library
```

**When to use:**
- Implementing R7RS library system in Patina (Phase 2+)
- Understanding library import/export semantics
- Reference for standard library organization

### ⚠️ Secondary Use Cases

#### 5. **Exception Handling**

Gauche has solid exception handling, but it's R6RS/R7RS hybrid.

**Files:** `src/error.c`, `lib/gauche/exception.scm`

**When to use:**
- Implementing `guard` and `raise` forms
- Understanding exception object structure
- Learning about condition types

#### 6. **Continuations and Control Flow**

Gauche implements both full continuations and partial continuations.

**Files:** `src/cont.c`, `lib/gauche/partcont.scm`

**When to use:**
- Phase 3+ when implementing `call/cc` (call-with-current-continuation)
- Understanding continuation implementation strategies
- Learning about delimited continuations (advanced)

### ❌ When NOT to Use Gauche

#### 1. **Basic Feature Testing**
**Use Chibi instead** - Chibi's `tests/r7rs-tests.scm` is the canonical test suite

#### 2. **Simple Derived Forms**
**Use Chibi instead** - Chibi's `lib/init-7.scm` has cleaner, simpler implementations

#### 3. **String Optimization**
**Use Chez instead** - Chez has more sophisticated string representation

#### 4. **Performance Patterns**
**Use Chez instead** - Chez is optimized for speed, Gauche for practicality

## Comparison Matrix

| Feature | Chibi | Gauche | Chez | Use for Patina |
|---------|-------|--------|------|----------------|
| **Test Suite** | ✅ Excellent | Good | Good | **Chibi** - canonical R7RS tests |
| **Simple Macros** | ✅ Excellent | Good | Excellent | **Chibi** - clean reference code |
| **Complex Macros** | Basic | ✅ **Excellent** | Excellent | **Gauche** - production patterns |
| **Nested Ellipsis** | Limited | ✅ **Full support** | Full support | **Gauche** - clear algorithm |
| **C Integration** | Good | ✅ **Excellent** | Good | **Gauche** - modern C patterns |
| **Standard Lib** | ✅ Minimal | Comprehensive | Comprehensive | **Chibi** for basics, **Gauche** for complex |
| **Performance** | Basic | Good | ✅ **Excellent** | **Chez** - when optimizing |
| **String Repr** | UTF-8 | UTF-8 | ✅ **uint32_t array** | **Chez** - for optimization ideas |
| **Documentation** | Good | ✅ **Excellent** | Good | **Gauche** - best docs |
| **R7RS Purity** | ✅ Pure R7RS | R7RS + SRFI | R6RS-focused | **Chibi** for pure R7RS |

## Practical Workflow

### Scenario 1: Implementing a Complex Macro

```bash
# 1. Check Chibi's test suite for expected behavior
grep -A 20 "test.*let-values" \
  ~/Project/reference/chibi-scheme/tests/r7rs-tests.scm

# 2. Look at Chibi's implementation (if it's derived)
grep -A 30 "define-syntax let-values" \
  ~/Project/reference/chibi-scheme/lib/init-7.scm

# 3. If Chibi's is too simple or doesn't work, check Gauche's pattern matcher
# Study: ~/Project/reference/Gauche/src/macro.c
# Focus on: pattern matching algorithm, ellipsis handling, auxiliary keywords

# 4. Understand Gauche's approach, then adapt to Rust
```

### Scenario 2: Implementing a Standard Library Function

```bash
# 1. Start with Chibi for simple cases
grep -A 10 "define.*fold" \
  ~/Project/reference/chibi-scheme/lib/srfi/1.scm

# 2. If complex, check Gauche for edge cases
grep -A 20 "define.*fold" \
  ~/Project/reference/Gauche/lib/srfi-1.scm

# 3. Compare approaches, choose the clearest
```

### Scenario 3: Understanding a C Implementation Pattern

```bash
# 1. Look at Gauche's C code (more modern, readable)
less ~/Project/reference/Gauche/src/number.c

# 2. Focus on structure and patterns, not exact code
# 3. Adapt to Rust idioms
```

## Key Gauche Files for Patina Development

### Highest Priority (Macro System)

```
src/macro.c              - Pattern matching engine ⭐⭐⭐
src/compile.c            - Macro expansion integration
lib/gauche/macro.scm     - Macro utilities
```

**Study these for:** Nested ellipsis, auxiliary keywords, pattern matching algorithms

### High Priority (Standard Library)

```
lib/scheme/base.scm      - R7RS base library ⭐⭐
lib/srfi-1.scm          - List library (fold, unfold, etc.)
lib/gauche/parameter.scm - Parameters (dynamic variables)
```

**Study these for:** Complex library implementations, edge cases

### Medium Priority (C Patterns)

```
src/eval.c               - Evaluator ⭐
src/port.c              - I/O ports
src/number.c            - Numeric tower
src/error.c             - Exceptions
```

**Study these for:** C implementation patterns to adapt to Rust

### Lower Priority (Advanced Features)

```
src/cont.c              - Continuations (Phase 3+)
lib/gauche/record.scm   - Record types
lib/gauche/lazy.scm     - Lazy evaluation
```

**Study these for:** Future phases, advanced features

## Gauche's Macro Pattern Matching Algorithm

Based on research documented in `internal/MACRO_ARCHITECTURE_DECISIONS.md`:

**Gauche's approach (simplified):**

```c
// From src/macro.c (conceptual, not exact code)

typedef struct {
    int depth;           // Nesting depth of this ellipsis
    int count;           // Number of matches at this depth
    ScmObj *bindings;    // Array of matched values
} EllipsisMatch;

// Pattern matching returns multi-dimensional bindings
// Example: ((x ...) ...) produces 2D array
// Access via: bindings[outer_index][inner_index]
```

**Key insights:**

1. **Depth tracking** - Each ellipsis knows its nesting level
2. **Multi-dimensional storage** - Bindings form N-D arrays for N ellipses
3. **Index-based expansion** - Template walks indices, not recursive structures
4. **Clear separation** - Pattern matching builds structure, template walks it

**When to use this approach:**
- ❌ Current: let-values macros fail
- ❌ Future: Implementing `syntax-case` (advanced macros)
- ❌ Future: Supporting arbitrary nested ellipsis patterns

**See:** `internal/NESTED_ELLIPSIS_ROADMAP.md` for 4-phase migration plan

## Testing Against Gauche

```bash
# Run a test with Gauche
gosh -e '(let-values (((a b) (values 1 2))) (+ a b))'

# Load a file with Gauche
gosh /path/to/test.scm

# Compare with Patina output
diff <(gosh -e '(expression)') <(cargo run --release -e '(expression)')

# Interactive REPL
gosh
```

## Documentation and References

**Online:**
- Official docs: https://practical-scheme.net/gauche/
- Manual: https://practical-scheme.net/gauche/man/
- Source browser: https://github.com/shirok/Gauche

**Local:**
```
~/Project/reference/Gauche/doc/
~/Project/reference/Gauche/HACKING.md
```

## Summary: When to Use Which Reference

### 🥇 **Chibi - Your Primary Reference**
- ✅ R7RS compliance testing
- ✅ Simple, clean implementations
- ✅ What to implement first
- ✅ Expected behavior and edge cases

### 🥈 **Gauche - For Complex Features**
- ✅ Macro system (nested ellipsis, complex patterns)
- ✅ Complex standard library procedures
- ✅ C implementation patterns
- ✅ Production-grade robustness

### 🥉 **Chez - For Optimization**
- ✅ High-performance string representation
- ✅ Compiler architecture (future)
- ✅ Advanced optimization techniques
- ⚠️ R6RS-focused (not R7RS), use selectively

## Current Status (2025-11-10)

**Gauche has been invaluable for:**
1. ✅ Understanding nested ellipsis patterns (macro research)
2. ✅ Designing migration path for complex macros (`NESTED_ELLIPSIS_ROADMAP.md`)
3. ✅ Identifying limitations in current macro implementation

**Next time to reference Gauche:**
1. When implementing nested ellipsis support (Phase 2+)
2. When let-values/let*-values macros are prioritized
3. When implementing library system
4. When debugging complex macro expansion issues

---

**Quick Decision Tree:**

```
Need to implement something?
├─ Is it a test case? → Chibi tests/r7rs-tests.scm
├─ Is it simple? → Chibi lib/init-7.scm
├─ Is it a complex macro? → Gauche src/macro.c ⭐
├─ Is it a complex library function? → Gauche lib/*.scm
├─ Need C patterns? → Gauche src/*.c
└─ Optimizing performance? → Chez (if relevant)
```

**Remember:** Chibi for basics, Gauche for complexity, Chez for optimization!
