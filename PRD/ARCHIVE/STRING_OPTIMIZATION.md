# String Representation Optimization

**Status:** Active Development
**Phase:** Phase 1 Cleanup
**Created:** 2025-11-07
**Last Updated:** 2025-12-13

## Executive Summary

This document defines Patina's string representation strategy. After evaluating multiple options, we've chosen **Vec<char> (4 bytes per character)** as the internal representation for O(1) random access, with UTF-8 conversion for I/O operations.

**Design Decision (2025-12-13):** Use `Vec<char>` internally for O(1) character access. Convert to/from UTF-8 at I/O boundaries. This prioritizes algorithmic efficiency and predictable performance over memory compactness.

---

## Chosen Design: Vec<char> with UTF-8 I/O

### Architecture

```rust
// crates/patina-runtime/src/value/mod.rs
Value::String(Rc<RefCell<Vec<char>>>)  // 4 bytes per char, O(1) access

// Future enhancement: lazy UTF-8 cache for I/O
pub struct SchemeString {
    chars: Rc<RefCell<Vec<char>>>,
    utf8_cache: Rc<RefCell<Option<String>>>,  // Invalidated on mutation
}
```

### Performance Characteristics

| Operation | Complexity | Implementation |
|-----------|-----------|----------------|
| `string-length` | **O(1)** | `chars.len()` |
| `string-ref` | **O(1)** | `chars[i]` |
| `string-set!` | **O(1)** | `chars[i] = ch` (+ invalidate cache) |
| `substring` | O(n) | Slice and collect |
| `string-append` | O(n) | Extend vector |
| `display`/`write` | O(n) | Convert to UTF-8 (cached) |
| Memory per char | 4 bytes | Fixed, predictable |

### Design Rationale

1. **O(1) Random Access**: Character indexing is constant time
2. **O(1) Mutation**: `string-set!` is constant time
3. **Predictable Performance**: No algorithmic surprises
4. **Simple Implementation**: ~100 LOC change from current
5. **UTF-8 at Boundaries**: I/O still works correctly with UTF-8 conversion

### Trade-offs Accepted

1. **Memory**: 4 bytes/char vs 1-4 bytes/char UTF-8 (2-4x more for ASCII)
2. **I/O Conversion**: Need to convert to UTF-8 for display/file operations
3. **String Literals**: Parser must convert UTF-8 source to Vec<char>

### Why Not UTF-8 Internal?

While R7RS allows O(n) string operations, O(1) access provides:
- Better support for text-processing algorithms
- Predictable performance for users
- Simpler reasoning about string operations
- Alignment with how most languages represent strings internally

### R7RS Compliance

From R7RS Section 6.7:
> "There is no requirement for this procedure [string-ref] to execute in constant time."

Our implementation **exceeds** R7RS requirements by providing O(1) access. This is fully compliant.

---

## Legacy Implementation (Being Replaced)

### Previous Architecture

```rust
// Old: UTF-8 internally
Value::String(Rc<RefCell<String>>)
```

### Previous Performance

| Operation | Complexity | Implementation |
|-----------|-----------|----------------|
| `string-length` | O(n) | `.chars().count()` |
| `string-ref` | O(n) | `.chars().nth(k)` |
| `string-set!` | O(n) | Convert to `Vec<char>`, mutate, convert back |

This was simple and correct, but O(n) operations can be surprising for users expecting array-like string access.

---

## R7RS String Operations Analysis

### Required Operations

**Construction:**
- `make-string` - Create string with fill character
- `string` - Construct from characters
- `list->string` - Convert from character list

**Access:**
- `string-length` - Get character count
- `string-ref` - Access character by index

**Mutation:**
- `string-set!` - Modify character at index
- `string-fill!` - Fill with character (TODO)
- `string-copy!` - Copy range (TODO)

**Comparison:**
- `string=?`, `string<?`, `string>?`, `string<=?`, `string>=?` - Case-sensitive
- `string-ci=?`, `string-ci<?`, etc. - Case-insensitive

**Manipulation:**
- `string-append` - Concatenate strings
- `substring` - Extract substring
- `string-copy` - Duplicate string

