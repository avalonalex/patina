//! TaggedValue - Compact 8-byte value representation
//!
//! This module provides a space-efficient value representation using tagged pointers.
//! All values fit in a single `u64`:
//!
//! - **Immediates** (no heap allocation): fixnums, characters, booleans, null, eof
//! - **Heap pointers**: pairs, vectors, strings, closures, and other objects
//!
//! ## Tagging Scheme
//!
//! Uses low 3 bits for type tags:
//! - `000` - Fixnum (61-bit signed integer)
//! - `001` - Special values (#t, #f, (), eof, unspecified)
//! - `010` - Character (Unicode codepoint)
//! - `011` - Pair (heap pointer)
//! - `100` - Vector (heap pointer)
//! - `101` - String (heap pointer)
//! - `110` - Closure (heap pointer)
//! - `111` - Object (other heap types, sub-tag in header)
//!
//! TAG_FIXNUM is 0 so fixnum arithmetic doesn't require masking.

use std::fmt;

/// Index into a heap arena (32-bit for compact representation)
///
/// This allows up to 4 billion objects per heap arena.
pub type HeapIndex = u32;

/// Compact 8-byte value representation using tagged pointers
///
/// This is the unified value type for all of Patina. It replaces the
/// larger `Value` enum with a space-efficient representation.
///
/// # Memory Layout
///
/// ```text
/// ┌────────────────────────────────────────────────────────┬─────┐
/// │                    Payload (61 bits)                   │ Tag │
/// │                                                        │(3b) │
/// └────────────────────────────────────────────────────────┴─────┘
/// ```
///
/// # Examples
///
/// ```ignore
/// // Immediate values (no heap allocation)
/// let n = TaggedValue::fixnum(42);
/// let c = TaggedValue::character('λ');
/// let t = TaggedValue::TRUE;
///
/// // Heap values (require Heap)
/// let pair = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
/// ```
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaggedValue(u64);

impl TaggedValue {
    /// Access the raw bits for use as a hash key (e.g., in SourceMap)
    pub fn raw_bits(self) -> u64 {
        self.0
    }

    // =========================================================================
    // Tag Constants
    // =========================================================================

    /// Number of bits used for tags
    const TAG_BITS: u32 = 3;

    /// Mask for extracting tag bits
    const TAG_MASK: u64 = 0b111;

    // Primary tags (low 3 bits)
    // TAG_FIXNUM is 0b000 so fixnum arithmetic doesn't need masking
    const TAG_FIXNUM: u64 = 0b000; // 61-bit signed integer (immediate)
    const TAG_SPECIAL: u64 = 0b001; // #t, #f, (), eof, unspecified (immediate)
    const TAG_CHAR: u64 = 0b010; // Unicode codepoint (immediate)
    const TAG_PAIR: u64 = 0b011; // Cons cell (heap pointer)
    const TAG_VECTOR: u64 = 0b100; // Vector (heap pointer)
    const TAG_STRING: u64 = 0b101; // String (heap pointer)
    const TAG_CLOSURE: u64 = 0b110; // Closure (heap pointer)
    const TAG_OBJECT: u64 = 0b111; // Other heap objects (sub-tag in header)

    // =========================================================================
    // Special Value Constants
    // =========================================================================

    /// Boolean false: `#f`
    pub const FALSE: Self = Self(Self::TAG_SPECIAL);
    /// Boolean true: `#t`
    pub const TRUE: Self = Self(0x08 | Self::TAG_SPECIAL);
    /// Empty list: `()`
    pub const NULL: Self = Self(0x10 | Self::TAG_SPECIAL);
    /// End of file marker
    pub const EOF: Self = Self(0x18 | Self::TAG_SPECIAL);
    /// Unspecified value (void)
    pub const UNSPECIFIED: Self = Self(0x20 | Self::TAG_SPECIAL);
    /// Debug-build poison written into swept pair slots by the GC
    /// (`heap/gc.rs`). Never produced by any constructor, so pair accessors
    /// can assert against it to turn use-after-free into an immediate panic.
    pub(crate) const GC_POISON: Self = Self(0xF8 | Self::TAG_SPECIAL);
    /// What an environment slot holds when the binding lives in another
    /// environment — an imported one, which is the exporting library's
    /// location rather than a copy of what it held (`Owner` in
    /// `environment.rs`). Never produced by any constructor and never handed
    /// out: every read of a slot resolves it, so a program cannot see it and
    /// cannot forge one.
    pub(crate) const FORWARDED: Self = Self(0xF0 | Self::TAG_SPECIAL);
    /// What the VM writes into a register its per-pc liveness map calls dead,
    /// before a collection or a capture, in a check build
    /// ([`GC_CHECK`](crate::heap::GC_CHECK)); a plain release build writes
    /// [`TaggedValue::UNSPECIFIED`] (#625, GC_PRD §11.1 invariant 3). Never
    /// produced by any constructor and never a value a program holds: the VM
    /// panics when an instruction or a call reads one, and the heap's write
    /// paths panic when one would be stored, so a wrong map is reported where
    /// the dead register is read instead of reading as a legal value. Readers
    /// that only display registers render it as `#<dead>`.
    pub const DEAD_SLOT: Self = Self(0xE8 | Self::TAG_SPECIAL);

