# Generic Desugarer Design

**Status:** Design Document
**Created:** 2025-12-13
**Related:** [SEXPR_SEPARATION_ARCHITECTURE.md](./SEXPR_SEPARATION_ARCHITECTURE.md), [VM_SPECIFICATION.md](./VM_SPECIFICATION.md)

---

## Overview

This document describes the design for a **generic desugarer** that can produce different IR representations for different backends:

- **Tree-walker**: `CoreExpr` with `Rc<Value>`
- **VM**: `VmCoreExpr` with `TaggedValue`

The key insight is that desugaring logic (parsing special forms, macro expansion, hygiene) is the same for all backends - only the output types differ.

---

## Goals

1. **Single implementation** of desugaring logic
2. **Backend-agnostic** - same code produces different IR types
3. **Zero-cost abstraction** - monomorphized at compile time
4. **Type-safe** - backend types fully specified via generics
5. **Extensible** - easy to add new backends

---

## Architecture

```
                        Value (from parser/macro expander)
                                    │
                                    ▼
                    ┌───────────────────────────────┐
                    │     Generic Desugarer<B>      │
                    │     where B: DesugarBackend   │
                    └───────────────────────────────┘
                                    │
                    ┌───────────────┴───────────────┐
                    │                               │
                    ▼                               ▼
        ┌───────────────────┐           ┌───────────────────┐
        │ TreeWalkerBackend │           │    VmBackend      │
        │                   │           │                   │
        │ Value → Rc<Value> │           │ Value → Tagged    │
        │ Expr  → CoreExpr  │           │ Expr  → VmCoreExpr│
        └───────────────────┘           └───────────────────┘
                    │                               │
                    ▼                               ▼
              CoreExpr                        VmCoreExpr
```

---

## Core Trait: `DesugarBackend`

```rust
/// Trait for backend-specific value and expression construction.
///
/// The desugarer is generic over this trait, allowing the same desugaring
/// logic to produce different output types for different backends.
pub trait DesugarBackend {
    /// The type used for literal values in this backend's IR.
    /// - Tree-walker: `Rc<Value>`
    /// - VM: `TaggedValue`
    type Datum: Clone;

    /// The type used for expressions in this backend's IR.
    /// - Tree-walker: `CoreExpr`
    /// - VM: `VmCoreExpr`
    type Expr;

    // =========================================================================
    // Datum Construction (for literals and quoted data)
    // =========================================================================

    /// Convert an integer to a datum
    fn datum_integer(&mut self, n: i64) -> Self::Datum;

    /// Convert a big integer to a datum
    fn datum_bigint(&mut self, n: &BigInt) -> Self::Datum;

    /// Convert a rational to a datum
    fn datum_rational(&mut self, r: &BigRational) -> Self::Datum;

    /// Convert a float to a datum
    fn datum_float(&mut self, f: f64) -> Self::Datum;

    /// Convert a boolean to a datum
    fn datum_boolean(&mut self, b: bool) -> Self::Datum;

    /// Convert a character to a datum
    fn datum_character(&mut self, c: char) -> Self::Datum;

    /// Convert a string to a datum
    fn datum_string(&mut self, s: &str) -> Self::Datum;

    /// Convert a symbol to a datum
    fn datum_symbol(&mut self, s: &Symbol) -> Self::Datum;

    /// Create a null (empty list) datum
    fn datum_null(&mut self) -> Self::Datum;

    /// Create a pair datum
    fn datum_pair(&mut self, car: Self::Datum, cdr: Self::Datum) -> Self::Datum;

    /// Create a vector datum
    fn datum_vector(&mut self, elements: Vec<Self::Datum>) -> Self::Datum;

    /// Create a bytevector datum
    fn datum_bytevector(&mut self, bytes: &[u8]) -> Self::Datum;

    // =========================================================================
    // Expression Construction
    // =========================================================================

    /// Create a literal expression
    fn expr_literal(&mut self, datum: Self::Datum) -> Self::Expr;

    /// Create a quoted expression
    fn expr_quote(&mut self, datum: Self::Datum) -> Self::Expr;

    /// Create a variable reference
    fn expr_var(&mut self, name: Symbol, scopes: ScopeSet) -> Self::Expr;

    /// Create a conditional expression
    fn expr_if(
        &mut self,
        test: Self::Expr,
        consequent: Self::Expr,
        alternate: Self::Expr,
    ) -> Self::Expr;

    /// Create a lambda expression
    fn expr_lambda(
        &mut self,
        params: Formals,
        body: Vec<Self::Expr>,
        binding_scope: Option<ScopeId>,
    ) -> Self::Expr;

    /// Create a function application
    fn expr_app(&mut self, func: Self::Expr, args: Vec<Self::Expr>) -> Self::Expr;

    /// Create a definition
    fn expr_define(&mut self, name: Symbol, value: Self::Expr) -> Self::Expr;

    /// Create an assignment
    fn expr_set(
        &mut self,
        name: Symbol,
        scopes: ScopeSet,
        value: Self::Expr,
    ) -> Self::Expr;

    /// Create a sequence (begin)
    fn expr_begin(&mut self, exprs: Vec<Self::Expr>) -> Self::Expr;

    /// Create an import expression
    fn expr_import(&mut self, import_sets: Vec<Self::Datum>) -> Self::Expr;
}
```

