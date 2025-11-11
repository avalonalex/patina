# Debugger and Runtime Hook System Design

**Status:** Research/Design phase
**Priority:** MEDIUM-HIGH (enables sophisticated debugging)
**Estimated Effort:** 2-3 weeks (phased approach)
**Last Updated:** 2025-11-09

---

## Overview

As Patina evolves toward a sophisticated runtime (TCO, bytecode VM, continuations), we need a well-designed hook system to support debugging, tracing, profiling, and introspection. This document researches debugger architectures and proposes a phased implementation.

**Current State:** We have basic trace debugging via `DebugConfig` (eval/apply/env/expand stages with indentation).

**Goal:** Build a comprehensive hook system that supports:
1. Interactive debugging (breakpoints, stepping, inspection)
2. Tracing and profiling
3. Future runtime models (TCO, bytecode, continuations)
4. REPL integration

---

## Current Debug Infrastructure

### What We Have (src/eval/debug.rs)

```rust
pub struct DebugConfig {
    enabled_stages: RefCell<HashSet<DebugStage>>,
    indent_level: RefCell<usize>,
}

pub enum DebugStage {
    Lex, Parse, Eval, Apply, Env, Expand
}
```

**Primitives available in Scheme:**
- `(debug-enable 'eval)` - Enable trace for a stage
- `(debug-disable 'eval)` - Disable trace
- `(debug-mode 'on)` - Enable all stages
- `(debug-status)` - Show enabled stages

**Usage example:**
```scheme
patina> (debug-enable 'eval)
patina> (+ 1 2)
[EVAL] Evaluating: (+ 1 2)
[EVAL]   Evaluating: +
[EVAL]   Evaluating: 1
[EVAL]   Evaluating: 2
3
```

**Current implementation location:**
- `src/eval/debug.rs` - DebugConfig struct
- `src/eval/primitives/debug.rs` - Scheme-level debug procedures
- `src/eval/mod.rs:66-69` - Trace hooks in eval_in_env()

---

## Research: How Other Schemes Do Debugging

### 1. Chibi Scheme - Procedure Tracing

**Approach:** Wrap procedures with tracing lambdas

```scheme
(define (make-tracer cell)
  (let ((proc (cdr cell)))
    (lambda args
      (show-trace cell args)               ; Before call
      (active-trace-depth (+ (active-trace-depth) 1))
      (let ((res (apply proc args)))       ; Actual call
        (active-trace-depth (- (active-trace-depth) 1))
        (show-trace-result cell args res)  ; After call
        res))))

(trace factorial)  ; Wraps factorial with tracer
```

**Key insights:**
- Uses environment cell mutation (set-cdr!)
- Tracks depth for indentation
- Simple but effective for procedure-level tracing
- Works at Scheme level, not interpreter level

**Location:** `~/Project/reference/chibi-scheme/lib/chibi/trace.scm`

---

### 2. Racket/DrRacket - Continuation Marks

**Approach:** Annotate continuations with metadata for stack inspection

**Continuation Marks:**
- Attach metadata to each call frame
- `(with-continuation-mark key value body)` - Install mark
- `(current-continuation-marks)` - Inspect marks
- Used for: stack traces, security checks, debugging

**Stepper Implementation:**
- Rewrites code to instrument expressions
- Uses continuation marks to store execution state
- Hooks: before eval, during eval, after eval
- Preserves full history for stepping backward

**Example:**
```scheme
(with-continuation-mark 'location (srcloc "foo.rkt" 10 5)
  (with-continuation-mark 'in-function 'factorial
    (factorial 5)))
```

**Key insights:**
- Continuation marks = metadata on call stack
- Enables stack inspection without breaking tail calls
- Stepper = AST rewriting + continuation marks
- Supports backward stepping (history tracking)

**Resources:**
- https://docs.racket-lang.org/reference/contmarks.html
- https://docs.racket-lang.org/drracket/debugger.html
- Paper: "Compiler and runtime support for continuation marks" (PLDI 2020)

---

### 3. GDB-style Debuggers - Breakpoint Strategy

**Breakpoint Implementation Techniques:**

1. **Software Breakpoints** (most common)
   - Replace instruction at breakpoint with interrupt
   - On hit: save state, call debugger, restore instruction, single-step

2. **Hardware Breakpoints** (limited number)
   - CPU-supported debug registers
   - No code modification needed

