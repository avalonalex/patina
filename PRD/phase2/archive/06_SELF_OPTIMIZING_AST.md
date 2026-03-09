# Self-Optimizing AST Nodes (Micro-Truffle)

**Priority:** ⭐ Medium-Low
**Complexity:** Medium-High (6-8 weeks)
**Impact:** Medium (smart interpreter performance)
**Status:** Research

---

## Overview

Borrow Truffle's idea of self-rewriting AST nodes, but without requiring GraalVM. AST nodes observe runtime behavior and specialize themselves based on what they see, achieving "smart interpreter" performance without a full JIT.

**Key Insight:** Most interpreter overhead comes from generic dispatch. If AST nodes can specialize themselves, we get many JIT benefits without the complexity.

---

## Traditional AST Interpreter

**Generic AST node:**
```rust
pub enum Expr {
    Add { left: Box<Expr>, right: Box<Expr> },
    // ... other nodes
}

impl Expr {
    fn eval(&self, env: &Environment) -> Value {
        match self {
            Expr::Add { left, right } => {
                let l_val = left.eval(env)?;
                let r_val = right.eval(env)?;
                generic_add(l_val, r_val)  // ← Always generic
            }
            _ => { /* ... */ }
        }
    }
}
```

**Problems:**
- Every `+` goes through generic_add (type dispatch)
- AST traversal overhead
- No specialization based on observed types

---

## Self-Optimizing AST

**Nodes rewrite themselves:**

```rust
pub trait ASTNode {
    fn execute(&mut self, env: &Environment) -> Value;
    fn specialize(&mut self, feedback: &TypeFeedback);
}

pub enum AddNode {
    Generic {
        left: Box<dyn ASTNode>,
        right: Box<dyn ASTNode>,
        profile: TypeProfile,
    },
    FixnumSpecialized {
        left: Box<dyn ASTNode>,
        right: Box<dyn ASTNode>,
    },
    FloatSpecialized {
        left: Box<dyn ASTNode>,
        right: Box<dyn ASTNode>,
    },
}

impl ASTNode for AddNode {
    fn execute(&mut self, env: &Environment) -> Value {
        match self {
            AddNode::Generic { left, right, profile } => {
                let l_val = left.execute(env)?;
                let r_val = right.execute(env)?;

                // Record observed types
                profile.observe(&l_val, &r_val);

                // Check if should specialize
                if profile.is_monomorphic() {
                    *self = self.specialize_based_on_profile(profile);
                }

                generic_add(l_val, r_val)
            }

            AddNode::FixnumSpecialized { left, right } => {
                let l_val = left.execute(env)?;
                let r_val = right.execute(env)?;

                // Fast path: assume fixnum
                if let (Value::Fixnum(l), Value::Fixnum(r)) = (&l_val, &r_val) {
                    match l.checked_add(*r) {
                        Some(result) => Value::Fixnum(result),
                        None => {
                            // Despecialize on overflow
                            *self = AddNode::Generic { /* ... */ };
                            generic_add(l_val, r_val)
                        }
                    }
                } else {
                    // Type mismatch, despecialize
                    *self = AddNode::Generic { /* ... */ };
                    generic_add(l_val, r_val)
                }
            }

            AddNode::FloatSpecialized { left, right } => {
                // Similar for float
                // ...
            }
        }
    }
}
```

---

## Specialization Strategies

### 1. Type Specialization

```rust
// Generic → Specialized based on observed types
AddNode::Generic { profile: "100% fixnum+fixnum" }
  ↓
AddNode::FixnumSpecialized

// Polymorphic → Multiple specialized variants
AddNode::Generic { profile: "60% fixnum, 30% float, 10% other" }
  ↓
AddNode::Polymorphic {
    fixnum_path: FixnumSpecialized,
    float_path: FloatSpecialized,
    generic_fallback: Generic,
}
```

### 2. Constant Folding

```rust
pub enum AddNode {
    // ...
    ConstantResult {
        value: Value,  // Pre-computed result!
    },
}

// If both children are constants, replace entire subtree
if left.is_constant() && right.is_constant() {
    let result = generic_add(left.eval(), right.eval());
    *self = AddNode::ConstantResult { value: result };
}
```

### 3. Inline Caching for Variable Lookup

```rust
pub enum VarRefNode {
    Generic {
        name: Symbol,
        profile: LookupProfile,
    },
    Cached {
        name: Symbol,
        cached_env: Weak<Environment>,  // Weak ref to environment
        cached_value: Value,
        version: u64,
    },
}

impl ASTNode for VarRefNode {
    fn execute(&mut self, env: &Environment) -> Value {
        match self {
            VarRefNode::Cached { cached_env, cached_value, version, name, .. } => {
                // Fast path: cache hit
                if let Some(cached) = cached_env.upgrade() {
                    if Arc::ptr_eq(&cached, env) && cached.version() == *version {
                        return cached_value.clone();  // Cache hit!
                    }
                }

                // Cache miss, lookup and update cache
                let value = env.lookup(name)?;
                *cached_env = Arc::downgrade(env);
                *cached_value = value.clone();
                *version = env.version();
                value
            }

            VarRefNode::Generic { name, profile } => {
                let value = env.lookup(name)?;

                // Decide if should cache
                if profile.lookup_count > CACHE_THRESHOLD {
                    *self = VarRefNode::Cached { /* ... */ };
                }

                value
            }
        }
    }
}
```

