# Phase 2: VM Research & Design

**Status:** Research Phase
**Timeline:** After R7RS completion (Phase 1)
**Goal:** Design a modern, high-performance Scheme VM incorporating novel techniques from 2023-2025 research

---

## Overview

This directory contains research documents for advanced VM techniques to explore when building Patina's bytecode VM. These ideas draw from cutting-edge research in programming language implementation, focusing on practical techniques that can be implemented without becoming a multi-year research project.

**Philosophy:**
- **Tree-walker (Phase 1):** Simple, correct, straightforward
- **VM (Phase 2):** Where we have fun with modern techniques!
- **Focus:** Implementable ideas with high impact/effort ratio

---

## Core Architecture Documents

These documents define the foundational VM architecture:

- **[VM_SPECIFICATION.md](./VM_SPECIFICATION.md)** - Complete VM design: bytecode ISA, execution model, memory layout
- **[VM_VALUE_ARCHITECTURE.md](./VM_VALUE_ARCHITECTURE.md)** - Dual value representation design (Value enum vs TaggedValue)
- **[TAGGED_POINTERS.md](./TAGGED_POINTERS.md)** - 8-byte tagged pointer scheme for VM performance
- **[SEXPR_SEPARATION_ARCHITECTURE.md](./SEXPR_SEPARATION_ARCHITECTURE.md)** - Value representation options (**Option D selected**: tree-walker uses Value, VM uses TaggedValue)

---

## Implementation Design Documents

These documents describe how to implement the VM:

- **[COMPILATION_DESIGN.md](./COMPILATION_DESIGN.md)** - VmCoreExpr to bytecode compilation: register allocation, flat closures, TCO
- **[DESUGARER_DESIGN.md](./DESUGARER_DESIGN.md)** - Generic desugarer with `DesugarBackend` trait for producing backend-specific IR
- **[VM_TESTING_DESIGN.md](./VM_TESTING_DESIGN.md)** - Testing framework: backend-agnostic tests, differential testing, benchmarks
- **[FILE_SYSTEM_ABSTRACTION.md](./FILE_SYSTEM_ABSTRACTION.md)** - FileSystem trait for testing, WASM support, and embedded stdlib

---

## Architecture Analysis & Data Type Abstractions

These documents provide architectural insights and abstraction designs:

- **[ARCHITECTURE_LESSONS.md](./ARCHITECTURE_LESSONS.md)** - Comparative analysis with Lua, V8, Chez, Chibi, Guile: value representation, closures, GC, dispatch
- **[STRING_ABSTRACTION_DESIGN.md](./STRING_ABSTRACTION_DESIGN.md)** - SchemeString trait design: swappable string implementations via feature flags

---

## Research Documents

### ⭐ Top Priority (Best Mix of Novelty + Implementability)

1. **[Meta-Tracing on Demand](./01_META_TRACING.md)** ⭐⭐⭐
   - Lightweight tracing JIT that learns hot paths
   - Inspired by PyPy, GraalVM
   - **Complexity:** Medium | **Impact:** Very High
   - Compiles only stable tail-recursive loops to native code

2. **[Effect-Typed Continuations](./02_EFFECT_CONTINUATIONS.md)** ⭐⭐⭐
   - Optimize `call/cc` using effect systems
   - Inspired by algebraic effects research (OCaml, Koka, Effekt)
   - **Complexity:** Medium | **Impact:** High
   - Avoids allocating continuations in most code

3. **[Adaptive Numeric Tower](./03_ADAPTIVE_NUMERIC.md)** ⭐⭐⭐
   - Profile-guided numeric specialization
   - Inspired by Julia, R
   - **Complexity:** Low-Medium | **Impact:** Very High
   - Specialized fast paths for common numeric types

---

### High Interest (More Complex but Very Cool)

4. **[Persistent Heap Snapshots](./04_PERSISTENT_HEAP.md)** ⭐⭐
   - Time-travel debugging via copy-on-write heap
   - Inspired by rr debugger, persistent data structures
   - **Complexity:** Medium-High | **Impact:** High (UX)
   - Nearly free REPL snapshots for time-travel

5. **[Delimited Continuation ISA](./05_DELIMITED_CONTINUATIONS.md)** ⭐⭐
   - Bytecode designed around first-class continuations
   - Inspired by Multicore OCaml, delimited control research
   - **Complexity:** High | **Impact:** Medium-High
   - Unified primitive for generators, async, coroutines

---

### Experimental (Research-Heavy but Fascinating)