3. **Interpreter Breakpoints** (our use case)
   - Check breakpoint table before each eval
   - More flexible than software breakpoints
   - Performance cost: O(1) hash lookup per expression

**Single Stepping:**
- Step Into: Evaluate next expression, descend into calls
- Step Over: Evaluate next expression at current level
- Step Out: Continue until current call returns
- Run to Cursor: Continue until specific location

**Key insights:**
- Interpreter breakpoints are conceptually simple
- Need: breakpoint table, current location tracking
- Performance: can be optimized by disabling when no breakpoints

---

## Hook System Design Principles

### 1. Separation of Concerns

**Hook System** (low-level)
- When: Before eval, after eval, on error, on call, on return
- What: Expression, environment, result
- No policy decisions

**Debugger** (high-level)
- Uses hook system
- Implements: breakpoints, stepping, inspection
- Policy decisions: when to pause, what to show

### 2. Future-Proof Architecture

Must work with:
- **Tree-walking interpreter** (current)
- **TCO interpreter** (explicit stack, no Rust recursion)
- **Bytecode VM** (future) - different hook points
- **JIT compiler** (far future) - may need different approach

### 3. Performance Considerations

**When debugger disabled:**
- Minimal overhead (single bool check)
- No allocation
- Inline-friendly

**When debugger enabled:**
- Accept overhead for rich information
- Allocate history/state as needed

---

## Proposed Hook System Architecture

### Phase 1: Hook Points (Foundation)

**Goal:** Add hook points to evaluator without breaking existing code

```rust
// src/eval/hooks.rs (NEW)

/// Hook point in evaluation lifecycle
#[derive(Debug, Clone, Copy)]
pub enum HookPoint {
    BeforeEval,
    AfterEval,
    BeforeApply,
    AfterApply,
    OnError,
    OnDefine,
    OnSet,
}

/// Information passed to hook callbacks
pub struct HookContext<'a> {
    pub point: HookPoint,
    pub expr: &'a Value,
    pub env: &'a Rc<Environment>,
    pub result: Option<&'a Value>,
    pub error: Option<&'a EvalError>,
    pub depth: usize,
}

/// Trait for hook handlers
pub trait HookHandler {
    /// Called at each hook point. Return true to continue, false to pause.
    fn on_hook(&mut self, ctx: &HookContext) -> HookAction;
}

pub enum HookAction {
    Continue,           // Normal execution
    Pause,              // Pause execution (for debugger)
    Skip,               // Skip this expression
    ReplaceResult(Value), // Override result
}

/// Hook manager - manages registered hooks
pub struct HookManager {
    handlers: Vec<Box<dyn HookHandler>>,
    enabled: bool,
}

impl HookManager {
    pub fn new() -> Self {
        Self { handlers: Vec::new(), enabled: false }
    }

    pub fn register(&mut self, handler: Box<dyn HookHandler>) {
        self.handlers.push(handler);
        self.enabled = true;
    }

    pub fn fire(&mut self, ctx: &HookContext) -> HookAction {
        if !self.enabled {
            return HookAction::Continue;
        }

        for handler in &mut self.handlers {
            match handler.on_hook(ctx) {
                HookAction::Continue => continue,
                action => return action,  // First non-continue wins
            }
        }
        HookAction::Continue
    }
}
```

**Integration into Evaluator:**

