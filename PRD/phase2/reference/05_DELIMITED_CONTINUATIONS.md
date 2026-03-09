# Bytecode ISA Around Delimited Continuations

**Priority:** ⭐⭐ Medium
**Complexity:** High (8-10 weeks)
**Impact:** Medium-High (enables modern control abstractions)
**Status:** Research

---

## Overview

Design the VM's bytecode ISA to treat delimited continuations as the primitive control operator, rather than full continuations. This makes `call/cc`, generators, async/await, and exceptions all special cases of one unified mechanism.

**Key Insight:** Full continuations (`call/cc`) are too powerful and hard to optimize. Delimited continuations provide the right abstraction level.

---

## Delimited vs Full Continuations

**Full continuation (call/cc):**
```scheme
(+ 1 (call/cc (lambda (k) (k 10))))
; Captures ENTIRE call stack
; Result: 11

(k 20)  ; Can invoke later, even from different context!
```

**Delimited continuation (shift/reset):**
```scheme
(+ 1 (reset (+ 2 (shift k (k (k 10))))))
; Captures stack only up to reset boundary
; Result: (+ 1 (+ 2 (+ 2 10))) = 15
```

**Why delimited is better:**
- Bounded (only capture stack slice)
- Composable (can have multiple prompts)
- Easier to implement efficiently
- Maps naturally to exceptions, generators, async

---

## Core Primitives

### Reset (Set Prompt/Boundary)

```rust
pub enum Opcode {
    Reset {
        prompt_tag: PromptTag,   // Unique prompt identifier
        body: Label,             // Code to execute
    },
}

impl VM {
    fn execute_reset(&mut self, prompt_tag: PromptTag, body: Label) -> Value {
        // Push continuation prompt
        let prompt = Prompt {
            tag: prompt_tag,
            stack_depth: self.call_stack.len(),
            frame_base: self.frame_pointer,
        };
        self.prompt_stack.push(prompt);

        // Execute body
        let result = self.execute_block(body)?;

        // Pop prompt
        self.prompt_stack.pop();

        Ok(result)
    }
}
```

### Shift (Capture Delimited Continuation)

```rust
pub enum Opcode {
    Shift {
        prompt_tag: PromptTag,
        continuation_var: Register,
        body: Label,
    },
}

impl VM {
    fn execute_shift(&mut self, prompt_tag: PromptTag, cont_var: Register, body: Label) -> Value {
        // Find matching prompt
        let prompt = self.find_prompt(prompt_tag)?;

        // Capture stack slice from current position to prompt
        let continuation = self.capture_delimited_continuation(prompt)?;

        // Bind continuation to variable
        self.set_register(cont_var, Value::DelimitedContinuation(continuation));

        // Execute body (outside the prompt)
        self.execute_block(body)
    }

    fn capture_delimited_continuation(&self, prompt: &Prompt) -> DelimitedContinuation {
        DelimitedContinuation {
            stack_slice: self.call_stack[prompt.stack_depth..].to_vec(),
            registers: self.registers.clone(),
            prompt_tag: prompt.tag,
        }
    }
}
```

---

## Implementing call/cc with Delimited Continuations

```scheme
;; R7RS call/cc in terms of shift/reset
(define (call/cc f)
  (reset (f (lambda (v) (shift k v)))))

;; Or with explicit prompt:
(define *top-level-prompt* (make-prompt-tag))

(define (call/cc f)
  (shift *top-level-prompt* k (f (lambda (v) (k v)))))
```

**Bytecode:**
```rust
// (call/cc f)
Reset { prompt_tag: TopLevel, body: {
    Shift { prompt_tag: TopLevel, continuation_var: r1, body: {
        LoadGlobal(r2, "f"),
        MakeClosure(r3, lambda { v => invoke_cont(r1, v) }),
        Call(r2, [r3], r4),
        Return(r4)
    }}
}}
```

---

## Modern Control Abstractions

### 1. Exceptions

```scheme
(define-syntax try
  (syntax-rules ()
    ((try body (catch var handler))
     (reset (body)
            ((exception k) (let ((var k)) handler))))))

(define (raise value)
  (shift exception value))
```

**Implementation:**
```rust
// VM-level exception support
pub enum Opcode {
    Try {
        body: Label,
        exception_handler: Label,
    },
    Raise { value: Register },
}

// Desugars to:
Reset {
    prompt_tag: ExceptionPrompt,
    body: {
        // ... user code ...
        Raise { value: r1 }  // → Shift ExceptionPrompt k r1
    }
}
```

### 2. Generators

```scheme
(define (make-generator proc)
  (let ((resume #f))
    (lambda ()
      (reset
        (if resume
            (resume)
            (proc (lambda (v)
                    (shift k
                      (set! resume k)
                      v))))))))

;; Usage:
(define gen (make-generator
  (lambda (yield)
    (yield 1)
    (yield 2)
    (yield 3))))

(gen)  ; → 1
(gen)  ; → 2
(gen)  ; → 3
```

**VM support:**
```rust
pub struct Generator {
    continuation: Option<DelimitedContinuation>,
    state: GeneratorState,
}

pub enum GeneratorState {
    Running,
    Suspended(Value),  // Last yielded value
    Complete,
}
```

### 3. Async/Await

