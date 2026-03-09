# Symbolic Execution + Interpretation Fusion

**Priority:** ⭐ Low-Medium (Research-heavy)
**Complexity:** Very High (8-12 weeks)
**Impact:** Medium (UX - smart REPL assistance)
**Status:** Research / Experimental

---

## Overview

Run parts of a Scheme program symbolically instead of concretely, maintaining both concrete values and symbolic constraints. This enables the REPL to answer questions like "for which values does this crash?" or "what inputs make this branch unreachable?"

**Key Insight:** Symbolic execution is usually for verification, but we can use it for interactive development assistance.

---

## What is Symbolic Execution?

**Concrete execution:**
```scheme
(define (divide a b)
  (if (= b 0)
      'error
      (/ a b)))

(divide 10 2)  ; → 5 (concrete value)
```

**Symbolic execution:**
```scheme
(divide 10 b)  ; where b is symbolic

; Explores both paths:
; Path 1: b = 0 → 'error
; Path 2: b ≠ 0 → (/ 10 b)

; REPL can answer:
; "When does (divide 10 b) return 'error?"
; → "When b = 0"
```

---

## Hybrid Concrete/Symbolic Values

```rust
pub enum Value {
    Concrete(ConcreteValue),
    Symbolic(SymbolicValue),
    Mixed(ConcreteValue, SymbolicValue),  // Both!
}

pub struct ConcreteValue {
    // Normal Scheme value
    inner: SchemeValue,
}

pub struct SymbolicValue {
    expr: SymbolicExpr,
    constraints: Vec<Constraint>,
}

pub enum SymbolicExpr {
    Variable(Symbol),                    // α
    Constant(ConcreteValue),             // 42
    BinaryOp(BinOp, Box<SymbolicExpr>, Box<SymbolicExpr>),  // (+ α β)
}

pub enum Constraint {
    Equal(SymbolicExpr, SymbolicExpr),      // α = β
    NotEqual(SymbolicExpr, SymbolicExpr),   // α ≠ β
    GreaterThan(SymbolicExpr, SymbolicExpr), // α > β
    // ...
}
```

---

## Execution Model

**Path exploration:**
```rust
impl VM {
    fn eval_symbolic(&mut self, expr: &Expr, env: &Environment) -> Vec<ExecutionPath> {
        match expr {
            Expr::If { condition, then_branch, else_branch } => {
                let cond_value = self.eval_symbolic(condition, env)?;

                match cond_value {
                    Value::Concrete(v) => {
                        // Concrete: take one path
                        if v.is_truthy() {
                            vec![self.eval_symbolic(then_branch, env)?]
                        } else {
                            vec![self.eval_symbolic(else_branch, env)?]
                        }
                    }

                    Value::Symbolic(sym) => {
                        // Symbolic: explore BOTH paths
                        let mut paths = vec![];

                        // Path 1: condition is true
                        let mut true_vm = self.clone();
                        true_vm.add_constraint(Constraint::IsTrue(sym.clone()));
                        if true_vm.is_satisfiable() {
                            paths.push(true_vm.eval_symbolic(then_branch, env)?);
                        }

                        // Path 2: condition is false
                        let mut false_vm = self.clone();
                        false_vm.add_constraint(Constraint::IsFalse(sym.clone()));
                        if false_vm.is_satisfiable() {
                            paths.push(false_vm.eval_symbolic(else_branch, env)?);
                        }

                        paths
                    }

                    Value::Mixed(concrete, symbolic) => {
                        // Use concrete value for execution,
                        // but track symbolic constraints
                        // ...
                    }
                }
            }
            _ => { /* ... */ }
        }
    }
}

pub struct ExecutionPath {
    result: Value,
    constraints: Vec<Constraint>,
    path_condition: PathCondition,
}
```

---

## REPL Integration

**Interactive symbolic execution:**

```scheme
patina> (define (safe-divide a b)
         (if (= b 0)
             #f
             (/ a b)))

patina> (symbolic-eval '(safe-divide 10 x))
Exploring symbolic paths...

Path 1: x = 0
  Result: #f
  Constraints: [x = 0]

Path 2: x ≠ 0
  Result: (/ 10 x)
  Constraints: [x ≠ 0]

patina> (when-is '(safe-divide 10 x) '#f)
When x = 0

patina> (when-crashes '(safe-divide 10 x))
Never crashes (both paths return valid values)

patina> (find-input '(lambda (x) (> (safe-divide 10 x) 5)))
Example input: x = 1
  Path: x ≠ 0 → (/ 10 1) = 10 > 5 ✓
```

---

## Constraint Solving

**Use SMT solver for path feasibility:**

```rust
use z3::*;  // Z3 SMT solver

pub struct ConstraintSolver {
    ctx: Context,
    solver: Solver,
}

impl ConstraintSolver {
    fn is_satisfiable(&self, constraints: &[Constraint]) -> bool {
        for constraint in constraints {
            self.add_constraint(constraint);
        }

        self.solver.check() == SatResult::Sat
    }

    fn find_model(&self, constraints: &[Constraint]) -> Option<Model> {
        self.add_constraints(constraints);

        if self.solver.check() == SatResult::Sat {
            self.solver.get_model()
        } else {
            None
        }
    }

    fn add_constraint(&self, constraint: &Constraint) {
        match constraint {
            Constraint::Equal(lhs, rhs) => {
                let lhs_expr = self.symbolic_to_z3(lhs);
                let rhs_expr = self.symbolic_to_z3(rhs);
                self.solver.assert(&lhs_expr._eq(&rhs_expr));
            }
            Constraint::GreaterThan(lhs, rhs) => {
                let lhs_expr = self.symbolic_to_z3(lhs);
                let rhs_expr = self.symbolic_to_z3(rhs);
                self.solver.assert(&lhs_expr.gt(&rhs_expr));
            }
            // ...
        }
    }
}
```