    // =========================================================================
    // Fixnum Constants and Operations
    // =========================================================================

    /// Minimum fixnum value (61-bit signed)
    pub const FIXNUM_MIN: i64 = -(1i64 << 60);
    /// Maximum fixnum value (61-bit signed)
    pub const FIXNUM_MAX: i64 = (1i64 << 60) - 1;

    /// Check if an i64 fits in a fixnum
    #[inline(always)]
    pub fn fits_fixnum(n: i64) -> bool {
        (Self::FIXNUM_MIN..=Self::FIXNUM_MAX).contains(&n)
    }

    /// Create a fixnum value
    ///
    /// # Panics
    ///
    /// Panics in debug mode if `n` doesn't fit in 61 bits.
    /// In release mode, the value is silently truncated.
    #[inline(always)]
    pub fn fixnum(n: i64) -> Self {
        debug_assert!(Self::fits_fixnum(n), "Fixnum overflow: {n}");
        // TAG_FIXNUM is 0, so we just shift left
        Self((n as u64) << Self::TAG_BITS)
    }

    /// Extract fixnum value without checking the tag
    ///
    /// # Safety
    ///
    /// Caller must ensure this is actually a fixnum via `is_fixnum()`.
    #[inline(always)]
    pub fn as_fixnum_unchecked(self) -> i64 {
        // Arithmetic right shift preserves sign
        (self.0 as i64) >> Self::TAG_BITS
    }

    /// Extract fixnum value, returning None if not a fixnum
    #[inline(always)]
    pub fn as_fixnum(self) -> Option<i64> {
        if self.is_fixnum() {
            Some(self.as_fixnum_unchecked())
        } else {
            None
        }
    }

    /// Fast fixnum addition with overflow check
    ///
    /// Returns `None` on overflow (caller should promote to BigInt).
    #[inline(always)]
    pub fn fixnum_add(self, other: Self) -> Option<Self> {
        debug_assert!(self.is_fixnum() && other.is_fixnum());

        let a = self.as_fixnum_unchecked();
        let b = other.as_fixnum_unchecked();

        match a.checked_add(b) {
            Some(sum) if Self::fits_fixnum(sum) => Some(Self::fixnum(sum)),
            _ => None, // Overflow
        }
    }

    /// Fast fixnum subtraction with overflow check
    #[inline(always)]
    pub fn fixnum_sub(self, other: Self) -> Option<Self> {
        debug_assert!(self.is_fixnum() && other.is_fixnum());

        let a = self.as_fixnum_unchecked();
        let b = other.as_fixnum_unchecked();

        match a.checked_sub(b) {
            Some(diff) if Self::fits_fixnum(diff) => Some(Self::fixnum(diff)),
            _ => None,
        }
    }

    /// Fast fixnum multiplication with overflow check
    #[inline(always)]
    pub fn fixnum_mul(self, other: Self) -> Option<Self> {
        debug_assert!(self.is_fixnum() && other.is_fixnum());

        let a = self.as_fixnum_unchecked();
        let b = other.as_fixnum_unchecked();

        match a.checked_mul(b) {
            Some(prod) if Self::fits_fixnum(prod) => Some(Self::fixnum(prod)),
            _ => None,
        }
    }

