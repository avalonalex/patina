# Product Requirements & Design Documents

This directory contains proposals, designs, and roadmaps for Patina's development phases.

## Project Vision

See [PROJECT_VISION.md](PROJECT_VISION.md) for the overall project vision and goals.

## Development Phases

Patina is being developed in four major phases:

### Phase 1: R7RS-small Compliance (Current)
**Status**: In Progress (47% complete)

Implement a complete R7RS-small Scheme interpreter.

- [R7RS Roadmap](phase1/R7RS_ROADMAP.md) - Detailed implementation roadmap

**Current Focus:**
- ✅ Lambda with closures (Complete!)
- 🚧 let/let*/letrec binding forms
- 🚧 Boolean operators (and, or)
- 🚧 Core list operations

### Phase 2: Gradual Typing (Future)
**Status**: Planned

Add optional static typing inspired by Typed Racket.

- Type annotations
- Type inference
- Gradual type checking
- Contract-based boundaries

### Phase 3: Reactive Concurrency (Future)
**Status**: Planned

Add reactive programming primitives inspired by Project Reactor.

- Observable streams
- Reactive operators
- Async/await semantics
- Backpressure handling

### Phase 4: Notebook System (Future)
**Status**: Designed

Terminal-based computational notebook with S-expression format.

**Design Documents:**
- [Notebook Overview](phase4/NOTEBOOK_OVERVIEW.md) - High-level summary
- [Notebook Format](phase4/NOTEBOOK_FORMAT.md) - File format specification
- [Notebook Design](phase4/NOTEBOOK_DESIGN.md) - Architecture and implementation
- [TUI Implementation](phase4/TUI_IMPLEMENTATION.md) - Terminal UI design
- [System Integration](phase4/SYSTEM_INTEGRATION.md) - Three-tier command system
- [Native Commands](phase4/NATIVE_COMMANDS.md) - Built-in commands
- [Design Decisions](phase4/DESIGN_DECISIONS.md) - Key design choices
- [Reproducibility](phase4/REPRODUCIBILITY.md) - Execution model
- [REPL Features](phase4/REPL_FEATURES.md) - Enhanced REPL

## Contributing

Before implementing new features, check:
1. Current phase objectives
2. Existing design documents
3. [Feature status](../docs/FEATURE_STATUS.md)

For Phase 1 (current), see [Development Guide](../docs/DEVELOPMENT.md).
