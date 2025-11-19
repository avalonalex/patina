# Patina Development Roadmap

Overall roadmap for Patina's four development phases.

## Current Status (2025-11-02)

**Phase 1: R7RS Compliance** - 47% complete

Major milestone: ✅ Lambda with full closure support implemented!

## Phase 1: R7RS-small Compliance (Current)

**Goal:** Complete, correct R7RS-small Scheme interpreter

**Timeline:** Weeks 1-16 (4 months)

**Status:** Week 4 - 47% complete

### Recent Accomplishments

- ✅ **Week 1-3**: Core infrastructure (lexer, parser, evaluator, REPL)
- ✅ **Week 4**: Lambda with closures (COMPLETE!)
  - Fixed/variadic/mixed arity
  - Proper environment capture
  - Higher-order functions working

### Next Priorities

**Weeks 5-6: Binding Forms**
- [ ] `let` - Local bindings (blocks 23% of failing tests!)
- [ ] `let*` - Sequential bindings
- [ ] `letrec` - Recursive bindings
- [ ] Boolean operators: `and`, `or`

**Weeks 7-8: Higher-Order & Lists**
- [ ] `apply` - Apply procedure to list
- [ ] `map`, `for-each` - List iteration
- [ ] `length`, `append`, `reverse` - List operations
- [ ] `list-ref`, `list-tail` - List access

**Weeks 9-10: Numbers**
- [ ] `abs`, `quotient`, `remainder`, `modulo`
- [ ] `zero?`, `positive?`, `negative?`, `odd?`, `even?`
- [ ] `max`, `min`
- [ ] `floor`, `ceiling`, `truncate`, `round`
- [ ] Better numeric tower support

**Weeks 11-12: Strings & Vectors**
- [ ] String operations (length, ref, append, etc.)
- [ ] Vector operations (length, ref, set!, etc.)
- [ ] Character operations

**Weeks 13-14: I/O Basics**
- [ ] `display`, `write`, `newline`
- [ ] String ports
- [ ] Basic file I/O

**Weeks 15-16: Final Push**
- [ ] `case` - Pattern matching
- [ ] Exception handling (`error`, `guard`)
- [ ] Remaining predicates
- [ ] Bug fixes and polish

**Detailed Plan:** See [phase1/R7RS_ROADMAP.md](phase1/R7RS_ROADMAP.md)

### Success Criteria

- [ ] 95%+ R7RS compliance test pass rate
- [ ] All `(scheme base)` procedures implemented
- [ ] Passes chibi-scheme R7RS test suite
- [ ] Documentation complete
- [ ] Examples and tutorials written

## Phase 2: Gradual Typing (Future)

**Goal:** Add optional static typing à la Typed Racket

**Timeline:** Weeks 17-28 (3 months)

**Status:** Planned

### Features

- Type annotations with `:`
  ```scheme
  (: factorial (Integer -> Integer))
  (define (factorial n)
    ...)
  ```
- Type inference where possible
- Gradual typing (typed/untyped code mix)
- Contract-based boundaries
- Occurrence typing
- Polymorphism with type variables

### Benefits

- Catch errors at "compile time"
- Better documentation
- IDE support potential
- Performance hints for optimizer

**Detailed Plan:** TBD in `phase2/` directory

## Phase 3: Reactive Concurrency (Future)

**Goal:** Add reactive programming primitives

**Timeline:** Weeks 29-40 (3 months)

**Status:** Planned

### Features

- Observable streams
  ```scheme
  (define stream (observable-from '(1 2 3 4 5)))
  (subscribe (map (* 2)) stream)
  ```
- Reactive operators (map, filter, flatMap, etc.)
- Async/await semantics
- Backpressure handling
- Hot/cold observables
- Schedulers

### Inspiration

- Project Reactor (Java)
- RxJS (JavaScript)
- Elixir's GenStage

**Detailed Plan:** TBD in `phase3/` directory

## Phase 4: Notebook System (Future)

**Goal:** Terminal-based computational notebook

**Timeline:** Weeks 41-56 (4 months)