    /// Fast fixnum comparison (less than)
    #[inline(always)]
    pub fn fixnum_lt(self, other: Self) -> bool {
        debug_assert!(self.is_fixnum() && other.is_fixnum());
        // Can compare directly since TAG_FIXNUM is 0 and sign is preserved
        (self.0 as i64) < (other.0 as i64)
    }

    /// Fast fixnum comparison (equal)
    #[inline(always)]
    pub fn fixnum_eq(self, other: Self) -> bool {
        debug_assert!(self.is_fixnum() && other.is_fixnum());
        self.0 == other.0
    }

    /// Fast fixnum comparison (less than or equal)
    #[inline(always)]
    pub fn fixnum_le(self, other: Self) -> bool {
        debug_assert!(self.is_fixnum() && other.is_fixnum());
        (self.0 as i64) <= (other.0 as i64)
    }

    // =========================================================================
    // Character Operations
    // =========================================================================

    /// Create a character value
    #[inline(always)]
    pub fn character(c: char) -> Self {
        Self(((c as u64) << Self::TAG_BITS) | Self::TAG_CHAR)
    }

    /// Extract character without checking the tag
    ///
    /// # Safety
    ///
    /// Caller must ensure this is actually a character via `is_char()`.
    #[inline(always)]
    pub fn as_char_unchecked(self) -> char {
        // Safe because we only create valid Unicode codepoints
        char::from_u32((self.0 >> Self::TAG_BITS) as u32).expect("Invalid character in TaggedValue")
    }

    /// Extract character, returning None if not a character
    #[inline(always)]
    pub fn as_char(self) -> Option<char> {
        if self.is_char() {
            Some(self.as_char_unchecked())
        } else {
            None
        }
    }

    // =========================================================================
    // Boolean Operations
    // =========================================================================

    /// Create a boolean value
    #[inline(always)]
    pub fn boolean(b: bool) -> Self {
        if b { Self::TRUE } else { Self::FALSE }
    }

    /// Extract boolean without checking
    ///
    /// Returns true for #t, false for #f.
    /// For non-boolean values, behavior is undefined.
    #[inline(always)]
    pub fn as_bool_unchecked(self) -> bool {
        self == Self::TRUE
    }

    /// Check if this value is truthy (not #f)
    ///
    /// In Scheme, only #f is falsy. All other values are truthy.
    #[inline(always)]
    pub fn is_truthy(self) -> bool {
        self != Self::FALSE
    }

    // =========================================================================
    // Type Predicates
    // =========================================================================

    /// Check if this is a fixnum (61-bit signed integer)
    #[inline(always)]
    pub fn is_fixnum(self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_FIXNUM
    }

    /// Check if this is a special value (#t, #f, (), eof, unspecified)
    #[inline(always)]
    pub fn is_special(self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_SPECIAL
    }

    /// Check if this is a character
    #[inline(always)]
    pub fn is_char(self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_CHAR
    }

    /// Check if this is a pair
    #[inline(always)]
    pub fn is_pair(self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_PAIR
    }

    /// Check if this is a vector
    #[inline(always)]
    pub fn is_vector(self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_VECTOR
    }

    /// Check if this is a string
    #[inline(always)]
    pub fn is_string(self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_STRING
    }

    /// Check if this is a closure
    #[inline(always)]
    pub fn is_closure(self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_CLOSURE
    }

    /// Check if this is a generic object (uses sub-tag in heap header)
    #[inline(always)]
    pub fn is_object(self) -> bool {
        (self.0 & Self::TAG_MASK) == Self::TAG_OBJECT
    }

    /// Check if this is the null value (empty list)
    #[inline(always)]
    pub fn is_null(self) -> bool {
        self == Self::NULL
    }

    /// Check if this is a boolean
    #[inline(always)]
    pub fn is_boolean(self) -> bool {
        self == Self::TRUE || self == Self::FALSE
    }

    /// Check if this is the EOF object
    #[inline(always)]
    pub fn is_eof(self) -> bool {
        self == Self::EOF
    }

    /// Check if this is an immediate value (no heap allocation)
    #[inline(always)]
    pub fn is_immediate(self) -> bool {
        matches!(
            self.0 & Self::TAG_MASK,
            Self::TAG_FIXNUM | Self::TAG_SPECIAL | Self::TAG_CHAR
        )
    }

    /// Check if this is a heap pointer
    #[inline(always)]
    pub fn is_heap_pointer(self) -> bool {
        !self.is_immediate()
    }