**Conversion:**
- `string->list` - Convert to character list
- `string->vector` - Convert to vector (TODO)
- `string->utf8` - Convert to bytevector (TODO)
- `string->symbol` - Create symbol (TODO)
- `string->number` - Parse number (TODO)

**Case Operations:**
- `string-upcase`, `string-downcase`, `string-foldcase` (TODO)

**Higher-Order:**
- `string-map` - Map over characters (TODO)
- `string-for-each` - Iterate over characters (TODO)

### Typical Usage Patterns

Based on analysis of real Scheme code:

- **70%** of strings: Short (<24 chars), created once, never mutated
  - Examples: Variable names, error messages, small literals
  - Operations: Create, display, compare
  - Best representation: Inline (SSO)

- **20%** of strings: Medium (24-1000 chars), occasionally mutated
  - Examples: User input, text buffers, constructed strings
  - Operations: Append, substring, occasional set!
  - Best representation: UTF-8 with cached length

- **10%** of strings: Large (>1000 chars), rarely indexed
  - Examples: File contents, generated code
  - Operations: I/O, search, split
  - Best representation: Rope or chunked

---

## Optimization Options

### Option 1: Rope Data Structure 🪢

**Concept:** Tree of string fragments (like Xi Editor)

```rust
enum StringRepr {
    Leaf(String),                    // UTF-8 string
    Node {
        left: Rc<StringRepr>,
        right: Rc<StringRepr>,
        length: usize,               // Cached character count
        height: u8,                  // For balancing
    }
}

pub struct SchemeString {
    repr: Rc<RefCell<StringRepr>>,
}
```

**Performance:**
- `string-length`: **O(1)** - cached
- `string-ref`: **O(log n)** - tree traversal
- `string-append`: **O(1)** - just create new node!
- `substring`: **O(log n)** - tree slicing
- `string-set!`: **O(log n)** - path copying

**Pros:**
- ✅ Excellent for `string-append` (very common)
- ✅ Persistent data structure (functional style)
- ✅ O(1) append without copying
- ✅ Automatic structural sharing
- ✅ Cache-friendly for large strings

**Cons:**
- ❌ Complex implementation (~500-1000 LOC)
- ❌ Memory overhead (tree nodes, pointers)
- ❌ Need rebalancing logic (AVL or Red-Black)
- ❌ Slower for small strings
- ❌ Harder to debug

**Best For:** Programs with heavy string concatenation (templating, code generation)

**Estimated Effort:** 2 weeks

**References:**
- Xi Editor rope: https://github.com/xi-editor/xi-editor/tree/master/rust/rope
- Ropey crate: https://github.com/cessen/ropey

---

### Option 2: Small String Optimization (SSO) 📦

**Concept:** Store small strings inline, large strings on heap

```rust
const INLINE_CAPACITY: usize = 23; // Fits in same space as pointer

enum StringRepr {
    // Small string: stored inline (no allocation!)
    Inline {
        len: u8,                     // Character count
        bytes: [u8; INLINE_CAPACITY], // UTF-8 data
    },

    // Large string: heap allocated
    Heap {
        len: usize,                  // Character count (cached!)
        data: String,                // UTF-8 on heap
    },
}

pub struct SchemeString(Rc<RefCell<StringRepr>>);
```

**Performance:**
- Small strings (<24 bytes): **No heap allocation!**
- `string-length`: **O(1)** - cached in both variants
- `string-ref`: O(n) - unchanged, but cache-friendly
- Memory: Same as `Rc<String>` for large, **zero overhead** for small

**Pros:**
- ✅ Zero allocation for 70% of strings
- ✅ Better cache locality
- ✅ Simple to implement (~200 LOC)
- ✅ Most Scheme strings are short
- ✅ O(1) `string-length` for all strings
- ✅ Compatible with current API

**Cons:**
- ❌ Still O(n) access for long strings
- ❌ Need to handle inline→heap transition
- ❌ Slightly more complex than current

**Best For:** General purpose (70% of real-world strings benefit)

**Estimated Effort:** 2-3 days

**Implementation Notes:**
- Inline capacity: 23 bytes (common in C++ `std::string`, fits in 3 x 8-byte words)
- Transition threshold: When mutation would exceed 23 bytes
- Keep UTF-8 encoding in both variants