6. **[Self-Optimizing AST Nodes](./06_SELF_OPTIMIZING_AST.md)** ⭐
   - Truffle-style specialization without Graal
   - Inspired by "micro-Truffle" research
   - **Complexity:** Medium-High | **Impact:** Medium
   - AST nodes rewrite themselves based on runtime feedback

7. **[Symbolic Execution Fusion](./07_SYMBOLIC_EXECUTION.md)** ⭐
   - Hybrid concrete/symbolic execution
   - Inspired by modern verification UX research
   - **Complexity:** Very High | **Impact:** Medium (UX)
   - REPL can answer "for which values does this crash?"

---

## Recommended Implementation Order

Based on complexity vs impact:

### Phase 2A: Foundation (Choose 1-2)
**Focus:** Get performance wins without too much complexity

1. **Adaptive Numeric Tower** (2-3 weeks)
   - Easiest to implement
   - Immediate 5-10x speedup on numeric code
   - Builds profiling infrastructure for other optimizations

2. **Effect-Typed Continuations** (3-4 weeks)
   - Makes `call/cc` practical
   - Required for good Scheme performance
   - Foundation for delimited continuations later

### Phase 2B: Advanced (Choose 1)
**Focus:** Major performance or UX breakthrough

3. **Meta-Tracing JIT** (6-8 weeks)
   - Biggest performance impact (10-100x on hot loops)
   - Builds on profiling from Phase 2A
   - Can start with interpreter-only, add JIT incrementally

4. **Persistent Heap Snapshots** (4-6 weeks)
   - Best-in-class debugging UX
   - Unique feature (few Schemes have this)
   - Relatively self-contained

### Phase 2C: Experimental (Optional)
**Focus:** Novel research contributions

5. **Delimited Continuation ISA** (8-10 weeks)
   - Rethink entire VM design
   - Publishable if done well
   - High risk, high reward

6. **Self-Optimizing AST** or **Symbolic Execution** (6-12 weeks each)
   - Research-heavy
   - Consider for Phase 3 or academic collaboration

---

## Success Criteria

Each technique should demonstrate:

1. **Measurability:** Clear benchmark showing impact
   - Numeric tower: 5-10x on numeric code
   - Meta-tracing: 10-100x on tight loops
   - Effect continuations: call/cc overhead <10%

2. **Maintainability:** Code remains understandable
   - Document algorithm clearly
   - Keep complexity localized
   - Good test coverage

3. **Incrementality:** Can be implemented in stages
   - Start with basic version
   - Add optimizations incrementally
   - Always have working system

---

## References & Further Reading

**General VM Design:**
- Crafting Interpreters (Bob Nystrom) - Bytecode VM basics
- Virtual Machines (Smith & Nair) - Comprehensive overview

**Tracing JITs:**
- "One VM to Rule Them All" (Würthinger et al., 2013)
- "Tracing the Meta-Level: PyPy's Tracing JIT Compiler" (Bolz et al., 2009)
- "Meta-Tracing Makes a Fast Racket" (Bolz & Tratt, 2021)

**Effect Systems:**
- "Algebraic Effects and Handlers" (Plotkin & Pretnar, 2013)
- "Multicore OCaml" (Dolan et al., 2014-2023)
- "Programming with Algebraic Effects and Handlers" (Bauer & Pretnar, 2015)

**Adaptive Specialization:**
- "Adaptive Optimization in Julia" (Julia team, 2012-2024)
- "Type Feedback for Bytecode Interpreters" (Brunthaler, 2010)

**Delimited Continuations:**
- "A Monadic Framework for Delimited Continuations" (Dybvig et al., 2007)
- "An Operational Foundation for Delimited Continuations" (Materzok & Biernacki, 2011)

**Time-Travel Debugging:**
- "rr: Lightweight Recording & Replay" (O'Callahan et al., 2017)
- "URDB: A Universal Reversible Debugger" (Akgul et al., 2004)

---

## Next Steps

1. **Read all research documents** in this directory
2. **Pick 1-2 techniques** for initial VM implementation
3. **Prototype in isolation** before integrating
4. **Benchmark rigorously** to validate approach
5. **Document findings** for future reference

**Remember:** The goal is a production-quality VM with novel features, not a research prototype. Choose techniques that are:
- ✅ Implementable in reasonable time (2-8 weeks)
- ✅ High impact on performance or UX
- ✅ Maintainable and understandable
- ✅ Novel enough to be interesting

---

**Let's build something cool!** 🚀