    // =========================================================================
    // Heap Pointer Operations
    // =========================================================================
    //
    // The index constructors are crate-private: a heap reference comes from
    // the heap that owns the slot, which stamps it with the slot's generation
    // in a check build (`heap/check.rs`). A reference minted anywhere else
    // would carry no stamp, so it would be refused as stale once its slot had
    // been reused, and would not be `eq?` to the heap's own reference.

    /// Create a pair pointer from a heap index
    #[inline(always)]
    pub(crate) fn pair(index: HeapIndex) -> Self {
        Self(((index as u64) << Self::TAG_BITS) | Self::TAG_PAIR)
    }

    /// Create a vector pointer from a heap index
    #[inline(always)]
    pub(crate) fn vector(index: HeapIndex) -> Self {
        Self(((index as u64) << Self::TAG_BITS) | Self::TAG_VECTOR)
    }

    /// Create a string pointer from a heap index
    #[inline(always)]
    pub(crate) fn string(index: HeapIndex) -> Self {
        Self(((index as u64) << Self::TAG_BITS) | Self::TAG_STRING)
    }

    /// Create a closure pointer from a heap index. No arena allocates a
    /// `TAG_CLOSURE` value today; the tag is reserved.
    #[inline(always)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn closure(index: HeapIndex) -> Self {
        Self(((index as u64) << Self::TAG_BITS) | Self::TAG_CLOSURE)
    }

    /// Create a generic object pointer from a heap index
    #[inline(always)]
    pub(crate) fn object(index: HeapIndex) -> Self {
        Self(((index as u64) << Self::TAG_BITS) | Self::TAG_OBJECT)
    }

    /// Where a check build keeps a heap reference's allocation generation:
    /// payload bits 40–55, above the 32-bit index in bits 3–34 that
    /// [`Self::heap_index`] reads, so the index needs no masking.
    #[cfg(any(debug_assertions, feature = "gc-check"))]
    const GENERATION_SHIFT: u32 = 40;

    /// This reference with its generation stamp set to `generation` (#621).
    /// Only the heap stamps, with the generation of the slot it names.
    #[cfg(any(debug_assertions, feature = "gc-check"))]
    #[inline(always)]
    pub(crate) fn with_generation(self, generation: u16) -> Self {
        const MASK: u64 = 0xFFFF << TaggedValue::GENERATION_SHIFT;
        Self((self.0 & !MASK) | (u64::from(generation) << Self::GENERATION_SHIFT))
    }

    /// The generation a check build stamped into this heap reference: that of
    /// its slot when the reference was made.
    #[cfg(any(debug_assertions, feature = "gc-check"))]
    #[inline(always)]
    pub(crate) fn generation(self) -> u16 {
        (self.0 >> Self::GENERATION_SHIFT) as u16
    }

    /// Extract heap index (unchecked)
    ///
    /// # Safety
    ///
    /// Caller must ensure this is a heap pointer via `is_heap_pointer()`.
    #[inline(always)]
    pub fn heap_index(self) -> HeapIndex {
        debug_assert!(self.is_heap_pointer());
        (self.0 >> Self::TAG_BITS) as HeapIndex
    }

    /// Get the raw tag of this value
    #[inline(always)]
    pub fn tag(self) -> u64 {
        self.0 & Self::TAG_MASK
    }

    /// Get the raw u64 representation (for debugging/serialization)
    #[inline(always)]
    pub fn raw(self) -> u64 {
        self.0
    }

    /// The value whose raw bits are `raw`, as [`Self::raw`] returned them.
    ///
    /// Crate-private, as the index constructors are: raw bits from anywhere
    /// but a value the heap made would be a reference without the heap's
    /// stamp (`heap/check.rs`). The `syntax_sources` prune in sweep, which
    /// reads only the index, is the one caller.
    #[inline(always)]
    pub(crate) fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    // =========================================================================
    // Type Name (for error messages)
    // =========================================================================

    /// Get the type name of this value (for error messages)
    pub fn type_name(self) -> &'static str {
        if self.is_fixnum() {
            "integer"
        } else if self == Self::TRUE || self == Self::FALSE {
            "boolean"
        } else if self == Self::NULL {
            "null"
        } else if self == Self::EOF {
            "eof-object"
        } else if self == Self::UNSPECIFIED {
            "unspecified"
        } else if self.is_char() {
            "character"
        } else if self.is_pair() {
            "pair"
        } else if self.is_vector() {
            "vector"
        } else if self.is_string() {
            "string"
        } else if self.is_closure() {
            "procedure"
        } else if self.is_object() {
            "object" // Sub-type determined by heap header
        } else {
            "unknown"
        }
    }
}