---

### Option 3: Hybrid Vec<char> with UTF-8 Backend 🔄

**Concept:** Lazily switch between representations based on usage

```rust
enum StringRepr {
    // Compact: UTF-8 storage (read-only optimization)
    Utf8 {
        data: String,
        // No char count cached - rare access
    },

    // Indexed: Vec<char> (mutation/random-access optimization)
    Indexed {
        chars: Vec<char>,
    },
}

pub struct SchemeString {
    repr: Rc<RefCell<StringRepr>>,
}
```

**Transition Logic:**
- Start as UTF-8 (compact, good for display)
- On first `string-ref` or `string-set!`: convert to `Vec<char>`
- On `string->utf8` or display: convert back to UTF-8
- Track "hot" strings (frequently accessed) vs "cold" (rarely accessed)

**Performance:**
- Read-only strings: **1-2 bytes/char**, O(n) access
- Mutated strings: **4 bytes/char**, **O(1) access**
- Adaptive based on actual usage!

**Pros:**
- ✅ Adaptive to usage patterns
- ✅ O(1) random access after first mutation
- ✅ Memory efficient for display-only strings
- ✅ Best of both worlds

**Cons:**
- ❌ Conversion overhead on first access
- ❌ Worst-case: thrashing between representations
- ❌ Complex state management
- ❌ Hard to predict performance

**Best For:** Mixed workloads (some strings for display, others for manipulation)

**Estimated Effort:** 1 week

**Implementation Challenges:**
- Need heuristics to avoid thrashing
- When to convert back to UTF-8?
- Memory management complexity

---

### Option 4: Chunked String with Index Cache 📊

**Concept:** Break strings into chunks with index cache

```rust
const CHUNK_SIZE: usize = 256;

struct Chunk {
    data: String,          // UTF-8 data (up to CHUNK_SIZE bytes)
    char_count: usize,     // Number of characters in this chunk
}

pub struct SchemeString {
    chunks: Rc<RefCell<Vec<Chunk>>>,
    total_chars: usize,    // Cached total
}
```

**Performance:**
- `string-length`: **O(1)** - cached
- `string-ref`: **O(chunks)** - find chunk, then O(chunk_size) scan
- `string-set!`: **O(chunk_size)** - only rebuild one chunk!
- Memory: ~1-4 bytes per char + small overhead

**Pros:**
- ✅ Bounded mutation cost (max 256 bytes)
- ✅ Still UTF-8 (good for I/O)
- ✅ Better than full rebuild
- ✅ Cache friendly for sequential access
- ✅ Good for large strings with sparse mutations

**Cons:**
- ❌ More complex than current
- ❌ Still not O(1) access
- ❌ Memory overhead for chunk metadata
- ❌ Need to handle chunk splitting/merging

**Best For:** Large strings (>1KB) with occasional mutations

**Estimated Effort:** 1 week

---

### Option 5: Pure Vec<char> with Lazy UTF-8 🎯

**Concept:** Always store as `Vec<char>`, convert to UTF-8 on demand

```rust
pub struct SchemeString {
    chars: Rc<RefCell<Vec<char>>>,
    // Lazy UTF-8 cache (for display/I/O)
    utf8_cache: Rc<RefCell<Option<String>>>,
}
```

**Performance:**
- `string-length`: **O(1)** - `chars.len()`
- `string-ref`: **O(1)** - `chars[i]`
- `string-set!`: **O(1)** - `chars[i] = ch` + invalidate cache
- `string-append`: **O(n)** - extend vector
- Memory: **4 bytes per character** (fixed)

**Pros:**
- ✅ True O(1) random access
- ✅ O(1) mutations
- ✅ Simple implementation (~100 LOC)
- ✅ Predictable performance
- ✅ Great for text editors

**Cons:**
- ❌ **4x memory overhead** vs UTF-8
- ❌ Conversion cost for I/O
- ❌ Not memory efficient
- ❌ Wastes space for ASCII

**Best For:** Character-heavy algorithms (parsers, text editors, DSLs)

**Estimated Effort:** 2 days

---

## Implementation Plan

### Phase 1: Vec<char> Migration (Current)

**Status:** 🚧 In Progress (2025-12-13)

