# Vonuvoli-Scheme Analysis - Macro Implementation

**Date:** 2025-11-08
**Repository:** `~/Project/reference/vonuvoli-scheme`
**Status:** Analysis Complete

---

## Executive Summary

Vonuvoli-scheme takes a **completely different approach** from chibi-scheme:
- **NO support for `define-syntax` or `syntax-rules`** - marked as `Unsupported`
- **Hardcodes special forms** like `when`, `unless`, `do` directly in Rust
- Uses a **compile-time transformation** approach (compiles to Expression AST)
- No hygienic macro system - everything is built-in

**Key insight:** This is a **simpler but less flexible** approach - good for performance and simplicity, but users cannot define their own macros.

---

## Architecture

### Approach: Built-in Syntax Forms (No User-Definable Macros)

**Vonuvoli's strategy:**
1. Define all special forms as `SyntaxPrimitiveV` enum variants
2. Compile each form to an internal `Expression` AST
3. No macro expansion - direct compilation

**Comparison with chibi:**
- Chibi: Macros → Expansion → Evaluation
- Vonuvoli: Syntax → Compilation → Evaluation

---

## Data Structures

### 1. SyntaxPrimitiveV Enum
**Location:** `sources/primitives_syntaxes.rs:41-102`

```rust
pub enum SyntaxPrimitiveV {
    Quote,
    QuasiQuote,
    UnQuote,
    UnQuoteSplicing,

    Begin,
    And,
    Or,

    If,
    When,       // Built-in!
    Unless,     // Built-in!
    Cond,
    Case,

    Do,         // Built-in!
    DoCond,
    While,
    WhileCond,
    Until,
    UntilCond,
    Loop,

    Guard,
    GuardCond,

    Locals,
    LetParallel,
    LetSequential,
    LetRecursiveParallel,
    LetRecursiveSequential,
    LetValuesParallel,
    LetValuesSequential,
    LetParameters,

    Define,
    ReDefine,
    DefineValues,
    ReDefineValues,

    Set,
    SetValues,

    Lambda,

    DefineRecord,
}
```

**Key observation:** `When`, `Unless`, `Do` are **enum variants**, not macros!

### 2. SyntaxPrimitive Enum
**Location:** `sources/primitives_syntaxes.rs:28-38`

```rust
pub enum SyntaxPrimitive {
    PrimitiveV(SyntaxPrimitiveV),
    Auxiliary,
    Unimplemented,
    Unsupported,    // ← define-syntax and syntax-rules!
    Reserved,
}
```

### 3. Expression AST
**Location:** Referenced in `sources/compiler.rs`

Vonuvoli compiles syntax forms to an internal `Expression` type:
- `Expression::ConditionalIf` - for `when`/`unless`/`cond`
- `Expression::Sequence` - for body expressions
- etc.

---

## Implementation: When/Unless

**Location:** `sources/compiler.rs:558-591`

```rust
fn compile_syntax_when_unless(
    &self,
    compilation: CompilerContext,
    syntax: SyntaxPrimitiveV,
    tokens: ValueVec
) -> Outcome<(CompilerContext, Expression)> {

    // Require at least 2 tokens: (when/unless test body...)
    let tokens_count = tokens.len();
    if tokens_count < 2 {
        fail! (0x3c364a9f);
    }

    // Disable definitions in guard/body
    let compilation = compilation.define_disable()?;

    // Compile all tokens (test + body forms)
    let (compilation, statements) = self.compile_0_vec(compilation, tokens)?;

    // Re-enable definitions
    let compilation = compilation.define_enable()?;

    // Extract guard (first) and body (rest)
    let (guard, statements) = vec_explode_1n(statements);

    // Wrap body in sequence (returns last value)
    let statements = Expression::Sequence(
        ExpressionSequenceOperator::ReturnLast,
        statements.into_boxed_slice()
    );

    // Determine if guard should be negated
    let negated = match syntax {
        SyntaxPrimitiveV::When => false,
        SyntaxPrimitiveV::Unless => true,
        _ => unreachable!(),
    };

    // Create conditional clause
    let clauses = vec![
        ExpressionConditionalIfClause::GuardAndExpression(
            ExpressionConditionalIfGuard::Expression(guard, negated),
            ExpressionValueConsumer::Ignore,
            statements
        ),
    ];

    let clauses = ExpressionConditionalIfClauses::Multiple(
        clauses.into_boxed_slice()
    );

    // Return conditional expression
    let expression = Expression::ConditionalIf(clauses);

    return Ok((compilation, expression));
}
```

**What this does:**
1. Parses `(when test body...)` into guard and body
2. Compiles to equivalent `Expression::ConditionalIf` AST node
3. Negates guard for `unless`

**Effectively implements:** `(when test body...) → (if test (begin body...))`

But hardcoded in Rust, not via macro expansion!

---

## Key Differences from Chibi

| Aspect | Chibi-Scheme | Vonuvoli-Scheme |
|--------|--------------|-----------------|
| **User macros** | ✅ Yes (`define-syntax`) | ❌ No (Unsupported) |
| **When/Unless** | Defined as macros | Built-in syntax |
| **Do loop** | Macro in Scheme | Built-in syntax |
| **Hygiene** | Full R7RS hygiene | N/A (no macros) |
| **Extensibility** | Users can add macros | Users cannot extend |
| **Implementation** | `er-macro-transformer` in Scheme | Rust compiler code |
| **Complexity** | High (full macro system) | Low (hardcoded forms) |
| **Performance** | Good (compiled macros) | Excellent (no expansion) |
| **R7RS compliance** | Full | Partial (no `define-syntax`) |

---

## Advantages of Vonuvoli's Approach

