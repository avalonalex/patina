# String Abstraction Design

**Status:** Design Document
**Created:** 2025-12-13
**Related:** [ARCHITECTURE_LESSONS.md](./ARCHITECTURE_LESSONS.md) §4

---

## Overview

This document describes a design for abstracting Scheme string implementations behind a common API, allowing different implementations to be swapped via feature flags without modifying the rest of the codebase.

---

## Table of Contents

1. [Problem Statement](#1-problem-statement)
2. [Design Goals](#2-design-goals)
3. [Current Implementation](#3-current-implementation)
4. [Proposed API](#4-proposed-api)
5. [Implementation Options](#5-implementation-options)
6. [Integration Strategy](#6-integration-strategy)
7. [Migration Plan](#7-migration-plan)
8. [Benchmarking](#8-benchmarking)
9. [Implementation Phases](#9-implementation-phases)
10. [Open Questions](#10-open-questions)

---

## 1. Problem Statement

### R7RS String Requirements

R7RS specifies that strings are:
- **Mutable:** `string-set!` modifies in place
- **Indexed by character:** `string-ref` returns the k-th character
- **O(1) access:** "essentially constant time" for `string-ref` and `string-set!`
- **Length in characters:** `string-length` returns character count, not bytes

### Current Trade-off

The current implementation uses `Vec<char>` which provides O(1) character access but uses 4 bytes per character (32 bits for Unicode codepoint). This is memory-inefficient for ASCII-heavy code.

### Desired Flexibility

Different use cases favor different implementations:

| Use Case | Preferred Implementation |
|----------|-------------------------|
| R7RS compliance testing | `Vec<char>` (guaranteed O(1)) |
| Memory-constrained | UTF-8 with index cache |
| Short strings dominant | Small string optimization |
| VM performance | Interned/indexed strings |

A clean abstraction allows selecting the right implementation without code changes.

---

## 2. Design Goals

1. **Zero runtime overhead:** Compile-time selection via feature flags, not trait objects
2. **API stability:** All implementations expose identical public API
3. **Minimal code changes:** Rest of codebase uses opaque `SchemeString` type
4. **Easy benchmarking:** Switch implementations with cargo feature flag
5. **Incremental adoption:** Can migrate gradually, starting with type alias
6. **R7RS compliance:** All implementations must satisfy spec requirements

### Non-Goals

- Runtime-switchable implementations (adds overhead)
- Supporting non-Unicode encodings
- Optimizing for extremely large strings (>1MB)

---

## 3. Current Implementation

### Location

```
crates/patina-core/src/value.rs
```

### Current Representation

```rust
pub enum Value {
    // ...
    String(Rc<RefCell<Vec<char>>>),
    // ...
}
```

### Usage Patterns

String operations are spread across:

| Location | Operations |
|----------|------------|
| `patina-core/src/value.rs` | Construction, Display |
| `patina-tree-walker/src/eval/primitives/strings.rs` | string-ref, string-set!, string-append, etc. |
| `patina-frontend/src/parser/` | String literal parsing |
| `patina-frontend/src/lexer/` | String token handling |

### Current Operations Used

```rust
// Construction
Vec::new()
s.chars().collect::<Vec<char>>()

// Access
vec[index]
vec.get(index)
vec.len()

// Mutation
vec[index] = char
vec.push(char)

// Iteration
vec.iter()
vec.into_iter()

// Conversion
vec.iter().collect::<String>()
String::from_iter(vec)
```

---

## 4. Proposed API

### Core Type

```rust
// crates/patina-core/src/string/mod.rs

/// Scheme string type - implementation selected via feature flag
///
/// All implementations guarantee:
/// - O(1) or O(1) amortized character access (string-ref)
/// - O(1) length query (string-length)
/// - Mutable characters (string-set!)
/// - UTF-8 compatible (stores Unicode codepoints)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SchemeString(/* implementation-specific */);
```

### Required Methods

```rust
impl SchemeString {
    //==========================================================================
    // Construction
    //==========================================================================

    /// Create empty string
    pub fn new() -> Self;

    /// Create from Rust str (copies data)
    pub fn from_str(s: &str) -> Self;

    /// Create from iterator of chars
    pub fn from_chars(chars: impl IntoIterator<Item = char>) -> Self;

    /// Create string of `len` copies of `fill` character
    pub fn from_fill(len: usize, fill: char) -> Self;

    //==========================================================================
    // Queries (all O(1))
    //==========================================================================

    /// Length in characters (not bytes)
    pub fn len(&self) -> usize;

    /// Is empty?
    pub fn is_empty(&self) -> bool;

    /// Get character at index (O(1) guaranteed)
    /// Returns None if index out of bounds
    pub fn char_at(&self, index: usize) -> Option<char>;

    /// Get character at index (O(1) guaranteed)
    /// Panics if index out of bounds
    pub fn char_at_unchecked(&self, index: usize) -> char;

    //==========================================================================
    // Mutation
    //==========================================================================

    /// Set character at index (O(1) guaranteed)
    /// Panics if index out of bounds
    pub fn set_char(&mut self, index: usize, c: char);

    /// Fill entire string with character
    pub fn fill(&mut self, c: char);

    //==========================================================================
    // Derived Operations
    //==========================================================================

    /// Extract substring [start, end)
    pub fn substring(&self, start: usize, end: usize) -> Self;

    /// Concatenate two strings
    pub fn append(&self, other: &Self) -> Self;

    /// Copy characters from `src` into `self` starting at `at`
    pub fn copy_from(&mut self, src: &Self, at: usize);

    //==========================================================================
    // Conversion
    //==========================================================================

    /// Convert to Rust String (for I/O, display)
    pub fn to_rust_string(&self) -> String;

    /// Iterate over characters
    pub fn chars(&self) -> impl Iterator<Item = char> + '_;

    /// Convert to Vec<char> (for compatibility)
    pub fn to_vec(&self) -> Vec<char>;
}
```

### Comparison Methods

```rust
impl SchemeString {
    /// Lexicographic comparison (for string<?, string>?, etc.)
    pub fn cmp_lexicographic(&self, other: &Self) -> std::cmp::Ordering;

    /// Case-insensitive comparison (for string-ci=?, etc.)
    pub fn eq_ignore_case(&self, other: &Self) -> bool;

    /// Case-insensitive lexicographic comparison
    pub fn cmp_ignore_case(&self, other: &Self) -> std::cmp::Ordering;
}
```

### Display Implementation

```rust
impl std::fmt::Display for SchemeString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for c in self.chars() {
            write!(f, "{}", c)?;
        }
        Ok(())
    }
}
```

---

## 5. Implementation Options

### Option A: `VecCharString` (Current)

```rust
/// O(1) indexing, 4 bytes per character
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SchemeString(Vec<char>);

impl SchemeString {
    pub fn from_str(s: &str) -> Self {
        Self(s.chars().collect())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn char_at(&self, index: usize) -> Option<char> {
        self.0.get(index).copied()
    }

    pub fn set_char(&mut self, index: usize, c: char) {
        self.0[index] = c;
    }

    pub fn to_rust_string(&self) -> String {
        self.0.iter().collect()
    }

    pub fn chars(&self) -> impl Iterator<Item = char> + '_ {
        self.0.iter().copied()
    }
}
```

**Characteristics:**
- Memory: 4 bytes/char + Vec overhead (~24 bytes)
- char_at: O(1) exact
- set_char: O(1) exact
- Best for: R7RS compliance, simplicity

---

### Option B: `CachedUtf8String`

```rust
/// UTF-8 storage with sparse character index for O(1)-ish access
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SchemeString {
    /// UTF-8 encoded data
    data: String,
    /// Number of characters (cached)
    char_count: usize,
    /// Byte offset for every CACHE_INTERVAL characters
    /// index_cache[i] = byte offset of character i * CACHE_INTERVAL
    index_cache: Vec<usize>,
}

const CACHE_INTERVAL: usize = 64;

impl SchemeString {
    pub fn from_str(s: &str) -> Self {
        let char_count = s.chars().count();
        let index_cache = Self::build_index_cache(s);
        Self {
            data: s.to_string(),
            char_count,
            index_cache,
        }
    }

    fn build_index_cache(s: &str) -> Vec<usize> {
        let mut cache = vec![0];
        let mut byte_offset = 0;
        for (i, c) in s.chars().enumerate() {
            byte_offset += c.len_utf8();
            if (i + 1) % CACHE_INTERVAL == 0 {
                cache.push(byte_offset);
            }
        }
        cache
    }

    pub fn len(&self) -> usize {
        self.char_count
    }

    pub fn char_at(&self, index: usize) -> Option<char> {
        if index >= self.char_count {
            return None;
        }

        // Find nearest cache entry
        let cache_idx = index / CACHE_INTERVAL;
        let byte_offset = self.index_cache[cache_idx];
        let char_offset = cache_idx * CACHE_INTERVAL;
        let remaining = index - char_offset;

        // Linear scan from cache point (at most CACHE_INTERVAL chars)
        self.data[byte_offset..].chars().nth(remaining)
    }

    pub fn set_char(&mut self, index: usize, c: char) {
        // More complex: may need to resize if char width changes
        let old_char = self.char_at(index).expect("index out of bounds");

        if c.len_utf8() == old_char.len_utf8() {
            // Same width: in-place replacement
            let byte_idx = self.byte_offset_of(index);
            let end = byte_idx + old_char.len_utf8();
            self.data.replace_range(byte_idx..end, &c.to_string());
        } else {
            // Different width: rebuild (expensive but rare)
            let mut chars: Vec<char> = self.data.chars().collect();
            chars[index] = c;
            self.data = chars.into_iter().collect();
            self.index_cache = Self::build_index_cache(&self.data);
        }
    }

    fn byte_offset_of(&self, char_index: usize) -> usize {
        let cache_idx = char_index / CACHE_INTERVAL;
        let byte_offset = self.index_cache[cache_idx];
        let char_offset = cache_idx * CACHE_INTERVAL;

        let mut offset = byte_offset;
        for c in self.data[byte_offset..].chars().take(char_index - char_offset) {
            offset += c.len_utf8();
        }
        offset
    }
}
```

**Characteristics:**
- Memory: 1-4 bytes/char (typically ~1.1 for ASCII) + cache overhead
- char_at: O(CACHE_INTERVAL) = O(64) = O(1) amortized
- set_char: O(1) if same width, O(n) if width changes
- Best for: Memory efficiency, ASCII-heavy workloads

---

### Option C: `SmallString`

```rust
/// Small string optimization: inline storage for short strings
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SchemeString {
    /// Inline storage for strings ≤ MAX_INLINE_CHARS characters
    Inline {
        len: u8,
        data: [char; MAX_INLINE_CHARS],
    },
    /// Heap storage for longer strings
    Heap(Box<Vec<char>>),
}

const MAX_INLINE_CHARS: usize = 7;  // 28 bytes for chars + 1 for len = 29 bytes

impl SchemeString {
    pub fn from_str(s: &str) -> Self {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() <= MAX_INLINE_CHARS {
            let mut data = ['\0'; MAX_INLINE_CHARS];
            for (i, c) in chars.iter().enumerate() {
                data[i] = *c;
            }
            Self::Inline {
                len: chars.len() as u8,
                data,
            }
        } else {
            Self::Heap(Box::new(chars))
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Inline { len, .. } => *len as usize,
            Self::Heap(v) => v.len(),
        }
    }

    pub fn char_at(&self, index: usize) -> Option<char> {
        match self {
            Self::Inline { len, data } => {
                if index < *len as usize {
                    Some(data[index])
                } else {
                    None
                }
            }
            Self::Heap(v) => v.get(index).copied(),
        }
    }

    pub fn set_char(&mut self, index: usize, c: char) {
        match self {
            Self::Inline { len, data } => {
                assert!(index < *len as usize);
                data[index] = c;
            }
            Self::Heap(v) => {
                v[index] = c;
            }
        }
    }
}
```

**Characteristics:**
- Memory: 29 bytes for ≤7 chars (no allocation), 4 bytes/char + Box for longer
- char_at: O(1) exact
- set_char: O(1) exact
- Best for: Many short strings (symbols, identifiers)

---

### Option D: `InternedString` (VM-specific)

```rust
/// Interned string with index into global string table
/// Best for VM where strings are mostly immutable after creation
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SchemeString(u32);  // Index into string table

/// Global string table (thread-local or with synchronization)
pub struct StringTable {
    strings: Vec<String>,
    /// For deduplication
    index: HashMap<String, u32>,
    /// Character index caches (lazily built)
    char_caches: Vec<Option<Vec<usize>>>,
}

thread_local! {
    static STRING_TABLE: RefCell<StringTable> = RefCell::new(StringTable::new());
}

impl SchemeString {
    pub fn from_str(s: &str) -> Self {
        STRING_TABLE.with(|table| {
            let mut table = table.borrow_mut();
            if let Some(&idx) = table.index.get(s) {
                Self(idx)
            } else {
                let idx = table.strings.len() as u32;
                table.strings.push(s.to_string());
                table.char_caches.push(None);
                table.index.insert(s.to_string(), idx);
                Self(idx)
            }
        })
    }

    pub fn len(&self) -> usize {
        STRING_TABLE.with(|table| {
            table.borrow().strings[self.0 as usize].chars().count()
        })
    }

    // Note: Mutation is complex with interning - may need copy-on-write
}
```

**Characteristics:**
- Memory: 4 bytes per reference, shared storage
- char_at: O(1) with cache, table lookup overhead
- set_char: Requires copy-on-write (breaks sharing)
- Best for: VM execution, immutable string workloads

---

## 6. Integration Strategy

### Feature Flags

```toml
# Cargo.toml (workspace or patina-core)

[features]
default = ["string-vec-char"]

# String implementation selection (mutually exclusive)
string-vec-char = []       # Vec<char> - O(1) exact, 4 bytes/char
string-utf8-cached = []    # UTF-8 + cache - memory efficient
string-small-opt = []      # Small string optimization
string-interned = []       # Interned strings (VM only)
```

### Module Structure

```
crates/patina-core/src/
├── string/
│   ├── mod.rs              # Re-exports based on feature
│   ├── vec_char.rs         # VecCharString implementation
│   ├── utf8_cached.rs      # CachedUtf8String implementation
│   ├── small_opt.rs        # SmallString implementation
│   └── interned.rs         # InternedString implementation
└── value.rs                # Uses string::SchemeString
```

### Module Selection

```rust
// crates/patina-core/src/string/mod.rs

#[cfg(feature = "string-vec-char")]
mod vec_char;
#[cfg(feature = "string-vec-char")]
pub use vec_char::SchemeString;

#[cfg(feature = "string-utf8-cached")]
mod utf8_cached;
#[cfg(feature = "string-utf8-cached")]
pub use utf8_cached::SchemeString;

#[cfg(feature = "string-small-opt")]
mod small_opt;
#[cfg(feature = "string-small-opt")]
pub use small_opt::SchemeString;

#[cfg(feature = "string-interned")]
mod interned;
#[cfg(feature = "string-interned")]
pub use interned::SchemeString;

// Compile error if none selected
#[cfg(not(any(
    feature = "string-vec-char",
    feature = "string-utf8-cached",
    feature = "string-small-opt",
    feature = "string-interned",
)))]
compile_error!("Select a string implementation feature");

// Compile error if multiple selected
#[cfg(any(
    all(feature = "string-vec-char", feature = "string-utf8-cached"),
    all(feature = "string-vec-char", feature = "string-small-opt"),
    all(feature = "string-vec-char", feature = "string-interned"),
    all(feature = "string-utf8-cached", feature = "string-small-opt"),
    all(feature = "string-utf8-cached", feature = "string-interned"),
    all(feature = "string-small-opt", feature = "string-interned"),
))]
compile_error!("Only one string implementation feature can be selected");
```

### Value Integration

```rust
// crates/patina-core/src/value.rs

use crate::string::SchemeString;

pub enum Value {
    // Before:
    // String(Rc<RefCell<Vec<char>>>),

    // After:
    String(Rc<RefCell<SchemeString>>),

    // ...
}
```

---

## 7. Migration Plan

### Phase 1: Extract and Wrap (Non-breaking)

1. Create `string/` module in `patina-core`
2. Move current `Vec<char>` logic into `vec_char.rs`
3. Wrap in `SchemeString` struct with full API
4. Update `Value` to use `SchemeString`
5. Update all call sites to use `SchemeString` methods
6. All tests must pass

**Estimated effort:** 2-3 hours

### Phase 2: Add Feature Flag Infrastructure

1. Add feature flags to `Cargo.toml`
2. Add module selection logic in `mod.rs`
3. Verify default (`string-vec-char`) works
4. All tests must pass

**Estimated effort:** 30 minutes

### Phase 3: Implement Alternative (Optional)

1. Choose one alternative implementation
2. Implement full `SchemeString` API
3. Test with: `cargo test --features string-utf8-cached`
4. Benchmark both implementations

**Estimated effort:** 2-4 hours per implementation

### Phase 4: Benchmarking and Selection

1. Create benchmark suite
2. Run benchmarks with each implementation
3. Document results
4. Choose default for different use cases

**Estimated effort:** 1-2 hours

---

## 8. Benchmarking

### Benchmark Suite

```rust
// benches/string_benchmark.rs

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use patina_core::string::SchemeString;

fn bench_char_at(c: &mut Criterion) {
    let sizes = [10, 100, 1000, 10000];
    let mut group = c.benchmark_group("char_at");

    for size in sizes {
        let s = SchemeString::from_str(&"a".repeat(size));

        // Access first, middle, last
        group.bench_with_input(BenchmarkId::new("first", size), &s, |b, s| {
            b.iter(|| s.char_at(0))
        });
        group.bench_with_input(BenchmarkId::new("middle", size), &s, |b, s| {
            b.iter(|| s.char_at(size / 2))
        });
        group.bench_with_input(BenchmarkId::new("last", size), &s, |b, s| {
            b.iter(|| s.char_at(size - 1))
        });
    }
    group.finish();
}

fn bench_set_char(c: &mut Criterion) {
    let sizes = [10, 100, 1000];
    let mut group = c.benchmark_group("set_char");

    for size in sizes {
        group.bench_with_input(BenchmarkId::new("middle", size), &size, |b, &size| {
            let mut s = SchemeString::from_str(&"a".repeat(size));
            b.iter(|| s.set_char(size / 2, 'b'))
        });
    }
    group.finish();
}

fn bench_construction(c: &mut Criterion) {
    let inputs = [
        ("short_ascii", "hello"),
        ("medium_ascii", &"a".repeat(100)),
        ("long_ascii", &"a".repeat(10000)),
        ("unicode", "hello 世界 🦀"),
    ];

    let mut group = c.benchmark_group("construction");
    for (name, input) in inputs {
        group.bench_with_input(BenchmarkId::new("from_str", name), &input, |b, &s| {
            b.iter(|| SchemeString::from_str(s))
        });
    }
    group.finish();
}

fn bench_iteration(c: &mut Criterion) {
    let s = SchemeString::from_str(&"a".repeat(1000));
    c.bench_function("iterate_1000", |b| {
        b.iter(|| {
            let mut sum = 0u32;
            for c in s.chars() {
                sum += c as u32;
            }
            sum
        })
    });
}

fn bench_memory(c: &mut Criterion) {
    // Memory usage comparison (manual measurement)
    let sizes = [10, 100, 1000, 10000];

    println!("\nMemory usage (approximate):");
    for size in sizes {
        let s = SchemeString::from_str(&"a".repeat(size));
        let mem = std::mem::size_of_val(&s);
        println!("  {} chars: {} bytes (struct), ~{} bytes (heap)",
                 size, mem, size * 4); // Adjust for impl
    }
}

criterion_group!(benches,
    bench_char_at,
    bench_set_char,
    bench_construction,
    bench_iteration,
);
criterion_main!(benches);
```

### Running Benchmarks

```bash
# Default implementation
cargo bench --features string-vec-char

# Alternative implementation
cargo bench --features string-utf8-cached

# Compare results
cargo bench --features string-vec-char -- --save-baseline vec-char
cargo bench --features string-utf8-cached -- --baseline vec-char
```

---

## 9. Implementation Phases

### Phase 1: Foundation (Recommended Now)

**Goal:** Extract current implementation, establish API

- [ ] Create `crates/patina-core/src/string/mod.rs`
- [ ] Create `crates/patina-core/src/string/vec_char.rs`
- [ ] Implement full `SchemeString` API for `VecCharString`
- [ ] Update `Value::String` to use `SchemeString`
- [ ] Update all primitives to use `SchemeString` methods
- [ ] Update parser/lexer if needed
- [ ] All tests pass

### Phase 2: Feature Flag Infrastructure (Recommended Now)

**Goal:** Enable switching implementations

- [ ] Add feature flags to `Cargo.toml`
- [ ] Add module selection logic
- [ ] Verify compile-time checks work
- [ ] Document usage in CLAUDE.md

### Phase 3: UTF-8 Cached Implementation (Future)

**Goal:** Memory-efficient alternative

- [ ] Implement `CachedUtf8String` in `utf8_cached.rs`
- [ ] Test with `cargo test --features string-utf8-cached`
- [ ] Benchmark against `vec-char`
- [ ] Document trade-offs

### Phase 4: Small String Optimization (Future)

**Goal:** Optimize for short strings

- [ ] Implement `SmallString` in `small_opt.rs`
- [ ] Test with `cargo test --features string-small-opt`
- [ ] Benchmark against `vec-char`
- [ ] Document trade-offs

### Phase 5: VM Integration (Phase 2 VM)

**Goal:** Optimal strings for VM

- [ ] Design VM-specific string representation
- [ ] Consider interning, immutability
- [ ] Integrate with `TaggedValue` system

---

## 10. Open Questions

### Q1: Mutation Semantics with Interning

If strings are interned, `string-set!` cannot modify shared storage. Options:
1. Copy-on-write (break sharing on mutation)
2. Never intern mutable strings
3. Don't support interned strings with mutation

**Recommendation:** Copy-on-write for correctness, track mutation flag.

### Q2: Hash Implementation

Different implementations may hash differently. Should we:
1. Require identical hashing (complex)
2. Hash by content (may be slow for some impls)
3. Not require Hash trait

**Recommendation:** Hash by content using `chars()` iterator.

### Q3: Eq Implementation

Similar question for equality:
1. Byte-level equality (fast but may differ)
2. Character-level equality (always correct)

**Recommendation:** Character-level equality for correctness.

### Q4: Thread Safety

Current design uses thread-local for interning. If we need multi-threaded:
1. Use `Arc` instead of `Rc`
2. Use concurrent hash map for interning
3. Keep single-threaded (simpler)

**Recommendation:** Keep single-threaded for now (R7RS-small doesn't require threads).

### Q5: Empty String Singleton

Should empty string be a singleton?
```rust
lazy_static! {
    static ref EMPTY: SchemeString = SchemeString::new();
}
```

**Recommendation:** Yes, for implementations where allocation is expensive.

---

## 11. References

### Patina Docs
- [ARCHITECTURE_LESSONS.md](./ARCHITECTURE_LESSONS.md) - String handling comparison
- [VM_VALUE_ARCHITECTURE.md](./VM_VALUE_ARCHITECTURE.md) - VM value representation

### External
- [R7RS §6.7 Strings](https://small.r7rs.org/attachment/r7rs.pdf) - Specification
- [Rust String Internals](https://doc.rust-lang.org/std/string/struct.String.html)
- [SmallVec crate](https://docs.rs/smallvec/) - Similar optimization pattern
- [compact_str crate](https://docs.rs/compact_str/) - Rust small string optimization