```rust
// src/eval/mod.rs

pub struct Evaluator {
    global_env: Rc<Environment>,
    debug: Rc<DebugConfig>,
    hooks: Rc<RefCell<HookManager>>,  // NEW
    call_depth: Rc<RefCell<usize>>,   // NEW - for tracking depth
}

fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    // Fire before-eval hook
    let ctx = HookContext {
        point: HookPoint::BeforeEval,
        expr,
        env,
        result: None,
        error: None,
        depth: *self.call_depth.borrow(),
    };

    match self.hooks.borrow_mut().fire(&ctx) {
        HookAction::Continue => {},
        HookAction::Pause => {
            // Pause execution - enter interactive debugger
            self.enter_debugger(&ctx)?;
        }
        HookAction::Skip => return Ok(Value::Unspecified),
        HookAction::ReplaceResult(val) => return Ok(val),
    }

    // Increment depth
    *self.call_depth.borrow_mut() += 1;

    // Existing debug trace
    if self.debug.is_enabled(debug::DebugStage::Eval) {
        eprintln!("[EVAL]{} Evaluating: {}", self.debug.current_indent(), expr);
        self.debug.indent();
    }

    // Actual evaluation
    let result = match expr {
        // ... existing evaluation logic ...
    };

    // Decrement depth
    *self.call_depth.borrow_mut() -= 1;

    // Fire after-eval hook (on success)
    if let Ok(ref val) = result {
        let ctx = HookContext {
            point: HookPoint::AfterEval,
            expr,
            env,
            result: Some(val),
            error: None,
            depth: *self.call_depth.borrow(),
        };
        self.hooks.borrow_mut().fire(&ctx);
    }

    // Fire on-error hook (on failure)
    if let Err(ref err) = result {
        let ctx = HookContext {
            point: HookPoint::OnError,
            expr,
            env,
            result: None,
            error: Some(err),
            depth: *self.call_depth.borrow(),
        };
        self.hooks.borrow_mut().fire(&ctx);
    }

    result
}
```

**Testing Hook System:**

```rust
// Example: Trace hook (reimplements current debug trace)
struct TraceHook {
    enabled: bool,
}

impl HookHandler for TraceHook {
    fn on_hook(&mut self, ctx: &HookContext) -> HookAction {
        if !self.enabled {
            return HookAction::Continue;
        }

        match ctx.point {
            HookPoint::BeforeEval => {
                let indent = "  ".repeat(ctx.depth);
                eprintln!("{}→ {}", indent, ctx.expr);
            }
            HookPoint::AfterEval => {
                let indent = "  ".repeat(ctx.depth);
                eprintln!("{}← {}", indent, ctx.result.unwrap());
            }
            _ => {}
        }

        HookAction::Continue
    }
}
```

---

### Phase 2: Breakpoints and Stepping

**Goal:** Interactive debugger with breakpoints

```rust
// src/debugger/mod.rs (NEW)

/// Source location for breakpoints
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceLocation {
    pub line: usize,
    pub column: usize,
    // Future: file path when we have modules
}

/// Breakpoint specification
pub struct Breakpoint {
    pub id: usize,
    pub location: SourceLocation,
    pub condition: Option<Value>,  // Future: conditional breakpoints
    pub enabled: bool,
}

/// Debugger state
pub enum DebuggerMode {
    Running,
    StepInto,    // Step to next expression (any depth)
    StepOver,    // Step to next expression at current depth
    StepOut,     // Run until depth decreases
    RunTo(SourceLocation),
}

pub struct Debugger {
    breakpoints: HashMap<SourceLocation, Breakpoint>,
    next_bp_id: usize,
    mode: DebuggerMode,
    paused_at: Option<SourceLocation>,
    target_depth: Option<usize>,  // For step-over/step-out
}

impl Debugger {
    pub fn new() -> Self {
        Self {
            breakpoints: HashMap::new(),
            next_bp_id: 0,
            mode: DebuggerMode::Running,
            paused_at: None,
            target_depth: None,
        }
    }

    pub fn add_breakpoint(&mut self, loc: SourceLocation) -> usize {
        let id = self.next_bp_id;
        self.next_bp_id += 1;

        self.breakpoints.insert(loc.clone(), Breakpoint {
            id,
            location: loc,
            condition: None,
            enabled: true,
        });

        id
    }

    pub fn remove_breakpoint(&mut self, id: usize) {
        self.breakpoints.retain(|_, bp| bp.id != id);
    }

    pub fn should_pause(&self, ctx: &HookContext, loc: &SourceLocation) -> bool {
        match self.mode {
            DebuggerMode::Running => {
                // Only pause at enabled breakpoints
                self.breakpoints.get(loc)
                    .map(|bp| bp.enabled)
                    .unwrap_or(false)
            }
            DebuggerMode::StepInto => {
                // Pause at every expression
                true
            }
            DebuggerMode::StepOver => {
                // Pause when back at target depth
                ctx.depth <= self.target_depth.unwrap_or(0)
            }
            DebuggerMode::StepOut => {
                // Pause when depth decreases
                ctx.depth < self.target_depth.unwrap_or(usize::MAX)
            }
            DebuggerMode::RunTo(ref target) => {
                // Pause at target location
                loc == target
            }
        }
    }

    pub fn step_into(&mut self) {
        self.mode = DebuggerMode::StepInto;
    }

    pub fn step_over(&mut self, current_depth: usize) {
        self.mode = DebuggerMode::StepOver;
        self.target_depth = Some(current_depth);
    }

    pub fn step_out(&mut self, current_depth: usize) {
        self.mode = DebuggerMode::StepOut;
        self.target_depth = Some(current_depth);
    }

    pub fn continue_execution(&mut self) {
        self.mode = DebuggerMode::Running;
        self.target_depth = None;
    }
}

impl HookHandler for Debugger {
    fn on_hook(&mut self, ctx: &HookContext) -> HookAction {
        if ctx.point != HookPoint::BeforeEval {
            return HookAction::Continue;
        }

        // Extract source location from expression
        // (Future: expressions need source location metadata)
        let loc = extract_location(ctx.expr);

        if self.should_pause(ctx, &loc) {
            self.paused_at = Some(loc);
            HookAction::Pause  // Signals evaluator to enter interactive mode
        } else {
            HookAction::Continue
        }
    }
}
```