---

## Generic Desugarer

```rust
/// Generic desugarer that works with any backend.
pub struct Desugarer<'a, B: DesugarBackend> {
    /// Backend for constructing output
    backend: B,

    /// Macro environment for expansion
    macro_env: &'a MacroEnv,

    /// Current scope set (for hygiene)
    current_scopes: ScopeSet,

    /// Names shadowed by current binding form (can't be macros)
    shadowed: HashSet<Symbol>,
}

impl<'a, B: DesugarBackend> Desugarer<'a, B> {
    pub fn new(backend: B, macro_env: &'a MacroEnv) -> Self {
        Self {
            backend,
            macro_env,
            current_scopes: ScopeSet::new(),
            shadowed: HashSet::new(),
        }
    }

    /// Desugar a Value to the backend's expression type.
    pub fn desugar(&mut self, expr: &Value) -> Result<B::Expr, DesugarError> {
        match expr {
            // Literals
            Value::Integer(n) => {
                let datum = self.backend.datum_integer(*n);
                Ok(self.backend.expr_literal(datum))
            }
            Value::BigInteger(n) => {
                let datum = self.backend.datum_bigint(n);
                Ok(self.backend.expr_literal(datum))
            }
            Value::Rational(r) => {
                let datum = self.backend.datum_rational(r);
                Ok(self.backend.expr_literal(datum))
            }
            Value::Real(f) => {
                let datum = self.backend.datum_float(*f);
                Ok(self.backend.expr_literal(datum))
            }
            Value::Boolean(b) => {
                let datum = self.backend.datum_boolean(*b);
                Ok(self.backend.expr_literal(datum))
            }
            Value::Character(c) => {
                let datum = self.backend.datum_character(*c);
                Ok(self.backend.expr_literal(datum))
            }
            Value::String(s) => {
                let datum = self.backend.datum_string(&s.borrow());
                Ok(self.backend.expr_literal(datum))
            }

            // Variable reference
            Value::Symbol(s) => {
                Ok(self.backend.expr_var(s.clone(), self.current_scopes.clone()))
            }
            Value::Identifier(id) => {
                let combined_scopes = self.current_scopes.union(&id.scopes);
                Ok(self.backend.expr_var(id.name.clone(), combined_scopes))
            }

            // Compound forms
            Value::Pair(p) => {
                let borrowed = p.borrow();
                self.desugar_form(&borrowed.0, &borrowed.1)
            }

            // Self-evaluating
            Value::Null => {
                let datum = self.backend.datum_null();
                Ok(self.backend.expr_literal(datum))
            }

            // Vectors are self-evaluating
            Value::Vector(v) => {
                let elements: Result<Vec<_>, _> = v.borrow()
                    .iter()
                    .map(|e| self.value_to_datum(e))
                    .collect();
                let datum = self.backend.datum_vector(elements?);
                Ok(self.backend.expr_literal(datum))
            }

            // Bytevectors are self-evaluating
            Value::Bytevector(bv) => {
                let datum = self.backend.datum_bytevector(&bv.borrow());
                Ok(self.backend.expr_literal(datum))
            }

            _ => Err(DesugarError::UnsupportedForm(format!("{:?}", expr))),
        }
    }

    /// Desugar a form (head . tail)
    fn desugar_form(&mut self, head: &Value, tail: &Value) -> Result<B::Expr, DesugarError> {
        // Get the name if head is a symbol/identifier
        let name = self.get_symbol_name(head);

        // Check for special forms first
        if let Some(name) = &name {
            if !self.shadowed.contains(*name) {
                match name.as_ref() {
                    "quote" => return self.desugar_quote(tail),
                    "if" => return self.desugar_if(tail),
                    "lambda" => return self.desugar_lambda(tail),
                    "define" => return self.desugar_define(tail),
                    "set!" => return self.desugar_set(tail),
                    "begin" => return self.desugar_begin(tail),
                    "import" => return self.desugar_import(tail),
                    _ => {}
                }
            }
        }

        // Check for macro
        if let Some(name) = &name {
            if !self.shadowed.contains(*name) {
                if let Some(macro_def) = self.macro_env.get_macro(name) {
                    let input = Value::cons(head.clone(), tail.clone());
                    let expanded = expand_macro(&input, macro_def, &self.current_scopes)?;
                    return self.desugar(&expanded);
                }
            }
        }

        // Regular application
        self.desugar_app(head, tail)
    }

    // =========================================================================
    // Special Form Handlers
    // =========================================================================

    fn desugar_quote(&mut self, args: &Value) -> Result<B::Expr, DesugarError> {
        let items = collect_list(args)?;
        if items.len() != 1 {
            return Err(DesugarError::InvalidQuote);
        }
        let datum = self.value_to_datum(&items[0])?;
        Ok(self.backend.expr_quote(datum))
    }

    fn desugar_if(&mut self, args: &Value) -> Result<B::Expr, DesugarError> {
        let items = collect_list(args)?;

        let (test_expr, then_expr, else_expr) = match items.len() {
            2 => {
                // (if test then) -> (if test then <unspecified>)
                let test = self.desugar(&items[0])?;
                let then = self.desugar(&items[1])?;
                let else_ = {
                    let datum = self.backend.datum_symbol(&Symbol::from("unspecified"));
                    self.backend.expr_literal(datum)
                };
                (test, then, else_)
            }
            3 => {
                let test = self.desugar(&items[0])?;
                let then = self.desugar(&items[1])?;
                let else_ = self.desugar(&items[2])?;
                (test, then, else_)
            }
            _ => return Err(DesugarError::InvalidIf),
        };

        Ok(self.backend.expr_if(test_expr, then_expr, else_expr))
    }

    fn desugar_lambda(&mut self, args: &Value) -> Result<B::Expr, DesugarError> {
        let items = collect_list(args)?;
        if items.is_empty() {
            return Err(DesugarError::InvalidLambda("missing formals".into()));
        }

        // Parse formals
        let formals = parse_formals(&items[0])?;

        // Get parameter names for shadowing
        let param_names = formals.param_names();

        // Desugar body with parameters shadowed
        let old_shadowed = std::mem::take(&mut self.shadowed);
        self.shadowed = old_shadowed.iter().cloned().collect();
        for name in &param_names {
            self.shadowed.insert(name.clone());
        }

        let body: Result<Vec<_>, _> = items[1..]
            .iter()
            .map(|e| self.desugar(e))
            .collect();

        self.shadowed = old_shadowed;

        Ok(self.backend.expr_lambda(formals, body?, None))
    }

    fn desugar_define(&mut self, args: &Value) -> Result<B::Expr, DesugarError> {
        let items = collect_list(args)?;
        if items.is_empty() {
            return Err(DesugarError::InvalidDefine);
        }

        match &items[0] {
            // (define name value)
            Value::Symbol(name) | Value::Identifier(IdentifierData { name, .. }) => {
                if items.len() != 2 {
                    return Err(DesugarError::InvalidDefine);
                }
                let value = self.desugar(&items[1])?;
                Ok(self.backend.expr_define(name.clone(), value))
            }
            // (define (name . params) body...) -> (define name (lambda params body...))
            Value::Pair(p) => {
                let borrowed = p.borrow();
                let name = self.expect_symbol(&borrowed.0)?;
                let formals = parse_formals_from_cdr(&borrowed.1)?;

                // Get parameter names for shadowing
                let param_names = formals.param_names();
                let old_shadowed = std::mem::take(&mut self.shadowed);
                self.shadowed = old_shadowed.iter().cloned().collect();
                for pname in &param_names {
                    self.shadowed.insert(pname.clone());
                }

                let body: Result<Vec<_>, _> = items[1..]
                    .iter()
                    .map(|e| self.desugar(e))
                    .collect();

                self.shadowed = old_shadowed;

                let lambda = self.backend.expr_lambda(formals, body?, None);
                Ok(self.backend.expr_define(name, lambda))
            }
            _ => Err(DesugarError::InvalidDefine),
        }
    }

    fn desugar_set(&mut self, args: &Value) -> Result<B::Expr, DesugarError> {
        let items = collect_list(args)?;
        if items.len() != 2 {
            return Err(DesugarError::InvalidSet);
        }

        let (name, scopes) = match &items[0] {
            Value::Symbol(s) => (s.clone(), self.current_scopes.clone()),
            Value::Identifier(id) => {
                let combined = self.current_scopes.union(&id.scopes);
                (id.name.clone(), combined)
            }
            _ => return Err(DesugarError::InvalidSet),
        };

        let value = self.desugar(&items[1])?;
        Ok(self.backend.expr_set(name, scopes, value))
    }

    fn desugar_begin(&mut self, args: &Value) -> Result<B::Expr, DesugarError> {
        let items = collect_list(args)?;
        let exprs: Result<Vec<_>, _> = items.iter().map(|e| self.desugar(e)).collect();
        Ok(self.backend.expr_begin(exprs?))
    }

    fn desugar_import(&mut self, args: &Value) -> Result<B::Expr, DesugarError> {
        let items = collect_list(args)?;
        let import_sets: Result<Vec<_>, _> = items
            .iter()
            .map(|e| self.value_to_datum(e))
            .collect();
        Ok(self.backend.expr_import(import_sets?))
    }

    fn desugar_app(&mut self, head: &Value, tail: &Value) -> Result<B::Expr, DesugarError> {
        let func = self.desugar(head)?;
        let args_list = collect_list(tail)?;
        let args: Result<Vec<_>, _> = args_list.iter().map(|e| self.desugar(e)).collect();
        Ok(self.backend.expr_app(func, args?))
    }

    // =========================================================================
    // Helper Methods
    // =========================================================================

    /// Convert a Value to a datum (for quoted data)
    fn value_to_datum(&mut self, value: &Value) -> Result<B::Datum, DesugarError> {
        match value {
            Value::Integer(n) => Ok(self.backend.datum_integer(*n)),
            Value::BigInteger(n) => Ok(self.backend.datum_bigint(n)),
            Value::Rational(r) => Ok(self.backend.datum_rational(r)),
            Value::Real(f) => Ok(self.backend.datum_float(*f)),
            Value::Boolean(b) => Ok(self.backend.datum_boolean(*b)),
            Value::Character(c) => Ok(self.backend.datum_character(*c)),
            Value::String(s) => Ok(self.backend.datum_string(&s.borrow())),
            Value::Symbol(s) => Ok(self.backend.datum_symbol(s)),
            Value::Identifier(id) => Ok(self.backend.datum_symbol(&id.name)),
            Value::Null => Ok(self.backend.datum_null()),
            Value::Pair(p) => {
                let borrowed = p.borrow();
                let car = self.value_to_datum(&borrowed.0)?;
                let cdr = self.value_to_datum(&borrowed.1)?;
                Ok(self.backend.datum_pair(car, cdr))
            }
            Value::Vector(v) => {
                let elements: Result<Vec<_>, _> = v.borrow()
                    .iter()
                    .map(|e| self.value_to_datum(e))
                    .collect();
                Ok(self.backend.datum_vector(elements?))
            }
            Value::Bytevector(bv) => {
                Ok(self.backend.datum_bytevector(&bv.borrow()))
            }
            _ => Err(DesugarError::NotADatum(format!("{:?}", value))),
        }
    }

    fn get_symbol_name(&self, value: &Value) -> Option<Symbol> {
        match value {
            Value::Symbol(s) => Some(s.clone()),
            Value::Identifier(id) => Some(id.name.clone()),
            _ => None,
        }
    }

    fn expect_symbol(&self, value: &Value) -> Result<Symbol, DesugarError> {
        self.get_symbol_name(value)
            .ok_or_else(|| DesugarError::ExpectedSymbol)
    }
}
```

