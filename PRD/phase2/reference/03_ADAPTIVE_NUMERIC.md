# Adaptive Numeric Tower

**Priority:** ⭐⭐⭐ High (Best implementability/impact ratio!)
**Complexity:** Low-Medium (2-3 weeks)
**Impact:** Very High (5-10x on numeric code)
**Status:** Research

---

## Overview

Use runtime profiling to specialize numeric operations based on observed types. Instead of always handling the full numeric tower (fixnum, bigint, rational, real, complex), generate specialized fast paths for common cases (e.g., fixnum-only arithmetic).

**Key Insight:** Most code uses only a subset of the numeric tower. Profile at runtime and specialize accordingly.

---

## The Numeric Tower Problem

**R7RS Scheme numeric tower:**
```
complex      (most general)
  ↑
real
  ↑
rational
  ↑
integer
  ↑
fixnum       (most specific)
```

**Naive addition implementation:**
```rust
fn add(a: Value, b: Value) -> Value {
    match (a, b) {
        (Value::Fixnum(x), Value::Fixnum(y)) => {
            // Check overflow
            match x.checked_add(y) {
                Some(result) => Value::Fixnum(result),
                None => Value::BigInt(BigInt::from(x) + BigInt::from(y))
            }
        }
        (Value::Fixnum(x), Value::BigInt(y)) => {
            Value::BigInt(BigInt::from(x) + y)
        }
        (Value::Real(x), Value::Real(y)) => {
            Value::Real(x + y)
        }
        (Value::Complex(r1, i1), Value::Complex(r2, i2)) => {
            Value::Complex(r1 + r2, i1 + i2)
        }
        // ... 20+ more cases for type coercion!
    }
}
```

**Cost:**
- Large match statement (poor branch prediction)
- Type checks on every operation
- Overflow checks
- Promotion overhead (fixnum → bigint → real → complex)

**Result:** Scheme arithmetic is **100-1000x slower than C**

---

## Adaptive Specialization Strategy

**Observe:** Most code is **monomorphic** (uses one numeric type consistently)

```scheme
;; This code ALWAYS uses fixnums
(define (sum n)
  (let loop ((i 0) (acc 0))
    (if (>= i n)
        acc
        (loop (+ i 1) (+ acc i)))))  ; ← Always fixnum + fixnum

;; This code ALWAYS uses reals
(define (avg lst)
  (/ (fold-left + 0.0 lst)
     (length lst)))  ; ← Always float ops
```

**Strategy:**
1. **Profile callsites** - Track observed types at each `+`, `-`, `*`, `/`
2. **Generate specialized versions** - Create fixnum-only, float-only variants
3. **Guard and dispatch** - Check types, use fast path if match, fallback if not

---

## Implementation Plan

### Phase 1: Profiling Infrastructure (3-5 days)

**Add type feedback to bytecode:**

```rust
pub struct TypeProfile {
    callsite_id: usize,
    observed_types: HashMap<TypeSignature, u32>,  // Type combo → count
    total_calls: u32,
}

#[derive(Hash, Eq, PartialEq)]
pub enum TypeSignature {
    FixnumFixnum,      // (fixnum, fixnum) → fixnum
    FloatFloat,        // (float, float) → float
    FixnumFloat,       // (fixnum, float) → float
    Generic,           // Mixed types
}

pub struct Bytecode {
    instructions: Vec<Opcode>,
    type_profiles: HashMap<usize, TypeProfile>,  // callsite → profile
}
```

**Collect profiles during execution:**
```rust
impl VM {
    fn execute_add(&mut self, dst: Register, src1: Register, src2: Register, callsite: usize) {
        let val1 = self.get_register(src1);
        let val2 = self.get_register(src2);

        // Record observed types
        let sig = self.get_type_signature(val1, val2);
        self.record_type_profile(callsite, sig);

        // Execute generic add
        let result = self.generic_add(val1, val2);
        self.set_register(dst, result);
    }

    fn record_type_profile(&mut self, callsite: usize, sig: TypeSignature) {
        let profile = self.type_profiles.entry(callsite).or_insert(TypeProfile::new());
        *profile.observed_types.entry(sig).or_insert(0) += 1;
        profile.total_calls += 1;

        // Check if we should specialize
        if profile.total_calls > SPECIALIZATION_THRESHOLD {
            self.maybe_specialize(callsite, profile);
        }
    }
}
```