**REPL Integration:**

```rust
// src/debugger/repl.rs (NEW)

impl Evaluator {
    /// Enter interactive debugger
    pub fn enter_debugger(&self, ctx: &HookContext) -> Result<(), EvalError> {
        println!("\n[Debugger] Paused at: {}", ctx.expr);
        println!("Depth: {}", ctx.depth);

        loop {
            print!("debug> ");
            std::io::stdout().flush().unwrap();

            let mut input = String::new();
            std::io::stdin().read_line(&mut input).unwrap();

            match input.trim() {
                "c" | "continue" => {
                    // Resume execution
                    return Ok(());
                }
                "s" | "step" => {
                    // Step into
                    self.debugger.borrow_mut().step_into();
                    return Ok(());
                }
                "n" | "next" => {
                    // Step over
                    self.debugger.borrow_mut().step_over(ctx.depth);
                    return Ok(());
                }
                "o" | "out" => {
                    // Step out
                    self.debugger.borrow_mut().step_out(ctx.depth);
                    return Ok(());
                }
                "p" | "print" => {
                    // Print current expression
                    println!("{}", ctx.expr);
                }
                cmd if cmd.starts_with("eval ") => {
                    // Evaluate expression in current environment
                    let expr_str = &cmd[5..];
                    match self.eval_str_in_env(expr_str, ctx.env) {
                        Ok(val) => println!("{}", val),
                        Err(e) => println!("Error: {}", e),
                    }
                }
                "env" => {
                    // Show environment bindings
                    println!("{:?}", ctx.env);
                }
                "bt" | "backtrace" => {
                    // Show call stack (requires call stack tracking)
                    self.print_backtrace();
                }
                "h" | "help" => {
                    println!("Commands:");
                    println!("  c, continue  - Continue execution");
                    println!("  s, step      - Step into next expression");
                    println!("  n, next      - Step over (same level)");
                    println!("  o, out       - Step out (up one level)");
                    println!("  p, print     - Print current expression");
                    println!("  eval <expr>  - Evaluate expression in current env");
                    println!("  env          - Show environment");
                    println!("  bt           - Show backtrace");
                    println!("  h, help      - This help");
                }
                "" => continue,
                cmd => {
                    println!("Unknown command: {}. Type 'h' for help.", cmd);
                }
            }
        }
    }
}
```

---

### Phase 3: Call Stack Tracking

**Problem:** We don't have a call stack - eval is recursive!

**Solution:** Maintain explicit call stack in evaluator

