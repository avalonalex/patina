# Macro System PRDs

This directory contains all design documents for Patina's macro system evolution, from the current R7RS-small `syntax-rules` to full R7RS-large compliance.

## Current State

Patina has a complete R7RS-small `syntax-rules` implementation with Racket-style scope-set hygiene (1226/1226 chibi tests passing). The macro expander lives in `crates/patina-macros/`.

## Document Index

### Overview
- **[R7RS_LARGE_GAP_ANALYSIS.md](R7RS_LARGE_GAP_ANALYSIS.md)** — Complete gap analysis against the R7RS-large macro fascicle. Lists every feature, its status, effort estimate, and dependencies.

### syntax-rules Extensions
- **[TAIL_PATTERNS.md](TAIL_PATTERNS.md)** — Tail patterns (`x ... y z`), ported from Chez Scheme's `each+` algorithm. Independent, small scope (~200-300 lines).
- **[CONSECUTIVE_ELLIPSES.md](CONSECUTIVE_ELLIPSES.md)** — Cartesian product semantics for `... ...` in templates, ported from Racket. Independent, small scope (~150-250 lines).
- **[SRFI_149_ADVANCED_MACROS.md](SRFI_149_ADVANCED_MACROS.md)** — Original catalog of non-R7RS patterns removed from test suite. Historical reference.

### Keyword Bindings
- **[SYNTAX_KEYWORD_BINDINGS_DESIGN.md](SYNTAX_KEYWORD_BINDINGS_DESIGN.md)** — Give core syntactic keywords real bindings instead of recognizing them by spelling, so import sets and export resolution reach them through the ordinary path. Retires three workarounds; fixes six conformance defects including a recorded backend divergence. Two staged PRs.

### syntax-case System
- **[SYNTAX_CASE_DESIGN.md](SYNTAX_CASE_DESIGN.md)** — Core `syntax-case` implementation design: syntax objects, pattern matching with fenders, `datum->syntax`, `quasisyntax`, etc.

### Specification
- **[spec/R7RS_LARGE_MACRO_FASCICLE.md](spec/R7RS_LARGE_MACRO_FASCICLE.md)** — Local copy of the full R7RS-large macro fascicle (chapters 1-8, all forms, examples, and references).

## Implementation Order

```
Phase 0 (independent, can do now):
  ├── TAIL_PATTERNS.md         — syntax-rules extension
  └── CONSECUTIVE_ELLIPSES.md  — syntax-rules extension

Phase 1 (foundation):
  └── Syntax objects + core procedures
      identifier?, quote-syntax, unwrap-syntax, syntax->datum,
      datum->syntax, bound-identifier=?, free-identifier=?,
      symbolic-identifier=?, generate-identifier, generate-temporaries,
      identifier-defined?

Phase 2 (core):
  └── syntax-case + syntax (template) + with-syntax + custom-ellipsis

Phase 3 (derived forms):
  ├── quasisyntax / unsyntax / unsyntax-splicing
  ├── erroneous-syntax
  ├── splicing-let-syntax / splicing-letrec-syntax
  └── make-variable-transformer / identifier-syntax

Phase 4 (advanced):
  ├── define-syntax-parameter / syntax-parameterize
  ├── define-property / identifier-property
  └── er-macro-transformer / ir-macro-transformer (as macros)

Phase 5 (optional):
  └── Rewrite syntax-rules as syntax-case macro
```

## References

- [R7RS-large macro fascicle](https://r7rs.org/large/fascicles/macro/1/macros-and-hygiene.html)
- Chez Scheme `s/syntax.ss` — reference `syntax-case` implementation
- Racket `src/expander/` — scope-set based expander (closest to Patina's approach)
- psyntax — portable `syntax-case` implementation