**Constants:**
```rust
const SPECIALIZATION_THRESHOLD: u32 = 100;  // Specialize after 100 calls
const MONOMORPHIC_RATIO: f32 = 0.95;        // 95%+ same type = monomorphic
```

---

### Phase 2: Specialized Code Generation (1 week)

**Generate specialized bytecode variants:**

```rust
impl VM {
    fn maybe_specialize(&mut self, callsite: usize, profile: &TypeProfile) {
        // Check if monomorphic (>95% same type)
        if let Some((dominant_sig, count)) = profile.most_common_type() {
            let ratio = count as f32 / profile.total_calls as f32;

            if ratio > MONOMORPHIC_RATIO {
                // Generate specialized version
                match dominant_sig {
                    TypeSignature::FixnumFixnum => {
                        self.emit_fixnum_add_specialized(callsite);
                    }
                    TypeSignature::FloatFloat => {
                        self.emit_float_add_specialized(callsite);
                    }
                    _ => {} // Don't specialize complex cases
                }
            }
        }
    }

    fn emit_fixnum_add_specialized(&mut self, callsite: usize) {
        // Replace generic add with:
        // 1. Guard: check both args are fixnum
        // 2. Fast path: fixnum addition with overflow check
        // 3. Deopt: fallback to generic if guard fails

        let specialized = vec![
            Opcode::GuardFixnum { reg: src1, deopt_label },
            Opcode::GuardFixnum { reg: src2, deopt_label },
            Opcode::FixnumAddChecked { dst, src1, src2, overflow_label },
            Opcode::Jump(done_label),

            // Overflow path: promote to bigint
            Opcode::Label(overflow_label),
            Opcode::PromoteToBigInt { src1 },
            Opcode::PromoteToBigInt { src2 },
            Opcode::BigIntAdd { dst, src1, src2 },
            Opcode::Jump(done_label),

            // Deopt path: fallback to generic
            Opcode::Label(deopt_label),
            Opcode::GenericAdd { dst, src1, src2 },

            Opcode::Label(done_label),
        ];

        self.patch_bytecode(callsite, specialized);
    }
}
```

**Specialized opcodes:**
```rust
pub enum Opcode {
    // Generic (slow)
    Add { dst: Register, src1: Register, src2: Register },

    // Specialized (fast)
    FixnumAddChecked { dst: Register, src1: Register, src2: Register, overflow: Label },
    FloatAdd { dst: Register, src1: Register, src2: Register },

    // Guards
    GuardFixnum { reg: Register, deopt: Label },
    GuardFloat { reg: Register, deopt: Label },

    // Type conversions
    PromoteToBigInt { src: Register },
    PromoteToFloat { src: Register },
}
```

---

### Phase 3: Guard Optimization (3-5 days)

**Avoid redundant guards:**

```rust
// Bad: Check type on every operation
loop {
    guard_fixnum(x);
    guard_fixnum(y);
    z = x + y;
    guard_fixnum(z);
    guard_fixnum(a);
    w = z + a;
}

// Good: Hoist loop-invariant guards
guard_fixnum(x);  // Once, outside loop
guard_fixnum(y);
loop {
    z = x + y;  // No guard needed (x, y are fixnum, z must be)
    guard_fixnum(a);
    w = z + a;
}
```