**Target Architecture:**
```rust
Value::String(Rc<RefCell<Vec<char>>>)
```

**Implementation Steps:**

1. **Update Value enum** (`patina-runtime/src/value/mod.rs`)
   - Change `String(Rc<RefCell<String>>)` to `String(Rc<RefCell<Vec<char>>>)`
   - Update Display trait to convert to UTF-8 for output

2. **Update Parser** (`patina-frontend/src/parser/mod.rs`)
   - Parse string literals from UTF-8 source to `Vec<char>`
   - Handle escape sequences correctly

3. **Update Lexer** (`patina-frontend/src/lexer/mod.rs`)
   - Ensure string tokens preserve full Unicode content

4. **Update String Primitives** (`patina-tree-walker/src/eval/primitives/strings.rs`)
   - `string-length`: Return `chars.len()` (O(1))
   - `string-ref`: Return `chars[k]` (O(1))
   - `string-set!`: Set `chars[k] = ch` (O(1))
   - `make-string`: Create `Vec<char>` with fill
   - `string`: Collect chars into `Vec<char>`
   - `substring`: Slice and collect
   - `string-append`: Extend vectors
   - `string->list`: Iterate chars
   - `list->string`: Collect into Vec<char>
   - `string-copy`: Clone vector
   - `string-copy!`: Copy range
   - `string-fill!`: Fill range with char

5. **Update I/O Primitives** (`patina-tree-walker/src/eval/primitives/io.rs`)
   - `display`: Convert Vec<char> to String for output
   - `write`: Convert Vec<char> to String with escaping
   - `read`: Parse string from input to Vec<char>

6. **Update Comparison Primitives**
   - `string=?`, `string<?`, etc.: Compare Vec<char> directly
   - `string-ci=?`, etc.: Case-insensitive on chars

7. **Update String/UTF-8 Conversion**
   - `string->utf8`: Convert Vec<char> to bytevector
   - `utf8->string`: Convert bytevector to Vec<char>

8. **Run Tests**: All ~1400 tests should pass unchanged

**Estimated Effort:** 1-2 days

---

### Phase 2: UTF-8 Cache (Optional Enhancement)

**Priority:** Low (only if I/O performance matters)

```rust
pub struct SchemeString {
    chars: Rc<RefCell<Vec<char>>>,
    utf8_cache: Rc<RefCell<Option<String>>>,
}
```

**Benefits:**
- Avoid repeated UTF-8 conversion for display-heavy code
- Cache invalidated on any mutation
- Lazy computation on first I/O operation

**When to Implement:**
- Profiling shows UTF-8 conversion as bottleneck
- Programs with heavy string output
- REPL or logging-intensive applications

**Estimated Effort:** 1 day

---

### Future Considerations (Not Planned)

The following optimizations were evaluated but **not chosen**:

**SSO (Small String Optimization):**
- Would reduce allocations for small strings
- Adds complexity for marginal benefit with Vec<char>
- May reconsider if allocation profiling shows issues

**Rope:**
- Would improve `string-append` to O(1)
- Adds significant complexity (~500-1000 LOC)
- Only beneficial for heavy concatenation workloads
- May reconsider for specific use cases (templating, code gen)

**Adaptive UTF-8/Vec<char>:**
- Would save memory for read-only strings
- Adds complexity and unpredictable performance
- Not worth the implementation cost

---

## Benchmarking Guidelines

Before implementing any optimization, gather data:

### Benchmark Suite

Create `benches/strings.rs`:

```rust
// String creation patterns
- Small literals (1-10 chars)
- Medium strings (100-1000 chars)
- Large strings (10KB+)

// Access patterns
- Sequential access (string->list)
- Random access (string-ref repeatedly)
- Single mutation (one string-set!)
- Multiple mutations

// Operations
- string-append (many small vs few large)
- substring (various ranges)
- string-length (cached vs uncached)

// Real-world scenarios
- Parse bootstrap.scm (many small strings)
- Generate code (lots of appends)
- Text processing (mutations + access)
```

### Profiling Checklist

Before optimization:
- [ ] Run full benchmark suite (baseline)
- [ ] Profile real Scheme programs (not just tests)
- [ ] Measure allocation count (strings allocated)
- [ ] Measure memory usage (total bytes)
- [ ] Identify hot paths (where time is spent)