```rust
// src/eval/call_stack.rs (NEW)

#[derive(Debug, Clone)]
pub struct StackFrame {
    pub expr: Value,
    pub env: Rc<Environment>,
    pub location: Option<SourceLocation>,
    pub name: Option<Rc<str>>,  // Procedure name if known
}

pub struct CallStack {
    frames: Vec<StackFrame>,
}

impl CallStack {
    pub fn new() -> Self {
        Self { frames: Vec::new() }
    }

    pub fn push(&mut self, frame: StackFrame) {
        self.frames.push(frame);
    }

    pub fn pop(&mut self) -> Option<StackFrame> {
        self.frames.pop()
    }

    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    pub fn backtrace(&self) -> Vec<&StackFrame> {
        self.frames.iter().rev().collect()
    }
}

// Integration into evaluator
pub struct Evaluator {
    global_env: Rc<Environment>,
    debug: Rc<DebugConfig>,
    hooks: Rc<RefCell<HookManager>>,
    call_stack: Rc<RefCell<CallStack>>,  // NEW
}

impl Evaluator {
    fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // Push frame
        self.call_stack.borrow_mut().push(StackFrame {
            expr: expr.clone(),
            env: env.clone(),
            location: extract_location(expr),
            name: extract_name(expr),
        });

        // Evaluate
        let result = /* ... existing eval logic ... */;

        // Pop frame
        self.call_stack.borrow_mut().pop();

        result
    }

    pub fn print_backtrace(&self) {
        println!("\nBacktrace:");
        for (i, frame) in self.call_stack.borrow().backtrace().iter().enumerate() {
            if let Some(name) = &frame.name {
                println!("  #{} in {}", i, name);
            } else {
                println!("  #{} {}", i, frame.expr);
            }
            if let Some(loc) = &frame.location {
                println!("     at line {}, column {}", loc.line, loc.column);
            }
        }
    }
}
```

**Note:** Call stack tracking will be ESSENTIAL for TCO implementation - we'll need it anyway!

---

### Phase 4: Source Location Tracking

**Problem:** Expressions don't know where they came from in source

**Solution:** Annotate AST with source locations during parsing

```rust
// src/value/mod.rs

pub struct SourceInfo {
    pub line: usize,
    pub column: usize,
    pub span: usize,  // Length in characters
    // Future: file path
}

// Wrapper for values with source info
pub struct Located<T> {
    pub value: T,
    pub location: Option<SourceInfo>,
}

// Two approaches:

// Approach 1: Wrap all Values (invasive)
pub enum Value {
    Located(Box<Located<Value>>),
    Boolean(bool),
    // ... other variants ...
}

// Approach 2: Separate source map (less invasive)
pub struct SourceMap {
    locations: HashMap<usize, SourceInfo>,  // Value pointer -> location
}
```

**Parser changes:**

```rust
// src/parser/mod.rs

impl Parser {
    fn parse_expr(&mut self) -> Result<Value, ParseError> {
        let start_pos = self.current_position();

        let value = /* ... parse value ... */;

        let end_pos = self.current_position();
        let location = SourceInfo {
            line: self.line,
            column: self.column,
            span: end_pos - start_pos,
        };

        // Attach location to value
        self.attach_location(value, location)
    }
}
```

**Tradeoffs:**
- **Approach 1 (wrap all values):** Simple, but increases memory usage
- **Approach 2 (source map):** Efficient, but requires value pointer stability

**Recommendation:** Start with Approach 2, migrate to Approach 1 if needed

---

## Future: Advanced Features

### 1. Conditional Breakpoints

```scheme
; Break when n < 0
(debug-break-when factorial (< n 0))
```

**Implementation:**
```rust
pub struct Breakpoint {
    pub id: usize,
    pub location: SourceLocation,
    pub condition: Option<Value>,  // Scheme expression to evaluate
    pub enabled: bool,
}

impl Debugger {
    fn should_pause(&self, ctx: &HookContext, loc: &SourceLocation) -> bool {
        if let Some(bp) = self.breakpoints.get(loc) {
            if !bp.enabled {
                return false;
            }

            // Evaluate condition in current environment
            if let Some(ref cond) = bp.condition {
                match self.evaluator.eval_in_env(cond, ctx.env) {
                    Ok(Value::Boolean(true)) => return true,
                    Ok(Value::Boolean(false)) => return false,
                    _ => return true,  // Break on evaluation error
                }
            }

            return true;
        }
        false
    }
}
```

---

### 2. Watch Expressions

```scheme
(debug-watch 'counter)
; Pause whenever counter changes value
```

**Implementation:**
```rust
pub struct WatchPoint {
    pub var_name: Rc<str>,
    pub last_value: Option<Value>,
}

impl HookHandler for WatchPoint {
    fn on_hook(&mut self, ctx: &HookContext) -> HookAction {
        if ctx.point == HookPoint::AfterEval || ctx.point == HookPoint::OnSet {
            if let Some(current_value) = ctx.env.get(&self.var_name) {
                if self.last_value.as_ref() != Some(&current_value) {
                    println!("Watch: {} changed from {:?} to {}",
                             self.var_name, self.last_value, current_value);
                    self.last_value = Some(current_value.clone());
                    return HookAction::Pause;
                }
            }
        }
        HookAction::Continue
    }
}
```