**Implementation:**
```rust
pub struct GuardHoisting {
    known_types: HashMap<Register, NumericType>,
}

impl GuardHoisting {
    fn optimize_guards(&mut self, bytecode: &mut [Opcode]) {
        for opcode in bytecode {
            match opcode {
                Opcode::GuardFixnum { reg, .. } => {
                    if self.known_types.get(reg) == Some(&NumericType::Fixnum) {
                        // Remove redundant guard
                        *opcode = Opcode::Nop;
                    } else {
                        // Record type
                        self.known_types.insert(*reg, NumericType::Fixnum);
                    }
                }
                Opcode::FixnumAddChecked { dst, src1, src2, .. } => {
                    // If both sources are fixnum, result is likely fixnum
                    // (unless overflow, but that's rare)
                    if self.known_types.get(src1) == Some(&NumericType::Fixnum)
                        && self.known_types.get(src2) == Some(&NumericType::Fixnum) {
                        // Optimistically assume no overflow
                        self.known_types.insert(*dst, NumericType::Fixnum);
                    }
                }
                _ => {}
            }
        }
    }
}
```

---

### Phase 4: Polymorphic Inline Caching (Optional, 3-5 days)

**Handle polymorphic callsites (2-3 types):**

```rust
// Observed types at callsite:
// 70% fixnum + fixnum
// 25% float + float
// 5% mixed

// Generated code:
// Fast path 1: fixnum
guard_fixnum(src1) && guard_fixnum(src2)?
  fixnum_add(dst, src1, src2)

// Fast path 2: float
: guard_float(src1) && guard_float(src2)?
  float_add(dst, src1, src2)

// Slow path: generic
: generic_add(dst, src1, src2)
```

**Implementation:**
```rust
pub enum SpecializedAdd {
    Monomorphic(TypeSignature, Vec<Opcode>),
    Polymorphic {
        variants: Vec<(TypeSignature, Vec<Opcode>)>,
        fallback: Vec<Opcode>,
    },
}
```

---

## Expected Performance

**Benchmark: Sum 1-1000000**

```scheme
(define (sum n)
  (let loop ((i 0) (acc 0))
    (if (>= i n)
        acc
        (loop (+ i 1) (+ acc i)))))
```

| Implementation | Time | Speedup |
|---------------|------|---------|
| Generic numeric tower | 10s | 1x |
| Specialized fixnum | 1s | 10x |
| Native C | 0.1s | 100x |

**Why 10x faster:**
- No type dispatch (direct fixnum arithmetic)
- No overflow checks in common case (or fast checks)
- Better branch prediction
- Smaller code size (fits in icache)

---

### Benchmark: Floating-point Average

```scheme
(define (avg lst)
  (/ (fold-left + 0.0 lst)
     (length lst)))
```

| Implementation | Time (1M floats) | Speedup |
|---------------|------------------|---------|
| Generic numeric tower | 5s | 1x |
| Specialized float | 0.5s | 10x |
| Native C | 0.1s | 50x |

---

## Adaptive Strategy: Learning Over Time

**Cold start (first 100 calls):**
```
Call 1-100: Use generic arithmetic, collect type profiles
```

**Warm-up (100-1000 calls):**
```
Call 101: Specialize based on observed types
Call 102-1000: Use specialized version, track deopt rate
```

**Steady state (1000+ calls):**
```
If deopt rate < 5%: Keep specialized version
If deopt rate > 20%: Revert to generic or re-specialize
```

**Adaptive refinement:**
```rust
impl VM {
    fn track_deoptimization(&mut self, callsite: usize) {
        let stats = &mut self.specialization_stats[callsite];
        stats.deopt_count += 1;

        let deopt_rate = stats.deopt_count as f32 / stats.total_calls as f32;

        if deopt_rate > DEOPT_THRESHOLD {
            // Too many deoptimizations, revert to generic
            self.despecialize(callsite);
        }
    }
}

const DEOPT_THRESHOLD: f32 = 0.2;  // 20% deopt rate
```

---

## Integration with Tracing JIT

**Synergy with meta-tracing:**

1. **Type profiling identifies hot paths** for tracing
2. **Specialized bytecode** is easier to trace (fewer branches)
3. **Traced code** can inline specialized arithmetic