---

## Backend Implementations

### Tree-Walker Backend

```rust
/// Backend for tree-walker interpreter.
/// Produces `CoreExpr` with `Rc<Value>` datums.
pub struct TreeWalkerBackend;

impl DesugarBackend for TreeWalkerBackend {
    type Datum = Rc<Value>;
    type Expr = CoreExpr;

    // Datum construction - wrap in Rc<Value>
    fn datum_integer(&mut self, n: i64) -> Self::Datum {
        Rc::new(Value::Integer(n))
    }

    fn datum_bigint(&mut self, n: &BigInt) -> Self::Datum {
        Rc::new(Value::BigInteger(n.clone()))
    }

    fn datum_rational(&mut self, r: &BigRational) -> Self::Datum {
        Rc::new(Value::Rational(r.clone()))
    }

    fn datum_float(&mut self, f: f64) -> Self::Datum {
        Rc::new(Value::Real(f))
    }

    fn datum_boolean(&mut self, b: bool) -> Self::Datum {
        Rc::new(Value::Boolean(b))
    }

    fn datum_character(&mut self, c: char) -> Self::Datum {
        Rc::new(Value::Character(c))
    }

    fn datum_string(&mut self, s: &str) -> Self::Datum {
        Rc::new(Value::String(Rc::new(RefCell::new(s.to_string()))))
    }

    fn datum_symbol(&mut self, s: &Symbol) -> Self::Datum {
        Rc::new(Value::Symbol(s.clone()))
    }

    fn datum_null(&mut self) -> Self::Datum {
        Rc::new(Value::Null)
    }

    fn datum_pair(&mut self, car: Self::Datum, cdr: Self::Datum) -> Self::Datum {
        // Unwrap Rc to get inner Value, then create mutable pair
        let car_val = (*car).clone();
        let cdr_val = (*cdr).clone();
        Rc::new(Value::Pair(Rc::new(RefCell::new((car_val, cdr_val)))))
    }

    fn datum_vector(&mut self, elements: Vec<Self::Datum>) -> Self::Datum {
        let values: Vec<Value> = elements.into_iter().map(|d| (*d).clone()).collect();
        Rc::new(Value::Vector(Rc::new(RefCell::new(values))))
    }

    fn datum_bytevector(&mut self, bytes: &[u8]) -> Self::Datum {
        Rc::new(Value::Bytevector(Rc::new(RefCell::new(bytes.to_vec()))))
    }

    // Expression construction
    fn expr_literal(&mut self, datum: Self::Datum) -> Self::Expr {
        CoreExpr::Literal(datum)
    }

    fn expr_quote(&mut self, datum: Self::Datum) -> Self::Expr {
        CoreExpr::Quote(datum)
    }

    fn expr_var(&mut self, name: Symbol, scopes: ScopeSet) -> Self::Expr {
        CoreExpr::Var { name, scopes }
    }

    fn expr_if(
        &mut self,
        test: Self::Expr,
        consequent: Self::Expr,
        alternate: Self::Expr,
    ) -> Self::Expr {
        CoreExpr::If {
            test: Rc::new(test),
            then: Rc::new(consequent),
            else_: Rc::new(alternate),
        }
    }

    fn expr_lambda(
        &mut self,
        params: Formals,
        body: Vec<Self::Expr>,
        binding_scope: Option<ScopeId>,
    ) -> Self::Expr {
        CoreExpr::Lambda {
            params,
            body,
            binding_scope,
        }
    }

    fn expr_app(&mut self, func: Self::Expr, args: Vec<Self::Expr>) -> Self::Expr {
        CoreExpr::App {
            func: Rc::new(func),
            args,
        }
    }

    fn expr_define(&mut self, name: Symbol, value: Self::Expr) -> Self::Expr {
        CoreExpr::Define {
            name,
            value: Rc::new(value),
        }
    }

    fn expr_set(
        &mut self,
        name: Symbol,
        scopes: ScopeSet,
        value: Self::Expr,
    ) -> Self::Expr {
        CoreExpr::Set {
            var: name,
            scopes,
            value: Rc::new(value),
        }
    }

    fn expr_begin(&mut self, exprs: Vec<Self::Expr>) -> Self::Expr {
        CoreExpr::Begin(exprs)
    }

    fn expr_import(&mut self, import_sets: Vec<Self::Datum>) -> Self::Expr {
        let values: Vec<Value> = import_sets.into_iter().map(|d| (*d).clone()).collect();
        CoreExpr::Import { import_sets: values }
    }
}
```