---

## Truffle-Style Node Replacement

**Truffle concept: Node chain evolution**

```
Uninitialized
    ↓ (first call)
Monomorphic (single type seen)
    ↓ (second type seen)
Polymorphic (2-3 types seen)
    ↓ (many types seen)
Generic (give up on specialization)
```

**Implementation:**
```rust
pub struct SpecializationChain {
    current: Box<dyn ASTNode>,
    specialization_count: usize,
}

impl SpecializationChain {
    fn execute(&mut self, env: &Environment) -> Value {
        let result = self.current.execute(env)?;

        // Check if node requested rewrite
        if self.current.should_rewrite() {
            let new_node = self.current.rewrite();
            self.current = new_node;
            self.specialization_count += 1;

            // Prevent infinite rewriting
            if self.specialization_count > MAX_REWRITES {
                self.current = Box::new(GenericNode::new());
            }
        }

        result
    }
}

const MAX_REWRITES: usize = 5;
```

---

## Compared to Full JIT

**What self-optimizing AST gives you:**
- ✅ Type specialization (90% of JIT benefit)
- ✅ Inline caching (fast variable lookup)
- ✅ Constant folding
- ✅ Adaptive optimization
- ❌ No native code generation
- ❌ No register allocation
- ❌ Still pays AST traversal cost

**Expected speedup: 3-10x vs generic interpreter**

---

## Integration with Other Techniques

### With Adaptive Numeric Tower (Section 03):
```rust
// AST node uses type profiles from bytecode
AddNode::Generic { profile } => {
    // Consult global type profile
    let bytecode_profile = vm.get_type_profile(this.callsite_id);

    if bytecode_profile.is_monomorphic() {
        // Specialize based on VM-level profile
        *self = self.specialize_from_bytecode(bytecode_profile);
    }
}
```

### With Meta-Tracing (Section 01):
```rust
// Hot AST paths become trace candidates
if self.execution_count > TRACE_THRESHOLD {
    vm.start_tracing_at_ast_node(self);
}
```

---

## Challenges

**Challenge 1: AST Memory Overhead**
- Problem: Specialized nodes consume more memory
- Solution: Share specialized nodes via interning, limit specializations

**Challenge 2: Thread Safety**
- Problem: Mutable AST nodes in multi-threaded interpreter
- Solution: Copy-on-write, or restrict to single-threaded

**Challenge 3: Deoptimization Loops**
- Problem: Node keeps specializing and deoptimizing
- Solution: Track deopt count, blacklist unstable nodes

---

## Micro-Truffle: Simplified Approach

**Instead of full Truffle complexity:**

```rust
// Simple 3-state specialization
pub enum OptimizationState {
    Unoptimized,
    Specialized(SpecializedVariant),
    Deoptimized,  // Don't try to optimize again
}

pub struct SmartASTNode {
    kind: NodeKind,
    state: OptimizationState,
    execution_count: u32,
}

impl SmartASTNode {
    fn execute(&mut self, env: &Environment) -> Value {
        self.execution_count += 1;

        match &mut self.state {
            OptimizationState::Unoptimized => {
                let result = self.execute_generic(env)?;

                // Try to specialize after warmup
                if self.execution_count > WARMUP_THRESHOLD {
                    if let Some(specialized) = self.try_specialize() {
                        self.state = OptimizationState::Specialized(specialized);
                    }
                }

                result
            }

            OptimizationState::Specialized(variant) => {
                // Try specialized path
                match variant.execute(env) {
                    Ok(result) => result,
                    Err(_type_mismatch) => {
                        // Deopt
                        self.state = OptimizationState::Deoptimized;
                        self.execute_generic(env)
                    }
                }
            }

            OptimizationState::Deoptimized => {
                // Just use generic path
                self.execute_generic(env)
            }
        }
    }
}
```

**Much simpler than full Truffle, still gets most benefits!**

---

## References

1. **"One VM to Rule Them All"** (Würthinger et al., 2013)
   - Truffle framework

2. **"Self-Optimizing AST Interpreters"** (Würthinger et al., 2012)
   - Core concept

3. **"Practical Partial Evaluation for High-Performance Dynamic Language Runtimes"** (Würthinger et al., 2017)
   - Graal + Truffle

4. **"ZipPy: A Fast and Lightweight Python Implementation"** (Daloze et al., 2014)
   - Truffle-based Python

---

## Implementation Timeline

**Week 1-2:** Design smart AST node framework
**Week 3-4:** Implement type specialization for arithmetic
**Week 5-6:** Add inline caching for variables
**Week 7-8:** Benchmarking and refinement

---

## Why (or Why Not)

**Pros:**
- No JIT infrastructure needed
- Portable (pure Rust)
- Easier to debug than JIT
- Good speedup (3-10x)

**Cons:**
- More complex than simple interpreter
- Less speedup than full JIT (only 3-10x vs 100x)
- Memory overhead for specialization
- Diminishing returns if adding tracing JIT later

**Verdict:** Consider only if:
- Don't want complexity of tracing JIT
- Want portable, pure-Rust solution
- 3-10x speedup is good enough

**Otherwise:** Adaptive numeric tower + meta-tracing gives better ROI! 🎯