---

### 3. Time-Travel Debugging (Racket-style Stepper)

**Concept:** Record full evaluation history, step backward

```rust
pub struct EvalHistory {
    steps: Vec<EvalStep>,
    current_index: usize,
}

pub struct EvalStep {
    pub expr: Value,
    pub env: Rc<Environment>,
    pub result: Value,
    pub depth: usize,
}

impl HookHandler for EvalHistory {
    fn on_hook(&mut self, ctx: &HookContext) -> HookAction {
        if ctx.point == HookPoint::AfterEval {
            self.steps.push(EvalStep {
                expr: ctx.expr.clone(),
                env: ctx.env.clone(),
                result: ctx.result.unwrap().clone(),
                depth: ctx.depth,
            });
        }
        HookAction::Continue
    }
}

// Debugger commands:
// - step-back: Decrement current_index
// - step-forward: Increment current_index
// - restart: Set current_index = 0
```

**Tradeoff:** High memory usage, but incredibly useful for understanding evaluation

---

### 4. Profiling Hook

```rust
pub struct Profiler {
    call_counts: HashMap<Rc<str>, usize>,
    call_times: HashMap<Rc<str>, Duration>,
    start_times: HashMap<Rc<str>, Instant>,
}

impl HookHandler for Profiler {
    fn on_hook(&mut self, ctx: &HookContext) -> HookAction {
        match ctx.point {
            HookPoint::BeforeApply => {
                if let Some(name) = extract_name(ctx.expr) {
                    *self.call_counts.entry(name.clone()).or_insert(0) += 1;
                    self.start_times.insert(name, Instant::now());
                }
            }
            HookPoint::AfterApply => {
                if let Some(name) = extract_name(ctx.expr) {
                    if let Some(start) = self.start_times.get(&name) {
                        let elapsed = start.elapsed();
                        *self.call_times.entry(name).or_insert(Duration::ZERO) += elapsed;
                    }
                }
            }
            _ => {}
        }
        HookAction::Continue
    }
}

impl Profiler {
    pub fn report(&self) {
        println!("\nProfile Report:");
        println!("{:<20} {:>10} {:>15}", "Function", "Calls", "Total Time");

        let mut entries: Vec<_> = self.call_counts.iter().collect();
        entries.sort_by_key(|(_, &count)| std::cmp::Reverse(count));

        for (name, count) in entries {
            let time = self.call_times.get(name).unwrap_or(&Duration::ZERO);
            println!("{:<20} {:>10} {:>15?}", name, count, time);
        }
    }
}
```

---

## Integration with Future Runtime Models

### Tree-Walking (Current)

**Hook points:** Before/after eval, before/after apply
**Call stack:** Explicit stack tracking (Phase 3)
**Breakpoints:** Check before every eval

✅ Straightforward implementation

---

### TCO with Explicit Stack

**When we implement TCO, we'll have:**
```rust
struct TcoEvaluator {
    stack: Vec<StackFrame>,  // Already needed for TCO!
    // ...
}
```

**Hook integration:**
```rust
impl TcoEvaluator {
    fn eval_loop(&mut self) -> Result<Value, EvalError> {
        while let Some(frame) = self.stack.last() {
            // Fire before-eval hook
            if self.hooks.borrow_mut().fire(&ctx) == HookAction::Pause {
                self.enter_debugger()?;
            }

            // Process frame
            // ...

            // Fire after-eval hook
        }
    }
}
```

✅ **Even easier than tree-walking!** Stack is already explicit.

---

### Bytecode VM (Future)

**Hook points change:**
- Before/after instruction (not expression)
- Instruction pointer instead of AST node
- Different granularity

**Approach:**
```rust
enum HookPoint {
    // High-level (works with bytecode)
    BeforeCall(usize),  // Function index
    AfterCall(usize),
    OnBreakpoint(usize), // Instruction pointer

    // Low-level (bytecode specific)
    BeforeInstruction(Instruction),
    AfterInstruction(Instruction),
}
```

**Debugger adaptation:**
- Breakpoints at instruction addresses, not expressions
- Decompile instructions for display
- Source maps map bytecode back to source

⚠️ **Requires adaptation but same architecture works**

