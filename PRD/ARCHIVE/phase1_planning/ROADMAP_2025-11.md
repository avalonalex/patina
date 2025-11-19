# Patina R7RS-small Implementation Roadmap

**Current Status:** 88% feature implementation, 402 passing tests
**Compliance Status:** 0% (missing library system)
**Target:** Full R7RS-small compliance

---

## 🎯 Quick Links

- **Detailed Checklist:** [PRD/phase1/R7RS_COMPLIANCE_CHECKLIST.md](PRD/phase1/R7RS_COMPLIANCE_CHECKLIST.md)
- **Implementation Status:** [PRD/phase1/IMPLEMENTATION_STATUS.md](PRD/phase1/IMPLEMENTATION_STATUS.md)
- **Feature Matrix:** [docs/FEATURE_STATUS.md](docs/FEATURE_STATUS.md)

---

## 🚨 Critical Path to Compliance

### Phase 1: Library System (Weeks 1-2) - **BLOCKING**
**Status:** ❌ Not Started | **Effort:** 1-2 weeks | **Tests:** 402 → 420

Without this, we cannot claim R7RS compliance. This reorganizes all our existing code.

**Deliverables:**
- `define-library`, `import`, `export` working
- All standard libraries defined (`(scheme base)`, `(scheme file)`, etc.)
- Existing code reorganized into libraries
- Library isolation and import mechanics tested

**Key Tasks:**
1. Implement library registry and loader
2. Add `define-library` syntax support
3. Implement import/export resolution
4. Reorganize `lib/bootstrap.scm` → `lib/scheme/base.scm`
5. Define all 15 standard libraries

### Phase 2: Exception Handling (Weeks 3) - **CRITICAL**
**Status:** ❌ Not Started | **Effort:** 3-5 days | **Tests:** 420 → 440

Needed for proper error handling throughout the system.

**Deliverables:**
- `error`, `raise`, `raise-continuable` working
- `guard` syntax for exception handling
- Exception object predicates
- Integration with I/O error handling

**Key Tasks:**
1. Add `Value::Exception` variant
2. Implement exception handler stack
3. Add `guard` special form
4. Create exception primitives

### Phase 3: I/O Foundation (Weeks 3-4) - **HIGH**
**Status:** 🟡 10% (display/write/newline only) | **Effort:** 1 week | **Tests:** 440 → 470

Basic I/O for real programs.

**Deliverables:**
- String ports working
- Basic I/O: `read`, `write`, `display`, `read-char`, `write-char`
- File I/O: `open-input-file`, `open-output-file`, `close-port`
- Port predicates and current port parameters

**Key Tasks:**
1. Implement port infrastructure (`Value::Port`)
2. String ports (no filesystem)
3. File I/O with error handling
4. Make current ports into parameters

### Phase 4: Missing Core Procedures (Week 5) - **MEDIUM-HIGH**
**Status:** 🟡 Partial | **Effort:** 1 week | **Tests:** 470 → 500

Complete `(scheme base)` implementation.

**Deliverables:**
- Quasiquote (backtick) syntax
- `define` function shorthand
- Character operations and predicates
- Bytevector support
- Symbol/string conversions

**Key Tasks:**
1. Implement quasiquote/unquote
2. Parse `(define (f x) ...)` shorthand
3. Add character comparison and conversion
4. Implement bytevectors
5. Add missing predicates and converters

---

## 📊 Milestones

### Milestone 1: Library Foundation (End of Week 2)
- ✅ Library system complete
- ✅ All standard libraries defined
- ✅ Existing tests pass with new structure
- 🎯 **Target:** 420 tests passing

### Milestone 2: Error Handling & I/O (End of Week 4)
- ✅ Exception handling complete
- ✅ Basic I/O working (string ports + file I/O)
- ✅ Integration tested
- 🎯 **Target:** 470 tests passing

### Milestone 3: Core Complete (End of Week 5)
- ✅ All `(scheme base)` procedures implemented
- ✅ Quasiquote working
- ✅ Bytevectors working
- 🎯 **Target:** 500 tests passing

### Milestone 4: R7RS Compliance (End of Week 6)
- ✅ Pass chibi r7rs-tests.scm
- ✅ Documentation complete
- ✅ Real-world programs work
- 🎯 **Target:** Full R7RS-small compliance

---

## ✅ What We Have (Don't Need to Redo)

