# Parser Architecture Evolution

**Status:** Planning
**Created:** 2025-11-20
**Phase:** 1 (R7RS Compliance)

## Executive Summary

The parser in `crates/patina-frontend/src/parser/mod.rs` is becoming increasingly complex as we add R7RS features. This document outlines the current state, pain points, and evolution strategies to maintain code quality while scaling to full R7RS compliance and beyond.

**Current state:** 500+ lines, monolithic `parse_number` method handling 10+ different number formats.

**Recommended path:** Incremental modularization → Two-stage parsing → Nanopass architecture (aligns with Phase 2 gradual typing).

## Current Architecture (As of 2025-11-20)

### Structure
```
crates/patina-frontend/
├── src/
│   ├── lexer/mod.rs           # Tokenization (450 lines)
│   ├── parser/mod.rs          # Parsing to Value (500 lines)
│   └── macro_expander/        # Hygienic macro expansion
```

### Parser Responsibilities
The parser currently handles:
1. **S-expression structure** - Lists, vectors, bytevectors, pairs
2. **Atomic values** - Numbers, strings, symbols, characters, booleans
3. **Number parsing** - The most complex subsystem:
   - R7RS numeric prefixes (#e, #i, #b, #o, #d, #x)
   - Multiple radixes (2, 8, 10, 16)
   - Exactness conversions
   - Complex numbers (rectangular a+bi, polar r@θ)
   - Rational numbers (n/d)
   - Special floats (+inf.0, -inf.0, +nan.0)
   - BigInt overflow handling
   - Integer/float discrimination

### Pain Points

#### 1. `parse_number` Complexity
**Location:** `crates/patina-frontend/src/parser/mod.rs:180-258`

The `parse_number` method (~80 lines) delegates to:
- `parse_number_with_prefix` (~130 lines) - Handles #e, #i, #b, #o, #d, #x
- `parse_rectangular` (~40 lines) - Handles a+bi complex
- `parse_polar` (~20 lines) - Handles r@θ complex
- `parse_real_component` (~30 lines) - Helper for complex parts

**Total:** ~280 lines just for number parsing (56% of parser!)

**Issues:**
- Hard to test individual components
- Difficult to add new numeric forms (e.g., exact complex)
- Error messages lack context
- Code duplication (special float handling in multiple places)

#### 2. Tight Coupling
- Lexer returns `Token::Number(String)` - parser must re-parse
- No intermediate representation between syntax and semantics
- Exactness conversions mixed with parsing logic

#### 3. Limited Extensibility
**Future R7RS features that will increase complexity:**
- Datum labels: `#1=(1 2 #1#)` - Circular structures
- Case sensitivity: `#!fold-case` directive
- More numeric formats: Exact complex, different exactness rules

**Phase 2+ features:**
- Gradual typing annotations (requires AST metadata)
- Type inference (requires multi-pass analysis)
- Optimizations (requires IR transformations)

## Evolution Options

### Option 1: Modular Number Parsers (SHORT-TERM)

**Timeline:** Immediate - Next 2-4 weeks
**Effort:** Low (incremental refactor)
**Risk:** Low

#### Structure
```
crates/patina-frontend/src/parser/
├── mod.rs                    # Main parser orchestration
└── numbers/
    ├── mod.rs                # NumberParser entry point
    ├── prefixes.rs           # Extract & validate #e, #i, #b, etc.
    ├── integers.rs           # i64/BigInt with radix support
    ├── rationals.rs          # n/d parsing & simplification
    ├── reals.rs              # f64 + special values (+inf.0, etc.)
    └── complex.rs            # Rectangular & polar forms
```

#### Implementation Sketch
```rust
// numbers/mod.rs
pub struct NumberParser;

impl NumberParser {
    pub fn parse(s: &str) -> Result<Value, ParseError> {
        // Fast path: check prefix to dispatch
        if s.starts_with('#') {
            PrefixedNumber::parse(s)
        } else if s.ends_with('i') || s.ends_with('I') {
            ComplexNumber::parse(s)
        } else if s.contains('/') {
            RationalNumber::parse(s)
        } else if s.contains('@') {
            PolarNumber::parse(s)
        } else {
            SimpleNumber::parse(s)
        }
    }
}

// prefixes.rs
#[derive(Debug, Clone)]
pub struct NumberPrefixes {
    pub exactness: Option<Exactness>,
    pub radix: u32,
}

impl NumberPrefixes {
    pub fn parse(s: &str) -> Result<(NumberPrefixes, &str), ParseError> {
        // Extract all prefixes, return prefixes + remaining digits
    }
}
```

#### Benefits
- ✅ **Testable:** Each number type tested independently
- ✅ **Readable:** Clear separation by number kind
- ✅ **Maintainable:** Bug fixes localized to specific modules
- ✅ **Incremental:** Refactor one piece at a time
- ✅ **No API changes:** Internal refactor only

#### Migration Path
1. Create `numbers/mod.rs` with dispatcher
2. Move `parse_rectangular` → `numbers/complex.rs` (no changes)
3. Move `parse_polar` → `numbers/complex.rs`
4. Extract prefix logic → `numbers/prefixes.rs`
5. Move rational parsing → `numbers/rationals.rs`
6. Move integer parsing → `numbers/integers.rs`
7. Move float parsing → `numbers/reals.rs`
8. Update `parse_number` to call `NumberParser::parse`
9. Add unit tests for each module
10. Remove old code

**Estimated effort:** 4-6 hours spread over multiple sessions

---

### Option 2: Two-Stage Parsing (MEDIUM-TERM)

**Timeline:** After Option 1, when adding datum labels
**Effort:** Medium (architectural change)
**Risk:** Medium

#### Concept
Separate **syntactic parsing** (what it looks like) from **semantic interpretation** (what it means):

```rust
// Stage 1: Parse to structured tokens
pub enum Datum {
    Number(NumberDatum),
    String(String),
    Symbol(String),
    List(Vec<Datum>),
    Vector(Vec<Datum>),
    // ...
}

pub struct NumberDatum {
    pub prefixes: Vec<Prefix>,
    pub digits: String,
    pub kind: NumberKind,  // Simple, Complex, Rational, etc.
}

// Stage 2: Interpret to runtime values
impl Datum {
    pub fn to_value(&self) -> Result<Value, EvalError> {
        // Apply exactness, radix, construct Value
    }
}
```

#### Benefits
- ✅ **Better errors:** "Invalid hex digit in #x1G" (know context)
- ✅ **Optimization opportunities:** Can transform Datum before evaluation
- ✅ **Datum labels:** `#1=` requires two-pass (mark, then resolve)
- ✅ **Cleaner separation:** Syntax vs semantics

#### When to Adopt
- When implementing datum labels (`#1=`, `#1#`)
- When error messages need more context
- When adding reader macros (if ever)

---

### Option 3: Parser Combinators (ALTERNATIVE)

**Timeline:** If rewriting parser from scratch
**Effort:** High
**Risk:** Medium

#### Using `nom` or similar
```rust
use nom::{
    branch::alt,
    bytes::complete::tag,
    combinator::{map, opt},
    sequence::tuple,
};

fn exactness(input: &str) -> IResult<&str, Exactness> {
    alt((
        map(tag("#e"), |_| Exactness::Exact),
        map(tag("#i"), |_| Exactness::Inexact),
    ))(input)
}

fn number(input: &str) -> IResult<&str, Value> {
    map(
        tuple((
            opt(exactness),
            opt(radix),
            digits,
        )),
        |(ex, rad, dig)| construct_number(ex, rad, dig)
    )(input)
}
```

#### Considerations
- **Pro:** Composable, well-tested, great errors
- **Pro:** Industry standard (used in many parsers)
- **Con:** External dependency (but `nom` is widely used)
- **Con:** Learning curve
- **Con:** May be overkill for Scheme's simple syntax

**Verdict:** Consider for Phase 2+ if parser becomes a bottleneck, but not needed for R7RS compliance.

---

### Option 4: Nanopass Architecture (LONG-TERM)

**Timeline:** Phase 2 (Gradual Typing)
**Effort:** High (complete redesign)
**Risk:** Medium-High initially, Low long-term

#### Vision
Embrace the `patina-ir/` crate with multi-pass compilation:

```
Source Text
    ↓
[Lexer] → Tokens
    ↓
[Reader/Parser] → Datum (S-expressions + metadata)
    ↓
[Expander] → Core Forms (post-macro-expansion)
    ↓
[Analyzer] → Typed Core Forms (Phase 2+)
    ↓
[Optimizer] → Optimized IR
    ↓
[Backend] → Execution (tree-walker/VM/JIT)
```

Each pass is a pure function: `Pass: Input → Output`

#### Example Passes

**Pass 1: Reader**
```rust
pub struct DatumReader;
impl DatumReader {
    pub fn read(tokens: Vec<Token>) -> Result<Vec<Datum>, ReaderError> {
        // Just build S-expression structure
        // No evaluation, no macro expansion
    }
}

pub enum Datum {
    Number(NumberDatum),
    Symbol(Symbol),
    List(Vec<Datum>),
    // ... includes source location metadata
}
```

**Pass 2: Macro Expander**
```rust
pub struct MacroExpander;
impl MacroExpander {
    pub fn expand(datum: Datum, env: &MacroEnv) -> Result<CoreForm, ExpansionError> {
        // Expands all macros, produces core forms
    }
}

pub enum CoreForm {
    Const(Value),
    Var(Symbol),
    Lambda { params: Vec<Symbol>, body: Vec<CoreForm> },
    App { func: Box<CoreForm>, args: Vec<CoreForm> },
    If { test: Box<CoreForm>, conseq: Box<CoreForm>, alt: Box<CoreForm> },
    // ... small set of core forms
}
```

**Pass 3: Type Checker (Phase 2)**
```rust
pub struct TypeChecker;
impl TypeChecker {
    pub fn check(form: CoreForm) -> Result<TypedForm, TypeError> {
        // Infer/check types, annotate AST
    }
}
```

#### Benefits
- ✅ **Clean separation:** Each pass does one thing
- ✅ **Testable:** Test each pass in isolation
- ✅ **Optimizable:** Insert optimization passes between stages
- ✅ **Extensible:** Add new passes without touching others
- ✅ **Industry proven:** Racket, Chez, Guile use nanopass
- ✅ **Phase 2 ready:** Perfect for gradual typing

#### References
- [Nanopass Framework](https://nanopass.org/)
- [Racket's Expander](https://docs.racket-lang.org/reference/syntax-model.html)
- [Chez Scheme Architecture](https://cisco.github.io/ChezScheme/)

#### When to Adopt
- **Must have:** When starting Phase 2 (gradual typing)
- **Should have:** If adding sophisticated optimizations
- **Nice to have:** When codebase feels too monolithic

---

## Recommended Timeline

### Phase 1 (Current - R7RS Compliance)

**Now → Next Month:**
- ✅ Implement **Option 1: Modular Number Parsers**
- ✅ Add comprehensive unit tests for each number module
- ✅ Document number parsing in `docs/ARCHITECTURE.md`

**When implementing datum labels:**
- ✅ Implement **Option 2: Two-Stage Parsing**
- ✅ Separate `Datum` from `Value`
- ✅ Two-pass reader for circular structures

**End of Phase 1:**
- ✅ All R7RS-small features working
- ✅ Clean, modular parser architecture
- ✅ Foundation ready for Phase 2

### Phase 2 (Gradual Typing)

**Early Phase 2:**
- ✅ Implement **Option 4: Nanopass Architecture**
- ✅ Separate reader, expander, type checker, compiler
- ✅ Integrate with `patina-ir/` crate

**Mid Phase 2:**
- ✅ Add type inference pass
- ✅ Add optimization passes (constant folding, inlining, etc.)
- ✅ Multiple backend support (tree-walker, VM, JIT)

---

## Immediate Next Steps

### 1. Create `numbers/` Module (Next Session)

**Acceptance Criteria:**
- [ ] `crates/patina-frontend/src/parser/numbers/mod.rs` exists
- [ ] `NumberParser::parse(s: &str) -> Result<Value, ParseError>` works
- [ ] All existing number tests pass
- [ ] At least one new unit test per number type

**Tasks:**
1. Create module structure
2. Move `parse_rectangular` → `numbers/complex.rs`
3. Move `parse_polar` → `numbers/complex.rs`
4. Extract prefix parsing → `numbers/prefixes.rs`
5. Add tests

**Estimated time:** 2-3 hours

### 2. Document Architecture (Ongoing)

**Acceptance Criteria:**
- [ ] `docs/ARCHITECTURE.md` has parser section
- [ ] Diagram showing module organization
- [ ] Examples of adding new number formats

### 3. Technical Debt Tracking

**Create issues for:**
- [ ] Parser modularization (link to this doc)
- [ ] Two-stage parsing for datum labels (future)
- [ ] Nanopass architecture (Phase 2)

---

## Success Metrics

### Short-term (Option 1)
- **Code organization:** Each number type in separate file (<100 lines each)
- **Test coverage:** >90% for each number module
- **Performance:** No regression (currently not a bottleneck)
- **Maintainability:** New contributors can add number formats easily

### Medium-term (Option 2)
- **Error messages:** Rich context in parse errors
- **Datum labels:** `#1=` and `#1#` working correctly
- **Code reuse:** Datum representation used in multiple places

### Long-term (Option 4)
- **Clean architecture:** Each pass <500 lines
- **Type checking:** Full gradual typing support
- **Optimizations:** Measurable performance improvements
- **Multiple backends:** Same frontend, multiple execution strategies

---

## References

### Internal Docs
- `docs/TEST_ORGANIZATION.md` - Testing strategy
- `CLAUDE.md` - Parser responsibilities
- `PRD/phase1/IMPLEMENTATION_STATUS.md` - Current progress

### External Resources
- [R7RS Specification](https://small.r7rs.org/) - Section 7.1 (Formal Syntax)
- [Nanopass Framework](https://nanopass.org/) - Multi-pass compiler architecture
- [Structure and Interpretation of Computer Programs](https://mitpress.mit.edu/sites/default/files/sicp/index.html) - Chapter 4 (Metacircular Evaluator)
- [Beautiful Racket](https://beautifulracket.com/) - Parser design patterns

---

## Appendix: Code Size Analysis

**Current parser (2025-11-20):**
```
crates/patina-frontend/src/parser/mod.rs:
  Total:                    500 lines
  Number parsing:           280 lines (56%)
  S-expression parsing:     150 lines (30%)
  Helpers:                   70 lines (14%)
```

**After Option 1 refactor:**
```
crates/patina-frontend/src/parser/
  mod.rs:                   220 lines (S-exp + orchestration)
  numbers/mod.rs:            50 lines (dispatcher)
  numbers/prefixes.rs:       80 lines
  numbers/integers.rs:       60 lines
  numbers/rationals.rs:      40 lines
  numbers/reals.rs:          50 lines
  numbers/complex.rs:        100 lines
  Total:                    600 lines (20% increase due to structure)
```

**Benefits of 20% size increase:**
- Each file is self-contained and testable
- Clear responsibility boundaries
- Easy to navigate and understand
- New number formats don't increase cognitive load

---

**Document Status:** Living document, update as architecture evolves.