// =========================================================================
// Debug and Display
// =========================================================================

impl fmt::Debug for TaggedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_fixnum() {
            write!(f, "TaggedValue::fixnum({})", self.as_fixnum_unchecked())
        } else if *self == Self::TRUE {
            write!(f, "TaggedValue::TRUE")
        } else if *self == Self::FALSE {
            write!(f, "TaggedValue::FALSE")
        } else if *self == Self::NULL {
            write!(f, "TaggedValue::NULL")
        } else if *self == Self::EOF {
            write!(f, "TaggedValue::EOF")
        } else if *self == Self::UNSPECIFIED {
            write!(f, "TaggedValue::UNSPECIFIED")
        } else if *self == Self::DEAD_SLOT {
            write!(f, "TaggedValue::DEAD_SLOT")
        } else if self.is_char() {
            write!(f, "TaggedValue::character({:?})", self.as_char_unchecked())
        } else if self.is_heap_pointer() {
            let kind = if self.is_pair() {
                "pair"
            } else if self.is_vector() {
                "vector"
            } else if self.is_string() {
                "string"
            } else if self.is_closure() {
                "closure"
            } else {
                "object"
            };
            write!(f, "TaggedValue::{kind}({}", self.heap_index())?;
            // Two references to one slot differ only in their stamps once the
            // slot is reused, so a check build shows it: without it a failed
            // `assert_eq!` prints the same text on both sides.
            #[cfg(any(debug_assertions, feature = "gc-check"))]
            write!(f, ", gen {}", self.generation())?;
            write!(f, ")")
        } else {
            write!(f, "TaggedValue({:#018x})", self.0)
        }
    }
}

/// An object-arena reference kept as a bare index rather than a value: the
/// VM's `CallFrame.closure`, which continuation snapshots copy.
///
/// In a check build it keeps the generation stamp of the reference it was
/// taken from, so the heap refuses it once its slot has been freed and
/// reused, as it refuses a stale [`TaggedValue`] (#621). Without the checks
/// it is the index alone, the size of a [`HeapIndex`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ObjectIndex {
    index: HeapIndex,
    #[cfg(any(debug_assertions, feature = "gc-check"))]
    generation: u16,
}

impl ObjectIndex {
    /// The index of `value` if it is an object reference.
    #[inline(always)]
    pub fn of(value: TaggedValue) -> Option<Self> {
        value.is_object().then(|| Self {
            index: value.heap_index(),
            #[cfg(any(debug_assertions, feature = "gc-check"))]
            generation: value.generation(),
        })
    }

    /// The slot in the object arena.
    #[inline(always)]
    pub fn index(self) -> HeapIndex {
        self.index
    }

    /// The object reference this index was taken from, stamp included.
    #[inline(always)]
    pub(crate) fn value(self) -> TaggedValue {
        let value = TaggedValue::object(self.index);
        #[cfg(any(debug_assertions, feature = "gc-check"))]
        let value = value.with_generation(self.generation);
        value
    }
}

impl fmt::Debug for ObjectIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectIndex({}", self.index)?;
        #[cfg(any(debug_assertions, feature = "gc-check"))]
        write!(f, ", gen {}", self.generation)?;
        write!(f, ")")
    }
}