### Core Language (100%)
- ✅ Special forms: quote, if, define, set!, lambda, begin
- ✅ Derived forms: let, let*, letrec, cond, case, and, or, do
- ✅ Multiple values: values, call-with-values
- ✅ **Full tail call optimization** (R7RS compliant!)
- ✅ Hygienic macros with syntax-rules (96%)

### Data Structures (100%)
- ✅ Lists: All 30 operations
- ✅ Strings: All 37 operations with UTF-8
- ✅ Vectors: All 37 operations
- ✅ Pairs and proper list operations

### Numbers (100%)
- ✅ Full numeric tower: Integer → BigInteger → Rational → Real → Complex
- ✅ All arithmetic and math functions
- ✅ Trigonometry, exponentials, logarithms
- ✅ Complex number operations
- ✅ GCD, LCM, exact-integer-sqrt, rationalize ✅ (just implemented!)

### Testing Infrastructure (100%)
- ✅ 402 comprehensive tests
- ✅ Test utilities and fixtures
- ✅ Chibi comparison framework

---

## ❌ What's Missing (Need to Implement)

### 🚨 CRITICAL
1. **Library System** - Zero compliance without this
2. **Exception Handling** - Needed for robust error handling
3. **I/O System** - Can't write real programs without this

### 🔥 HIGH
4. **Quasiquote** - Commonly used in real code
5. **Character Operations** - Many predicates and conversions missing
6. **Bytevectors** - Binary data handling

### 🔵 MEDIUM
7. **Define Function Shorthand** - Quality of life
8. **Symbol/String Conversions** - Various utility functions
9. **eval** procedure - Sometimes needed
10. **Parameter Objects** - For dynamic rebinding

### 🟣 LOW
11. **Continuations** (call/cc) - Complex, rarely used
12. **Lazy Evaluation** - delay/force
13. **Process Context** - command-line, exit, etc.
14. **Time Operations** - current-jiffy, etc.

---

## 📈 Progress Tracking

### Week-by-Week Goals

**Week 1:** Library system foundation
- Define library syntax and parser
- Implement library registry
- Basic import/export working

**Week 2:** Library system completion
- All standard libraries defined
- Reorganize existing code
- Tests passing with new structure

**Week 3:** Exception handling + I/O Phase 1
- Exception infrastructure
- String ports
- Basic I/O primitives

**Week 4:** I/O Phase 2
- File I/O
- Port parameters
- Error integration

**Week 5:** Missing core procedures
- Quasiquote
- Character operations
- Bytevectors

**Week 6:** Testing & polish
- Chibi test suite integration
- Documentation
- Real-world program testing

---

## 🎓 Learning Resources

### R7RS Specification
- **Location:** `spec/r7rs-small-spec/`
- **Key sections:**
  - Section 5.6: Libraries
  - Section 6.11: Exceptions
  - Section 6.13: Input and output

### Reference Implementation
- **Chibi Scheme:** `~/Project/reference/chibi-scheme`
- **Key files:**
  - `lib/init-7.scm` - Core library organization
  - `lib/scheme/base.sld` - Base library definition
  - `tests/r7rs-tests.scm` - Official test suite (2,516 lines)

### Existing Documentation
- `CLAUDE.md` - Project structure and guidelines
- `docs/` - User-facing documentation
- `PRD/` - Design documents and planning

---

## 🚀 Getting Started

### For Contributors

1. **Read the spec:** Start with R7RS Section 5.6 (Libraries)
2. **Study chibi:** Look at `~/Project/reference/chibi-scheme/lib/`
3. **Check the checklist:** [R7RS_COMPLIANCE_CHECKLIST.md](PRD/phase1/R7RS_COMPLIANCE_CHECKLIST.md)
4. **Run tests:** `cargo test --package patina-tests`
5. **Pick a task:** Start with unchecked items in the checklist

### Quick Commands

```bash
# Run all tests
cargo test

# Run specific test suite
cargo test --package patina-tests --test compliance

# Run REPL
cargo run --release

# Build documentation
cargo doc --open

# Format code
cargo fmt

# Lint
cargo clippy
```

---

## 📞 Status Updates

**Last Updated:** 2025-11-11
**Current Focus:** Completing R7RS compliance checklist
**Next Steps:** Begin library system implementation (Week 1)

---

**Note:** This is a living document. Update it as implementation progresses.