**Example:**
```rust
// Phase 1: Adaptive specialization identifies fixnum loop
guard_fixnum(i);
guard_fixnum(acc);
loop {
    i = fixnum_add(i, 1);
    acc = fixnum_add(acc, i);
}

// Phase 2: Meta-tracer compiles to native
// mov rax, [i]
// add rax, 1        ; ← Direct CPU instruction!
// mov [i], rax
// mov rbx, [acc]
// add rbx, rax
// mov [acc], rbx
```

**Combined speedup: 100x (10x from specialization × 10x from tracing)**

---

## Challenges & Solutions

### Challenge 1: Over-specialization
**Problem:** Generate too many specialized versions (code bloat)

**Solution:**
- Limit max specializations per callsite (e.g., 3)
- Evict least-recently-used specializations
- Only specialize hot callsites (>90% of runtime)

### Challenge 2: Deoptimization Overhead
**Problem:** Frequent deopt kills performance

**Solution:**
- Track deopt reasons, adjust guards
- Polymorphic inline cache (2-3 variants)
- Blacklist unstable callsites

### Challenge 3: Memory Overhead
**Problem:** Type profiles consume memory

**Solution:**
- Use compact representation (bit vectors)
- Sample profiling (every Nth call)
- Discard profiles for cold code

---

## Scheme-Specific Optimizations

### Exact Integer Arithmetic
```scheme
;; Scheme requires exact integer arithmetic (no overflow)
(+ 9223372036854775807 1)  ; ← Must produce exact bigint, not wrap
```

**Optimization:**
```rust
// Most additions don't overflow
Opcode::FixnumAddChecked { dst, src1, src2, overflow_label }

// Emit:
// mov rax, [src1]
// add rax, [src2]
// jo overflow_label      ; ← Jump on overflow (rare)
// mov [dst], rax
```

**Fast path: 1 cycle** (when no overflow)
**Slow path: Promote to bigint** (rare)

### Generic Number Coercion
```scheme
;; Scheme auto-coerces: fixnum + float → float
(+ 42 3.14)  ; ← 45.14
```

**Optimization:**
```rust
// Polymorphic inline cache
if is_fixnum(src1) && is_fixnum(src2) {
    fixnum_add(dst, src1, src2);
} else if is_number(src1) && is_number(src2) {
    // Coerce to common type
    let (a, b) = coerce_to_common_type(src1, src2);
    specialized_add_for_type(dst, a, b);
} else {
    error("Type error");
}
```

---

## References

**Type Specialization:**
1. "Self: The Power of Simplicity" (Ungar & Smith, 1991)
   - Pioneering work on polymorphic inline caching

2. "Adaptive Optimization in the Java HotSpot VM" (Paleczny et al., 2001)
   - Type profiling and adaptive specialization

3. "Julia: A Fast Dynamic Language for Technical Computing" (Bezanson et al., 2012)
   - Multiple dispatch with type specialization

**Implementation References:**
- PyPy's numeric specialization
- V8's hidden classes and inline caches
- Julia's type inference and specialization

---

## Next Steps

1. **Week 1:** Implement type profiling infrastructure
2. **Week 2:** Add specialized fixnum arithmetic opcodes
3. **Week 3:** Guard optimization and benchmarking

**Milestone:** Fixnum-heavy code runs 10x faster

**Success Metric:**
- Numeric benchmarks: 5-10x speedup
- Minimal deoptimization (<5% of calls)
- Low memory overhead (<1MB for profiles)

---

## Why This Matters

**Current state:** Scheme arithmetic is slow (100-1000x slower than C)

**With adaptive specialization:**
- Common case (fixnum, float) is fast (10-50x slower than C)
- Uncommon case (bigint, rational, complex) works correctly
- No static types required!

**Result:** Scheme gets competitive performance on numeric code 🚀

**Best part:** This is the **easiest** high-impact optimization to implement!