### VM Backend

```rust
/// Backend for VM.
/// Produces `VmCoreExpr` with `TaggedValue` datums.
pub struct VmBackend<'a> {
    heap: &'a mut VmHeap,
}

impl<'a> VmBackend<'a> {
    pub fn new(heap: &'a mut VmHeap) -> Self {
        Self { heap }
    }
}

impl DesugarBackend for VmBackend<'_> {
    type Datum = TaggedValue;
    type Expr = VmCoreExpr;

    // Datum construction - create TaggedValue
    fn datum_integer(&mut self, n: i64) -> Self::Datum {
        if TaggedValue::fits_fixnum(n) {
            TaggedValue::fixnum(n)
        } else {
            self.heap.alloc_bigint(BigInt::from(n))
        }
    }

    fn datum_bigint(&mut self, n: &BigInt) -> Self::Datum {
        self.heap.alloc_bigint(n.clone())
    }

    fn datum_rational(&mut self, r: &BigRational) -> Self::Datum {
        self.heap.alloc_rational(r.clone())
    }

    fn datum_float(&mut self, f: f64) -> Self::Datum {
        TaggedValue::float(f)
    }

    fn datum_boolean(&mut self, b: bool) -> Self::Datum {
        if b { TaggedValue::TRUE } else { TaggedValue::FALSE }
    }

    fn datum_character(&mut self, c: char) -> Self::Datum {
        TaggedValue::character(c)
    }

    fn datum_string(&mut self, s: &str) -> Self::Datum {
        self.heap.alloc_string(s.to_string())
    }

    fn datum_symbol(&mut self, s: &Symbol) -> Self::Datum {
        self.heap.intern_symbol(s)
    }

    fn datum_null(&mut self) -> Self::Datum {
        TaggedValue::NULL
    }

    fn datum_pair(&mut self, car: Self::Datum, cdr: Self::Datum) -> Self::Datum {
        self.heap.alloc_pair(car, cdr)
    }

    fn datum_vector(&mut self, elements: Vec<Self::Datum>) -> Self::Datum {
        self.heap.alloc_vector(elements)
    }

    fn datum_bytevector(&mut self, bytes: &[u8]) -> Self::Datum {
        self.heap.alloc_bytevector(bytes.to_vec())
    }

    // Expression construction
    fn expr_literal(&mut self, datum: Self::Datum) -> Self::Expr {
        VmCoreExpr::Literal(datum)
    }

    fn expr_quote(&mut self, datum: Self::Datum) -> Self::Expr {
        VmCoreExpr::Quote(datum)
    }

    fn expr_var(&mut self, name: Symbol, scopes: ScopeSet) -> Self::Expr {
        VmCoreExpr::Var { name, scopes }
    }

    fn expr_if(
        &mut self,
        test: Self::Expr,
        consequent: Self::Expr,
        alternate: Self::Expr,
    ) -> Self::Expr {
        VmCoreExpr::If {
            test: Box::new(test),
            then: Box::new(consequent),
            else_: Box::new(alternate),
        }
    }

    fn expr_lambda(
        &mut self,
        params: Formals,
        body: Vec<Self::Expr>,
        binding_scope: Option<ScopeId>,
    ) -> Self::Expr {
        VmCoreExpr::Lambda {
            params,
            body,
            binding_scope,
        }
    }

    fn expr_app(&mut self, func: Self::Expr, args: Vec<Self::Expr>) -> Self::Expr {
        VmCoreExpr::App {
            func: Box::new(func),
            args,
        }
    }

    fn expr_define(&mut self, name: Symbol, value: Self::Expr) -> Self::Expr {
        VmCoreExpr::Define {
            name,
            value: Box::new(value),
        }
    }

    fn expr_set(
        &mut self,
        name: Symbol,
        scopes: ScopeSet,
        value: Self::Expr,
    ) -> Self::Expr {
        VmCoreExpr::Set {
            var: name,
            scopes,
            value: Box::new(value),
        }
    }

    fn expr_begin(&mut self, exprs: Vec<Self::Expr>) -> Self::Expr {
        VmCoreExpr::Begin(exprs)
    }

    fn expr_import(&mut self, import_sets: Vec<Self::Datum>) -> Self::Expr {
        VmCoreExpr::Import { import_sets }
    }
}
```