---

## Implementation Timeline

### Phase 1: Hook Infrastructure (1 week)
**Goal:** Add hook system without breaking existing code

**Tasks:**
1. Create `src/eval/hooks.rs` with HookPoint, HookHandler, HookManager
2. Add HookManager to Evaluator struct
3. Add hook firing to eval_in_env (before/after eval)
4. Add call_depth tracking
5. Implement TraceHook to replicate current debug behavior
6. Tests: verify hooks fire at correct times

**Deliverable:** Hook system working, backward compatible

---

### Phase 2: Interactive Debugger (1 week)
**Goal:** Breakpoints and stepping

**Tasks:**
1. Create `src/debugger/mod.rs` with Debugger struct
2. Implement breakpoint management (add/remove/enable/disable)
3. Implement stepping modes (step-into/over/out)
4. Add enter_debugger() REPL
5. Scheme primitives: `(debug-break location)`, `(debug-step)`, etc.
6. Tests: breakpoint behavior, stepping logic

**Deliverable:** Interactive debugger REPL with stepping

---

### Phase 3: Call Stack + Source Locations (3-5 days)
**Goal:** Better error messages and backtraces

**Tasks:**
1. Create `src/eval/call_stack.rs`
2. Integrate call stack into evaluator
3. Add source location tracking to parser (approach 2: source map)
4. Extract location helper functions
5. Implement backtrace printing
6. Tests: verify call stack accuracy

**Deliverable:** Stack traces in errors and debugger

---

### Phase 4: Advanced Features (optional, 1 week)
**Goal:** Conditional breakpoints, watch points, profiling

**Tasks:**
1. Conditional breakpoints (evaluate expression in env)
2. Watch points (track variable changes)
3. Profiling hook (timing, call counts)
4. Time-travel debugging (record history)
5. Scheme API for all features

**Deliverable:** Full-featured debugger

---

### Total Estimated Time: 2-3 weeks

**Phased approach allows:**
- Early value (Phase 1-2 very useful)
- Incremental development
- Each phase independently useful
- Can defer Phase 4 indefinitely

---

## Scheme-Level API

```scheme
; Hook management
(debug-install-hook hook-procedure)  ; (lambda (point expr env result) ...)
(debug-remove-hook hook-procedure)

; Breakpoints
(debug-break line column)            ; Set breakpoint
(debug-break-when expr condition)    ; Conditional breakpoint
(debug-clear line column)            ; Remove breakpoint
(debug-list-breakpoints)             ; List all breakpoints

; Stepping (when paused)
(debug-step-into)                    ; Step into next expression
(debug-step-over)                    ; Step over at current level
(debug-step-out)                     ; Step out of current function
(debug-continue)                     ; Resume execution

; Inspection
(debug-backtrace)                    ; Print call stack
(debug-locals)                       ; Show local variables
(debug-eval expr)                    ; Eval in current context

; Watch points
(debug-watch 'variable)              ; Watch variable for changes
(debug-unwatch 'variable)

; Profiling
(debug-profile-start)
(debug-profile-stop)
(debug-profile-report)

; Tracing (existing)
(debug-enable 'eval)
(debug-disable 'eval)
(debug-mode 'on)
(debug-status)
```

---

## Performance Considerations

### When Debugger Disabled (Production)

```rust
// Minimal overhead - single bool check
if self.hooks.enabled {  // Usually false
    self.hooks.fire(&ctx);
}
```

**Overhead:** ~1-2 CPU cycles per eval (branch prediction helps)

---

### When Debugger Enabled (Development)

**Overhead depends on features:**
- Breakpoint check: O(1) hash lookup
- Call stack tracking: O(1) push/pop
- Source location: Already attached to values
- Hook dispatch: O(num_handlers) but usually 1-3

**Acceptable overhead:** 2-5x slowdown acceptable in debug mode

---

### Optimization: Compile-Time Flags

```rust
#[cfg(feature = "debugger")]
fn eval_in_env(...) {
    // Full hook support
}

#[cfg(not(feature = "debugger"))]
fn eval_in_env(...) {
    // No hooks, maximum performance
}
```

**Benefit:** Zero overhead when debugger disabled at compile time

---

## Testing Strategy

### Unit Tests