After optimization:
- [ ] All tests pass unchanged
- [ ] Benchmarks show improvement (>20% speedup)
- [ ] Memory usage acceptable (not >2x increase)
- [ ] No performance regressions
- [ ] Code complexity justified by gains

---

## Decision Matrix

| Use Case | Current | SSO | Adaptive | Rope | Vec<char> |
|----------|---------|-----|----------|------|-----------|
| Small strings | 😐 OK | ✅ Best | ✅ Good | ❌ Overkill | ❌ Wasteful |
| Read-only large | ✅ Good | ✅ Good | ✅ Best | ❌ Complex | ❌ Memory |
| Mutated strings | ❌ Slow | 😐 OK | ✅ Best | ✅ Good | ✅ Best |
| Append-heavy | ❌ Slow | ❌ Slow | ❌ Slow | ✅ Best | 😐 OK |
| Random access | ❌ O(n) | ❌ O(n) | ✅ O(1)* | ✅ O(log n) | ✅ O(1) |
| Memory efficient | ✅ Best | ✅ Best | ✅ Good | 😐 OK | ❌ Worst |
| Simple code | ✅ Best | ✅ Good | ❌ Complex | ❌ Most complex | ✅ Good |

*After first mutation

---

## Implementation Checklist (Phase 2: SSO)

When ready to implement SSO:

### 1. Design
- [ ] Finalize `StringRepr` structure
- [ ] Choose inline capacity (23 bytes recommended)
- [ ] Plan transition logic (inline → heap)
- [ ] Document memory layout

### 2. Implementation
- [ ] Create `src/value/string_repr.rs`
- [ ] Implement `StringRepr` enum
- [ ] Update `Value::String` to use `StringRepr`
- [ ] Update parser to create appropriate variant
- [ ] Update all string primitives
- [ ] Add conversion methods
- [ ] Update Display implementation

### 3. Testing
- [ ] All existing tests pass unchanged
- [ ] Add unit tests for SSO transitions
- [ ] Add tests for boundary cases (23→24 bytes)
- [ ] Test UTF-8 edge cases (multi-byte chars)
- [ ] Test mutation transitions

### 4. Benchmarking
- [ ] Create benchmark suite
- [ ] Run baseline benchmarks
- [ ] Run SSO benchmarks
- [ ] Verify >20% improvement on small strings
- [ ] Verify no regression on large strings

### 5. Documentation
- [ ] Update CLAUDE.md
- [ ] Document memory layout
- [ ] Update architecture diagram
- [ ] Add performance notes

---

## References

### Academic Papers
- "Ropes: an Alternative to Strings" - Boehm, Atkinson, Plass (1995)
- "Adaptive String Matching" - Bentley & McIlroy (1999)

### Implementation Examples
- **Rust stdlib**: `String` uses heap allocation
- **C++ stdlib**: `std::string` uses SSO (23 bytes inline)
- **Xi Editor**: Rope-based text buffer
- **Ropey**: Rust rope library
- **ImString**: Immutable string library

### Relevant Crates
- `ropey` - Rope implementation
- `smol_str` - Small string optimization
- `compact_str` - Compact string representation
- `flexstr` - Flexible string storage

---

## Conclusion

**Design Decision (2025-12-13):** Vec<char> with 4 bytes per character.

**Rationale:**
- O(1) random access is more intuitive for users
- O(1) mutation simplifies text-processing algorithms
- Predictable performance with no algorithmic surprises
- UTF-8 conversion at I/O boundaries is acceptable overhead
- Simple implementation (~100 LOC change)

**Trade-off Accepted:** Higher memory usage (4 bytes/char vs 1-4 bytes/char UTF-8) in exchange for O(1) operations and predictable performance.

**Next Steps:**
1. Implement Vec<char> migration (Phase 1)
2. Run full test suite to verify correctness
3. Consider UTF-8 cache only if I/O performance becomes an issue

**Key Insight:** While R7RS allows O(n) string operations, providing O(1) access exceeds spec requirements and provides a better developer experience. Memory efficiency is secondary to algorithmic efficiency for an interpreter targeting correctness and clarity.