```scheme
(define (async body)
  (lambda ()
    (reset (body (lambda (promise)
                    (shift k
                      (schedule-continuation k promise)))))))

(define-syntax await
  (syntax-rules ()
    ((await promise)
     (shift k
       (schedule-continuation k promise)))))
```

---

## Bytecode ISA Design

**Core control flow opcodes:**

```rust
pub enum Opcode {
    // Delimited continuations (primitive)
    Reset { tag: PromptTag, body: Label },
    Shift { tag: PromptTag, var: Register, body: Label },

    // High-level (desugar to reset/shift)
    Try { body: Label, handler: Label },
    Raise { value: Register },
    Yield { value: Register },
    Await { promise: Register },

    // Traditional
    Call { func: Register, args: Vec<Register>, dst: Register },
    TailCall { func: Register, args: Vec<Register> },
    Return { value: Register },
}
```

**Prompt tags:**
```rust
pub enum PromptTag {
    TopLevel,           // For call/cc
    Exception,          // For try/catch
    Generator,          // For yield
    Async,              // For async/await
    Custom(Symbol),     // User-defined
}
```

---

## Stack Management

**Traditional stack:**
```
[frame 1][frame 2][frame 3][frame 4]
          ↑                         ↑
    call/cc captures ALL      current
```

**With prompts:**
```
[frame 1]<prompt>[frame 2][frame 3]<prompt>[frame 4]
          ↑               ↑                 ↑        ↑
        boundary      boundary          current  boundary

shift captures: [frame 2][frame 3]  (bounded!)
```

**Implementation:**
```rust
pub struct CallStack {
    frames: Vec<Frame>,
    prompts: Vec<Prompt>,  // Stack of active prompts
}

pub struct Prompt {
    tag: PromptTag,
    stack_depth: usize,     // Where in call stack
    continuation: Option<Box<dyn Fn(Value) -> Value>>,  // Saved continuation
}
```

---

## Performance Optimization

### 1. Inline Common Patterns

```rust
// Instead of actual reset/shift for every exception:
Opcode::Raise { value } => {
    // Fast path: unwind to exception handler
    self.unwind_to_exception_handler(value);
}

// vs full delimited continuation:
Opcode::Shift { tag: Exception, ... } => {
    let cont = self.capture_delimited_continuation()?;
    // ... slower
}
```

### 2. Stack Copying vs Heap Allocation

```rust
pub enum DelimitedContinuation {
    Small {
        // Stack-allocated (for shallow continuations)
        frames: ArrayVec<Frame, 16>,
    },
    Large {
        // Heap-allocated (for deep continuations)
        frames: Vec<Frame>,
    },
}
```

---

## Integration with Effect System

**Combine with effect-typed continuations (Section 02):**

```rust
pub enum EffectKind {
    Exception(PromptTag),   // Maps to specific prompt
    Generator(PromptTag),
    Async(PromptTag),
    Full,
}

impl VM {
    fn optimize_shift(&mut self, tag: PromptTag, body: Label) {
        let effect = self.infer_effect(tag, body);

        match effect {
            EffectKind::Exception(_) => {
                // Use fast exception unwind
                self.emit_exception_unwind();
            }
            EffectKind::Generator(_) => {
                // Use stack copying
                self.emit_generator_yield();
            }
            _ => {
                // Full delimited continuation
                self.emit_shift(tag, body);
            }
        }
    }
}
```

---

## Challenges

**Challenge 1: Multiple Prompts**
- Solution: Prompt stack with tags, match by tag on shift

**Challenge 2: Prompt Mismatch**
- Problem: shift without matching reset
- Solution: Implicit top-level prompt, or error

**Challenge 3: Performance**
- Problem: Continuation capture overhead
- Solution: Combine with effect typing (Section 02) to optimize common cases

---

## References

1. **"A Monadic Framework for Delimited Continuations"** (Dybvig et al., 2007)
   - Scheme perspective on delimited continuations

2. **"Multicore OCaml"** (Dolan et al., 2014-2023)
   - Production implementation of effect handlers

3. **"Delimcc Library"** (Kiselyov, 2010)
   - Efficient implementation of delimited continuations

4. **"One-Shot Delimited Continuations"** (Bruggeman et al., 1996)
   - Optimization techniques

---

## Implementation Timeline

**Week 1-2:** Design ISA with reset/shift primitives
**Week 3-4:** Implement prompt stack and continuation capture
**Week 5-6:** Desugar call/cc, exceptions to reset/shift
**Week 7-8:** Optimize common patterns (exception fast path)
**Week 9-10:** Add generators, async/await support

---

## Why This Matters

**Current state:** Scheme has `call/cc` (powerful but slow)

**With delimited continuations:**
- Exceptions: Fast (just unwind)
- Generators: Practical (bounded capture)
- Async/Await: Possible (first-class in Scheme!)
- call/cc: Still available (desugar to reset/shift)

**Result:** Modern control abstractions with good performance 🎯

---

## Alternative: Start Simpler

**Phase 1 (2-3 weeks):** Just exceptions
- Add Try/Raise opcodes
- Implement as unwind (no real continuations)
- Fast, simple

**Phase 2 (3-4 weeks):** Generators
- Add Yield opcode
- Stack copying for resume
- Still bounded

**Phase 3 (3-4 weeks):** Full delimited continuations
- Add Reset/Shift
- Refactor exceptions/generators to use them
- Complete, composable

This incremental approach might be more practical!