---

## Usage

### Tree-Walker

```rust
use patina_frontend::desugarer::{Desugarer, TreeWalkerBackend};

let backend = TreeWalkerBackend;
let mut desugarer = Desugarer::new(backend, &macro_env);
let core_expr: CoreExpr = desugarer.desugar(&value)?;
```

### VM

```rust
use patina_vm::desugarer::{Desugarer, VmBackend};

let mut heap = VmHeap::new();
let backend = VmBackend::new(&mut heap);
let mut desugarer = Desugarer::new(backend, &macro_env);
let vm_expr: VmCoreExpr = desugarer.desugar(&value)?;
```

---

## Module Organization

```
patina-core/
├── desugarer/
│   ├── mod.rs              # Re-exports
│   ├── trait.rs            # DesugarBackend trait
│   ├── generic.rs          # Desugarer<B> implementation
│   ├── formals.rs          # Formals parsing (shared)
│   └── error.rs            # DesugarError

patina-frontend/
├── desugarer/
│   ├── mod.rs              # Re-exports Desugarer + TreeWalkerBackend
│   └── tree_walker.rs      # TreeWalkerBackend implementation

patina-vm/
├── desugarer/
│   ├── mod.rs              # Re-exports Desugarer + VmBackend
│   └── vm_backend.rs       # VmBackend implementation
```

