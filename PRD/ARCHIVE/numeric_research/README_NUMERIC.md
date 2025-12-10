# Numeric Tower Implementation - Document Index

## 📍 Start Here

**⭐ CANONICAL GUIDE**: [`NUMERIC_SUMMARY.md`](./NUMERIC_SUMMARY.md)

This is the **authoritative document** for implementing the numeric tower.
Start here for the implementation plan and key decisions.

## 📚 Document Hierarchy

### Primary Documents (PRD/phase1/)

1. **[NUMERIC_SUMMARY.md](./NUMERIC_SUMMARY.md)** ⭐ CANONICAL
   - Executive summary
   - Implementation plan (7 phases)
   - Quick wins and priorities
   - **Start here for implementation**

2. **[NUMERIC_TOWER_REVISED.md](./NUMERIC_TOWER_REVISED.md)** 📖 DETAILED DESIGN
   - Complete design based on chibi-scheme analysis
   - Detailed code examples for each operation
   - Type representations and conversion rules
   - **Reference when implementing specific features**

3. **[NUMERIC_TOWER_DESIGN.md](./NUMERIC_TOWER_DESIGN.md)** ⚠️ DEPRECATED
   - Original design (before chibi analysis)
   - Kept for historical reference only
   - **Do not use for implementation**

### Supporting Documents (internal/)

4. **[internal/README_NUMERIC_ANALYSIS.md](../../internal/README_NUMERIC_ANALYSIS.md)** 📂 INDEX
   - Index to chibi-scheme analysis documents
   - Navigation guide for analysis files

5. **[internal/rust_numeric_patterns.md](../../internal/rust_numeric_patterns.md)** 🦀 RUST PATTERNS
   - Rust-specific implementation patterns
   - Code examples adapted from chibi
   - **Use when writing Rust code**

6. **[internal/chibi_numeric_tower_analysis.md](../../internal/chibi_numeric_tower_analysis.md)** 🔍 DEEP DIVE
   - Complete analysis of chibi-scheme implementation
   - Line-by-line reference to chibi source
   - **Use for understanding design rationale**

7. **[internal/numeric_implementation_guide.md](../../internal/numeric_implementation_guide.md)** ⚡ QUICK REF
   - Quick code snippets
   - Common patterns
   - **Use for quick lookups during coding**

## 🎯 How to Use These Documents

### When starting implementation:
1. Read **NUMERIC_SUMMARY.md** (the canonical guide)
2. Understand the 7-phase implementation plan
3. Review critical implementation points

### When implementing a specific feature:
1. Check **NUMERIC_SUMMARY.md** for the overview
2. Consult **NUMERIC_TOWER_REVISED.md** for detailed design
3. Reference **rust_numeric_patterns.md** for Rust code examples
4. Use **numeric_implementation_guide.md** for quick snippets

### When debugging or understanding why:
1. Check **chibi_numeric_tower_analysis.md** for design rationale
2. Reference actual chibi-scheme source code
3. Cross-check with R7RS specification

## 🗺️ External References

- **R7RS Spec**: `../../spec/r7rs-small-spec/` (Section 6.2 - Numbers)
- **Chibi-Scheme**: `~/Project/reference/chibi-scheme`
- **Patina Tests**: `../../tests/compliance/numbers.rs`

## 📊 Quick Summary

**Problem**: Need to implement R7RS-compliant numeric tower

**Solution**: Use type-implicit exactness (based on chibi-scheme)

**Current Status**: Patina's Value enum is already 80% correct!

**What's Needed**:
1. Fix Complex representation
2. Proper division semantics (/ returns rationals)
3. Overflow detection and BigInt promotion
4. Implement the operations

**Estimated Effort**: 10-13 days across 7 phases

**Quick Win**: Implement Phase 2 (division) to unlock GCD tests

## 🚀 Getting Started

```bash
# Read the canonical guide
cat PRD/phase1/NUMERIC_SUMMARY.md

# Check current test status
cargo test --test compliance numbers

# Create a feature branch
git checkout -b feature/numeric-tower

# Start with Phase 2 (recommended)
# Implement quotient, remainder, modulo
```

## ❓ Questions?

Refer to the **Document Hierarchy** section above to find the right document for your question.

**Implementation questions** → NUMERIC_SUMMARY.md
**Design questions** → NUMERIC_TOWER_REVISED.md
**Code patterns** → rust_numeric_patterns.md
**Why chibi does X?** → chibi_numeric_tower_analysis.md

---

**Last Updated**: 2025-11-04
**Status**: Ready for implementation