---

## Use Cases

### 1. Debugging: Find Crashing Inputs

```scheme
patina> (define (buggy-parse str)
         (if (< (string-length str) 5)
             (string-ref str 10)  ; BUG: out of bounds!
             'ok))

patina> (find-crash-input 'buggy-parse)
Input causing crash: str with length < 5
Example: "abc"
Reason: string-ref index 10 out of bounds (length = 3)
```

### 2. Test Generation

```scheme
patina> (define (classify-triangle a b c)
         (cond
           ((or (<= a 0) (<= b 0) (<= c 0)) 'invalid)
           ((and (= a b) (= b c)) 'equilateral)
           ((or (= a b) (= b c) (= a c)) 'isosceles)
           (else 'scalene)))

patina> (generate-tests 'classify-triangle 3)
Generated test inputs covering all branches:
1. (0, 1, 1) → 'invalid        (a <= 0)
2. (5, 5, 5) → 'equilateral    (a = b = c)
3. (3, 3, 5) → 'isosceles      (a = b)
4. (3, 4, 5) → 'scalene        (all different)
```

### 3. Unreachable Code Detection

```scheme
patina> (define (dead-code x)
         (if (> x 10)
             'large
             (if (< x 0)
                 'negative
                 (if (> x 20)  ; UNREACHABLE!
                     'very-large
                     'small))))

patina> (check-reachability 'dead-code)
Warning: Unreachable branch detected
  Line 5: (if (> x 20) 'very-large 'small)
  Reason: Path constraint [x > 10, x >= 0] implies x in [0, 10]
         which contradicts x > 20
```

---

## Challenges

### Challenge 1: Path Explosion
**Problem:** Exponential number of paths (2^n for n branches)

**Solution:**
- Bound exploration depth
- Heuristic path prioritization
- Symbolic execution only on request (not automatic)

### Challenge 2: Complex Constraints
**Problem:** SMT solver can't handle all Scheme operations

**Solution:**
- Support only tractable theories (linear arithmetic, equality)
- Fall back to concrete execution for complex operations
- Approximate constraints when exact solving fails

### Challenge 3: Performance
**Problem:** Symbolic execution is slow

**Solution:**
- Only use interactively (user-requested)
- Cache symbolic execution results
- Incremental constraint solving

---

## Minimal Implementation

**Start simple:**

```rust
pub struct SimpleSymbolicVM {
    // Only handle integers and booleans
    symbolic_vars: HashMap<Symbol, SymbolicInt>,
    concrete_vm: VM,  // Fallback to concrete
}

pub struct SymbolicInt {
    expr: IntExpr,
    constraints: Vec<IntConstraint>,
}

pub enum IntExpr {
    Var(Symbol),
    Const(i64),
    Add(Box<IntExpr>, Box<IntExpr>),
    Sub(Box<IntExpr>, Box<IntExpr>),
}

pub enum IntConstraint {
    Eq(IntExpr, IntExpr),
    Lt(IntExpr, IntExpr),
    Gt(IntExpr, IntExpr),
}
```

**Limitations:**
- Only integer arithmetic
- No lists, strings, complex data
- Bounded path exploration
- Manual invocation only

**Still useful for:**
- Finding numeric bugs
- Test input generation for simple functions
- Demonstrating concept

---

## Implementation Timeline

**Week 1-2:** Design symbolic value representation
**Week 3-4:** Implement integer symbolic execution
**Week 5-6:** Integrate Z3 SMT solver
**Week 7-8:** REPL commands (when-is, find-input, etc.)
**Week 9-10:** Path exploration strategies
**Week 11-12:** Polish UX and documentation

---

## References

1. **"KLEE: Unassisted and Automatic Generation of High-Coverage Tests for Complex Systems Programs"** (Cadar et al., 2008)
   - Classic symbolic execution tool

2. **"Rosette: A Solver-Aided Programming Language"** (Torlak & Bodik, 2013-2014)
   - Symbolic execution for Racket (Scheme-like)

3. **"Angr: A Binary Analysis Framework"** (Shoshitaishvili et al., 2016)
   - Modern symbolic execution engine

4. **Z3 Theorem Prover** (Microsoft Research)
   - SMT solver for constraint solving

---

## Alternative: Simpler Approaches

### 1. Concolic Testing (Easier)
- Run concrete execution, collect path constraints
- Generate new inputs to explore other paths
- No full symbolic execution needed

### 2. Property-Based Testing (Easier)
- Generate random inputs (QuickCheck-style)
- No symbolic execution at all
- Still finds bugs effectively

### 3. Type-Directed Testing (Easier)
- Use type information to generate tests
- Simpler than symbolic execution

---

## Verdict

**Symbolic execution is cool but:**
- Very high complexity (12+ weeks)
- Limited practical benefit for most users
- Better alternatives exist (property testing)

**Recommendation:**
- Only pursue if very interested in research
- Or as Phase 3+ (after VM is mature)
- Consider simpler alternatives first

**Priority:** Low (cool research project, not critical for VM) 🔬

---

## But If You Do It...

It would be **unique** - no other Scheme has this!

Could enable:
- "Smart REPL" with AI-like assistance
- Automatic test generation
- Verification assistance
- Publishable research 📝

Just be prepared for the complexity! 😅
