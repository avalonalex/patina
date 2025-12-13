# Patina Documentation

Documentation for **completed and implemented features** in Patina.

## Contents

| Document | Description |
|----------|-------------|
| [MACRO_SYSTEM.md](MACRO_SYSTEM.md) | Comprehensive macro system architecture (syntax-rules, hygiene, scope sets) |
| [TEST_ORGANIZATION.md](TEST_ORGANIZATION.md) | Test structure, running tests, and test guidelines |
| [reference_impls/](reference_impls/) | Notes on reference Scheme implementations (Chibi, Chez, Gauche, Koka) |

## Guidelines

This directory is for documentation of **implemented features only**.

- **Planning docs** go in `PRD/phase1/` or `PRD/phase2/`
- **Research docs** go in `PRD/` subdirectories
- **Outdated docs** get archived to `PRD/ARCHIVE/`

## Current Status

Patina has achieved **100% R7RS-small compliance** (1159/1159 chibi r7rs-tests.scm passing).

See `scheme_tests/reports/compatibility.md` for the latest test results.
