# Core IR Migration Strategy

**Status:** Planning
**Timeline:** End of Phase 1 → Early Phase 2
**Goal:** Decouple syntax tree from tree-walker to enable multi-backend architecture

---

## Executive Summary

Patina currently uses the `Value` enum for both AST representation (after parsing) and runtime values (during evaluation). This dual-purpose design is simple and correct, mirroring how Chibi-scheme works, but creates coupling that prevents backend-specific optimizations.

**The Solution:** Introduce `CoreExpr` (already defined in `patina-ir`) as an intermediate representation between parsing and evaluation. This enables:
- ✅ Lightweight AST representation (9 variants vs 23)
- ✅ Type-safe transformations
- ✅ Backend-specific optimizations
- ✅ Multiple backends sharing same frontend
- ✅ Source location tracking (future)

**Migration Strategy:** Gradual, non-breaking transition that maintains tree-walker simplicity while enabling future VM and JIT backends.

---

## Table of Contents

1. [Current Architecture Analysis](#current-architecture-analysis)
2. [The CoreExpr Solution](#the-coreexpr-solution)
3. [The Desugarer Module](#the-desugarer-module)
4. [Migration Phases](#migration-phases)
5. [Implementation Guide](#implementation-guide)
6. [Multi-Backend Future](#multi-backend-future)
7. [Success Metrics](#success-metrics)

---

## Current Architecture Analysis

### How It Works Today

```
Source Text → Lexer → Tokens → Parser → Value (AST) → Evaluator → Value (Runtime)
```

The `Value` enum (23 variants) serves dual purposes:

#### As AST (Compile-time)
```rust
// Parser produces Value representing source code structure
Value::Symbol("foo")              // Variable reference
Value::Pair(...)                  // Function application: (f x y)
Value::Pair([lambda, args, body]) // Lambda definition
```

#### As Runtime Values (Evaluation-time)
```rust
// Evaluator produces Value representing computed results
Value::Integer(42)                // Computed number
Value::Procedure(...)             // Created by lambda evaluation
Value::Symbol("foo")              // Quoted symbol data
```

### The Problems

#### 1. **Loss of Source Information**
Parser immediately discards token position data:

```rust
// parser/mod.rs:54-60
Token::Identifier(s) => {
    let val = Value::Symbol(Rc::from(s.as_str()));
    self.advance()?;  // Token with position info discarded!
    Ok(val)
}
```

**Impact:**
- Error messages can't show line/column numbers
- Macro expansion errors lack context
- Stack traces don't show definition sites
- Debugging is significantly harder

See `SOURCE_INFO_PLAN.md` for detailed analysis of this problem.

#### 2. **Heavyweight Runtime Values**
The `Value` enum includes variants never produced by parser:

```rust
pub enum Value {
    // Parser produces these:
    Symbol(Rc<str>),           // Identifiers
    Pair(Rc<(Value, Value)>),  // Lists/applications
    Integer(i64),              // Literals

    // Never from parser, only at runtime:
    Procedure(Procedure),      // Created by lambda evaluation
    Macro { name, data },      // Created by define-syntax
    Library(Rc<Library>),      // Created by import
    Promise(...),              // Created by delay
    InputPort/OutputPort,      // Created by open-file
    Values(Vec<Value>),        // Created by values primitive
    Unspecified,               // Returned by define/set!
    Eof,                       // Returned by read-char
    // ... 23 variants total
}
```

**Impact:**
- Every `match expr` in evaluator handles all 23 variants
- Pattern matching is verbose (see `eval/mod.rs:388-429`)
- Memory overhead for AST nodes during parsing
- Can't optimize for different contexts

#### 3. **Tight Coupling**
Frontend depends on runtime's `Value`:

```
patina-frontend (parser) → patina-runtime (Value)
                         ↓
patina-tree-walker (evaluator) → patina-runtime (Value)
```

**Problems:**
- Can't have backend-specific AST representations
- VM backend wants bytecode, but parser produces `Value`
- JIT backend wants typed IR, but gets generic `Value`
- No compile-time enforcement of AST vs runtime separation

#### 4. **No Type-Level Distinction**

```rust
// These have the same type but different semantics:
let ast: Value = parser.parse()?;           // Source code structure
let result: Value = evaluator.eval(&ast)?;  // Computed value
```

**Impact:**
- Can't enforce "this function only works on AST" at compile time
- Easy to accidentally mix contexts
- Helper methods can't be specialized for AST vs runtime

### Why This Design Was Chosen

This is **not a mistake**—it's a valid design choice:

- ✅ Chibi-scheme uses the same approach
- ✅ Simple to implement initially
- ✅ Natural for Lisp/Scheme (homoiconic)
- ✅ Works well for tree-walker
- ✅ Macros naturally transform `Value` → `Value`

**The problem:** This simplicity blocks multi-backend architecture.

---

## The CoreExpr Solution

### CoreExpr: Lightweight Core IR

Already defined in `crates/patina-ir/src/core_expr.rs`:

```rust
/// Core Scheme expressions after macro expansion and desugaring
#[derive(Debug, Clone)]
pub enum CoreExpr {
    /// Literal values: 42, #t, "hello"
    Literal(Value),

    /// Variable reference: x, my-function
    Var(Symbol),

    /// Quote: 'x, '(1 2 3)
    Quote(Value),

    /// Lambda: (lambda (x y) body)
    Lambda {
        params: Formals,
        body: Vec<CoreExpr>,
    },

    /// Conditional: (if test then else)
    If {
        test: Box<CoreExpr>,
        then: Box<CoreExpr>,
        else_: Box<CoreExpr>,
    },

    /// Assignment: (set! x 42)
    Set {
        var: Symbol,
        value: Box<CoreExpr>,
    },

    /// Sequencing: (begin expr1 expr2)
    Begin(Vec<CoreExpr>),

    /// Top-level definition: (define x 42)
    Define {
        name: Symbol,
        value: Box<CoreExpr>,
    },

    /// Application: (f x y)
    App {
        func: Box<CoreExpr>,
        args: Vec<CoreExpr>,
    },

    // Optional optimized forms:
    PrimCall { prim, args },  // After optimization recognizes primitives
    Let { bindings, body },   // After optimization recognizes let pattern
}
```

### Key Benefits

#### 1. **Minimal Surface Area**
Only **9 core forms** (vs 23 Value variants):
- `lambda`, `if`, `set!`, `define`, `begin`, `quote` (special forms)
- Variable reference, literal, application
- Optional: `PrimCall`, `Let` (optimizations)

All derived forms (`let`, `cond`, `and`, `or`, `case`, etc.) are desugared away.

#### 2. **Type-Safe Structure**
Each form has explicit structure:

```rust
// Compare:
Value::Pair(...)  // Could be (if ...), (lambda ...), (+ ...), or anything!

// vs
CoreExpr::If { test, then, else_ }  // Compiler knows it's an if
CoreExpr::Lambda { params, body }    // Compiler knows it's a lambda
CoreExpr::App { func, args }         // Compiler knows it's application
```

#### 3. **Optimization-Ready**
`CoreExpr` can be extended with backend-specific forms:

```rust
// Future optimization passes can add:
CoreExpr::PrimCall {
    prim: Primitive::Add,
    args: vec![...],
}
// VM can generate optimized bytecode for known primitives

CoreExpr::Let { ... }
// Recognized let patterns can use specialized binding instructions
```

#### 4. **Source Location Ready**
Easy to wrap with metadata:

```rust
pub struct Syntax<T = CoreExpr> {
    pub data: T,
    pub source: Option<SourceInfo>,
}

// Parser produces:
Syntax { data: CoreExpr::Lambda { ... }, source: Some(span) }
```

See `SOURCE_INFO_PLAN.md` for full design.

### Comparison: Value vs CoreExpr

| Aspect | Value (Current) | CoreExpr (Future) |
|--------|----------------|-------------------|
| **Variants** | 23 | 9 core + 2 optional |
| **Purpose** | AST + Runtime | AST only |
| **Structure** | Generic pairs | Typed nodes |
| **Pattern matching** | Verbose, error-prone | Exhaustive, type-safe |
| **Memory** | Heavy (all runtime variants) | Light (only AST forms) |
| **Optimization** | Difficult | Natural |
| **Backend coupling** | Tight | Decoupled |
| **Source tracking** | Difficult | Natural |

---

## The Desugarer Module

### Role and Responsibility

The desugarer is the **boundary between frontend and backend**:

```
┌──────────────────────────────────────────────────────────────┐
│ FRONTEND (Homoiconic, Simple)                                │
│                                                               │
│ Source → Lexer → Parser → Value → Macro Expander → Value    │
│                                                               │
│ • Parser stays simple (produces runtime Values)              │
│ • Macros work naturally (transform Value → Value)            │
│ • Maintains Scheme's "code is data" philosophy               │
└───────────────────────────────┬──────────────────────────────┘
                                │
                                ↓
                    ┌───────────────────────┐
                    │   DESUGARER           │
                    │   (Value → CoreExpr)  │
                    └───────────┬───────────┘
                                │
                                ↓
┌───────────────────────────────┴──────────────────────────────┐
│ BACKEND (Optimized, Type-Safe)                               │
│                                                               │
│  Tree-Walker      Bytecode VM      JIT Compiler              │
│  (CoreExpr)       (CoreExpr→BC)    (CoreExpr→Native)         │
│                                                               │
│ • Lightweight IR (9 variants vs 23)                          │
│ • Type-safe transformations                                  │
│ • Backend-specific optimizations                             │
└──────────────────────────────────────────────────────────────┘
```

### Responsibilities

#### 1. **Desugar Derived Forms**
Convert high-level syntax to core forms:

```scheme
;; Input (Value):
(let ((x 1) (y 2)) (+ x y))

;; Output (CoreExpr):
((lambda (x y) (+ x y)) 1 2)
```

```scheme
;; Input (Value):
(cond ((< x 0) 'negative)
      ((= x 0) 'zero)
      (else 'positive))

;; Output (CoreExpr):
(if (< x 0)
    'negative
    (if (= x 0)
        'zero
        'positive))
```

```scheme
;; Input (Value):
(and a b c)

;; Output (CoreExpr):
(if a (if b c #f) #f)
```

#### 2. **Validate Structure**
Ensure well-formed syntax:

```rust
fn desugar_lambda(&self, args: &Value) -> Result<CoreExpr, Error> {
    // (lambda (x y) body1 body2 ...)
    let (params, body) = parse_lambda_syntax(args)?;

    // Validate:
    if body.is_empty() {
        return Err(Error::EmptyLambdaBody);
    }

    // Check for duplicate parameters
    check_no_duplicates(&params)?;

    Ok(CoreExpr::Lambda {
        params: convert_formals(params)?,
        body: body.iter()
            .map(|e| self.desugar(e))
            .collect::<Result<_, _>>()?,
    })
}
```

#### 3. **Convert to Typed Representation**
Transform generic pairs to typed nodes:

```rust
fn desugar(&self, value: &Value) -> Result<CoreExpr, Error> {
    match value {
        // Self-evaluating → Literal
        Value::Integer(_) | Value::Boolean(_) | Value::String(_)
        | Value::Character(_) => {
            Ok(CoreExpr::Literal(value.clone()))
        }

        // Symbol → Variable reference
        Value::Symbol(s) => {
            Ok(CoreExpr::Var(s.clone()))
        }

        // Lists → Special forms or application
        Value::Pair(_) => {
            let (car, cdr) = value.as_pair()?;

            if let Value::Symbol(sym) = car {
                match sym.as_ref() {
                    // Core forms
                    "quote" => self.desugar_quote(cdr),
                    "lambda" => self.desugar_lambda(cdr),
                    "if" => self.desugar_if(cdr),
                    "set!" => self.desugar_set(cdr),
                    "define" => self.desugar_define(cdr),
                    "begin" => self.desugar_begin(cdr),

                    // Derived forms (desugar to core)
                    "let" => self.desugar_let(cdr),
                    "let*" => self.desugar_let_star(cdr),
                    "letrec" => self.desugar_letrec(cdr),
                    "cond" => self.desugar_cond(cdr),
                    "case" => self.desugar_case(cdr),
                    "and" => self.desugar_and(cdr),
                    "or" => self.desugar_or(cdr),
                    "when" => self.desugar_when(cdr),
                    "unless" => self.desugar_unless(cdr),
                    "do" => self.desugar_do(cdr),

                    // Application
                    _ => self.desugar_app(value),
                }
            } else {
                // ((f x) y) - application with complex operator
                self.desugar_app(value)
            }
        }

        // Vectors, bytevectors → Literal
        Value::Vector(_) | Value::Bytevector(_) => {
            Ok(CoreExpr::Literal(value.clone()))
        }

        // Runtime-only values (should not appear in AST)
        Value::Procedure(_) | Value::Macro { .. } | Value::Library(_) => {
            Err(Error::RuntimeValueInAST(value.clone()))
        }

        _ => Ok(CoreExpr::Literal(value.clone())),
    }
}
```

### Example Desugarings

#### `let` → `lambda` application

```rust
fn desugar_let(&self, args: &Value) -> Result<CoreExpr, Error> {
    // (let ((x 1) (y 2)) body1 body2 ...)
    let (bindings, body) = parse_let_syntax(args)?;

    let mut params = Vec::new();
    let mut args = Vec::new();

    for (var, val) in bindings {
        params.push(var);
        args.push(self.desugar(&val)?);
    }

    let body_exprs: Vec<CoreExpr> = body.iter()
        .map(|e| self.desugar(e))
        .collect::<Result<_, _>>()?;

    // ((lambda (params...) body...) args...)
    Ok(CoreExpr::App {
        func: Box::new(CoreExpr::Lambda {
            params: Formals::Fixed(params),
            body: body_exprs,
        }),
        args,
    })
}
```

#### `cond` → nested `if`

```rust
fn desugar_cond(&self, clauses: &Value) -> Result<CoreExpr, Error> {
    let clauses = list_to_vec(clauses)?;

    if clauses.is_empty() {
        return Err(Error::EmptyCond);
    }

    let mut result = None;

    // Process clauses in reverse (build from inside out)
    for clause in clauses.iter().rev() {
        let (test, exprs) = parse_cond_clause(clause)?;

        let consequent = if exprs.len() == 1 {
            self.desugar(&exprs[0])?
        } else {
            CoreExpr::Begin(
                exprs.iter()
                    .map(|e| self.desugar(e))
                    .collect::<Result<_, _>>()?
            )
        };

        // Handle 'else' clause
        if matches!(test, Value::Symbol(s) if s.as_ref() == "else") {
            result = Some(consequent);
        } else {
            let alternate = result.unwrap_or(CoreExpr::Literal(Value::Unspecified));
            result = Some(CoreExpr::If {
                test: Box::new(self.desugar(&test)?),
                then: Box::new(consequent),
                else_: Box::new(alternate),
            });
        }
    }

    Ok(result.unwrap())
}
```

#### `and` → short-circuit `if`

```rust
fn desugar_and(&self, args: &Value) -> Result<CoreExpr, Error> {
    let exprs = list_to_vec(args)?;

    match exprs.len() {
        0 => Ok(CoreExpr::Literal(Value::Boolean(true))),
        1 => self.desugar(&exprs[0]),
        _ => {
            // (and a b c) → (if a (and b c) #f)
            let first = self.desugar(&exprs[0])?;
            let rest = self.desugar_and(&vec_to_list(&exprs[1..]))?;

            Ok(CoreExpr::If {
                test: Box::new(first),
                then: Box::new(rest),
                else_: Box::new(CoreExpr::Literal(Value::Boolean(false))),
            })
        }
    }
}
```

### Module Structure

```
crates/patina-frontend/src/
├── desugarer/
│   ├── mod.rs           # Main Desugarer struct
│   ├── error.rs         # DesugarError type
│   ├── bindings.rs      # let, let*, letrec desugaring
│   ├── conditionals.rs  # cond, case desugaring
│   ├── boolean.rs       # and, or desugaring
│   ├── iteration.rs     # do desugaring
│   └── utils.rs         # Helper functions
```

---

## Migration Phases

### Phase 1: Keep Current Architecture (Now - R7RS Completion)

**Status:** Current phase
**Duration:** Until 80% R7RS compliance
**Goal:** Complete Phase 1 R7RS features without disruption

```
Parser → Value → Tree-Walker(Value) → Value
```

**Actions:**
- ✅ Continue using Value as AST
- ✅ Focus on R7RS compliance
- ✅ Complete macro system
- ✅ Implement remaining primitives
- ⏸️ Defer CoreExpr integration

**Rationale:**
- Current architecture works
- Don't disrupt Phase 1 progress
- Learn what desugaring patterns are needed

### Phase 2: Build Desugarer (Post R7RS Core)

**Status:** Next phase
**Duration:** 2-3 weeks
**Goal:** Create Value → CoreExpr conversion

```
Parser → Value → Macro Expander → Value
                                    ↓
                              Desugarer → CoreExpr
                                    ↓
                         Tree-Walker(Value) [unchanged]
```

**Tasks:**

1. **Create desugarer module** (Week 1)
   - Set up `crates/patina-frontend/src/desugarer/`
   - Implement core form conversion
   - Handle derived forms
   - Add comprehensive tests

2. **Add CoreExpr evaluator** (Week 2)
   - Create `crates/patina-tree-walker/src/eval/core_eval.rs`
   - Implement `eval_core_expr()` function
   - Keep existing `eval()` for Value

3. **Integration and testing** (Week 3)
   - Add `Interpreter::eval_core_expr()` method
   - Create compatibility tests (Value path vs CoreExpr path)
   - Benchmark performance comparison
   - Fix any bugs

**Success Criteria:**
- ✅ All tests pass with both Value and CoreExpr paths
- ✅ CoreExpr path produces identical results
- ✅ Performance is equal or better

### Phase 3: Migrate Tree-Walker (Gradual)

**Status:** Future
**Duration:** 2-3 weeks
**Goal:** Make CoreExpr the primary evaluation path

```
Parser → Value → Macro Expander → Value → Desugarer → CoreExpr
                                                          ↓
                                                   Tree-Walker(CoreExpr)
```

**Tasks:**

1. **Update Procedure representation** (Week 1)
   ```rust
   pub enum Procedure {
       Lambda {
           params: Formals,
           body: Vec<CoreExpr>,  // Changed from Vec<Value>
           env: Rc<Environment>,
       },
       // ...
   }
   ```

2. **Remove Value-based eval path** (Week 2)
   - Delete old `eval()` implementation
   - Remove Value-based pattern matching
   - Update all call sites

3. **Update REPL and tools** (Week 3)
   - REPL uses CoreExpr path
   - Update debug tools
   - Update error reporting

**Success Criteria:**
- ✅ Tree-walker only evaluates CoreExpr
- ✅ All tests pass
- ✅ REPL works correctly
- ✅ Error messages are good

### Phase 4: Add Source Location Tracking (Optional)

**Status:** Future
**Duration:** 3-4 weeks
**Goal:** Rich error messages with source locations

```
Parser → Syntax<Value> → Macro Expander → Syntax<Value>
              ↓
        Desugarer → Syntax<CoreExpr>
              ↓
        Tree-Walker(Syntax<CoreExpr>)
```

See `SOURCE_INFO_PLAN.md` for full design.

**Benefits:**
- ✅ Error messages show line/column numbers
- ✅ Stack traces show definition sites
- ✅ Macro expansion errors have context
- ✅ Better debugging experience

### Phase 5: Enable Multi-Backend (Phase 2 of project)

**Status:** Future (Phase 2)
**Duration:** Ongoing
**Goal:** Support multiple execution backends

```
                                           ┌→ Tree-Walker(CoreExpr)
Parser → Value → Macros → CoreExpr ───────┼→ Bytecode VM
                                           └→ JIT Compiler
```

See `PRD/MULTI_BACKEND_STRATEGY.md` and `PRD/phase2/` for details.

**Benefits:**
- ✅ Choose backend per use case
- ✅ VM for performance
- ✅ JIT for long-running code
- ✅ Tree-walker for debugging

---

## Implementation Guide

### Step 1: Set Up Desugarer Module

```bash
# Create module structure
mkdir -p crates/patina-frontend/src/desugarer
touch crates/patina-frontend/src/desugarer/{mod.rs,error.rs,bindings.rs,conditionals.rs,boolean.rs,iteration.rs,utils.rs}
```

**File: `crates/patina-frontend/src/desugarer/mod.rs`**

```rust
mod error;
mod bindings;
mod conditionals;
mod boolean;
mod iteration;
mod utils;

pub use error::DesugarError;

use patina_ir::CoreExpr;
use patina_runtime::Value;
use std::rc::Rc;

pub struct Desugarer {
    // Future: add options, configuration
}

impl Desugarer {
    pub fn new() -> Self {
        Self {}
    }

    /// Desugar a Value (surface syntax) to CoreExpr (core IR)
    pub fn desugar(&self, value: &Value) -> Result<CoreExpr, DesugarError> {
        match value {
            // Self-evaluating
            Value::Boolean(_) | Value::Integer(_) | Value::BigInteger(_)
            | Value::Rational(_) | Value::Real(_) | Value::Complex(_, _)
            | Value::Character(_) | Value::String(_) | Value::Bytevector(_) => {
                Ok(CoreExpr::Literal(value.clone()))
            }

            // Variable reference
            Value::Symbol(s) => Ok(CoreExpr::Var(s.clone())),

            // Empty list (unusual in AST, but possible)
            Value::Null => Ok(CoreExpr::Literal(Value::Null)),

            // Lists (special forms or application)
            Value::Pair(_) => self.desugar_list(value),

            // Vectors (literal)
            Value::Vector(_) => Ok(CoreExpr::Literal(value.clone())),

            // Runtime-only values (error)
            Value::Procedure(_) | Value::Macro { .. } | Value::Library(_)
            | Value::InputPort | Value::OutputPort | Value::Promise(_)
            | Value::Unspecified | Value::Eof | Value::Values(_) => {
                Err(DesugarError::RuntimeValueInAST {
                    value: value.clone(),
                    context: "Cannot desugar runtime-only value".to_string(),
                })
            }
        }
    }

    fn desugar_list(&self, value: &Value) -> Result<CoreExpr, DesugarError> {
        let (car, cdr) = value.as_pair()
            .map_err(|_| DesugarError::InvalidSyntax("Expected pair".to_string()))?;

        if let Value::Symbol(sym) = car {
            match sym.as_ref() {
                // Core special forms
                "quote" => self.desugar_quote(cdr),
                "lambda" => self.desugar_lambda(cdr),
                "if" => self.desugar_if(cdr),
                "set!" => self.desugar_set(cdr),
                "define" => self.desugar_define(cdr),
                "begin" => self.desugar_begin(cdr),

                // Derived forms - desugar to core
                "let" => bindings::desugar_let(self, cdr),
                "let*" => bindings::desugar_let_star(self, cdr),
                "letrec" => bindings::desugar_letrec(self, cdr),
                "letrec*" => bindings::desugar_letrec_star(self, cdr),
                "let-values" => bindings::desugar_let_values(self, cdr),
                "let*-values" => bindings::desugar_let_star_values(self, cdr),

                "cond" => conditionals::desugar_cond(self, cdr),
                "case" => conditionals::desugar_case(self, cdr),
                "when" => conditionals::desugar_when(self, cdr),
                "unless" => conditionals::desugar_unless(self, cdr),

                "and" => boolean::desugar_and(self, cdr),
                "or" => boolean::desugar_or(self, cdr),

                "do" => iteration::desugar_do(self, cdr),

                // Application
                _ => self.desugar_app(value),
            }
        } else {
            // ((lambda ...) args) or other complex operator
            self.desugar_app(value)
        }
    }

    fn desugar_quote(&self, args: &Value) -> Result<CoreExpr, DesugarError> {
        // (quote datum)
        let datum = utils::expect_one_arg(args, "quote")?;
        Ok(CoreExpr::Quote(datum))
    }

    fn desugar_lambda(&self, args: &Value) -> Result<CoreExpr, DesugarError> {
        // (lambda formals body ...)
        let (formals, body) = utils::parse_lambda_syntax(args)?;

        if body.is_empty() {
            return Err(DesugarError::EmptyBody("lambda".to_string()));
        }

        let body_exprs: Vec<CoreExpr> = body.iter()
            .map(|e| self.desugar(e))
            .collect::<Result<_, _>>()?;

        Ok(CoreExpr::Lambda {
            params: utils::convert_formals(formals)?,
            body: body_exprs,
        })
    }

    fn desugar_if(&self, args: &Value) -> Result<CoreExpr, DesugarError> {
        let args_vec = utils::list_to_vec(args)?;

        let (test, then, else_) = match args_vec.as_slice() {
            [test, then] => {
                // Two-arg if: (if test then) → (if test then #<unspecified>)
                (test, then, &Value::Unspecified)
            }
            [test, then, else_] => (test, then, else_),
            _ => return Err(DesugarError::WrongArgCount {
                form: "if".to_string(),
                expected: "2 or 3".to_string(),
                got: args_vec.len(),
            }),
        };

        Ok(CoreExpr::If {
            test: Box::new(self.desugar(test)?),
            then: Box::new(self.desugar(then)?),
            else_: Box::new(self.desugar(else_)?),
        })
    }

    fn desugar_set(&self, args: &Value) -> Result<CoreExpr, DesugarError> {
        // (set! var value)
        let (var, value) = utils::expect_two_args(args, "set!")?;

        let var_sym = match var {
            Value::Symbol(s) => s.clone(),
            _ => return Err(DesugarError::InvalidSyntax(
                "set! requires a symbol".to_string()
            )),
        };

        Ok(CoreExpr::Set {
            var: var_sym,
            value: Box::new(self.desugar(value)?),
        })
    }

    fn desugar_define(&self, args: &Value) -> Result<CoreExpr, DesugarError> {
        let args_vec = utils::list_to_vec(args)?;

        match args_vec.as_slice() {
            // (define var value)
            [Value::Symbol(name), value] => {
                Ok(CoreExpr::Define {
                    name: name.clone(),
                    value: Box::new(self.desugar(value)?),
                })
            }

            // (define (name params...) body...)
            [Value::Pair(_), body @ ..] => {
                let (name, params) = utils::parse_define_function(&args_vec[0])?;

                if body.is_empty() {
                    return Err(DesugarError::EmptyBody("define".to_string()));
                }

                let lambda = CoreExpr::Lambda {
                    params: utils::convert_formals(params)?,
                    body: body.iter()
                        .map(|e| self.desugar(e))
                        .collect::<Result<_, _>>()?,
                };

                Ok(CoreExpr::Define {
                    name,
                    value: Box::new(lambda),
                })
            }

            _ => Err(DesugarError::InvalidSyntax(
                "define requires (define var value) or (define (name ...) body)".to_string()
            )),
        }
    }

    fn desugar_begin(&self, args: &Value) -> Result<CoreExpr, DesugarError> {
        let exprs = utils::list_to_vec(args)?;

        if exprs.is_empty() {
            return Err(DesugarError::EmptyBody("begin".to_string()));
        }

        let core_exprs: Vec<CoreExpr> = exprs.iter()
            .map(|e| self.desugar(e))
            .collect::<Result<_, _>>()?;

        // Optimize: (begin expr) → expr
        if core_exprs.len() == 1 {
            Ok(core_exprs.into_iter().next().unwrap())
        } else {
            Ok(CoreExpr::Begin(core_exprs))
        }
    }

    fn desugar_app(&self, value: &Value) -> Result<CoreExpr, DesugarError> {
        // (func arg1 arg2 ...)
        let list = utils::list_to_vec(value)?;

        if list.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "Cannot evaluate empty list".to_string()
            ));
        }

        let func = self.desugar(&list[0])?;
        let args: Vec<CoreExpr> = list[1..].iter()
            .map(|e| self.desugar(e))
            .collect::<Result<_, _>>()?;

        Ok(CoreExpr::App {
            func: Box::new(func),
            args,
        })
    }
}

impl Default for Desugarer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desugar_literal() {
        let desugarer = Desugarer::new();

        let value = Value::Integer(42);
        let result = desugarer.desugar(&value).unwrap();

        assert!(matches!(result, CoreExpr::Literal(Value::Integer(42))));
    }

    #[test]
    fn test_desugar_variable() {
        let desugarer = Desugarer::new();

        let value = Value::Symbol(Rc::from("x"));
        let result = desugarer.desugar(&value).unwrap();

        assert!(matches!(result, CoreExpr::Var(_)));
    }

    // Add more tests...
}
```

### Step 2: Implement Helper Modules

See `crates/patina-frontend/src/desugarer/` for full implementation of:
- `bindings.rs` - let, let*, letrec desugaring
- `conditionals.rs` - cond, case desugaring
- `boolean.rs` - and, or desugaring
- `iteration.rs` - do desugaring
- `utils.rs` - Common parsing helpers

### Step 3: Add CoreExpr Evaluator

**File: `crates/patina-tree-walker/src/eval/core_eval.rs`**

```rust
use patina_ir::CoreExpr;
use patina_runtime::{Value, Environment, Procedure};
use crate::eval::error::EvalError;
use std::rc::Rc;

pub struct CoreEvaluator {
    // Future: add profiling, tracing, etc.
}

impl CoreEvaluator {
    pub fn new() -> Self {
        Self {}
    }

    pub fn eval(
        &self,
        expr: &CoreExpr,
        env: &Rc<Environment>
    ) -> Result<Value, EvalError> {
        match expr {
            CoreExpr::Literal(v) => Ok(v.clone()),

            CoreExpr::Var(sym) => {
                env.get(sym.as_ref())
                    .ok_or_else(|| EvalError::UndefinedVariable(sym.to_string()))
            }

            CoreExpr::Quote(v) => Ok(v.clone()),

            CoreExpr::Lambda { params, body } => {
                Ok(Value::Procedure(Procedure::Lambda {
                    params: params.clone(),
                    body: body.clone(),
                    env: env.clone(),
                }))
            }

            CoreExpr::If { test, then, else_ } => {
                let test_val = self.eval(test, env)?;

                if test_val.is_truthy() {
                    self.eval(then, env)
                } else {
                    self.eval(else_, env)
                }
            }

            CoreExpr::Set { var, value } => {
                let val = self.eval(value, env)?;
                env.set(var.as_ref(), val)?;
                Ok(Value::Unspecified)
            }

            CoreExpr::Begin(exprs) => {
                let mut result = Value::Unspecified;
                for expr in exprs {
                    result = self.eval(expr, env)?;
                }
                Ok(result)
            }

            CoreExpr::Define { name, value } => {
                let val = self.eval(value, env)?;
                env.define(name.as_ref(), val);
                Ok(Value::Unspecified)
            }

            CoreExpr::App { func, args } => {
                let func_val = self.eval(func, env)?;

                let arg_vals: Vec<Value> = args.iter()
                    .map(|a| self.eval(a, env))
                    .collect::<Result<_, _>>()?;

                self.apply(func_val, arg_vals, env)
            }

            CoreExpr::PrimCall { prim, args } => {
                // Future optimization: direct primitive dispatch
                let arg_vals: Vec<Value> = args.iter()
                    .map(|a| self.eval(a, env))
                    .collect::<Result<_, _>>()?;

                self.apply_primitive(prim, arg_vals)
            }

            CoreExpr::Let { bindings, body } => {
                // Future optimization: specialized let binding
                let new_env = Rc::new(Environment::new_with_parent(env.clone()));

                for (var, val_expr) in bindings {
                    let val = self.eval(val_expr, env)?;
                    new_env.define(var.as_ref(), val);
                }

                self.eval(body, &new_env)
            }
        }
    }

    fn apply(
        &self,
        func: Value,
        args: Vec<Value>,
        env: &Rc<Environment>
    ) -> Result<Value, EvalError> {
        // Application logic (same as current tree-walker)
        // See crates/patina-tree-walker/src/eval/application.rs
        todo!("Implement application")
    }

    fn apply_primitive(
        &self,
        prim: &patina_ir::Primitive,
        args: Vec<Value>
    ) -> Result<Value, EvalError> {
        // Future: optimized primitive dispatch
        todo!("Implement primitive calls")
    }
}
```

### Step 4: Integrate with Interpreter API

**File: `crates/patina-interpreter/src/lib.rs`**

```rust
use patina_frontend::desugarer::Desugarer;
use patina_tree_walker::CoreEvaluator;
use patina_ir::CoreExpr;

pub struct Interpreter {
    parser: Parser,
    macro_expander: MacroExpander,
    desugarer: Desugarer,           // NEW
    evaluator: CoreEvaluator,        // NEW
    env: Rc<Environment>,
}

impl Interpreter {
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
            macro_expander: MacroExpander::new(),
            desugarer: Desugarer::new(),
            evaluator: CoreEvaluator::new(),
            env: create_global_env(),
        }
    }

    /// Evaluate a string (single expression)
    pub fn eval_str(&mut self, input: &str) -> Result<Value, InterpreterError> {
        // 1. Parse to Value (surface syntax)
        let value = self.parser.parse(input)?;

        // 2. Expand macros (Value → Value)
        let expanded = self.macro_expander.expand(value)?;

        // 3. Desugar to CoreExpr (Value → CoreExpr)
        let core_expr = self.desugarer.desugar(&expanded)?;

        // 4. Evaluate (CoreExpr → Value)
        let result = self.evaluator.eval(&core_expr, &self.env)?;

        Ok(result)
    }

    /// Evaluate a program (multiple expressions)
    pub fn eval_program(&mut self, input: &str) -> Result<Value, InterpreterError> {
        let values = self.parser.parse_multiple(input)?;

        let mut result = Value::Unspecified;

        for value in values {
            let expanded = self.macro_expander.expand(value)?;
            let core_expr = self.desugarer.desugar(&expanded)?;
            result = self.evaluator.eval(&core_expr, &self.env)?;
        }

        Ok(result)
    }
}
```

### Step 5: Add Comprehensive Tests

```rust
// crates/patina-tests/tests/desugarer_tests.rs
use patina_frontend::desugarer::Desugarer;
use patina_ir::CoreExpr;
use patina_runtime::Value;

#[test]
fn test_let_desugaring() {
    let desugarer = Desugarer::new();

    // Input: (let ((x 1) (y 2)) (+ x y))
    let input = parse("(let ((x 1) (y 2)) (+ x y))");
    let result = desugarer.desugar(&input).unwrap();

    // Expected: ((lambda (x y) (+ x y)) 1 2)
    match result {
        CoreExpr::App { func, args } => {
            assert!(matches!(*func, CoreExpr::Lambda { .. }));
            assert_eq!(args.len(), 2);
        }
        _ => panic!("Expected application"),
    }
}

#[test]
fn test_cond_desugaring() {
    let desugarer = Desugarer::new();

    // Input: (cond ((< x 0) 'neg) ((= x 0) 'zero) (else 'pos))
    let input = parse("(cond ((< x 0) 'neg) ((= x 0) 'zero) (else 'pos))");
    let result = desugarer.desugar(&input).unwrap();

    // Expected: nested ifs
    match result {
        CoreExpr::If { test, then, else_ } => {
            // (if (< x 0) 'neg (if (= x 0) 'zero 'pos))
            assert!(matches!(*else_, CoreExpr::If { .. }));
        }
        _ => panic!("Expected if"),
    }
}

// crates/patina-tests/tests/core_eval_tests.rs
use patina_tree_walker::CoreEvaluator;
use patina_ir::CoreExpr;

#[test]
fn test_core_eval_lambda() {
    let evaluator = CoreEvaluator::new();
    let env = create_test_env();

    // ((lambda (x) (+ x 1)) 5)
    let expr = CoreExpr::App {
        func: Box::new(CoreExpr::Lambda {
            params: Formals::Fixed(vec![Rc::from("x")]),
            body: vec![CoreExpr::App {
                func: Box::new(CoreExpr::Var(Rc::from("+"))),
                args: vec![
                    CoreExpr::Var(Rc::from("x")),
                    CoreExpr::Literal(Value::Integer(1)),
                ],
            }],
        }),
        args: vec![CoreExpr::Literal(Value::Integer(5))],
    };

    let result = evaluator.eval(&expr, &env).unwrap();
    assert_eq!(result, Value::Integer(6));
}

// crates/patina-tests/tests/parity_tests.rs
/// Test that Value-based and CoreExpr-based evaluation produce same results
#[test]
fn test_value_coreexpr_parity() {
    let test_cases = vec![
        "(+ 1 2 3)",
        "(if #t 10 20)",
        "(let ((x 5)) (* x x))",
        "(cond ((< 3 2) 'no) (else 'yes))",
        "((lambda (x y) (+ x y)) 3 4)",
        // ... hundreds more
    ];

    for expr in test_cases {
        let value_result = eval_with_value_path(expr).unwrap();
        let core_result = eval_with_core_path(expr).unwrap();

        assert_eq!(
            value_result,
            core_result,
            "Mismatch for: {}",
            expr
        );
    }
}
```

---

## Multi-Backend Future

Once CoreExpr is integrated, adding new backends is straightforward:

### Bytecode VM Backend

```rust
// crates/patina-vm/src/compiler.rs
pub struct BytecodeCompiler {
    constants: Vec<Value>,
    code: Vec<OpCode>,
}

impl BytecodeCompiler {
    pub fn compile(&mut self, expr: &CoreExpr) -> Result<(), CompileError> {
        match expr {
            CoreExpr::Literal(v) => {
                let idx = self.add_constant(v.clone());
                self.emit(OpCode::LoadConst(idx));
            }

            CoreExpr::Var(sym) => {
                self.emit(OpCode::LoadVar(self.intern(sym)));
            }

            CoreExpr::Lambda { params, body } => {
                // Compile closure
                let code = self.compile_lambda(params, body)?;
                let idx = self.add_constant(Value::Bytecode(code));
                self.emit(OpCode::MakeClosure(idx));
            }

            CoreExpr::If { test, then, else_ } => {
                self.compile(test)?;
                let jump_if_false = self.emit_jump(OpCode::JumpIfFalse(0));
                self.compile(then)?;
                let jump_to_end = self.emit_jump(OpCode::Jump(0));
                self.patch_jump(jump_if_false);
                self.compile(else_)?;
                self.patch_jump(jump_to_end);
            }

            CoreExpr::App { func, args } => {
                // Compile function
                self.compile(func)?;

                // Compile arguments
                for arg in args {
                    self.compile(arg)?;
                }

                // Call
                self.emit(OpCode::Call(args.len() as u8));
            }

            // ... handle all CoreExpr variants
        }
        Ok(())
    }
}

// crates/patina-vm/src/vm.rs
pub struct VM {
    stack: Vec<Value>,
    ip: usize,
    code: Vec<OpCode>,
    constants: Vec<Value>,
    globals: Rc<Environment>,
}

impl VM {
    pub fn run(&mut self) -> Result<Value, EvalError> {
        while self.ip < self.code.len() {
            match self.code[self.ip] {
                OpCode::LoadConst(idx) => {
                    self.stack.push(self.constants[idx].clone());
                }
                OpCode::LoadVar(sym) => {
                    let val = self.globals.get(&sym)?;
                    self.stack.push(val);
                }
                OpCode::Call(argc) => {
                    let func = self.stack.pop().unwrap();
                    let args: Vec<_> = (0..argc)
                        .map(|_| self.stack.pop().unwrap())
                        .collect();
                    let result = self.apply(func, args)?;
                    self.stack.push(result);
                }
                // ... handle all opcodes
            }
            self.ip += 1;
        }
        Ok(self.stack.pop().unwrap())
    }
}
```

### Backend Selection

```rust
// crates/patina-interpreter/src/backend.rs
pub trait Backend {
    fn eval(&mut self, expr: &CoreExpr, env: &Rc<Environment>)
        -> Result<Value, EvalError>;
}

pub struct TreeWalkingBackend {
    evaluator: CoreEvaluator,
}

impl Backend for TreeWalkingBackend {
    fn eval(&mut self, expr: &CoreExpr, env: &Rc<Environment>)
        -> Result<Value, EvalError> {
        self.evaluator.eval(expr, env)
    }
}

pub struct BytecodeBackend {
    compiler: BytecodeCompiler,
    vm: VM,
}

impl Backend for BytecodeBackend {
    fn eval(&mut self, expr: &CoreExpr, env: &Rc<Environment>)
        -> Result<Value, EvalError> {
        let bytecode = self.compiler.compile(expr)?;
        self.vm.run_bytecode(bytecode, env)
    }
}

// In Interpreter:
pub struct Interpreter {
    backend: Box<dyn Backend>,
    // ... rest
}

impl Interpreter {
    pub fn with_backend(kind: BackendKind) -> Self {
        let backend: Box<dyn Backend> = match kind {
            BackendKind::TreeWalking => Box::new(TreeWalkingBackend::new()),
            BackendKind::Bytecode => Box::new(BytecodeBackend::new()),
            BackendKind::JIT => Box::new(JITBackend::new()),
        };

        Self { backend, /* ... */ }
    }
}
```

See:
- `PRD/MULTI_BACKEND_STRATEGY.md` - Full backend abstraction design
- `PRD/phase2/VM_SPECIFICATION.md` - Detailed VM instruction set
- `PRD/phase2/README.md` - VM optimization research

---

## Success Metrics

### Phase 2 (Desugarer Implementation)

✅ **Correctness:**
- All existing tests pass with CoreExpr path
- Parity tests show identical results (Value vs CoreExpr)
- No regressions in functionality

✅ **Performance:**
- CoreExpr evaluation is ≥ Value evaluation speed
- Memory usage is equal or better
- Benchmark suite shows no slowdowns

✅ **Code Quality:**
- Desugarer has >90% test coverage
- Clear separation of concerns
- Well-documented code

### Phase 3 (Tree-Walker Migration)

✅ **Simplicity:**
- Tree-walker evaluator has fewer lines of code
- Pattern matching is cleaner (9 cases vs 23)
- Easier to understand and maintain

✅ **Reliability:**
- All 435 tests pass
- REPL works correctly
- Error messages are clear

### Phase 4 (Source Locations - Optional)

✅ **User Experience:**
- Error messages show file:line:column
- Stack traces show definition sites
- Macro errors have full context

### Phase 5 (Multi-Backend)

✅ **Architecture:**
- Clean backend abstraction
- Backends share same frontend
- Easy to add new backends

✅ **Performance:**
- Bytecode VM is 3-10x faster than tree-walker
- JIT is 10-100x faster on hot loops
- Users can choose appropriate backend

---

## Conclusion

The migration to CoreExpr is a **gradual, low-risk transition** that:

1. **Maintains current simplicity** during Phase 1
2. **Decouples frontend from backend** for future flexibility
3. **Simplifies tree-walker** by reducing variants from 23 to 9
4. **Enables multiple backends** without disrupting existing code
5. **Supports source tracking** for better error messages

The key insight: **CoreExpr makes the tree-walker simpler, not more complex**.

### Next Steps

1. ✅ Complete Phase 1 R7RS compliance (current focus)
2. ⏳ Implement desugarer module (2-3 weeks)
3. ⏳ Add CoreExpr evaluator alongside Value evaluator
4. ⏳ Migrate tree-walker to use CoreExpr
5. ⏳ Add VM backend (Phase 2)

### References

- `crates/patina-ir/src/core_expr.rs` - CoreExpr definition
- `PRD/MULTI_BACKEND_STRATEGY.md` - Backend abstraction
- `PRD/phase2/VM_SPECIFICATION.md` - VM design
- `PRD/phase1/SOURCE_INFO_PLAN.md` - Source location tracking
- `internal/reference_impls/CHIBI_REFERENCE.md` - How Chibi handles AST

---

**The path forward is clear, incremental, and low-risk. CoreExpr is the key to multi-backend flexibility while preserving tree-walker simplicity.**
