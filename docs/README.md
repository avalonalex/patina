# Patina Documentation

Documentation for **completed and implemented features** in Patina.

## Contents

| Document | Description |
|----------|-------------|
| [MACRO_SYSTEM.md](MACRO_SYSTEM.md) | Macro system architecture (syntax-rules, hygiene, scope sets) |
| [TEST_ORGANIZATION.md](TEST_ORGANIZATION.md) | Test structure, running tests, and test guidelines |
| [reference_impls/](reference_impls/) | Notes on reference Scheme implementations (Chibi, Chez, Gauche, Koka) |

### VM Backend (Phase 2A — complete)

| Document | Description |
|----------|-------------|
| [VM_DECISIONS.md](VM_DECISIONS.md) | Settled architecture decisions (master reference) |
| [VM_ISA.md](VM_ISA.md) | Instruction set architecture and semantics |
| [VM_COMPILER.md](VM_COMPILER.md) | 2 pre-passes + 5-pass compiler pipeline |
| [VM_RUNTIME.md](VM_RUNTIME.md) | VmState, execution loop, control primitives |
| [VM_TESTING.md](VM_TESTING.md) | Testing layers and commands |

## Current Status

Both backends achieve **100% R7RS-small compliance**:
- **VM backend:** 1226/1226 chibi r7rs-tests.scm passing
- **Tree-walker:** 1226/1226 chibi r7rs-tests.scm passing
