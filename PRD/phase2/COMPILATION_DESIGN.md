# Bytecode Compilation Design

**Status:** Design Document
**Created:** 2025-12-13
**Related:** [DESUGARER_DESIGN.md](./DESUGARER_DESIGN.md), [VM_SPECIFICATION.md](./VM_SPECIFICATION.md)

---

## Overview

This document describes how to compile `VmCoreExpr` (the desugared IR) into bytecode for the VM. The compilation pipeline is:

```
Value (from parser)
    |
    v
VmCoreExpr (from desugarer)
    |
    v
Bytecode (this document)
    |
    v
VM Execution
```

---

## Table of Contents

1. [Compiler Architecture](#compiler-architecture)
2. [CoreExpr to Bytecode Translation](#coreexpr-to-bytecode-translation)
3. [Register Allocation](#register-allocation)
4. [Closure Compilation](#closure-compilation)
5. [Control Flow Compilation](#control-flow-compilation)
6. [Continuation Support](#continuation-support)
7. [Constant Pool Management](#constant-pool-management)
8. [Code Objects](#code-objects)
9. [Optimization Opportunities](#optimization-opportunities)
10. [Implementation Plan](#implementation-plan)

---

## Compiler Architecture

### Component Diagram

```
                    VmCoreExpr
                        |
                        v
            +------------------------+
            |       Compiler         |
            |------------------------|
            | - Environment tracker  |
            | - Register allocator   |
            | - Label generator      |
            | - Constant pool        |
            +------------------------+
                        |
                        v
            +------------------------+
            |      CodeObject        |
            |------------------------|
            | - instructions: Vec    |
            | - constants: Vec       |
            | - num_registers: u16   |
            | - source_map: SourceMap|
            +------------------------+
```

### Core Data Structures

```rust
/// The bytecode compiler.
pub struct Compiler {
    /// Constants referenced by the code.
    constants: ConstantPool,

    /// Generated instructions.
    instructions: Vec<Opcode>,

    /// Current register allocation state.
    register_allocator: RegisterAllocator,

    /// Label generator for jumps.
    label_gen: LabelGenerator,

    /// Maps label IDs to instruction indices.
    labels: HashMap<LabelId, usize>,

    /// Forward references to labels (to be patched).
    forward_refs: Vec<ForwardRef>,

    /// Environment mapping: variable name -> register or environment slot.
    env_tracker: EnvironmentTracker,

    /// Current scope depth (for closure capture analysis).
    scope_depth: usize,

    /// Collected free variables for current lambda.
    free_vars: Vec<Symbol>,
}

/// Forward reference to a label that needs patching.
struct ForwardRef {
    instruction_index: usize,
    label_id: LabelId,
}

/// Tracks variable locations during compilation.
struct EnvironmentTracker {
    /// Stack of scopes, each mapping names to locations.
    scopes: Vec<HashMap<Symbol, VarLocation>>,
}

/// Where a variable is located at runtime.
#[derive(Clone)]
enum VarLocation {
    /// In a register (local variable or parameter).
    Register(Register),

    /// In the closure's environment (captured variable).
    Closure(usize),

    /// Global variable (looked up by name at runtime).
    Global(Symbol),
}
```

---

## CoreExpr to Bytecode Translation

### Translation Functions

The core translation is a recursive function over `VmCoreExpr`:

```rust
impl Compiler {
    /// Compile an expression, placing result in `dst` register.
    /// Returns the register containing the result.
    pub fn compile_expr(
        &mut self,
        expr: &VmCoreExpr,
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        match expr {
            VmCoreExpr::Literal(value) => self.compile_literal(value, dst),
            VmCoreExpr::Quote(value) => self.compile_quote(value, dst),
            VmCoreExpr::Var { name, scopes } => self.compile_var(name, scopes, dst),
            VmCoreExpr::If { test, then, else_ } => {
                self.compile_if(test, then, else_, dst)
            }
            VmCoreExpr::Lambda { params, body, binding_scope } => {
                self.compile_lambda(params, body, *binding_scope, dst)
            }
            VmCoreExpr::App { func, args } => self.compile_app(func, args, dst),
            VmCoreExpr::Define { name, value } => self.compile_define(name, value, dst),
            VmCoreExpr::Set { var, scopes, value } => {
                self.compile_set(var, scopes, value, dst)
            }
            VmCoreExpr::Begin(exprs) => self.compile_begin(exprs, dst),
            VmCoreExpr::Import { import_sets } => self.compile_import(import_sets, dst),
        }
    }
}
```

### Literal Compilation

```rust
impl Compiler {
    fn compile_literal(
        &mut self,
        value: &TaggedValue,
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        let dst = dst.unwrap_or_else(|| self.register_allocator.alloc());

        // Check if value can be immediate (fits in instruction)
        if value.is_immediate() {
            self.emit(Opcode::LoadImmediate { dst, value: *value });
        } else {
            // Add to constant pool
            let const_idx = self.constants.add(*value);
            self.emit(Opcode::LoadConst { dst, constant_index: const_idx });
        }

        Ok(dst)
    }

    fn compile_quote(
        &mut self,
        value: &TaggedValue,
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        // Quoted values are always loaded from constant pool
        // (they may be compound structures)
        let dst = dst.unwrap_or_else(|| self.register_allocator.alloc());
        let const_idx = self.constants.add(*value);
        self.emit(Opcode::LoadConst { dst, constant_index: const_idx });
        Ok(dst)
    }
}
```

### Variable Access

```rust
impl Compiler {
    fn compile_var(
        &mut self,
        name: &Symbol,
        scopes: &ScopeSet,
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        let dst = dst.unwrap_or_else(|| self.register_allocator.alloc());

        // Look up variable location
        match self.env_tracker.lookup(name) {
            Some(VarLocation::Register(reg)) => {
                if dst != reg {
                    self.emit(Opcode::Move { dst, src: reg });
                }
            }
            Some(VarLocation::Closure(slot)) => {
                self.emit(Opcode::LoadClosure { dst, slot: slot as u16 });
            }
            Some(VarLocation::Global(sym)) => {
                self.emit(Opcode::LoadGlobal { dst, name: sym.clone() });
            }
            None => {
                // Assume global if not found in local scope
                self.emit(Opcode::LoadGlobal { dst, name: name.clone() });
            }
        }

        Ok(dst)
    }

    fn compile_set(
        &mut self,
        name: &Symbol,
        scopes: &ScopeSet,
        value: &VmCoreExpr,
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        // Compile value
        let value_reg = self.compile_expr(value, None)?;

        // Store to variable location
        match self.env_tracker.lookup(name) {
            Some(VarLocation::Register(reg)) => {
                self.emit(Opcode::Move { dst: reg, src: value_reg });
            }
            Some(VarLocation::Closure(slot)) => {
                self.emit(Opcode::StoreClosure { src: value_reg, slot: slot as u16 });
            }
            Some(VarLocation::Global(sym)) | None => {
                self.emit(Opcode::StoreGlobal { src: value_reg, name: name.clone() });
            }
        }

        // set! returns unspecified
        let dst = dst.unwrap_or_else(|| self.register_allocator.alloc());
        self.emit(Opcode::LoadImmediate { dst, value: TaggedValue::UNSPECIFIED });

        self.register_allocator.free(value_reg);
        Ok(dst)
    }

    fn compile_define(
        &mut self,
        name: &Symbol,
        value: &VmCoreExpr,
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        // Compile value
        let value_reg = self.compile_expr(value, None)?;

        // Store to global (define always creates global binding)
        self.emit(Opcode::StoreGlobal { src: value_reg, name: name.clone() });

        // define returns unspecified
        let dst = dst.unwrap_or_else(|| self.register_allocator.alloc());
        self.emit(Opcode::LoadImmediate { dst, value: TaggedValue::UNSPECIFIED });

        self.register_allocator.free(value_reg);
        Ok(dst)
    }
}
```

### Conditional Compilation

```rust
impl Compiler {
    fn compile_if(
        &mut self,
        test: &VmCoreExpr,
        then: &VmCoreExpr,
        else_: &VmCoreExpr,
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        let dst = dst.unwrap_or_else(|| self.register_allocator.alloc());

        // Compile test
        let test_reg = self.compile_expr(test, None)?;

        // Generate labels
        let else_label = self.label_gen.next();
        let end_label = self.label_gen.next();

        // Branch to else if test is false
        self.emit_forward_jump(Opcode::JumpUnless {
            condition: test_reg,
            target: 0, // Will be patched
        }, else_label);

        self.register_allocator.free(test_reg);

        // Compile then branch
        let then_reg = self.compile_expr(then, Some(dst))?;

        // Jump over else branch
        self.emit_forward_jump(Opcode::Jump { target: 0 }, end_label);

        // Else branch
        self.place_label(else_label);
        let else_reg = self.compile_expr(else_, Some(dst))?;

        // End
        self.place_label(end_label);

        Ok(dst)
    }
}
```

### Begin (Sequence) Compilation

```rust
impl Compiler {
    fn compile_begin(
        &mut self,
        exprs: &[VmCoreExpr],
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        if exprs.is_empty() {
            let dst = dst.unwrap_or_else(|| self.register_allocator.alloc());
            self.emit(Opcode::LoadImmediate { dst, value: TaggedValue::UNSPECIFIED });
            return Ok(dst);
        }

        // Compile all but last expression, discarding results
        for expr in &exprs[..exprs.len() - 1] {
            let reg = self.compile_expr(expr, None)?;
            self.register_allocator.free(reg);
        }

        // Compile last expression into destination
        self.compile_expr(&exprs[exprs.len() - 1], dst)
    }
}
```

### Application Compilation

```rust
impl Compiler {
    fn compile_app(
        &mut self,
        func: &VmCoreExpr,
        args: &[VmCoreExpr],
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        let dst = dst.unwrap_or_else(|| self.register_allocator.alloc());

        // Compile function
        let func_reg = self.compile_expr(func, None)?;

        // Compile arguments into contiguous registers
        let args_base = self.register_allocator.alloc_range(args.len() as u16);
        for (i, arg) in args.iter().enumerate() {
            let arg_reg = Register(args_base.0 + i as u16);
            self.compile_expr(arg, Some(arg_reg))?;
        }

        // Emit call instruction
        self.emit(Opcode::Call {
            func: func_reg,
            args: RegisterSlice {
                base: args_base,
                count: args.len() as u16,
            },
            dst,
            profile_id: self.profile_id_gen.next(),
        });

        // Free registers
        self.register_allocator.free(func_reg);
        self.register_allocator.free_range(args_base, args.len() as u16);

        Ok(dst)
    }

    /// Compile a tail call (used in tail position).
    fn compile_tail_call(
        &mut self,
        func: &VmCoreExpr,
        args: &[VmCoreExpr],
    ) -> Result<(), CompileError> {
        // Compile function
        let func_reg = self.compile_expr(func, None)?;

        // Compile arguments into contiguous registers
        let args_base = self.register_allocator.alloc_range(args.len() as u16);
        for (i, arg) in args.iter().enumerate() {
            let arg_reg = Register(args_base.0 + i as u16);
            self.compile_expr(arg, Some(arg_reg))?;
        }

        // Emit tail call (no dst - doesn't return here)
        self.emit(Opcode::TailCall {
            func: func_reg,
            args: RegisterSlice {
                base: args_base,
                count: args.len() as u16,
            },
        });

        Ok(())
    }
}
```

---

## Register Allocation

### Simple Linear Scan

For the initial implementation, use a simple linear allocator:

```rust
/// Simple register allocator.
pub struct RegisterAllocator {
    /// Next available register.
    next: u16,

    /// Stack of freed registers (for reuse).
    free_stack: Vec<Register>,

    /// High water mark (maximum registers used).
    high_water: u16,
}

impl RegisterAllocator {
    pub fn new() -> Self {
        Self {
            next: 0,
            free_stack: Vec::new(),
            high_water: 0,
        }
    }

    /// Allocate a single register.
    pub fn alloc(&mut self) -> Register {
        if let Some(reg) = self.free_stack.pop() {
            reg
        } else {
            let reg = Register(self.next);
            self.next += 1;
            self.high_water = self.high_water.max(self.next);
            reg
        }
    }

    /// Allocate a contiguous range of registers.
    pub fn alloc_range(&mut self, count: u16) -> Register {
        // Can't reuse freed registers for ranges (must be contiguous)
        let base = Register(self.next);
        self.next += count;
        self.high_water = self.high_water.max(self.next);
        base
    }

    /// Free a register for reuse.
    pub fn free(&mut self, reg: Register) {
        self.free_stack.push(reg);
    }

    /// Free a range of registers.
    pub fn free_range(&mut self, base: Register, count: u16) {
        for i in 0..count {
            self.free_stack.push(Register(base.0 + i));
        }
    }

    /// Get the number of registers needed for this function.
    pub fn num_registers(&self) -> u16 {
        self.high_water
    }
}
```

### Future: Graph Coloring

For better register utilization, implement graph coloring:

```rust
/// Future: Graph-based register allocator.
pub struct GraphRegisterAllocator {
    /// Interference graph: which registers can't share the same physical register.
    interference: Graph<Register>,

    /// Liveness analysis results.
    liveness: HashMap<InstructionIndex, HashSet<Register>>,
}
```

---

## Closure Compilation

### Free Variable Analysis

Before compiling a lambda, analyze which variables are free:

```rust
impl Compiler {
    /// Find free variables in an expression.
    fn analyze_free_vars(
        &self,
        expr: &VmCoreExpr,
        bound: &HashSet<Symbol>,
    ) -> HashSet<Symbol> {
        match expr {
            VmCoreExpr::Var { name, .. } => {
                if bound.contains(name) {
                    HashSet::new()
                } else {
                    let mut free = HashSet::new();
                    free.insert(name.clone());
                    free
                }
            }
            VmCoreExpr::Lambda { params, body, .. } => {
                let mut new_bound = bound.clone();
                for param in params.names() {
                    new_bound.insert(param.clone());
                }

                let mut free = HashSet::new();
                for expr in body {
                    free.extend(self.analyze_free_vars(expr, &new_bound));
                }
                free
            }
            VmCoreExpr::If { test, then, else_ } => {
                let mut free = self.analyze_free_vars(test, bound);
                free.extend(self.analyze_free_vars(then, bound));
                free.extend(self.analyze_free_vars(else_, bound));
                free
            }
            VmCoreExpr::App { func, args } => {
                let mut free = self.analyze_free_vars(func, bound);
                for arg in args {
                    free.extend(self.analyze_free_vars(arg, bound));
                }
                free
            }
            VmCoreExpr::Begin(exprs) => {
                let mut free = HashSet::new();
                for expr in exprs {
                    free.extend(self.analyze_free_vars(expr, bound));
                }
                free
            }
            VmCoreExpr::Set { var, value, .. } => {
                let mut free = HashSet::new();
                if !bound.contains(var) {
                    free.insert(var.clone());
                }
                free.extend(self.analyze_free_vars(value, bound));
                free
            }
            VmCoreExpr::Define { value, .. } => {
                self.analyze_free_vars(value, bound)
            }
            VmCoreExpr::Literal(_) | VmCoreExpr::Quote(_) | VmCoreExpr::Import { .. } => {
                HashSet::new()
            }
        }
    }
}
```

### Lambda Compilation

```rust
impl Compiler {
    fn compile_lambda(
        &mut self,
        params: &Formals,
        body: &[VmCoreExpr],
        binding_scope: Option<ScopeId>,
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        let dst = dst.unwrap_or_else(|| self.register_allocator.alloc());

        // Analyze free variables
        let bound: HashSet<_> = params.names().into_iter().cloned().collect();
        let mut free_vars: HashSet<Symbol> = HashSet::new();
        for expr in body {
            free_vars.extend(self.analyze_free_vars(expr, &bound));
        }

        // Filter out globals (only capture locals)
        let free_vars: Vec<_> = free_vars
            .into_iter()
            .filter(|v| self.env_tracker.is_local(v))
            .collect();

        // Compile lambda body as separate CodeObject
        let code_id = self.compile_lambda_body(params, body, &free_vars)?;

        // Load free variables into registers
        let free_var_regs = self.register_allocator.alloc_range(free_vars.len() as u16);
        for (i, var) in free_vars.iter().enumerate() {
            let reg = Register(free_var_regs.0 + i as u16);
            self.compile_var(var, &ScopeSet::new(), Some(reg))?;
        }

        // Create closure
        self.emit(Opcode::MakeClosure {
            dst,
            code: code_id,
            free_vars: RegisterSlice {
                base: free_var_regs,
                count: free_vars.len() as u16,
            },
        });

        self.register_allocator.free_range(free_var_regs, free_vars.len() as u16);

        Ok(dst)
    }

    fn compile_lambda_body(
        &mut self,
        params: &Formals,
        body: &[VmCoreExpr],
        free_vars: &[Symbol],
    ) -> Result<CodeObjectId, CompileError> {
        // Create sub-compiler for lambda body
        let mut sub_compiler = Compiler::new();

        // Set up environment tracker with parameters
        for (i, param) in params.names().iter().enumerate() {
            sub_compiler.env_tracker.define_local(param.clone(), Register(i as u16));
        }

        // Set up closure variables
        for (i, var) in free_vars.iter().enumerate() {
            sub_compiler.env_tracker.define_closure(var.clone(), i);
        }

        // Compile body
        let result_reg = sub_compiler.compile_begin_in_tail_position(body)?;

        // Emit return
        sub_compiler.emit(Opcode::Return { value: result_reg });

        // Build CodeObject
        let code_object = sub_compiler.build_code_object(
            params.clone(),
            free_vars.len(),
        );

        // Register in constant pool
        Ok(self.constants.add_code_object(code_object))
    }
}
```

---

## Control Flow Compilation

### Tail Position Detection

Track whether we're compiling in tail position for TCO:

```rust
impl Compiler {
    /// Compile expression in tail position (may emit tail call).
    fn compile_tail_expr(&mut self, expr: &VmCoreExpr) -> Result<Register, CompileError> {
        match expr {
            // Tail call optimization
            VmCoreExpr::App { func, args } => {
                self.compile_tail_call(func, args)?;
                // Return a dummy register (won't be used)
                Ok(Register(0))
            }

            // If in tail position: both branches are tail
            VmCoreExpr::If { test, then, else_ } => {
                let test_reg = self.compile_expr(test, None)?;

                let else_label = self.label_gen.next();
                self.emit_forward_jump(Opcode::JumpUnless {
                    condition: test_reg,
                    target: 0,
                }, else_label);

                self.register_allocator.free(test_reg);

                // Then branch in tail position
                self.compile_tail_expr(then)?;

                // Else branch in tail position
                self.place_label(else_label);
                self.compile_tail_expr(else_)
            }

            // Begin: last expression is tail
            VmCoreExpr::Begin(exprs) => {
                if exprs.is_empty() {
                    let reg = self.register_allocator.alloc();
                    self.emit(Opcode::LoadImmediate {
                        dst: reg,
                        value: TaggedValue::UNSPECIFIED
                    });
                    return Ok(reg);
                }

                for expr in &exprs[..exprs.len() - 1] {
                    let reg = self.compile_expr(expr, None)?;
                    self.register_allocator.free(reg);
                }

                self.compile_tail_expr(&exprs[exprs.len() - 1])
            }

            // Other expressions: compile normally
            _ => self.compile_expr(expr, None),
        }
    }

    fn compile_begin_in_tail_position(
        &mut self,
        exprs: &[VmCoreExpr],
    ) -> Result<Register, CompileError> {
        if exprs.is_empty() {
            let reg = self.register_allocator.alloc();
            self.emit(Opcode::LoadImmediate {
                dst: reg,
                value: TaggedValue::UNSPECIFIED
            });
            return Ok(reg);
        }

        for expr in &exprs[..exprs.len() - 1] {
            let reg = self.compile_expr(expr, None)?;
            self.register_allocator.free(reg);
        }

        self.compile_tail_expr(&exprs[exprs.len() - 1])
    }
}
```

### Loop Compilation (for `do`)

The `do` macro expands to named-let, which becomes a letrec with self-reference:

```scheme
(do ((i 0 (+ i 1)))
    ((= i 10) i)
  (display i))

;; Expands to approximately:
(letrec ((loop (lambda (i)
                 (if (= i 10)
                     i
                     (begin
                       (display i)
                       (loop (+ i 1)))))))
  (loop 0))
```

The compiler recognizes self-tail-calls and compiles to `JumpBack`:

```rust
impl Compiler {
    fn compile_self_tail_call(
        &mut self,
        target_label: LabelId,
        args: &[VmCoreExpr],
        param_regs: &[Register],
    ) -> Result<(), CompileError> {
        // Compile arguments
        let temp_regs: Vec<_> = args.iter()
            .map(|arg| self.compile_expr(arg, None))
            .collect::<Result<_, _>>()?;

        // Move to parameter registers
        for (dst, src) in param_regs.iter().zip(temp_regs.iter()) {
            if dst != src {
                self.emit(Opcode::Move { dst: *dst, src: *src });
            }
        }

        // Free temp registers
        for reg in temp_regs {
            self.register_allocator.free(reg);
        }

        // Jump back to loop start
        let loop_id = self.loop_id_gen.next();
        self.emit(Opcode::JumpBack {
            target: self.labels[&target_label],
            loop_id,
        });

        Ok(())
    }
}
```

---

## Continuation Support

### call/cc Compilation

```rust
impl Compiler {
    fn compile_callcc(
        &mut self,
        func_expr: &VmCoreExpr,
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        let dst = dst.unwrap_or_else(|| self.register_allocator.alloc());

        // Compile the function
        let func_reg = self.compile_expr(func_expr, None)?;

        // Emit call/cc instruction
        self.emit(Opcode::CallCC {
            func: func_reg,
            dst,
        });

        self.register_allocator.free(func_reg);

        Ok(dst)
    }
}
```

### Delimited Continuations

```rust
impl Compiler {
    fn compile_reset(
        &mut self,
        tag: PromptTag,
        body: &VmCoreExpr,
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        let dst = dst.unwrap_or_else(|| self.register_allocator.alloc());

        let body_label = self.label_gen.next();
        let end_label = self.label_gen.next();

        // Establish prompt
        self.emit_forward_jump(Opcode::Reset {
            prompt_tag: tag,
            body: 0, // Patched
            cleanup: None,
        }, body_label);

        // Jump past body (Reset sets up exception-like handler)
        self.emit_forward_jump(Opcode::Jump { target: 0 }, end_label);

        // Body
        self.place_label(body_label);
        self.compile_expr(body, Some(dst))?;

        // End
        self.place_label(end_label);

        Ok(dst)
    }

    fn compile_shift(
        &mut self,
        tag: PromptTag,
        k_var: Symbol,
        body: &VmCoreExpr,
        dst: Option<Register>,
    ) -> Result<Register, CompileError> {
        let dst = dst.unwrap_or_else(|| self.register_allocator.alloc());
        let k_reg = self.register_allocator.alloc();

        // Bind continuation variable
        self.env_tracker.define_local(k_var, k_reg);

        let body_label = self.label_gen.next();

        // Capture continuation up to prompt
        self.emit_forward_jump(Opcode::Shift {
            prompt_tag: tag,
            continuation_var: k_reg,
            body: 0, // Patched
        }, body_label);

        // Body (with k bound)
        self.place_label(body_label);
        self.compile_expr(body, Some(dst))?;

        Ok(dst)
    }
}
```

---

## Constant Pool Management

```rust
/// Pool of constants referenced by bytecode.
pub struct ConstantPool {
    /// Values (literals, quoted data).
    values: Vec<TaggedValue>,

    /// Code objects (compiled lambdas).
    code_objects: Vec<CodeObject>,

    /// Deduplication map for values.
    value_indices: HashMap<TaggedValue, ConstantIndex>,
}

impl ConstantPool {
    pub fn new() -> Self {
        Self {
            values: Vec::new(),
            code_objects: Vec::new(),
            value_indices: HashMap::new(),
        }
    }

    /// Add a value, returning its index. Deduplicates.
    pub fn add(&mut self, value: TaggedValue) -> ConstantIndex {
        if let Some(&idx) = self.value_indices.get(&value) {
            return idx;
        }

        let idx = ConstantIndex(self.values.len() as u32);
        self.values.push(value);
        self.value_indices.insert(value, idx);
        idx
    }

    /// Add a code object, returning its ID.
    pub fn add_code_object(&mut self, code: CodeObject) -> CodeObjectId {
        let id = CodeObjectId(self.code_objects.len() as u32);
        self.code_objects.push(code);
        id
    }
}
```

---

## Code Objects

```rust
/// A compiled unit of code (function body).
pub struct CodeObject {
    /// Unique identifier.
    pub id: CodeObjectId,

    /// Function name (for debugging).
    pub name: Option<Symbol>,

    /// Bytecode instructions.
    pub instructions: Vec<Opcode>,

    /// Constant pool.
    pub constants: ConstantPool,

    /// Number of registers needed.
    pub num_registers: u16,

    /// Parameter information.
    pub params: Formals,

    /// Number of closure slots.
    pub num_closure_slots: usize,

    /// Source location mapping.
    pub source_map: SourceMap,
}

impl Compiler {
    pub fn build_code_object(
        self,
        params: Formals,
        num_closure_slots: usize,
    ) -> CodeObject {
        // Patch forward references
        let mut instructions = self.instructions;
        for fwd_ref in &self.forward_refs {
            let target = self.labels[&fwd_ref.label_id];
            patch_jump(&mut instructions[fwd_ref.instruction_index], target);
        }

        CodeObject {
            id: CodeObjectId(0), // Will be assigned when added to constant pool
            name: None,
            instructions,
            constants: self.constants,
            num_registers: self.register_allocator.num_registers(),
            params,
            num_closure_slots,
            source_map: SourceMap::new(),
        }
    }
}
```

---

## Optimization Opportunities

### Peephole Optimizations

Apply after initial code generation:

```rust
pub fn peephole_optimize(instructions: &mut Vec<Opcode>) {
    let mut i = 0;
    while i < instructions.len() {
        // Eliminate redundant moves
        if let Opcode::Move { dst, src } = instructions[i] {
            if dst == src {
                instructions.remove(i);
                continue;
            }
        }

        // Combine LoadImmediate + Jump into conditional
        if i + 1 < instructions.len() {
            if let (
                Opcode::LoadImmediate { dst, value },
                Opcode::JumpIf { condition, target }
            ) = (&instructions[i], &instructions[i + 1]) {
                if dst == condition && value == &TaggedValue::TRUE {
                    instructions[i] = Opcode::Jump { target: *target };
                    instructions.remove(i + 1);
                    continue;
                }
            }
        }

        i += 1;
    }
}
```

### Future: SSA Form

For more advanced optimizations:

```rust
/// Future: SSA-form intermediate representation.
pub struct SsaFunction {
    blocks: Vec<BasicBlock>,
    entry: BlockId,
}

pub struct BasicBlock {
    phi_nodes: Vec<PhiNode>,
    instructions: Vec<SsaInstruction>,
    terminator: Terminator,
}
```

---

## Implementation Plan

### Phase 1: Basic Compiler (2-3 weeks)

1. **Core data structures**
   - `Compiler` struct
   - `RegisterAllocator` (simple linear)
   - `ConstantPool`
   - `CodeObject`

2. **Expression compilation**
   - Literals, variables
   - `if`, `begin`
   - Function application (non-tail)

3. **Basic testing**
   - Compile simple expressions
   - Verify bytecode correctness

### Phase 2: Functions & Closures (2-3 weeks)

1. **Lambda compilation**
   - Free variable analysis
   - `MakeClosure` generation
   - Nested functions

2. **Tail call optimization**
   - Tail position detection
   - `TailCall` instruction

3. **Define handling**
   - Global definitions
   - Function definitions

### Phase 3: Advanced Features (2-3 weeks)

1. **Continuation support**
   - `call/cc` compilation
   - `Reset`/`Shift` for delimited continuations

2. **Loop optimization**
   - Self-tail-call detection
   - `JumpBack` for hot loops

3. **Peephole optimization**
   - Basic patterns
   - Dead code elimination

### Phase 4: Polish (1-2 weeks)

1. **Source maps**
   - Track source locations
   - Error messages with locations

2. **Debug support**
   - Breakpoint insertion
   - Variable inspection

3. **Benchmarking**
   - Compare with tree-walker
   - Profile hot spots

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_compile_literal() {
    let mut compiler = Compiler::new();
    let expr = VmCoreExpr::Literal(TaggedValue::fixnum(42));
    let reg = compiler.compile_expr(&expr, None).unwrap();

    assert_eq!(compiler.instructions.len(), 1);
    assert!(matches!(
        compiler.instructions[0],
        Opcode::LoadImmediate { dst: _, value } if value == TaggedValue::fixnum(42)
    ));
}

#[test]
fn test_compile_if() {
    let mut compiler = Compiler::new();
    let expr = VmCoreExpr::If {
        test: Box::new(VmCoreExpr::Literal(TaggedValue::TRUE)),
        then: Box::new(VmCoreExpr::Literal(TaggedValue::fixnum(1))),
        else_: Box::new(VmCoreExpr::Literal(TaggedValue::fixnum(2))),
    };

    let reg = compiler.compile_expr(&expr, None).unwrap();

    // Should have: LoadImmediate, JumpUnless, LoadImmediate, Jump, LoadImmediate
    assert!(compiler.instructions.len() >= 5);
}
```

### Integration Tests

```rust
#[test]
fn test_compile_and_run_factorial() {
    let code = "(define (fact n) (if (= n 0) 1 (* n (fact (- n 1))))) (fact 5)";
    let result = compile_and_run(code).unwrap();
    assert_eq!(result, TaggedValue::fixnum(120));
}

#[test]
fn test_tail_call_optimization() {
    // This should not overflow the stack
    let code = "
        (define (loop n acc)
          (if (= n 0)
              acc
              (loop (- n 1) (+ acc 1))))
        (loop 1000000 0)
    ";
    let result = compile_and_run(code).unwrap();
    assert_eq!(result, TaggedValue::fixnum(1000000));
}
```

---

## References

- [VM_SPECIFICATION.md](./VM_SPECIFICATION.md) - Bytecode ISA definition
- [DESUGARER_DESIGN.md](./DESUGARER_DESIGN.md) - Input IR (VmCoreExpr)
- [TAGGED_POINTERS.md](./TAGGED_POINTERS.md) - TaggedValue representation
- "Compiling with Continuations" (Appel) - Continuation compilation
- "Modern Compiler Implementation" (Appel) - Register allocation, SSA