```rust
// tests/debugger/hooks.rs

#[test]
fn test_hook_fires_before_eval() {
    let mut evaluator = Evaluator::new();
    let mut fired = false;

    evaluator.hooks.borrow_mut().register(Box::new(TestHook {
        on_before_eval: |ctx| { fired = true; HookAction::Continue }
    }));

    evaluator.eval_str("(+ 1 2)").unwrap();
    assert!(fired);
}

#[test]
fn test_breakpoint_pauses_execution() {
    let mut evaluator = Evaluator::new();
    let debugger = Debugger::new();

    debugger.add_breakpoint(SourceLocation { line: 1, column: 0 });
    evaluator.hooks.borrow_mut().register(Box::new(debugger));

    // Should pause at breakpoint
    // (need to mock stdin/stdout for interactive test)
}
```

### Integration Tests

```scheme
; tests/fixtures/debugger/stepping.scm

(define (factorial n)
  (if (<= n 1)
      1
      (* n (factorial (- n 1)))))

; Test:
; 1. Set breakpoint at line 2
; 2. Call (factorial 3)
; 3. Verify pauses at breakpoint
; 4. Step into recursive call
; 5. Verify depth increases
; 6. Step out
; 7. Verify depth decreases
```

---

## Comparison with Current Debug System

| Feature | Current (DebugConfig) | Proposed (Hook System) |
|---------|------------------------|------------------------|
| **Trace eval** | ✅ Print to stderr | ✅ Via TraceHook |
| **Breakpoints** | ❌ Not supported | ✅ Interactive pause |
| **Stepping** | ❌ Not supported | ✅ Step into/over/out |
| **Call stack** | ❌ No stack | ✅ Explicit tracking |
| **Inspect vars** | ❌ Not supported | ✅ Eval in context |
| **Profiling** | ❌ Not supported | ✅ Via ProfileHook |
| **Extensibility** | ⚠️ Hard-coded | ✅ Plugin hooks |
| **Performance** | ✅ Minimal overhead | ✅ Similar when disabled |

---

## Recommendations

### Immediate Next Steps

1. **Implement Phase 1 (Hook Infrastructure)** - 1 week
   - Foundational, enables everything else
   - Low risk, backward compatible
   - Immediately useful for custom tracing

2. **Implement Phase 2 (Interactive Debugger)** - 1 week
   - High value for development
   - Makes debugging complex macros much easier
   - Great demo feature

3. **Defer Phase 3-4** until after TCO
   - Call stack will be needed for TCO anyway
   - Source locations are nice-to-have
   - Advanced features can wait

---

### Long-Term Vision

**Year 1 (Phase 1 complete):**
- Basic hook system ✅
- Interactive debugger with breakpoints ✅
- Tracing and profiling ✅

**Year 2 (Phase 2+):**
- Time-travel debugging (Racket-style stepper)
- Debugger protocol (like DAP - Debug Adapter Protocol)
- IDE integration (VSCode, Emacs)

**Year 3 (Future phases):**
- Performance profiling with flame graphs
- Memory profiling (GC pressure, allocation tracking)
- Security auditing (capability tracking)

---

## References

**Chibi Scheme:**
- `~/Project/reference/chibi-scheme/lib/chibi/trace.scm`
- Procedure wrapping approach

**Racket:**
- https://docs.racket-lang.org/reference/contmarks.html
- Continuation marks for stack inspection
- https://docs.racket-lang.org/drracket/debugger.html
- Stepper implementation via AST rewriting

**GDB/LLDB:**
- Software breakpoints (instruction replacement)
- Single-step flag (CPU debug mode)

**Papers:**
- "Compiler and runtime support for continuation marks" (PLDI 2020)
- "A Portable Debugger for Standard ML" (useful for functional language debugging)

---

## Conclusion

A well-designed hook system provides:

1. **Foundation for debugging** - Breakpoints, stepping, inspection
2. **Extensibility** - Users can write custom hooks
3. **Future-proof** - Works with TCO, bytecode VM, etc.
4. **Performance** - Minimal overhead when disabled

**Recommended approach:**
- Phase 1 (hooks) → Phase 2 (debugger) → defer Phase 3-4
- Total: 2 weeks for highly useful debugging infrastructure
- Integrates seamlessly with future TCO work
- Positions Patina as "interpreter with great debugging facilities" ✅

**Next step:** Implement Phase 1 hook infrastructure after TCO is complete (or in parallel if different developer).