---

## Advantages

1. **Single implementation** - Desugaring logic written once
2. **Type-safe** - Compiler enforces correct types for each backend
3. **Zero-cost** - Monomorphized at compile time, no runtime dispatch
4. **Extensible** - Add new backends by implementing `DesugarBackend`
5. **Testable** - Can test desugaring logic once, backends separately

---

## Potential Extensions

### Quasiquote Support

Add methods for quasiquote handling:

```rust
trait DesugarBackend {
    // ... existing methods ...

    /// Create an unquote marker (for quasiquote processing)
    fn expr_unquote(&mut self, expr: Self::Expr) -> Self::Expr;

    /// Create an unquote-splicing marker
    fn expr_unquote_splicing(&mut self, expr: Self::Expr) -> Self::Expr;
}
```

### Source Location Tracking

Add source location to expressions:

```rust
trait DesugarBackend {
    // ... existing methods ...

    /// Attach source location to an expression
    fn with_source_loc(&mut self, expr: Self::Expr, loc: SourceLoc) -> Self::Expr;
}
```

### Debug Backend

For debugging/testing, create a backend that produces a human-readable form:

```rust
pub struct DebugBackend;

impl DesugarBackend for DebugBackend {
    type Datum = String;
    type Expr = String;

    fn datum_integer(&mut self, n: i64) -> Self::Datum {
        n.to_string()
    }

    fn expr_if(&mut self, test: Self::Expr, then: Self::Expr, else_: Self::Expr) -> Self::Expr {
        format!("(if {} {} {})", test, then, else_)
    }
    // ...
}
```

---

## Migration Path

### Phase 1: Extract Trait

1. Define `DesugarBackend` trait in `patina-core`
2. Keep existing desugarer working

### Phase 2: Implement Generic Desugarer

1. Create `Desugarer<B>` generic type
2. Move logic from existing desugarer
3. Implement `TreeWalkerBackend`
4. Verify all tests pass

### Phase 3: Add VM Backend

1. Implement `VmBackend`
2. Create `VmCoreExpr` type
3. Test with VM

---

## References

- Current desugarer: `patina-frontend/src/desugarer/mod.rs`
- CoreExpr: `patina-core/src/core_expr.rs`
- [SEXPR_SEPARATION_ARCHITECTURE.md](./SEXPR_SEPARATION_ARCHITECTURE.md) - Overall architecture
- [VM_SPECIFICATION.md](./VM_SPECIFICATION.md) - VM design