### 1. Simplicity
- No macro expander needed
- No hygiene implementation
- Straightforward compiler architecture

### 2. Performance
- No macro expansion phase
- Direct compilation to AST
- Easier to optimize

### 3. Predictability
- All special forms behave consistently
- No user-defined macros to debug
- Simpler error messages

---

## Disadvantages of Vonuvoli's Approach

### 1. Not R7RS Compliant
- Missing `define-syntax` (required by R7RS)
- Missing `syntax-rules` (required by R7RS)
- Users cannot define `when`/`unless` themselves

### 2. Not Extensible
- Cannot add new special forms without modifying Rust code
- Limits metaprogramming capabilities
- Cannot implement macros like `when-let`, `cond-expand`, etc.

### 3. Maintenance Burden
- Every new special form requires Rust code
- More enum variants to maintain
- Harder to experiment with syntax

---

## Relevance to Patina

### What We Can Learn

**1. Alternative implementation strategy exists:**
   - Could hardcode `when`, `unless`, `do` in Rust
   - Would get us to 99% tests passing faster
   - But wouldn't be R7RS compliant

**2. Compilation vs. Expansion:**
   - Vonuvoli compiles to Expression AST
   - We could do similar: expand macros to Value AST
   - Then evaluate the expanded AST

**3. Error handling:**
   - Vonuvoli checks arity at compile time
   - We should do similar during macro expansion

### Should We Follow Vonuvoli's Approach?

**❌ No - Not Recommended**

**Reasons:**
1. **R7RS compliance is our goal** - we need `define-syntax`
2. **Extensibility matters** - users should define macros
3. **We're already close** - 242/245 tests passing
4. **Chibi's approach is proven** - and well-documented

**However:**
- Could be a **fallback** if macro implementation proves too difficult
- Could **temporarily** hardcode `when`/`unless`/`do` to unblock tests
- Demonstrates that **built-in special forms are viable**

---

## Hybrid Approach (Possible Strategy)

**Phase 1: Quick wins (1 week)**
- Hardcode `when`, `unless`, `do` as special forms
- Get to 245/245 tests passing
- Move on to other features

**Phase 2: Proper macros (4-6 weeks later)**
- Implement full `define-syntax` + `syntax-rules`
- Replace hardcoded forms with macro definitions in bootstrap
- Achieve full R7RS compliance

**Pros:**
- Unblocks development
- Gets us to 99%+ quickly
- Can still achieve R7RS compliance later

**Cons:**
- Temporary code that will be replaced
- Not learning macro implementation now
- May be harder to retrofit hygiene later

---

## Code Snippets Worth Studying

### 1. Conditional Compilation
**From:** `sources/compiler.rs:558-591`

Shows how to compile `when` to an if-expression with proper body sequencing.

**Takeaway:** Even without macros, we can transform syntax at compile/expansion time.

### 2. Token Processing
```rust
let (guard, statements) = vec_explode_1n(statements);
let statements = Expression::Sequence(
    ExpressionSequenceOperator::ReturnLast,
    statements.into_boxed_slice()
);
```

**Takeaway:** Helper functions for extracting first/rest are useful.

### 3. Arity Checking
```rust
let tokens_count = tokens.len();
if tokens_count < 2 {
    fail! (0x3c364a9f);
}
```

**Takeaway:** Validate arity early, fail fast with clear errors.

---

## Comparison Summary

### Chibi (Full Macro System)
```
User code: (define-syntax when ...)
           ↓
er-macro-transformer
           ↓
Pattern matching + template expansion
           ↓
Hygienic renaming
           ↓
Expanded code: (if test (begin ...))
           ↓
Evaluation
```

### Vonuvoli (Built-in Syntax)
```
User code: (when test body...)
           ↓
Hardcoded compiler function
           ↓
Direct AST construction: ConditionalIf
           ↓
Evaluation
```

### Our Goal (Proper R7RS Macros)
```
User code: (when test body...)
           ↓
Macro lookup (from define-syntax)
           ↓
Pattern matching
           ↓
Template expansion
           ↓
Hygiene
           ↓
Expanded: (if test (begin body...))
           ↓
Evaluation
```

---

## Recommendation for Patina

**Follow Chibi's approach, not Vonuvoli's.**

**Rationale:**
1. R7RS compliance requires `define-syntax`
2. Macro system is educational and powerful
3. Only 3 tests remaining - proper solution is worth it
4. Chibi's implementation is well-documented
5. Our research is already complete

**Timeline:**
- Vonuvoli approach: 1 week to 99% (temporary)
- Chibi approach: 3-4 weeks to 99% (permanent)
- Difference: 2-3 weeks for proper R7RS compliance

**Verdict:** Invest the 2-3 extra weeks for proper macros.

---

## Conclusion

Vonuvoli-scheme demonstrates that:
- **Built-in syntax is simpler** than user-defined macros
- **Compilation approach works** for special forms
- **R7RS compliance is optional** for some implementations

But for Patina:
- **R7RS compliance is a goal**
- **Macro system is worth implementing**
- **Chibi's approach is the right path**

Vonuvoli is an interesting data point showing an alternative design, but not the path we should follow.

---

## Files Analyzed

1. `sources/primitives_syntaxes.rs` - Syntax primitive enum
2. `sources/native_syntaxes.rs` - Native syntax wrapper
3. `sources/compiler.rs` - When/unless compilation (lines 558-591)
4. `sources/libraries_r7rs.rs` - R7RS exports (shows Unsupported status)

**Total lines analyzed:** ~300 lines of Rust
**Key insight:** Vonuvoli is NOT a reference for macro implementation

---

**Status:** Analysis Complete
**Recommendation:** Continue with Chibi-based macro implementation
**Estimated impact on timeline:** None (confirms our current approach)