**Status:** Designed, not implemented

### Features

- S-expression notebook format (`.scm.nb`)
- Cell-based editing in terminal
- Three-tier command system:
  1. Native Scheme
  2. Table operations
  3. Shell integration
- Dependency tracking
- Reproducible execution
- Export to various formats

### Design Complete

All design documents in [phase4/](phase4/):
- Format specification
- Architecture design
- TUI implementation plan
- Command system design
- Integration strategies

**Detailed Plan:** See [phase4/NOTEBOOK_OVERVIEW.md](phase4/NOTEBOOK_OVERVIEW.md)

## Timeline Overview

```
Month 1-2:  Phase 1 Foundation       ✅ DONE
Month 3-4:  Phase 1 Completion       🚧 IN PROGRESS (47%)
Month 5-7:  Phase 2 Gradual Typing   ⏸️  Planned
Month 8-10: Phase 3 Reactive         ⏸️  Planned
Month 11-14: Phase 4 Notebook        ⏸️  Planned
```

## Dependencies

```
Phase 1 (R7RS)
    ↓
Phase 2 (Typing) ────┐
    ↓                │
Phase 3 (Reactive) ──┼─→ Phase 4 (Notebook)
                     │
          (Optional)─┘
```

- Phase 2 & 3 are independent (can be done in parallel or skipped)
- Phase 4 requires Phase 1
- Phase 4 enhanced by Phases 2 & 3 but doesn't require them

## Risk Mitigation

### Phase 1 Risks

- **Tail call optimization complexity** → Start with trampolining, optimize later
- **Macro hygiene complexity** → Use simple syntax-rules first, expand later
- **Numeric tower completeness** → Prioritize commonly-used operations

### Phase 2 Risks

- **Type system complexity** → Start with simple types, add gradually
- **Backward compatibility** → Keep untyped code working
- **Learning curve** → Extensive documentation and examples

### Phase 3 Risks

- **Performance overhead** → Benchmark and optimize critical paths
- **API design** → Study existing reactive libraries
- **Complexity** → Keep core simple, add operators incrementally

### Phase 4 Risks

- **TUI complexity** → Use proven library (Ratatui)
- **Format stability** → Design carefully upfront
- **Scope creep** → MVP first, enhance later

## Success Metrics

### Phase 1
- 95%+ R7RS test pass rate
- Chibi-scheme compatibility
- Good error messages
- 1000+ lines of example code

### Phase 2
- Type checker catches 80%+ of runtime errors
- Smooth gradual typing experience
- Good type error messages
- 500+ lines of typed examples

### Phase 3
- Standard reactive operators implemented
- Low overhead (<20% vs non-reactive)
- Clear documentation
- 300+ lines of reactive examples

### Phase 4
- Notebook format stable
- Pleasant TUI experience
- Shell integration working
- 50+ example notebooks

## Community & Adoption

### Target Audiences

1. **Phase 1:** Scheme learners, educators, R7RS users
2. **Phase 2:** Typed functional programming enthusiasts
3. **Phase 3:** Reactive programming users, async developers
4. **Phase 4:** Data scientists, researchers, notebook users

### Documentation Strategy

- Comprehensive guides for each phase
- Example programs demonstrating features
- Video tutorials (Phase 4)
- Blog posts on design decisions

## Related Documents

- [PROJECT_VISION.md](PROJECT_VISION.md) - Overall project vision
- [phase1/R7RS_ROADMAP.md](phase1/R7RS_ROADMAP.md) - Detailed Phase 1 plan
- [phase4/NOTEBOOK_OVERVIEW.md](phase4/NOTEBOOK_OVERVIEW.md) - Phase 4 design summary
- [NEXT_STEPS.md](NEXT_STEPS.md) - Immediate next steps (may be outdated)

## Updates

This roadmap is updated after each major milestone:
- 2025-11-02: Lambda implementation complete, updated Phase 1 progress
- Previous updates: See git history

---

For current implementation status, see [../docs/FEATURE_STATUS.md](../docs/FEATURE_STATUS.md).
