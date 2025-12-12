# R7RS Portable Libraries Research

**Last Updated**: 2025-12-11
**Status**: Research / Future Work

## Overview

This document catalogs portable R7RS library collections that could be integrated into Patina as "batteries included" and serve as additional compliance tests. Unlike R6RS which has the consolidated [scheme-libraries](https://github.com/scheme-libraries/scheme-libraries) repository, the R7RS ecosystem is more fragmented since R7RS-large is still under development.

## Key Repository Collections

### 1. TaylanUB/scheme-srfis (Recommended Primary Source)

**Repository**: https://github.com/TaylanUB/scheme-srfis

The most promising collection for Patina integration.

**Key Features**:
- Well-maintained R7RS SRFI implementations
- Tested with Chibi-scheme (our primary reference implementation)
- Uses `.sld` files with `(srfi n)` naming convention
- Minimal `cond-expand` usage - assumes sane platform properties
- Assumes full numeric tower, Unicode support

**Library Organization**:
```
srfi/
├── 1.sld    # List Library
├── 2.sld    # AND-LET*
├── ...
└── n.sld    # SRFI n
```

Libraries are named `(srfi n)` where n is the SRFI number.

**Potential SRFIs to Import**:
- SRFI-1: List Library (extended list operations)
- SRFI-13: String Library
- SRFI-14: Character Sets
- SRFI-26: Cut/Cute (partial application)
- SRFI-41: Streams (lazy lists)
- SRFI-64: Testing framework
- SRFI-69: Hash tables (basic)
- SRFI-113: Sets and bags
- SRFI-125: Hash tables (intermediate)
- SRFI-128: Comparators
- SRFI-132: Sort libraries
- SRFI-133: Vector library

### 2. chaw/r7rs-libs

**Repository**: https://github.com/chaw/r7rs-libs

**Key Features**:
- Some require R7RS-large Red Edition
- Includes SRFI-151 (Bitwise Operations) reference implementation
- Repackaged algorithms: automatic differentiation, purely functional data structures
- Currently targets Sagittarius primarily

**Notable Libraries**:
- SRFI-151: Bitwise operations
- Automatic differentiation
- Functional data structures (from Okasaki)

### 3. pogrmman/Scheme-Libs

**Repository**: https://github.com/pogrmman/Scheme-Libs

General-purpose R7RS libraries collection. Less comprehensive but may have useful utilities.

### 4. Official SRFI Repositories

**Organization**: https://github.com/scheme-requests-for-implementation

Each SRFI has its own GitHub repository with:
- Specification document
- Reference implementation (often R7RS compatible)
- Test suite

**Example**: https://github.com/scheme-requests-for-implementation/srfi-1

## Package Managers / Registries

### Snow-fort (R7RS Primary)

**URL**: http://snow-fort.org/

- R7RS-specific package repository
- Community-contributed libraries
- Standard format for R7RS library distribution

### Akku.scm (R6RS + R7RS)

**URL**: https://akkuscm.org/

- Supports both R6RS and R7RS
- Mirrors Snow packages
- Project-based dependency management
- Has R7RS to R6RS translator

**Package Browser**: https://akkuscm.org/packages/

### chez-srfi (R6RS, reference)

While R6RS-focused, useful as reference for SRFI semantics:
- Provides 60+ SRFI implementations
- Well-tested against multiple implementations

## R7RS-large Status

### Specification Development

**Repository**: https://codeberg.org/scheme/r7rs

R7RS-large is developed by assigning SRFIs to color-coded "dockets":
- **Red Edition**: Foundation (finalized)
- **Tangerine Edition**: Data structures (finalized)
- **Orange Edition**: Numbers (in progress)
- **Amber Edition**: I/O, networking (planned)

### Implementation Support

| Implementation | Red Edition | Tangerine Edition |
|---------------|-------------|-------------------|
| Gauche        | ✅ Full     | ✅ Full           |
| Sagittarius   | ✅ Full     | ✅ Full           |
| Chibi 0.8+    | ✅ Full     | Partial           |
| Larceny       | Partial     | Partial           |

### Red Edition Libraries (Priority for Patina)

These are finalized and should be supported:

| Library | SRFI | Description |
|---------|------|-------------|
| `(scheme list)` | SRFI-1 | List library |
| `(scheme vector)` | SRFI-133 | Vector library |
| `(scheme sort)` | SRFI-132 | Sorting |
| `(scheme set)` | SRFI-113 | Sets and bags |
| `(scheme charset)` | SRFI-14 | Character sets |
| `(scheme hash-table)` | SRFI-125 | Hash tables |
| `(scheme ilist)` | SRFI-116 | Immutable lists |
| `(scheme rlist)` | SRFI-101 | Random-access lists |
| `(scheme ideque)` | SRFI-134 | Immutable deques |
| `(scheme text)` | SRFI-135 | Immutable texts |
| `(scheme generator)` | SRFI-158 | Generators |
| `(scheme lseq)` | SRFI-127 | Lazy sequences |
| `(scheme stream)` | SRFI-41 | Streams |
| `(scheme box)` | SRFI-111 | Boxes |
| `(scheme comparator)` | SRFI-128 | Comparators |

### Tangerine Edition Libraries

| Library | SRFI | Description |
|---------|------|-------------|
| `(scheme bitwise)` | SRFI-151 | Bitwise operations |
| `(scheme fixnum)` | SRFI-143 | Fixnums |
| `(scheme flonum)` | SRFI-144 | Flonums |
| `(scheme division)` | SRFI-141 | Integer division |
| `(scheme bytevector)` | R6RS | Bytevectors |

## Integration Strategy for Patina

### Phase 1: Test Suite Integration

Use portable libraries as compliance tests:

1. **Import test suites** from SRFI reference implementations
2. **Run against Patina** to identify gaps
3. **Track compatibility** in `docs/FEATURE_STATUS.md`

### Phase 2: Library Bundling

Bundle commonly-used SRFIs with Patina:

```
lib/
├── scheme/           # R7RS-small (existing)
│   ├── base.sld
│   └── ...
├── srfi/             # SRFI libraries (new)
│   ├── 1.sld         # List library
│   ├── 13.sld        # String library
│   └── ...
└── patina/           # Patina extensions (existing)
    └── control.sld   # Delimited continuations
```

### Phase 3: R7RS-large Compliance

Target Red Edition compliance:

1. Implement missing primitives
2. Add `(scheme *)` library aliases
3. Pass Red Edition test suites

### Priority Order

1. **SRFI-1** (List library) - Most commonly used
2. **SRFI-13/14** (Strings/Char sets) - Text processing
3. **SRFI-64** (Testing) - Enables running more test suites
4. **SRFI-125/128** (Hash tables/Comparators) - Data structures
5. **SRFI-133** (Vectors) - Already have basics, need extensions
6. **SRFI-132** (Sorting) - Common utility

## Documentation Resources

- **Portable R7RS Guide**: https://docs.scheme.org/guide/r7rs-portable/
- **SRFI Support Table**: https://docs.scheme.org/srfi/support/
- **R7RS-large Wiki**: https://codeberg.org/scheme/r7rs/wiki

## License Considerations

Most SRFI reference implementations use permissive licenses (MIT, BSD) that allow bundling. Each SRFI's license should be verified before inclusion.

**Common licenses**:
- MIT License (most SRFIs)
- BSD 3-Clause
- Public Domain

## Next Steps

1. [ ] Clone TaylanUB/scheme-srfis and test against Patina
2. [ ] Identify which SRFIs work out-of-box
3. [ ] Create compatibility matrix
4. [ ] Prioritize fixes based on SRFI popularity
5. [ ] Bundle working SRFIs with Patina distribution
6. [ ] Add `(scheme *)` R7RS-large aliases

## References

- [R7RS-small specification](https://standards.scheme.org/official/r7rs.pdf)
- [R7RS-large development](https://codeberg.org/scheme/r7rs)
- [SRFI homepage](https://srfi.schemers.org/)
- [Scheme documentation hub](https://docs.scheme.org/)