impl fmt::Display for TaggedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display inline values nicely, show placeholders for heap values
        // (heap values need Heap access to display fully)
        if self.is_fixnum() {
            write!(f, "{}", self.as_fixnum_unchecked())
        } else if *self == Self::TRUE {
            write!(f, "#t")
        } else if *self == Self::FALSE {
            write!(f, "#f")
        } else if *self == Self::NULL {
            write!(f, "()")
        } else if *self == Self::EOF {
            write!(f, "#<eof>")
        } else if *self == Self::UNSPECIFIED {
            write!(f, "#<unspecified>")
        } else if *self == Self::DEAD_SLOT {
            write!(f, "#<dead>")
        } else if self.is_char() {
            let c = self.as_char_unchecked();
            match c {
                ' ' => write!(f, "#\\space"),
                '\n' => write!(f, "#\\newline"),
                '\t' => write!(f, "#\\tab"),
                '\r' => write!(f, "#\\return"),
                _ if !c.is_control() && !c.is_whitespace() => write!(f, "#\\{}", c),
                _ => write!(f, "#\\x{:04x}", c as u32),
            }
        } else if self.is_pair() {
            write!(f, "#<pair:{}>", self.heap_index())
        } else if self.is_vector() {
            write!(f, "#<vector:{}>", self.heap_index())
        } else if self.is_string() {
            write!(f, "#<string:{}>", self.heap_index())
        } else if self.is_closure() {
            write!(f, "#<closure:{}>", self.heap_index())
        } else if self.is_object() {
            write!(f, "#<object:{}>", self.heap_index())
        } else {
            write!(f, "#<unknown:{:#x}>", self.0)
        }
    }
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size() {
        assert_eq!(std::mem::size_of::<TaggedValue>(), 8);
    }

    // -------------------------------------------------------------------------
    // Fixnum tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fixnum_zero() {
        let v = TaggedValue::fixnum(0);
        assert!(v.is_fixnum());
        assert_eq!(v.as_fixnum_unchecked(), 0);
    }

    #[test]
    fn test_fixnum_positive() {
        for n in [1, 42, 1000, 1_000_000, 1_000_000_000_000i64] {
            let v = TaggedValue::fixnum(n);
            assert!(v.is_fixnum());
            assert_eq!(v.as_fixnum_unchecked(), n);
        }
    }

    #[test]
    fn test_fixnum_negative() {
        for n in [-1, -42, -1000, -1_000_000, -1_000_000_000_000i64] {
            let v = TaggedValue::fixnum(n);
            assert!(v.is_fixnum());
            assert_eq!(v.as_fixnum_unchecked(), n);
        }
    }

    #[test]
    fn test_fixnum_bounds() {
        // Test near the boundaries
        let max = TaggedValue::FIXNUM_MAX;
        let min = TaggedValue::FIXNUM_MIN;

        assert!(TaggedValue::fits_fixnum(max));
        assert!(TaggedValue::fits_fixnum(min));
        assert!(!TaggedValue::fits_fixnum(max + 1));
        assert!(!TaggedValue::fits_fixnum(min - 1));

        let v_max = TaggedValue::fixnum(max);
        let v_min = TaggedValue::fixnum(min);
        assert_eq!(v_max.as_fixnum_unchecked(), max);
        assert_eq!(v_min.as_fixnum_unchecked(), min);
    }

    #[test]
    fn test_fixnum_add() {
        let a = TaggedValue::fixnum(100);
        let b = TaggedValue::fixnum(42);
        let result = a.fixnum_add(b).unwrap();
        assert_eq!(result.as_fixnum_unchecked(), 142);
    }

    #[test]
    fn test_fixnum_add_negative() {
        let a = TaggedValue::fixnum(100);
        let b = TaggedValue::fixnum(-42);
        let result = a.fixnum_add(b).unwrap();
        assert_eq!(result.as_fixnum_unchecked(), 58);
    }

    #[test]
    fn test_fixnum_add_overflow() {
        let a = TaggedValue::fixnum(TaggedValue::FIXNUM_MAX);
        let b = TaggedValue::fixnum(1);
        assert!(a.fixnum_add(b).is_none());
    }

    #[test]
    fn test_fixnum_sub() {
        let a = TaggedValue::fixnum(100);
        let b = TaggedValue::fixnum(42);
        let result = a.fixnum_sub(b).unwrap();
        assert_eq!(result.as_fixnum_unchecked(), 58);
    }

    #[test]
    fn test_fixnum_sub_overflow() {
        let a = TaggedValue::fixnum(TaggedValue::FIXNUM_MIN);
        let b = TaggedValue::fixnum(1);
        assert!(a.fixnum_sub(b).is_none());
    }

    #[test]
    fn test_fixnum_mul() {
        let a = TaggedValue::fixnum(100);
        let b = TaggedValue::fixnum(42);
        let result = a.fixnum_mul(b).unwrap();
        assert_eq!(result.as_fixnum_unchecked(), 4200);
    }

    #[test]
    fn test_fixnum_mul_overflow() {
        let a = TaggedValue::fixnum(TaggedValue::FIXNUM_MAX);
        let b = TaggedValue::fixnum(2);
        assert!(a.fixnum_mul(b).is_none());
    }

    #[test]
    fn test_fixnum_comparisons() {
        let a = TaggedValue::fixnum(100);
        let b = TaggedValue::fixnum(42);
        let c = TaggedValue::fixnum(100);

        assert!(!a.fixnum_lt(b)); // 100 < 42 is false
        assert!(b.fixnum_lt(a)); // 42 < 100 is true
        assert!(!a.fixnum_lt(c)); // 100 < 100 is false

        assert!(a.fixnum_eq(c)); // 100 == 100
        assert!(!a.fixnum_eq(b)); // 100 == 42 is false

        assert!(a.fixnum_le(c)); // 100 <= 100
        assert!(!a.fixnum_le(b)); // 100 <= 42 is false
        assert!(b.fixnum_le(a)); // 42 <= 100
    }

    #[test]
    fn test_fixnum_negative_comparisons() {
        let neg = TaggedValue::fixnum(-10);
        let pos = TaggedValue::fixnum(10);
        let zero = TaggedValue::fixnum(0);

        assert!(neg.fixnum_lt(zero));
        assert!(neg.fixnum_lt(pos));
        assert!(zero.fixnum_lt(pos));
        assert!(!pos.fixnum_lt(neg));
    }

    // -------------------------------------------------------------------------
    // Special value tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_special_values() {
        assert!(TaggedValue::TRUE.is_special());
        assert!(TaggedValue::FALSE.is_special());
        assert!(TaggedValue::NULL.is_special());
        assert!(TaggedValue::EOF.is_special());
        assert!(TaggedValue::UNSPECIFIED.is_special());
    }

    #[test]
    fn test_boolean() {
        assert!(TaggedValue::TRUE.is_boolean());
        assert!(TaggedValue::FALSE.is_boolean());
        assert!(!TaggedValue::NULL.is_boolean());

        assert_eq!(TaggedValue::boolean(true), TaggedValue::TRUE);
        assert_eq!(TaggedValue::boolean(false), TaggedValue::FALSE);
    }

    #[test]
    fn test_truthy() {
        // Only #f is falsy
        assert!(!TaggedValue::FALSE.is_truthy());

        // Everything else is truthy
        assert!(TaggedValue::TRUE.is_truthy());
        assert!(TaggedValue::NULL.is_truthy());
        assert!(TaggedValue::fixnum(0).is_truthy());
        assert!(TaggedValue::character('a').is_truthy());
    }

    #[test]
    fn test_null() {
        assert!(TaggedValue::NULL.is_null());
        assert!(!TaggedValue::FALSE.is_null());
        assert!(!TaggedValue::fixnum(0).is_null());
    }

    #[test]
    fn test_eof() {
        assert!(TaggedValue::EOF.is_eof());
        assert!(!TaggedValue::NULL.is_eof());
    }

    // -------------------------------------------------------------------------
    // Character tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_character_ascii() {
        for c in ['a', 'Z', '0', ' ', '\n', '\t'] {
            let v = TaggedValue::character(c);
            assert!(v.is_char());
            assert_eq!(v.as_char_unchecked(), c);
        }
    }

    #[test]
    fn test_character_unicode() {
        for c in ['λ', '中', '🦀', '∀', '∞'] {
            let v = TaggedValue::character(c);
            assert!(v.is_char());
            assert_eq!(v.as_char_unchecked(), c);
        }
    }

    // -------------------------------------------------------------------------
    // Heap pointer tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_pair_pointer() {
        let v = TaggedValue::pair(42);
        assert!(v.is_pair());
        assert!(v.is_heap_pointer());
        assert!(!v.is_immediate());
        assert_eq!(v.heap_index(), 42);
    }

    #[test]
    fn test_vector_pointer() {
        let v = TaggedValue::vector(123);
        assert!(v.is_vector());
        assert!(v.is_heap_pointer());
        assert_eq!(v.heap_index(), 123);
    }

    #[test]
    fn test_string_pointer() {
        let v = TaggedValue::string(456);
        assert!(v.is_string());
        assert!(v.is_heap_pointer());
        assert_eq!(v.heap_index(), 456);
    }

    #[test]
    fn test_closure_pointer() {
        let v = TaggedValue::closure(789);
        assert!(v.is_closure());
        assert!(v.is_heap_pointer());
        assert_eq!(v.heap_index(), 789);
    }

    #[test]
    fn test_object_pointer() {
        let v = TaggedValue::object(999);
        assert!(v.is_object());
        assert!(v.is_heap_pointer());
        assert_eq!(v.heap_index(), 999);
    }

    // -------------------------------------------------------------------------
    // Type discrimination tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_type_discrimination() {
        let fixnum = TaggedValue::fixnum(42);
        let boolean = TaggedValue::TRUE;
        let null = TaggedValue::NULL;
        let char = TaggedValue::character('x');
        let pair = TaggedValue::pair(0);
        let vector = TaggedValue::vector(0);
        let string = TaggedValue::string(0);
        let closure = TaggedValue::closure(0);
        let object = TaggedValue::object(0);

        // Each type should only match its own predicate
        assert!(fixnum.is_fixnum() && !fixnum.is_char() && !fixnum.is_pair());
        assert!(boolean.is_boolean() && !boolean.is_fixnum());
        assert!(null.is_null() && null.is_special());
        assert!(char.is_char() && !char.is_fixnum());
        assert!(pair.is_pair() && !pair.is_vector());
        assert!(vector.is_vector() && !vector.is_string());
        assert!(string.is_string() && !string.is_closure());
        assert!(closure.is_closure() && !closure.is_object());
        assert!(object.is_object() && !object.is_pair());
    }

    // -------------------------------------------------------------------------
    // Debug formatting tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_debug_format() {
        assert_eq!(
            format!("{:?}", TaggedValue::fixnum(42)),
            "TaggedValue::fixnum(42)"
        );
        assert_eq!(format!("{:?}", TaggedValue::TRUE), "TaggedValue::TRUE");
        assert_eq!(format!("{:?}", TaggedValue::FALSE), "TaggedValue::FALSE");
        assert_eq!(format!("{:?}", TaggedValue::NULL), "TaggedValue::NULL");
        assert_eq!(
            format!("{:?}", TaggedValue::character('λ')),
            "TaggedValue::character('λ')"
        );
        let expected = if crate::heap::GC_CHECK {
            "TaggedValue::pair(5, gen 0)"
        } else {
            "TaggedValue::pair(5)"
        };
        assert_eq!(format!("{:?}", TaggedValue::pair(5)), expected);
        assert_eq!(
            format!("{:?}", TaggedValue::DEAD_SLOT),
            "TaggedValue::DEAD_SLOT"
        );
    }

    /// A retired register's fill is a special immediate of its own: no heap
    /// reference for the marker to follow, and equal to no value a program
    /// can make, the GC's own markers included.
    #[test]
    fn dead_slot_is_a_distinct_immediate_shown_as_dead() {
        let dead = TaggedValue::DEAD_SLOT;
        assert!(dead.is_special() && dead.is_immediate());
        for other in [
            TaggedValue::FALSE,
            TaggedValue::TRUE,
            TaggedValue::NULL,
            TaggedValue::EOF,
            TaggedValue::UNSPECIFIED,
            TaggedValue::GC_POISON,
            TaggedValue::FORWARDED,
        ] {
            assert_ne!(dead, other);
        }
        assert_eq!(dead.to_string(), "#<dead>");
    }

    #[cfg(any(debug_assertions, feature = "gc-check"))]
    #[test]
    fn generation_stamp_leaves_the_index_and_tag_alone() {
        let max_index = TaggedValue::object(HeapIndex::MAX);
        let stamped = max_index.with_generation(u16::MAX);
        assert_eq!(stamped.heap_index(), HeapIndex::MAX);
        assert!(stamped.is_object());
        assert_eq!(stamped.generation(), u16::MAX);
        assert_eq!(stamped.with_generation(3).generation(), 3);
        assert_ne!(stamped, max_index, "the stamp is part of identity");
        assert_eq!(
            format!("{:?}", TaggedValue::pair(7).with_generation(2)),
            "TaggedValue::pair(7, gen 2)"
        );
        let index = ObjectIndex::of(stamped).expect("an object reference");
        assert_eq!(index.index(), HeapIndex::MAX);
        assert_eq!(index.value(), stamped);
        assert_eq!(ObjectIndex::of(TaggedValue::pair(1)), None);
    }
}
