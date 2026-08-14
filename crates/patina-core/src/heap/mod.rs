//! Heap - Arena-based allocation for TaggedValue heap objects
//!
//! This module provides the heap arena for allocating objects that don't fit
//! in immediate TaggedValues. The heap uses typed storage for common object
//! types to avoid dynamic dispatch.
//!
//! ## Heap Object Types
//!
//! - **Pairs**: Cons cells `(car . cdr)`
//! - **Vectors**: Mutable arrays
//! - **Strings**: Immutable UTF-8 strings
//! - **Objects**: Other types (BigInt, Rational, Symbol, etc.)
//!
//! ## Memory Management
//!
//! Currently uses simple arena allocation without garbage collection.
//! Future versions will add reference counting or tracing GC.
//!
//! ## Module Organization
//!
//! - `mod.rs`: Core heap types, allocation, and non-numeric operations
//! - `numeric.rs`: All numeric operations (arithmetic, division, complex, number theory)
//! - `gc.rs`: Mark-and-sweep garbage collection (see `docs/GC_DESIGN.md`)

pub mod gc;
mod numeric;

use crate::tagged_value::{HeapIndex, TaggedValue};
use num_bigint::BigInt;
use num_rational::BigRational;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

// ============================================================================
// Shared Heap Type
// ============================================================================

/// Shared heap for TaggedValue allocations.
///
/// The heap is wrapped in `Rc<RefCell<...>>` so it can be shared across
/// multiple components (evaluator, environment, closures) without threading
/// a mutable reference through every function call.
///
/// # Example
///
/// ```ignore
/// let heap = SharedHeap::new();
/// let pair = heap.borrow_mut().alloc_pair(TaggedValue::fixnum(1), TaggedValue::fixnum(2));
/// ```
pub type SharedHeap = Rc<RefCell<Heap>>;

/// Result of draining [`Heap::take_gc_freed_bits`]: the raw bits of slots
/// reclaimed since the last drain, or `Overflowed` when the exact set was
/// lost to the recording cap and a consumer must treat its entire
/// raw-bits-keyed map as stale (design §9.1).
#[derive(Debug)]
pub enum GcFreedBits {
    Exact(Vec<u64>),
    Overflowed,
}

/// Type alias for the complex parameter value type returned by `Heap::get_parameter`.
///
/// Contains (values_stack, converter) where:
/// - `values_stack`: The stack of parameter values (most recent on top)
/// - `converter`: Optional converter procedure for the parameter
pub type ParameterData = (Rc<RefCell<Vec<TaggedValue>>>, Option<TaggedValue>);

/// Create a new shared heap
pub fn new_shared_heap() -> SharedHeap {
    Rc::new(RefCell::new(Heap::new()))
}

// ============================================================================
// Heap Object Types
// ============================================================================

/// Type tag for generic heap objects (TAG_OBJECT)
///
/// Objects with the TAG_OBJECT tag use this sub-tag in their header
/// to identify the actual type.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeapObjectType {
    BigInt = 0,
    Rational = 1,
    Real = 2,
    Complex = 3,
    Symbol = 4,
    Bytevector = 5,
    Port = 6,
    Macro = 7,
    Record = 8,
    RecordType = 9,
    Continuation = 10,
    PromptTag = 11,
    Promise = 12,
    Parameter = 13,
    Library = 14,
    Environment = 15,
    Exception = 16,
    Identifier = 17,
    Procedure = 18,
    Values = 19,
    LabelPlaceholder = 20,
    /// VM bytecode closure (patina-vm Phase 2)
    VmClosure = 21,
    /// Mutable cell for captured variables that are `set!` after capture (patina-vm Phase 2)
    MutableCell = 22,
    /// Opaque handle to a full VM continuation stored in VmState's side table (Phase 2 A6)
    VmContinuationRef = 23,
    /// Opaque handle to a delimited VM continuation stored in VmState's side table (Phase 2 A6)
    VmDelimitedContinuationRef = 24,
    /// Tombstone left by the GC sweep; the slot is on the free list awaiting reuse
    Free = 25,
}

/// State of a promise for lazy evaluation
///
/// Uses TaggedValue for efficient storage without Value conversion overhead.
#[derive(Debug, Clone, Copy)]
pub enum PromiseState {
    /// Not yet evaluated - contains the thunk to evaluate
    Delayed(TaggedValue),
    /// Evaluated - contains the cached result
    Forced(TaggedValue),
}

/// Generic heap object data
///
/// This enum holds the actual data for objects that use TAG_OBJECT.
///
/// Adding a variant? The GC tracer's exhaustive match in
/// `heap/gc.rs::trace_object_children` will fail to compile until you add an
/// arm. Put the variant in the leaf arm ONLY if it embeds no `TaggedValue`,
/// `Rc<Environment>`, `Rc<CpsContinuation>`, or `CpsExpr` — a value-bearing
/// variant misfiled as a leaf is a use-after-free, not a compile error.
#[derive(Debug, Clone)]
pub enum HeapObjectData {
    BigInt(BigInt),
    Rational(BigRational),
    Real(f64),
    Complex {
        real: TaggedValue,
        imag: TaggedValue,
    },
    Symbol(Rc<str>),
    Bytevector(Vec<u8>),
    /// Exception object with kind, message, and irritants (as TaggedValues)
    Exception {
        kind: crate::error::ExceptionKind,
        message: String,
        irritants: Vec<TaggedValue>,
    },
    Procedure(Rc<crate::procedure::Procedure>),
    Port(Rc<crate::port::Port>),
    Macro(Rc<crate::compiled_macro::CompiledMacro>),
    RecordType(Rc<crate::record_type::RecordTypeDescriptor>),
    Record {
        record_type: Rc<crate::record_type::RecordTypeDescriptor>,
        fields: Rc<RefCell<Vec<TaggedValue>>>,
    },
    Identifier {
        name: Rc<str>,
        scopes: crate::scope::ScopeSet,
    },
    Continuation(Rc<crate::continuation::CpsContinuation>),
    Parameter {
        values: Rc<RefCell<Vec<TaggedValue>>>,
        converter: Option<TaggedValue>,
    },
    Promise(Rc<RefCell<PromiseState>>),
    Library(Rc<crate::library::Library>),
    Values(Vec<TaggedValue>),
    EnvironmentSpecifier {
        env: Rc<crate::environment::Environment>,
        mutable: bool,
    },
    PromptTag(Rc<crate::cps_expr::PromptTag>),
    LabelPlaceholder(usize),
    /// A mutable cell used to box variables that are `set!` after being captured
    /// by a VM closure. The cell holds the current value and is shared by
    /// reference among all closures that captured the same binding.
    MutableCell(RefCell<TaggedValue>),
    /// A VM bytecode closure: code id + captured free variables + globals env.
    VmClosure {
        /// Serialised as `u32` to avoid a direct dependency on patina-vm types.
        code_id: u32,
        free_vars: Vec<TaggedValue>,
        /// The global environment this closure was compiled against.
        /// `LoadGlobal`/`StoreGlobal` use this instead of `VmState::globals`.
        globals: Rc<crate::environment::Environment>,
    },
    /// Opaque handle to a full VM continuation (call/cc style).
    /// The actual `VmContinuation` data lives in `VmState::continuation_store`.
    VmContinuationRef(u64),
    /// Opaque handle to a delimited VM continuation.
    /// The actual `VmDelimitedContinuation` data lives in `VmState::delimited_continuation_store`.
    VmDelimitedContinuationRef(u64),
    /// Tombstone written by the GC sweep. Overwriting the dead slot (rather
    /// than leaving stale data until reuse) drops `Rc` payloads eagerly,
    /// which is what breaks closure ↔ environment cycles. Accessing a Free
    /// slot through `get_object` is a use-after-free bug.
    Free,
}

impl HeapObjectData {
    /// Get the type tag for this object
    pub fn object_type(&self) -> HeapObjectType {
        match self {
            HeapObjectData::BigInt(_) => HeapObjectType::BigInt,
            HeapObjectData::Rational(_) => HeapObjectType::Rational,
            HeapObjectData::Real(_) => HeapObjectType::Real,
            HeapObjectData::Complex { .. } => HeapObjectType::Complex,
            HeapObjectData::Symbol(_) => HeapObjectType::Symbol,
            HeapObjectData::Bytevector(_) => HeapObjectType::Bytevector,
            HeapObjectData::Exception { .. } => HeapObjectType::Exception,
            HeapObjectData::Procedure(_) => HeapObjectType::Procedure,
            HeapObjectData::Port(_) => HeapObjectType::Port,
            HeapObjectData::Macro(_) => HeapObjectType::Macro,
            HeapObjectData::RecordType(_) => HeapObjectType::RecordType,
            HeapObjectData::Record { .. } => HeapObjectType::Record,
            HeapObjectData::Identifier { .. } => HeapObjectType::Identifier,
            HeapObjectData::Continuation(_) => HeapObjectType::Continuation,
            HeapObjectData::Parameter { .. } => HeapObjectType::Parameter,
            HeapObjectData::Promise(_) => HeapObjectType::Promise,
            HeapObjectData::Library(_) => HeapObjectType::Library,
            HeapObjectData::Values(_) => HeapObjectType::Values,
            HeapObjectData::EnvironmentSpecifier { .. } => HeapObjectType::Environment,
            HeapObjectData::PromptTag(_) => HeapObjectType::PromptTag,
            HeapObjectData::LabelPlaceholder(_) => HeapObjectType::LabelPlaceholder,
            HeapObjectData::VmClosure { .. } => HeapObjectType::VmClosure,
            HeapObjectData::MutableCell(_) => HeapObjectType::MutableCell,
            HeapObjectData::VmContinuationRef(_) => HeapObjectType::VmContinuationRef,
            HeapObjectData::VmDelimitedContinuationRef(_) => {
                HeapObjectType::VmDelimitedContinuationRef
            }
            HeapObjectData::Free => HeapObjectType::Free,
        }
    }
}

// ============================================================================
// Heap Arena
// ============================================================================

/// Heap arena for managing TaggedValue allocations
///
/// Uses typed storage for common object types to avoid dynamic dispatch.
/// Each object type has its own vector for cache-friendly access.
///
/// # Example
///
/// ```ignore
/// let mut heap = Heap::new();
///
/// // Allocate a pair
/// let pair = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::fixnum(2));
/// assert_eq!(heap.car(pair).as_fixnum_unchecked(), 1);
/// assert_eq!(heap.cdr(pair).as_fixnum_unchecked(), 2);
///
/// // Allocate a vector
/// let vec = heap.alloc_vector(vec![TaggedValue::fixnum(1), TaggedValue::fixnum(2)]);
/// assert_eq!(heap.vector_len(vec), 2);
/// ```
#[derive(Debug)]
pub struct Heap {
    /// Pair storage: (car, cdr) tuples
    pairs: Vec<(TaggedValue, TaggedValue)>,

    /// Vector storage
    vectors: Vec<Vec<TaggedValue>>,

    /// String storage (mutable Vec<char> for O(1) character access)
    strings: Vec<Vec<char>>,

    /// Generic object storage (BigInt, Rational, Symbol, etc.)
    objects: Vec<HeapObjectData>,

    /// Symbol intern table (name -> object index)
    symbol_table: std::collections::HashMap<String, HeapIndex>,

    /// Free list for pairs (indices of freed pairs)
    free_pairs: Vec<HeapIndex>,

    /// Free list for vectors
    free_vectors: Vec<HeapIndex>,

    /// Free list for strings
    free_strings: Vec<HeapIndex>,

    /// Free list for objects
    free_objects: Vec<HeapIndex>,

    /// Allocations since the last GC (drives the collection trigger)
    allocs_since_gc: usize,

    /// Allocation count at which `note_alloc` raises the collection-pending
    /// flag. `usize::MAX` (the default) is inert: only `request_gc` raises
    /// the flag. A backend installs its policy's threshold via
    /// `GcController::current_threshold` when heap and controller are paired,
    /// and `GcController::collect` re-installs it after each collection (the
    /// adaptive `2 × live` term changes only there).
    gc_threshold: usize,

    /// The collection-pending flag: raised by `note_alloc` on threshold
    /// crossing and by `request_gc`; lowered by `sweep`. Lives outside the
    /// heap's `RefCell` (shared `Cell` handles via `gc_pending_handle`) so a
    /// backend safe point is a single load with no borrow — the whole point
    /// of the design §6.1 trigger redesign: the safe point used to ask a
    /// question whose answer only changes when something allocates.
    gc_pending: Rc<Cell<bool>>,

    /// Raw bits of `TaggedValue`s whose slots sweep reclaimed, for pruning
    /// diagnostics maps keyed by raw bits (`SourceMap` — design §9.1): a
    /// reused slot must not inherit the old datum's source location. `None`
    /// (the default) means nobody consumes them and sweep records nothing.
    gc_freed_bits: Option<Vec<u64>>,

    /// Set instead of growing `gc_freed_bits` past its cap; consumers must
    /// then treat *every* entry as possibly stale.
    gc_freed_overflow: bool,

    /// Nesting depth of scopes that hold live values unreachable from any
    /// registered root (nested trampolines, library-body loops). Collection
    /// is only legal at the outermost level — see `docs/GC_DESIGN.md` §7.
    gc_defer_depth: u32,

    /// Collections performed, and slots reclaimed by the last one. Recorded
    /// by `sweep` so `(gc-stats)` can report real collector activity without
    /// reaching into a backend-private collector.
    gc_collections: u64,
    gc_last_swept: usize,

    /// Next id for `VmContinuationRef` / `VmDelimitedContinuationRef`
    /// handles. Heap-owned so ids are unique across both continuation kinds
    /// and every `VmState` sharing this heap — the weak side tables
    /// (design §9.5) key on these ids and must never see one alias two
    /// entries. Monotonic, never reused.
    next_vm_continuation_id: u64,
}

/// `eqv?` on two inexact reals: identical bit patterns, not IEEE `==`.
///
/// R7RS 6.1 holds `eqv?` on inexact numbers only when they "yield the same
/// results ... when passed as arguments to any other procedure that can be
/// defined as a finite composition of Scheme's standard arithmetic
/// procedures". IEEE `==` misses that in both directions: it reports `+nan.0`
/// unequal to itself, and reports `0.0` equal to `-0.0` even though
/// `(/ 1.0 0.0)` is `+inf.0` while `(/ 1.0 -0.0)` is `-inf.0`.
///
/// Numeric `=` must NOT use this -- there IEEE semantics are correct. See
/// `numeric.rs` `num_equal`. `tagged_value_hash_depth` hashes reals by bit
/// pattern too, with all NaNs collapsed to one bucket, so hashing agrees.
#[inline]
fn real_eqv(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

impl Heap {
    /// Create a new empty heap
    pub fn new() -> Self {
        Self::with_capacity(0, 0, 0)
    }

    /// Create a heap with pre-allocated capacity
    pub fn with_capacity(pairs: usize, vectors: usize, strings: usize) -> Self {
        Self {
            pairs: Vec::with_capacity(pairs),
            vectors: Vec::with_capacity(vectors),
            strings: Vec::with_capacity(strings),
            objects: Vec::new(),
            symbol_table: std::collections::HashMap::new(),
            free_pairs: Vec::new(),
            free_vectors: Vec::new(),
            free_strings: Vec::new(),
            free_objects: Vec::new(),
            allocs_since_gc: 0,
            gc_threshold: usize::MAX,
            gc_pending: Rc::new(Cell::new(false)),
            gc_freed_bits: None,
            gc_freed_overflow: false,
            gc_defer_depth: 0,
            gc_collections: 0,
            gc_last_swept: 0,
            next_vm_continuation_id: 0,
        }
    }

    // =========================================================================
    // GC bookkeeping (see heap/gc.rs and docs/GC_DESIGN.md)
    // =========================================================================

    /// Allocations since the last collection.
    pub fn allocs_since_gc(&self) -> usize {
        self.allocs_since_gc
    }

    /// The single invariant of the trigger design: the pending flag is up
    /// whenever the counter has reached the threshold (or `(gc)` raised it
    /// directly).
    #[inline]
    fn refresh_gc_pending(&self) {
        if self.allocs_since_gc >= self.gc_threshold {
            self.gc_pending.set(true);
        }
    }

    /// Count one allocation, raising the collection-pending flag when the
    /// policy's threshold is crossed. Called by every `alloc_*`; this is
    /// where the collection decision is *made*, so safe points only have to
    /// read the flag (design §6.1).
    #[inline]
    fn note_alloc(&mut self) {
        self.allocs_since_gc += 1;
        self.refresh_gc_pending();
    }

    /// A handle to the collection-pending flag. Dispatch loops clone this
    /// once at entry so their per-iteration safe point is a single load with
    /// no `RefCell` borrow.
    pub fn gc_pending_handle(&self) -> Rc<Cell<bool>> {
        self.gc_pending.clone()
    }

    /// Install the allocation threshold at which `note_alloc` raises the
    /// pending flag. Called with `GcController::current_threshold` when heap
    /// and controller are paired and again after each collection (the
    /// adaptive term changes only there). Raises the flag immediately if the
    /// counter already exceeds the new threshold.
    pub fn set_gc_threshold(&mut self, threshold: usize) {
        self.gc_threshold = threshold;
        self.refresh_gc_pending();
    }

    /// Ask for a collection at the next backend safe point (the `(gc)`
    /// primitive cannot collect in place: it runs mid-evaluation, where live
    /// values sit in Rust locals no root provider can see).
    ///
    /// Raises the pending flag — honored regardless of the installed
    /// threshold — and `sweep` lowers it.
    pub fn request_gc(&mut self) {
        self.gc_pending.set(true);
    }

    /// Start recording the raw bits of slots sweep reclaims, for
    /// diagnostics-map pruning (design §9.1). Idempotent. Callers must
    /// periodically drain with [`Heap::take_gc_freed_bits`]; the buffer is
    /// capped, and overflowing it degrades the next drain to "prune
    /// everything", never to unbounded growth.
    pub fn enable_gc_freed_tracking(&mut self) {
        if self.gc_freed_bits.is_none() {
            self.gc_freed_bits = Some(Vec::new());
        }
    }

    /// Drain the slots reclaimed since the last drain. Entries in a
    /// raw-bits-keyed map for `Exact` bits are stale and must be dropped; on
    /// `Overflowed` the exact set was lost and the whole map must be treated
    /// as stale.
    pub fn take_gc_freed_bits(&mut self) -> GcFreedBits {
        if std::mem::take(&mut self.gc_freed_overflow) {
            if let Some(bits) = &mut self.gc_freed_bits {
                bits.clear();
            }
            GcFreedBits::Overflowed
        } else {
            GcFreedBits::Exact(
                self.gc_freed_bits
                    .as_mut()
                    .map(std::mem::take)
                    .unwrap_or_default(),
            )
        }
    }

    /// Depth of active GC-deferring scopes. A backend safe point may only
    /// collect when this is at its own baseline — see `docs/GC_DESIGN.md` §7
    /// and [`GcDeferGuard`](crate::GcDeferGuard).
    pub fn gc_defer_depth(&self) -> u32 {
        self.gc_defer_depth
    }

    pub(crate) fn enter_gc_defer(&mut self) {
        self.gc_defer_depth += 1;
    }

    pub(crate) fn exit_gc_defer(&mut self) {
        debug_assert!(
            self.gc_defer_depth > 0,
            "unbalanced GC defer: exit without a matching enter"
        );
        self.gc_defer_depth -= 1;
    }

    /// Collections performed against this heap.
    pub fn gc_collections(&self) -> u64 {
        self.gc_collections
    }

    /// Slots reclaimed by the most recent collection.
    pub fn gc_last_swept(&self) -> usize {
        self.gc_last_swept
    }

    // =========================================================================
    // Pair Operations
    // =========================================================================

    /// Allocate a new pair (cons cell).
    ///
    /// Callers may hold partial structures in Rust locals across `alloc_*`
    /// calls: collection runs only at the backend loops' safe points, never
    /// inside allocation (see `docs/GC_DESIGN.md` §6), so such partials
    /// need no GC root.
    #[inline]
    pub fn alloc_pair(&mut self, car: TaggedValue, cdr: TaggedValue) -> TaggedValue {
        self.note_alloc();
        let index = if let Some(free) = self.free_pairs.pop() {
            self.pairs[free as usize] = (car, cdr);
            free
        } else {
            let index = self.pairs.len() as HeapIndex;
            self.pairs.push((car, cdr));
            index
        };
        TaggedValue::pair(index)
    }

    /// Get pair contents
    #[inline(always)]
    pub fn get_pair(&self, ptr: TaggedValue) -> (TaggedValue, TaggedValue) {
        debug_assert!(ptr.is_pair());
        let pair = self.pairs[ptr.heap_index() as usize];
        debug_assert!(
            pair.0 != TaggedValue::GC_POISON,
            "use-after-free: pair slot {} was reclaimed by the GC",
            ptr.heap_index()
        );
        pair
    }

    /// Get car of a pair
    #[inline(always)]
    pub fn car(&self, ptr: TaggedValue) -> TaggedValue {
        self.get_pair(ptr).0
    }

    /// Get cdr of a pair
    #[inline(always)]
    pub fn cdr(&self, ptr: TaggedValue) -> TaggedValue {
        self.get_pair(ptr).1
    }

    /// Set car of a pair (for set-car!)
    #[inline(always)]
    pub fn set_car(&mut self, ptr: TaggedValue, value: TaggedValue) {
        debug_assert!(ptr.is_pair());
        debug_assert!(
            self.pairs[ptr.heap_index() as usize].1 != TaggedValue::GC_POISON,
            "use-after-free: pair slot {} was reclaimed by the GC",
            ptr.heap_index()
        );
        self.pairs[ptr.heap_index() as usize].0 = value;
    }

    /// Set cdr of a pair (for set-cdr!)
    #[inline(always)]
    pub fn set_cdr(&mut self, ptr: TaggedValue, value: TaggedValue) {
        debug_assert!(ptr.is_pair());
        debug_assert!(
            self.pairs[ptr.heap_index() as usize].0 != TaggedValue::GC_POISON,
            "use-after-free: pair slot {} was reclaimed by the GC",
            ptr.heap_index()
        );
        self.pairs[ptr.heap_index() as usize].1 = value;
    }

    // =========================================================================
    // Vector Operations
    // =========================================================================

    /// Allocate a new vector
    pub fn alloc_vector(&mut self, elements: Vec<TaggedValue>) -> TaggedValue {
        self.note_alloc();
        let index = if let Some(free) = self.free_vectors.pop() {
            self.vectors[free as usize] = elements;
            free
        } else {
            let index = self.vectors.len() as HeapIndex;
            self.vectors.push(elements);
            index
        };
        TaggedValue::vector(index)
    }

    /// Allocate a vector filled with a value
    pub fn alloc_vector_fill(&mut self, len: usize, fill: TaggedValue) -> TaggedValue {
        self.alloc_vector(vec![fill; len])
    }

    /// Get vector length
    #[inline(always)]
    pub fn vector_len(&self, ptr: TaggedValue) -> usize {
        debug_assert!(ptr.is_vector());
        self.vectors[ptr.heap_index() as usize].len()
    }

    /// Get vector element
    #[inline(always)]
    pub fn vector_ref(&self, ptr: TaggedValue, index: usize) -> TaggedValue {
        debug_assert!(ptr.is_vector());
        self.vectors[ptr.heap_index() as usize][index]
    }

    /// Set vector element (for vector-set!)
    #[inline(always)]
    pub fn vector_set(&mut self, ptr: TaggedValue, index: usize, value: TaggedValue) {
        debug_assert!(ptr.is_vector());
        self.vectors[ptr.heap_index() as usize][index] = value;
    }

    /// Get a slice of the vector
    pub fn vector_slice(&self, ptr: TaggedValue) -> &[TaggedValue] {
        debug_assert!(ptr.is_vector());
        &self.vectors[ptr.heap_index() as usize]
    }

    /// Get a mutable slice of the vector
    pub fn vector_slice_mut(&mut self, ptr: TaggedValue) -> &mut [TaggedValue] {
        debug_assert!(ptr.is_vector());
        &mut self.vectors[ptr.heap_index() as usize]
    }

    // =========================================================================
    // String Operations
    // =========================================================================

    /// Allocate a new string from a String (converts to Vec<char> internally)
    pub fn alloc_string(&mut self, s: String) -> TaggedValue {
        self.alloc_string_chars(s.chars().collect())
    }

    /// Allocate a new string from Vec<char> (primary method)
    pub fn alloc_string_chars(&mut self, chars: Vec<char>) -> TaggedValue {
        self.note_alloc();
        let index = if let Some(free) = self.free_strings.pop() {
            self.strings[free as usize] = chars;
            free
        } else {
            let index = self.strings.len() as HeapIndex;
            self.strings.push(chars);
            index
        };
        TaggedValue::string(index)
    }

    /// Allocate a string from a &str
    pub fn alloc_str(&mut self, s: &str) -> TaggedValue {
        self.alloc_string(s.to_string())
    }

    /// Get string characters as a slice (O(1) access)
    #[inline(always)]
    pub fn get_string_chars(&self, ptr: TaggedValue) -> &[char] {
        debug_assert!(ptr.is_string());
        &self.strings[ptr.heap_index() as usize]
    }

    /// Get mutable access to string characters
    #[inline(always)]
    pub fn get_string_chars_mut(&mut self, ptr: TaggedValue) -> &mut Vec<char> {
        debug_assert!(ptr.is_string());
        &mut self.strings[ptr.heap_index() as usize]
    }

    /// Set a single character in a native heap string
    #[inline]
    pub fn string_set_char(&mut self, ptr: TaggedValue, index: usize, ch: char) {
        debug_assert!(ptr.is_string());
        self.strings[ptr.heap_index() as usize][index] = ch;
    }

    /// Convert a native heap string to a UTF-8 String (for I/O, file paths, etc.)
    pub fn get_string_as_utf8(&self, ptr: TaggedValue) -> String {
        self.get_string_chars(ptr).iter().collect()
    }

    /// Get string length in characters (O(1))
    #[inline(always)]
    pub fn string_char_len(&self, ptr: TaggedValue) -> usize {
        self.get_string_chars(ptr).len()
    }

    // =========================================================================
    // Generic Object Operations
    // =========================================================================

    /// Allocate a BigInt
    pub fn alloc_bigint(&mut self, n: BigInt) -> TaggedValue {
        self.alloc_object(HeapObjectData::BigInt(n))
    }

    /// Allocate a Rational
    pub fn alloc_rational(&mut self, r: BigRational) -> TaggedValue {
        self.alloc_object(HeapObjectData::Rational(r))
    }

    /// Allocate a Real (boxed f64)
    pub fn alloc_real(&mut self, f: f64) -> TaggedValue {
        self.alloc_object(HeapObjectData::Real(f))
    }

    /// Allocate a Complex number
    pub fn alloc_complex(&mut self, real: TaggedValue, imag: TaggedValue) -> TaggedValue {
        self.alloc_object(HeapObjectData::Complex { real, imag })
    }

    /// Allocate a bytevector
    pub fn alloc_bytevector(&mut self, bytes: Vec<u8>) -> TaggedValue {
        self.alloc_object(HeapObjectData::Bytevector(bytes))
    }

    /// Allocate an exception object
    ///
    /// Creates a native TaggedValue exception with the given kind, message, and irritants.
    pub fn alloc_exception(
        &mut self,
        kind: crate::error::ExceptionKind,
        message: String,
        irritants: Vec<TaggedValue>,
    ) -> TaggedValue {
        self.alloc_object(HeapObjectData::Exception {
            kind,
            message,
            irritants,
        })
    }

    /// Allocate a native procedure
    pub fn alloc_procedure(&mut self, proc: Rc<crate::procedure::Procedure>) -> TaggedValue {
        self.alloc_object(HeapObjectData::Procedure(proc))
    }

    /// Allocate a native port
    pub fn alloc_port(&mut self, port: Rc<crate::port::Port>) -> TaggedValue {
        self.alloc_object(HeapObjectData::Port(port))
    }

    /// Allocate a native compiled macro
    pub fn alloc_macro(&mut self, mac: Rc<crate::compiled_macro::CompiledMacro>) -> TaggedValue {
        self.alloc_object(HeapObjectData::Macro(mac))
    }

    /// Allocate a native record type descriptor
    pub fn alloc_record_type(
        &mut self,
        rtd: Rc<crate::record_type::RecordTypeDescriptor>,
    ) -> TaggedValue {
        self.alloc_object(HeapObjectData::RecordType(rtd))
    }

    /// Allocate a native record instance
    pub fn alloc_record(
        &mut self,
        record_type: Rc<crate::record_type::RecordTypeDescriptor>,
        fields: Rc<RefCell<Vec<TaggedValue>>>,
    ) -> TaggedValue {
        self.alloc_object(HeapObjectData::Record {
            record_type,
            fields,
        })
    }

    /// Get exception data from a TaggedValue (if it's an Exception object)
    pub fn get_exception(
        &self,
        tagged: TaggedValue,
    ) -> Option<(&crate::error::ExceptionKind, &str, &[TaggedValue])> {
        if !tagged.is_object() {
            return None;
        }
        let index = tagged.heap_index() as usize;
        match self.objects.get(index)? {
            HeapObjectData::Exception {
                kind,
                message,
                irritants,
            } => Some((kind, message, irritants)),
            _ => None,
        }
    }

    /// Intern a symbol (returns existing if already interned)
    pub fn intern_symbol(&mut self, name: &str) -> TaggedValue {
        if let Some(&index) = self.symbol_table.get(name) {
            TaggedValue::object(index)
        } else {
            let tagged = self.alloc_object(HeapObjectData::Symbol(Rc::from(name)));
            self.symbol_table
                .insert(name.to_string(), tagged.heap_index());
            tagged
        }
    }

    /// Allocate a native identifier with scope set
    pub fn alloc_identifier(
        &mut self,
        name: Rc<str>,
        scopes: crate::scope::ScopeSet,
    ) -> TaggedValue {
        self.alloc_object(HeapObjectData::Identifier { name, scopes })
    }

    /// Allocate a native Continuation object (first-class continuation)
    pub fn alloc_continuation(
        &mut self,
        k: Rc<crate::continuation::CpsContinuation>,
    ) -> TaggedValue {
        self.alloc_object(HeapObjectData::Continuation(k))
    }

    /// Allocate a native Parameter object
    pub fn alloc_parameter(
        &mut self,
        values: Rc<RefCell<Vec<TaggedValue>>>,
        converter: Option<TaggedValue>,
    ) -> TaggedValue {
        self.alloc_object(HeapObjectData::Parameter { values, converter })
    }

    /// Allocate a native Promise object
    pub fn alloc_promise(&mut self, state: Rc<RefCell<PromiseState>>) -> TaggedValue {
        self.alloc_object(HeapObjectData::Promise(state))
    }

    /// Allocate a native Library object
    pub fn alloc_library(&mut self, lib: Rc<crate::library::Library>) -> TaggedValue {
        self.alloc_object(HeapObjectData::Library(lib))
    }

    /// Get a Library from a TaggedValue, if it is one
    pub fn get_library(&self, tv: TaggedValue) -> Option<&Rc<crate::library::Library>> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Library(lib) => Some(lib),
            _ => None,
        }
    }

    /// Check if value is a library
    #[inline]
    pub fn is_library(&self, tv: TaggedValue) -> bool {
        if !tv.is_object() {
            return false;
        }
        matches!(self.get_object(tv), HeapObjectData::Library(_))
    }

    /// Allocate a native Values object (multiple return values)
    pub fn alloc_values(&mut self, values: Vec<TaggedValue>) -> TaggedValue {
        self.alloc_object(HeapObjectData::Values(values))
    }

    /// Get native Values contents as a slice, if it is one
    pub fn get_values(&self, tv: TaggedValue) -> Option<&[TaggedValue]> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Values(vals) => Some(vals),
            _ => None,
        }
    }

    /// Allocate a parser label placeholder
    pub fn alloc_label_placeholder(&mut self, label: usize) -> TaggedValue {
        self.alloc_object(HeapObjectData::LabelPlaceholder(label))
    }

    /// Allocate a native EnvironmentSpecifier object
    pub fn alloc_environment_specifier(
        &mut self,
        env: Rc<crate::environment::Environment>,
        mutable: bool,
    ) -> TaggedValue {
        self.alloc_object(HeapObjectData::EnvironmentSpecifier { env, mutable })
    }

    /// Get the environment and mutability from a TaggedValue, if it is an EnvironmentSpecifier
    pub fn get_environment_specifier(
        &self,
        tv: TaggedValue,
    ) -> Option<(&Rc<crate::environment::Environment>, bool)> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::EnvironmentSpecifier { env, mutable } => Some((env, *mutable)),
            _ => None,
        }
    }

    /// Allocate a native PromptTag object
    pub fn alloc_prompt_tag(&mut self, tag: Rc<crate::cps_expr::PromptTag>) -> TaggedValue {
        self.alloc_object(HeapObjectData::PromptTag(tag))
    }

    /// Get the PromptTag from a TaggedValue, if it is one
    pub fn get_prompt_tag(&self, tv: TaggedValue) -> Option<&Rc<crate::cps_expr::PromptTag>> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::PromptTag(tag) => Some(tag),
            _ => None,
        }
    }

    // =========================================================================
    // MutableCell Operations (patina-vm Phase 2 — set! on captured variables)
    // =========================================================================

    /// Allocate a `MutableCell` containing `val`.
    pub fn alloc_mutable_cell(&mut self, val: TaggedValue) -> TaggedValue {
        self.alloc_object(HeapObjectData::MutableCell(RefCell::new(val)))
    }

    /// Read the value inside a `MutableCell`.
    ///
    /// Returns `None` if `ptr` does not point to a `MutableCell`.
    pub fn read_mutable_cell(&self, ptr: TaggedValue) -> Option<TaggedValue> {
        if !ptr.is_object() {
            return None;
        }
        match self.get_object(ptr) {
            HeapObjectData::MutableCell(cell) => Some(*cell.borrow()),
            _ => None,
        }
    }

    /// Write a new value into a `MutableCell`.
    ///
    /// Returns `false` if `ptr` does not point to a `MutableCell`.
    pub fn write_mutable_cell(&self, ptr: TaggedValue, val: TaggedValue) -> bool {
        if !ptr.is_object() {
            return false;
        }
        match self.get_object(ptr) {
            HeapObjectData::MutableCell(cell) => {
                *cell.borrow_mut() = val;
                true
            }
            _ => false,
        }
    }

    /// Returns `true` if `ptr` is a `MutableCell`.
    pub fn is_mutable_cell(&self, ptr: TaggedValue) -> bool {
        ptr.is_object() && matches!(self.get_object(ptr), HeapObjectData::MutableCell(_))
    }

    // =========================================================================
    // VM Closure Operations (patina-vm Phase 2)
    // =========================================================================

    /// Allocate a VM closure on the heap.
    ///
    /// `code_id` is a `u32` (the raw `CodeObjectId(u32)` value) to avoid
    /// a direct crate dependency on `patina-vm`.
    pub fn alloc_vm_closure(
        &mut self,
        code_id: u32,
        free_vars: Vec<TaggedValue>,
        globals: Rc<crate::environment::Environment>,
    ) -> TaggedValue {
        self.alloc_object(HeapObjectData::VmClosure {
            code_id,
            free_vars,
            globals,
        })
    }

    /// Retrieve the `(code_id, free_vars)` pair from a VM closure pointer.
    ///
    /// Returns `None` if `val` is not a `VmClosure` object.
    pub fn get_vm_closure(&self, val: TaggedValue) -> Option<(u32, Vec<TaggedValue>)> {
        if !val.is_object() {
            return None;
        }
        match self.get_object(val) {
            HeapObjectData::VmClosure {
                code_id, free_vars, ..
            } => Some((*code_id, free_vars.clone())),
            _ => None,
        }
    }

    /// Get only the code id of a VM closure, without cloning its captured
    /// free variables. This is the call-path accessor — `get_vm_closure`
    /// clones the whole `free_vars` vector, which callers that just need to
    /// dispatch the closure would immediately discard.
    pub fn get_vm_closure_code_id(&self, val: TaggedValue) -> Option<u32> {
        if !val.is_object() {
            return None;
        }
        match self.get_object(val) {
            HeapObjectData::VmClosure { code_id, .. } => Some(*code_id),
            _ => None,
        }
    }

    /// Get the globals environment from a VM closure by heap index.
    pub fn get_vm_closure_globals(
        &self,
        heap_index: crate::tagged_value::HeapIndex,
    ) -> Option<Rc<crate::environment::Environment>> {
        match self.objects.get(heap_index as usize)? {
            HeapObjectData::VmClosure { globals, .. } => Some(globals.clone()),
            _ => None,
        }
    }

    /// Read one free-variable slot from a VM closure.
    ///
    /// Returns `None` if the heap index does not point to a `VmClosure` or
    /// the slot is out of range.
    pub fn get_vm_closure_free_var(
        &self,
        heap_index: crate::tagged_value::HeapIndex,
        slot: usize,
    ) -> Option<TaggedValue> {
        match self.objects.get(heap_index as usize)? {
            HeapObjectData::VmClosure { free_vars, .. } => free_vars.get(slot).copied(),
            _ => None,
        }
    }

    /// Write one free-variable slot in a VM closure (for `set!` on captured vars).
    ///
    /// Returns `false` if the heap index or slot is out of range.
    pub fn set_vm_closure_free_var(
        &mut self,
        heap_index: crate::tagged_value::HeapIndex,
        slot: usize,
        val: TaggedValue,
    ) -> bool {
        match self.objects.get_mut(heap_index as usize) {
            Some(HeapObjectData::VmClosure { free_vars, .. }) if slot < free_vars.len() => {
                free_vars[slot] = val;
                true
            }
            _ => false,
        }
    }

    /// Allocate an opaque handle for a full VM continuation (stored externally
    /// in VmState), minting its id. Ids come from one heap-owned counter
    /// shared by both continuation kinds and every `VmState` on this heap, so
    /// an id names at most one side-table entry ever — the weak-table
    /// machinery (gc.rs §9.5) relies on ref objects never aliasing.
    pub fn alloc_vm_continuation_ref(&mut self) -> (TaggedValue, u64) {
        let id = self.mint_vm_continuation_id();
        (self.alloc_object(HeapObjectData::VmContinuationRef(id)), id)
    }

    /// Get the continuation id from a `VmContinuationRef` TaggedValue.
    pub fn get_vm_continuation_ref(&self, tv: TaggedValue) -> Option<u64> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::VmContinuationRef(id) => Some(*id),
            _ => None,
        }
    }

    /// Allocate an opaque handle for a delimited VM continuation, minting its
    /// id (same counter as `alloc_vm_continuation_ref`).
    pub fn alloc_vm_delimited_continuation_ref(&mut self) -> (TaggedValue, u64) {
        let id = self.mint_vm_continuation_id();
        (
            self.alloc_object(HeapObjectData::VmDelimitedContinuationRef(id)),
            id,
        )
    }

    fn mint_vm_continuation_id(&mut self) -> u64 {
        let id = self.next_vm_continuation_id;
        self.next_vm_continuation_id += 1;
        id
    }

    /// Get the continuation id from a `VmDelimitedContinuationRef` TaggedValue.
    pub fn get_vm_delimited_continuation_ref(&self, tv: TaggedValue) -> Option<u64> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::VmDelimitedContinuationRef(id) => Some(*id),
            _ => None,
        }
    }

    /// Returns true if `tv` is a VM continuation (full or delimited).
    pub fn is_vm_continuation(&self, tv: TaggedValue) -> bool {
        if !tv.is_object() {
            return false;
        }
        matches!(
            self.get_object(tv),
            HeapObjectData::VmContinuationRef(_) | HeapObjectData::VmDelimitedContinuationRef(_)
        )
    }

    /// Allocate a generic object
    fn alloc_object(&mut self, data: HeapObjectData) -> TaggedValue {
        self.note_alloc();
        let index = if let Some(free) = self.free_objects.pop() {
            self.objects[free as usize] = data;
            free
        } else {
            let index = self.objects.len() as HeapIndex;
            self.objects.push(data);
            index
        };
        TaggedValue::object(index)
    }

    /// Get object data reference
    #[inline(always)]
    pub fn get_object(&self, ptr: TaggedValue) -> &HeapObjectData {
        debug_assert!(ptr.is_object());
        let data = &self.objects[ptr.heap_index() as usize];
        debug_assert!(
            !matches!(data, HeapObjectData::Free),
            "use-after-free: object slot {} was reclaimed by the GC",
            ptr.heap_index()
        );
        data
    }

    /// Get object type
    pub fn get_object_type(&self, ptr: TaggedValue) -> HeapObjectType {
        self.get_object(ptr).object_type()
    }

    /// Get identifier data (name and scopes) from a native Identifier object
    #[inline]
    pub fn get_identifier_data(
        &self,
        tv: TaggedValue,
    ) -> Option<(&Rc<str>, &crate::scope::ScopeSet)> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Identifier { name, scopes } => Some((name, scopes)),
            _ => None,
        }
    }

    /// Get identifier data (name and scopes) from a native Identifier object (owned version)
    ///
    /// Returns owned `(Rc<str>, ScopeSet)` for cases where the caller needs
    /// owned data (e.g., when the heap borrow must be released).
    pub fn get_identifier_data_any(
        &self,
        tv: TaggedValue,
    ) -> Option<(Rc<str>, crate::scope::ScopeSet)> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Identifier { name, scopes } => Some((name.clone(), scopes.clone())),
            _ => None,
        }
    }

    // =========================================================================
    // Type Predicates for Object Types
    // =========================================================================
    //
    // These methods check if a TaggedValue is a specific object type.
    // They provide TaggedValue parity with Value's type checking.

    /// Check if value is a symbol
    #[inline]
    pub fn is_symbol(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Symbol(_))
    }

    /// Check if value is a BigInt
    #[inline]
    pub fn is_bigint(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::BigInt(_))
    }

    /// Check if value is a Rational
    #[inline]
    pub fn is_rational(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Rational(_))
    }

    /// Check if value is a Real (boxed f64)
    #[inline]
    pub fn is_real(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Real(_))
    }

    /// Check if value is a Complex number
    #[inline]
    pub fn is_complex(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Complex { .. })
    }

    /// Check if value is a bytevector
    #[inline]
    pub fn is_bytevector(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Bytevector(_))
    }

    /// Check if value is an exception
    #[inline]
    pub fn is_exception(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Exception { .. })
    }

    /// Check if value is a promise
    #[inline]
    pub fn is_promise(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Promise(_))
    }

    /// Check if value is a procedure (native variant, closure, VM closure, or continuation)
    #[inline]
    pub fn is_procedure(&self, tv: TaggedValue) -> bool {
        tv.is_closure()
            || (tv.is_object()
                && matches!(
                    self.get_object(tv),
                    HeapObjectData::Procedure(_)
                        | HeapObjectData::VmClosure { .. }
                        | HeapObjectData::VmContinuationRef(_)
                        | HeapObjectData::VmDelimitedContinuationRef(_)
                ))
    }

    /// Check if value is a macro
    #[inline]
    pub fn is_macro(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Macro(_))
    }

    /// Check if value is a port
    #[inline]
    pub fn is_port(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Port(_))
    }

    /// Check if value is a record
    #[inline]
    pub fn is_record(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Record { .. })
    }

    /// Check if value is a record type
    #[inline]
    pub fn is_record_type(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::RecordType(_))
    }

    /// Check if value is an environment specifier
    #[inline]
    pub fn is_environment(&self, tv: TaggedValue) -> bool {
        tv.is_object()
            && matches!(
                self.get_object(tv),
                HeapObjectData::EnvironmentSpecifier { .. }
            )
    }

    /// Check if value is an identifier
    #[inline]
    pub fn is_identifier(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Identifier { .. })
    }

    /// Get the name from a symbol or identifier
    ///
    /// Returns Some(name) if the value is a Symbol or Identifier, None otherwise.
    /// This is useful for quasiquote and other contexts where both symbol and
    /// identifier names need to be checked.
    #[inline]
    pub fn get_symbol_or_identifier_name(&self, tv: TaggedValue) -> Option<&str> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Symbol(s) => Some(s.as_ref()),
            HeapObjectData::Identifier { name, .. } => Some(name.as_ref()),
            _ => None,
        }
    }

    /// Check if a value is a symbol or identifier with the given name
    #[inline]
    pub fn is_named(&self, tv: TaggedValue, name: &str) -> bool {
        self.get_symbol_or_identifier_name(tv)
            .is_some_and(|n| n == name)
    }

    /// Check if value is a continuation
    #[inline]
    pub fn is_continuation(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Continuation(_))
    }

    /// Check if value is a continuation prompt tag
    #[inline]
    pub fn is_prompt_tag(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::PromptTag(_))
    }

    /// Check if value is a parameter
    #[inline]
    pub fn is_parameter(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Parameter { .. })
    }

    /// Get the Procedure from a TaggedValue, if it is one
    #[inline]
    pub fn get_procedure(
        &self,
        tv: TaggedValue,
    ) -> Option<std::rc::Rc<crate::procedure::Procedure>> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Procedure(p) => Some(p.clone()),
            _ => None,
        }
    }

    /// Get the Port from a TaggedValue, if it is one
    #[inline]
    pub fn get_port(&self, tv: TaggedValue) -> Option<&Rc<crate::port::Port>> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Port(p) => Some(p),
            _ => None,
        }
    }

    /// Get the CompiledMacro from a TaggedValue, if it is one
    #[inline]
    pub fn get_macro(&self, tv: TaggedValue) -> Option<&Rc<crate::compiled_macro::CompiledMacro>> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Macro(m) => Some(m),
            _ => None,
        }
    }

    /// Get the RecordTypeDescriptor from a TaggedValue, if it is one
    #[inline]
    pub fn get_record_type(
        &self,
        tv: TaggedValue,
    ) -> Option<&Rc<crate::record_type::RecordTypeDescriptor>> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::RecordType(rtd) => Some(rtd),
            _ => None,
        }
    }

    /// Get the record type and fields from a TaggedValue, if it is a Record
    #[inline]
    #[allow(clippy::type_complexity)]
    pub fn get_record(
        &self,
        tv: TaggedValue,
    ) -> Option<(
        &Rc<crate::record_type::RecordTypeDescriptor>,
        &Rc<RefCell<Vec<TaggedValue>>>,
    )> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Record {
                record_type,
                fields,
            } => Some((record_type, fields)),
            _ => None,
        }
    }

    /// Get the CpsContinuation from a TaggedValue, if it is one
    #[inline]
    pub fn get_continuation(
        &self,
        tv: TaggedValue,
    ) -> Option<std::rc::Rc<crate::continuation::CpsContinuation>> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Continuation(k) => Some(k.clone()),
            _ => None,
        }
    }

    /// Get the Parameter fields from a TaggedValue, if it is one
    ///
    /// Returns (values_stack, converter) tuple.
    #[inline]
    pub fn get_parameter(&self, tv: TaggedValue) -> Option<ParameterData> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Parameter { values, converter } => Some((values.clone(), *converter)),
            _ => None,
        }
    }

    /// Get the Promise from a TaggedValue, if it is one
    #[inline]
    pub fn get_promise(
        &self,
        tv: TaggedValue,
    ) -> Option<std::rc::Rc<std::cell::RefCell<PromiseState>>> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Promise(p) => Some(p.clone()),
            _ => None,
        }
    }

    /// Check if value is a Values (multiple return values)
    #[inline]
    pub fn is_values(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Values(_))
    }

    /// Get the Values contents as Vec<TaggedValue>, if it is one
    pub fn get_values_as_tagged(&self, tv: TaggedValue) -> Option<Vec<TaggedValue>> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Values(vals) => Some(vals.clone()),
            _ => None,
        }
    }

    /// Get string contents as String, if the value is a string
    ///
    /// Returns None if not a string.
    pub fn get_string_contents(&self, tv: TaggedValue) -> Option<String> {
        if tv.is_string() {
            Some(self.get_string_as_utf8(tv))
        } else {
            None
        }
    }

    /// Get the length of a vector, or None if not a vector
    pub fn try_vector_len(&self, tv: TaggedValue) -> Option<usize> {
        if tv.is_vector() {
            Some(self.vector_len(tv))
        } else {
            None
        }
    }

    /// Get all elements of a vector as Vec<TaggedValue>, or None if not a vector
    pub fn try_vector_to_vec(&self, tv: TaggedValue) -> Option<Vec<TaggedValue>> {
        if tv.is_vector() {
            Some(self.vector_slice(tv).to_vec())
        } else {
            None
        }
    }

    /// Get car and cdr of a pair, or None if not a pair
    pub fn try_pair(&self, tv: TaggedValue) -> Option<(TaggedValue, TaggedValue)> {
        if tv.is_pair() {
            Some((self.car(tv), self.cdr(tv)))
        } else {
            None
        }
    }

    /// Get the type name of a TaggedValue (for error messages)
    pub fn type_name(&self, tv: TaggedValue) -> &'static str {
        // Check immediate types first
        if tv.is_fixnum() {
            return "integer";
        }
        if tv == TaggedValue::TRUE || tv == TaggedValue::FALSE {
            return "boolean";
        }
        if tv == TaggedValue::NULL {
            return "null";
        }
        if tv == TaggedValue::EOF {
            return "eof-object";
        }
        if tv == TaggedValue::UNSPECIFIED {
            return "unspecified";
        }
        if tv.is_char() {
            return "character";
        }

        // Check tagged heap types
        if tv.is_pair() {
            return "pair";
        }
        if tv.is_vector() {
            return "vector";
        }
        if tv.is_string() {
            return "string";
        }
        if tv.is_closure() {
            return "procedure";
        }

        // Check object types
        if tv.is_object() {
            match self.get_object(tv) {
                HeapObjectData::BigInt(_) => "integer",
                HeapObjectData::Rational(_) => "rational",
                HeapObjectData::Real(_) => "real",
                HeapObjectData::Complex { .. } => "complex",
                HeapObjectData::Symbol(_) => "symbol",
                HeapObjectData::Bytevector(_) => "bytevector",
                HeapObjectData::Exception { .. } => "error-object",
                HeapObjectData::Procedure(_) => "procedure",
                HeapObjectData::Port(_) => "port",
                HeapObjectData::Macro(_) => "macro",
                HeapObjectData::RecordType(_) => "record-type",
                HeapObjectData::Record { .. } => "record",
                HeapObjectData::Identifier { .. } => "identifier",
                HeapObjectData::Continuation(_) => "continuation",
                HeapObjectData::Parameter { .. } => "parameter",
                HeapObjectData::Promise(_) => "promise",
                HeapObjectData::Library(_) => "library",
                HeapObjectData::Values(_) => "values",
                HeapObjectData::EnvironmentSpecifier { .. } => "environment",
                HeapObjectData::PromptTag(_) => "continuation-prompt-tag",
                HeapObjectData::LabelPlaceholder(_) => "label-placeholder",
                HeapObjectData::VmClosure { .. } => "procedure",
                HeapObjectData::MutableCell(_) => "mutable-cell",
                HeapObjectData::VmContinuationRef(_) => "continuation",
                HeapObjectData::VmDelimitedContinuationRef(_) => "continuation",
                HeapObjectData::Free => "gc-freed-slot",
            }
        } else {
            "unknown"
        }
    }

    /// Check if value is any kind of number (fixnum, bigint, rational, real, complex)
    #[inline]
    pub fn is_number(&self, tv: TaggedValue) -> bool {
        tv.is_fixnum()
            || (tv.is_object()
                && matches!(
                    self.get_object(tv),
                    HeapObjectData::BigInt(_)
                        | HeapObjectData::Rational(_)
                        | HeapObjectData::Real(_)
                        | HeapObjectData::Complex { .. }
                ))
    }

    /// Check if value is an integer (fixnum or bigint)
    #[inline]
    pub fn is_integer(&self, tv: TaggedValue) -> bool {
        tv.is_fixnum()
            || (tv.is_object() && matches!(self.get_object(tv), HeapObjectData::BigInt(_)))
    }

    /// Check if a number is exact (fixnum, BigInt, or Rational)
    #[inline]
    pub fn is_exact_number(&self, tv: TaggedValue) -> bool {
        if tv.is_fixnum() {
            return true;
        }
        if !tv.is_object() {
            return false;
        }
        matches!(
            self.get_object(tv),
            HeapObjectData::BigInt(_) | HeapObjectData::Rational(_)
        )
    }

    /// Check if a number is inexact (Real, or Complex with any inexact part)
    #[inline]
    pub fn is_inexact_number(&self, tv: TaggedValue) -> bool {
        // Fixnums are never inexact
        if tv.is_fixnum() {
            return false;
        }
        if !tv.is_object() {
            return false;
        }
        match self.get_object(tv) {
            HeapObjectData::Real(_) => true,
            HeapObjectData::Complex { real, imag } => {
                // Complex is inexact if either part is inexact
                self.is_inexact_number(*real) || self.is_inexact_number(*imag)
            }
            _ => false,
        }
    }

    /// Check if a value is exact zero (integer 0, not inexact 0.0)
    /// Used for R7RS real?/rational? predicates on complex numbers
    #[inline]
    pub fn is_exact_zero(&self, tv: TaggedValue) -> bool {
        if tv.is_fixnum() {
            return tv.as_fixnum_unchecked() == 0;
        }
        if !tv.is_object() {
            return false;
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(n) => n.sign() == num_bigint::Sign::NoSign,
            HeapObjectData::Rational(r) => {
                use num_traits::Zero;
                r.is_zero()
            }
            _ => false,
        }
    }

    /// R7RS real? predicate: true for non-complex numbers, or complex with exact zero imaginary
    #[inline]
    pub fn is_real_r7rs(&self, tv: TaggedValue) -> bool {
        if tv.is_fixnum() {
            return true;
        }
        if !tv.is_object() {
            return false;
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(_) | HeapObjectData::Rational(_) | HeapObjectData::Real(_) => {
                true
            }
            HeapObjectData::Complex { imag, .. } => self.is_exact_zero(*imag),
            _ => false,
        }
    }

    /// R7RS rational? predicate: true for exact numbers, finite inexact reals,
    /// and complex with exact zero imaginary and finite real part
    #[inline]
    pub fn is_rational_r7rs(&self, tv: TaggedValue) -> bool {
        if tv.is_fixnum() {
            return true;
        }
        if !tv.is_object() {
            return false;
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(_) | HeapObjectData::Rational(_) => true,
            HeapObjectData::Real(f) => f.is_finite(),
            HeapObjectData::Complex { real, imag } => {
                self.is_exact_zero(*imag) && self.numeric_is_finite(*real).unwrap_or(false)
            }
            _ => false,
        }
    }

    /// R7RS integer? predicate: true for exact integers, inexact integers (e.g., 3.0),
    /// and complex with exact zero imaginary and integer real part
    #[inline]
    pub fn is_integer_r7rs(&self, tv: TaggedValue) -> bool {
        if tv.is_fixnum() {
            return true;
        }
        if !tv.is_object() {
            return false;
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(_) => true,
            HeapObjectData::Rational(r) => {
                use num_bigint::BigInt;
                r.denom() == &BigInt::from(1)
            }
            HeapObjectData::Real(f) => f.is_finite() && f.trunc() == *f,
            HeapObjectData::Complex { real, imag } => {
                self.is_exact_zero(*imag) && self.is_integer_r7rs(*real)
            }
            _ => false,
        }
    }

    // =========================================================================
    // Equality Methods (eq?, eqv?, equal?)
    // =========================================================================

    /// R7RS eq? - pointer/identity equality
    /// Returns true if the two values are the same object
    #[inline]
    pub fn values_eq(&self, a: TaggedValue, b: TaggedValue) -> bool {
        use std::rc::Rc;

        // For immediate values, raw equality is correct
        if a.raw() == b.raw() {
            return true;
        }

        // For heap object types, compare by Rc pointer equality
        if a.is_object() && b.is_object() {
            match (self.get_object(a), self.get_object(b)) {
                (HeapObjectData::Procedure(a), HeapObjectData::Procedure(b)) => Rc::ptr_eq(a, b),
                (HeapObjectData::RecordType(a), HeapObjectData::RecordType(b)) => Rc::ptr_eq(a, b),
                (
                    HeapObjectData::Record {
                        record_type: rt_a,
                        fields: f_a,
                    },
                    HeapObjectData::Record {
                        record_type: rt_b,
                        fields: f_b,
                    },
                ) => Rc::ptr_eq(rt_a, rt_b) && Rc::ptr_eq(f_a, f_b),
                _ => false,
            }
        } else {
            false
        }
    }

    /// R7RS eqv? - value equality for numbers, pointer equality otherwise
    #[inline]
    pub fn values_eqv(&self, a: TaggedValue, b: TaggedValue) -> bool {
        use num_bigint::BigInt;
        use std::rc::Rc;

        // Fast path: identical raw values
        if a.raw() == b.raw() {
            return true;
        }

        // Check if both are fixnums (already handled by raw comparison above)
        // If one is fixnum and other isn't, check for BigInt equivalence
        if a.is_fixnum() {
            if b.is_object()
                && let Some(b_big) = self.get_bigint(b)
            {
                return BigInt::from(a.as_fixnum_unchecked()) == *b_big;
            }
            return false;
        }
        if b.is_fixnum() {
            if a.is_object()
                && let Some(a_big) = self.get_bigint(a)
            {
                return *a_big == BigInt::from(b.as_fixnum_unchecked());
            }
            return false;
        }

        // Both are heap objects - compare by type and value for numbers
        if !a.is_object() || !b.is_object() {
            return false;
        }

        match (self.get_object(a), self.get_object(b)) {
            // BigInt comparison
            (HeapObjectData::BigInt(a_val), HeapObjectData::BigInt(b_val)) => a_val == b_val,
            // Rational comparison
            (HeapObjectData::Rational(a_val), HeapObjectData::Rational(b_val)) => a_val == b_val,
            (HeapObjectData::Real(a_val), HeapObjectData::Real(b_val)) => real_eqv(*a_val, *b_val),
            // Complex comparison - recursively compare real and imaginary parts
            (
                HeapObjectData::Complex {
                    real: a_real,
                    imag: a_imag,
                },
                HeapObjectData::Complex {
                    real: b_real,
                    imag: b_imag,
                },
            ) => self.values_eqv(*a_real, *b_real) && self.values_eqv(*a_imag, *b_imag),
            // Symbol comparison - should have same raw if same symbol (interned)
            // Already checked by raw comparison above
            (HeapObjectData::Symbol(a_sym), HeapObjectData::Symbol(b_sym)) => a_sym == b_sym,
            // Native Identifier comparison (eqv? compares by name only, like symbols)
            (
                HeapObjectData::Identifier { name: a, .. },
                HeapObjectData::Identifier { name: b, .. },
            ) => a.as_ref() == b.as_ref(),
            (HeapObjectData::Symbol(a), HeapObjectData::Identifier { name: b, .. }) => {
                a.as_ref() == b.as_ref()
            }
            (HeapObjectData::Identifier { name: a, .. }, HeapObjectData::Symbol(b)) => {
                a.as_ref() == b.as_ref()
            }
            // Procedure - pointer equality for eqv?
            (HeapObjectData::Procedure(a), HeapObjectData::Procedure(b)) => Rc::ptr_eq(a, b),
            // RecordType - pointer equality for eqv?
            (HeapObjectData::RecordType(a), HeapObjectData::RecordType(b)) => Rc::ptr_eq(a, b),
            // Record - pointer equality for eqv?
            (
                HeapObjectData::Record {
                    record_type: rt_a,
                    fields: f_a,
                },
                HeapObjectData::Record {
                    record_type: rt_b,
                    fields: f_b,
                },
            ) => Rc::ptr_eq(rt_a, rt_b) && Rc::ptr_eq(f_a, f_b),
            _ => false,
        }
    }

    /// R7RS equal? - deep structural equality
    /// Compares pairs, vectors, strings, and bytevectors recursively
    pub fn values_equal(&self, a: TaggedValue, b: TaggedValue) -> bool {
        use num_bigint::BigInt;
        use std::rc::Rc;

        // Fast path: identical raw values
        if a.raw() == b.raw() {
            return true;
        }

        // Check immediate values
        if a.is_fixnum() && b.is_fixnum() {
            return false; // Already checked by raw comparison
        }

        // Handle fixnum vs BigInt
        if a.is_fixnum() {
            if b.is_object()
                && let Some(b_big) = self.get_bigint(b)
            {
                return BigInt::from(a.as_fixnum_unchecked()) == *b_big;
            }
            return false;
        }
        if b.is_fixnum() {
            if a.is_object()
                && let Some(a_big) = self.get_bigint(a)
            {
                return *a_big == BigInt::from(b.as_fixnum_unchecked());
            }
            return false;
        }

        // Both are heap objects
        if !a.is_object() || !b.is_object() {
            return false;
        }

        match (self.get_object(a), self.get_object(b)) {
            // Numeric types - use eqv? semantics
            (HeapObjectData::BigInt(a_val), HeapObjectData::BigInt(b_val)) => a_val == b_val,
            (HeapObjectData::Rational(a_val), HeapObjectData::Rational(b_val)) => a_val == b_val,
            (HeapObjectData::Real(a_val), HeapObjectData::Real(b_val)) => real_eqv(*a_val, *b_val),
            (
                HeapObjectData::Complex {
                    real: a_real,
                    imag: a_imag,
                },
                HeapObjectData::Complex {
                    real: b_real,
                    imag: b_imag,
                },
            ) => self.values_equal(*a_real, *b_real) && self.values_equal(*a_imag, *b_imag),

            // Symbol comparison
            (HeapObjectData::Symbol(a_sym), HeapObjectData::Symbol(b_sym)) => a_sym == b_sym,

            // Native Bytevector comparison
            (HeapObjectData::Bytevector(a_bytes), HeapObjectData::Bytevector(b_bytes)) => {
                a_bytes == b_bytes
            }
            // Native Procedure - pointer equality for equal?
            (HeapObjectData::Procedure(a), HeapObjectData::Procedure(b)) => Rc::ptr_eq(a, b),
            // Native RecordType - pointer equality for equal?
            (HeapObjectData::RecordType(a), HeapObjectData::RecordType(b)) => Rc::ptr_eq(a, b),
            // Native Record - deep structural comparison for equal?
            (
                HeapObjectData::Record {
                    record_type: rt_a,
                    fields: f_a,
                },
                HeapObjectData::Record {
                    record_type: rt_b,
                    fields: f_b,
                },
            ) => {
                if !Rc::ptr_eq(rt_a, rt_b) {
                    return false;
                }
                let fa = f_a.borrow();
                let fb = f_b.borrow();
                if fa.len() != fb.len() {
                    return false;
                }
                for (av, bv) in fa.iter().zip(fb.iter()) {
                    if !self.tagged_values_equal(*av, *bv) {
                        return false;
                    }
                }
                true
            }
            // Native pairs - deep comparison
            // Note: We can't easily compare native pairs here because we only have
            // HeapObjectData, not the pair storage. Native pairs use TAG_PAIR which
            // doesn't go through get_object(). Handle this case specially.
            _ => false,
        }
    }

    /// Deep structural comparison for native pairs
    pub fn pairs_equal(&self, a: TaggedValue, b: TaggedValue) -> bool {
        if !a.is_pair() || !b.is_pair() {
            return false;
        }
        let a_car = self.car(a);
        let a_cdr = self.cdr(a);
        let b_car = self.car(b);
        let b_cdr = self.cdr(b);
        self.tagged_values_equal(a_car, b_car) && self.tagged_values_equal(a_cdr, b_cdr)
    }

    /// Deep structural comparison for native vectors
    pub fn vectors_equal(&self, a: TaggedValue, b: TaggedValue) -> bool {
        if !a.is_vector() || !b.is_vector() {
            return false;
        }
        let a_slice = self.vector_slice(a);
        let b_slice = self.vector_slice(b);
        if a_slice.len() != b_slice.len() {
            return false;
        }
        for (av, bv) in a_slice.iter().zip(b_slice.iter()) {
            if !self.tagged_values_equal(*av, *bv) {
                return false;
            }
        }
        true
    }

    /// Full equal? implementation that handles all value types
    pub fn tagged_values_equal(&self, a: TaggedValue, b: TaggedValue) -> bool {
        // Fast path: identical raw values
        if a.raw() == b.raw() {
            return true;
        }

        // Check for null
        if a.is_null() && b.is_null() {
            return true;
        }
        if a.is_null() || b.is_null() {
            return false;
        }

        // Check for pairs (native heap pairs)
        if a.is_pair() && b.is_pair() {
            return self.pairs_equal(a, b);
        }

        // Check for vectors (native heap vectors)
        if a.is_vector() && b.is_vector() {
            return self.vectors_equal(a, b);
        }

        // Check for native bytevectors
        if let (Some(a_bytes), Some(b_bytes)) =
            (self.get_bytevector_bytes(a), self.get_bytevector_bytes(b))
        {
            return a_bytes == b_bytes;
        }

        // Check for native strings
        if a.is_string() && b.is_string() {
            return self.get_string_chars(a) == self.get_string_chars(b);
        }

        // Check for characters
        if a.is_char() && b.is_char() {
            return false; // If equal, raw would match
        }

        // Check for booleans
        if a.is_boolean() && b.is_boolean() {
            return false; // If equal, raw would match
        }

        // Check for fixnums
        if a.is_fixnum() && b.is_fixnum() {
            return false; // If equal, raw would match
        }

        // Use values_equal for other heap objects
        self.values_equal(a, b)
    }

    /// Compute a hash for a TaggedValue consistent with `equal?`.
    ///
    /// Objects that are `equal?` produce the same hash. Uses a depth limit
    /// to avoid excessive recursion on deep structures.
    pub fn tagged_value_hash(&self, tv: TaggedValue) -> u64 {
        self.tagged_value_hash_depth(tv, 8)
    }

    fn tagged_value_hash_depth(&self, tv: TaggedValue, depth: u32) -> u64 {
        // Null
        if tv.is_null() {
            return 0x517cc1b727220a95;
        }
        // Boolean
        if tv.is_boolean() {
            return if tv.is_truthy() {
                0x9e3779b97f4a7c15
            } else {
                0x6c62272e07bb0142
            };
        }
        // Fixnum
        if tv.is_fixnum() {
            let mut h = tv.as_fixnum_unchecked() as u64;
            h = h.wrapping_mul(0x517cc1b727220a95);
            h ^= h >> 32;
            return h;
        }
        // Char
        if tv.is_char() {
            let c = tv.as_char_unchecked() as u64;
            return c.wrapping_mul(0x9e3779b97f4a7c15);
        }
        // String (native)
        if tv.is_string() {
            let chars = self.get_string_chars(tv);
            let mut h: u64 = 0xcbf29ce484222325; // FNV offset basis
            for &c in chars {
                h ^= c as u64;
                h = h.wrapping_mul(0x100000001b3); // FNV prime
            }
            return h;
        }
        // Pair
        if tv.is_pair() {
            if depth == 0 {
                return 0x01000193;
            }
            let car = self.car(tv);
            let cdr = self.cdr(tv);
            let h1 = self.tagged_value_hash_depth(car, depth - 1);
            let h2 = self.tagged_value_hash_depth(cdr, depth - 1);
            return h1.wrapping_mul(31).wrapping_add(h2);
        }
        // Vector
        if tv.is_vector() {
            if depth == 0 {
                return 0x811c9dc5;
            }
            let len = self.vector_len(tv);
            let mut h: u64 = len as u64;
            let limit = len.min(8); // Hash at most 8 elements
            for i in 0..limit {
                let elem = self.vector_ref(tv, i);
                h = h
                    .wrapping_mul(31)
                    .wrapping_add(self.tagged_value_hash_depth(elem, depth - 1));
            }
            return h;
        }
        // Heap objects
        if tv.is_object() {
            match self.get_object(tv) {
                HeapObjectData::BigInt(n) => {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    n.hash(&mut hasher);
                    return hasher.finish();
                }
                HeapObjectData::Rational(r) => {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    r.numer().hash(&mut hasher);
                    r.denom().hash(&mut hasher);
                    return hasher.finish();
                }
                HeapObjectData::Real(f) => {
                    if f.is_nan() {
                        return 0x7ff8000000000000;
                    }
                    return f.to_bits();
                }
                HeapObjectData::Complex { real, imag } => {
                    let h1 = self.tagged_value_hash_depth(*real, depth);
                    let h2 = self.tagged_value_hash_depth(*imag, depth);
                    return h1.wrapping_mul(31).wrapping_add(h2);
                }
                HeapObjectData::Symbol(s) => {
                    let mut h: u64 = 0xcbf29ce484222325;
                    for c in s.chars() {
                        h ^= c as u64;
                        h = h.wrapping_mul(0x100000001b3);
                    }
                    return h;
                }
                HeapObjectData::Bytevector(bytes) => {
                    let mut h: u64 = 0xcbf29ce484222325;
                    for &b in bytes.iter() {
                        h ^= b as u64;
                        h = h.wrapping_mul(0x100000001b3);
                    }
                    return h;
                }
                _ => {
                    return tv.heap_index() as u64;
                }
            }
        }
        // Unspecified, eof, etc.
        0
    }

    /// Check if a numeric value is finite
    /// Returns None if not a number, Some(true) if finite, Some(false) if infinite/NaN
    #[inline]
    pub fn numeric_is_finite(&self, tv: TaggedValue) -> Option<bool> {
        // Fixnums are always finite
        if tv.is_fixnum() {
            return Some(true);
        }
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(_) | HeapObjectData::Rational(_) => Some(true),
            HeapObjectData::Real(f) => Some(f.is_finite()),
            HeapObjectData::Complex { real, imag } => {
                // Complex is finite if both parts are finite
                let real_finite = self.numeric_is_finite(*real).unwrap_or(false);
                let imag_finite = self.numeric_is_finite(*imag).unwrap_or(false);
                Some(real_finite && imag_finite)
            }
            _ => None,
        }
    }

    /// Check if a numeric value is infinite
    /// Returns None if not a number, Some(true) if infinite, Some(false) otherwise
    #[inline]
    pub fn numeric_is_infinite(&self, tv: TaggedValue) -> Option<bool> {
        // Fixnums are never infinite
        if tv.is_fixnum() {
            return Some(false);
        }
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(_) | HeapObjectData::Rational(_) => Some(false),
            HeapObjectData::Real(f) => Some(f.is_infinite()),
            HeapObjectData::Complex { real, imag } => {
                // Complex is infinite if either part is infinite
                let real_inf = self.numeric_is_infinite(*real).unwrap_or(false);
                let imag_inf = self.numeric_is_infinite(*imag).unwrap_or(false);
                Some(real_inf || imag_inf)
            }
            _ => None,
        }
    }

    /// Check if a numeric value is NaN
    /// Returns None if not a number, Some(true) if NaN, Some(false) otherwise
    #[inline]
    pub fn numeric_is_nan(&self, tv: TaggedValue) -> Option<bool> {
        // Fixnums are never NaN
        if tv.is_fixnum() {
            return Some(false);
        }
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(_) | HeapObjectData::Rational(_) => Some(false),
            HeapObjectData::Real(f) => Some(f.is_nan()),
            HeapObjectData::Complex { real, imag } => {
                // Complex is NaN if either part is NaN
                let real_nan = self.numeric_is_nan(*real).unwrap_or(false);
                let imag_nan = self.numeric_is_nan(*imag).unwrap_or(false);
                Some(real_nan || imag_nan)
            }
            _ => None,
        }
    }

    // =========================================================================
    // Data Extraction Methods
    // =========================================================================
    //
    // These methods extract the underlying data from TaggedValues.

    /// Get symbol name (if value is a symbol)
    #[inline]
    pub fn get_symbol_name(&self, tv: TaggedValue) -> Option<&str> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Symbol(s) => Some(s.as_ref()),
            _ => None,
        }
    }

    /// Get BigInt value (if value is a bigint)
    #[inline]
    pub fn get_bigint(&self, tv: TaggedValue) -> Option<&BigInt> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::BigInt(n) => Some(n),
            _ => None,
        }
    }

    /// Get Rational value (if value is a rational)
    #[inline]
    pub fn get_rational(&self, tv: TaggedValue) -> Option<&BigRational> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Rational(r) => Some(r),
            _ => None,
        }
    }

    /// Get Real value (if value is a boxed f64)
    #[inline]
    pub fn get_real(&self, tv: TaggedValue) -> Option<f64> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Real(f) => Some(*f),
            _ => None,
        }
    }

    /// Get Complex parts (if value is complex)
    #[inline]
    pub fn get_complex(&self, tv: TaggedValue) -> Option<(TaggedValue, TaggedValue)> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Complex { real, imag } => Some((*real, *imag)),
            _ => None,
        }
    }

    /// Get bytevector bytes (if value is a native bytevector)
    #[inline]
    pub fn get_bytevector(&self, tv: TaggedValue) -> Option<&[u8]> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Bytevector(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Check if value is a bytevector
    #[inline]
    pub fn is_any_bytevector(&self, tv: TaggedValue) -> bool {
        tv.is_object() && matches!(self.get_object(tv), HeapObjectData::Bytevector(_))
    }

    /// Get bytevector length, None if not a bytevector
    #[inline]
    pub fn bytevector_len(&self, tv: TaggedValue) -> Option<usize> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Bytevector(bytes) => Some(bytes.len()),
            _ => None,
        }
    }

    /// Get byte at index, None if not bytevector or out of bounds
    #[inline]
    pub fn bytevector_u8_ref(&self, tv: TaggedValue, index: usize) -> Option<u8> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Bytevector(bytes) => bytes.get(index).copied(),
            _ => None,
        }
    }

    /// Get bytevector bytes as cloned Vec<u8>
    pub fn get_bytevector_bytes(&self, tv: TaggedValue) -> Option<Vec<u8>> {
        if !tv.is_object() {
            return None;
        }
        match self.get_object(tv) {
            HeapObjectData::Bytevector(bytes) => Some(bytes.clone()),
            _ => None,
        }
    }

    /// Set byte at index for a bytevector.
    /// Returns true if successful, false if not a bytevector.
    /// Panics if index is out of bounds (caller should validate).
    pub fn bytevector_u8_set(&mut self, tv: TaggedValue, index: usize, byte: u8) -> bool {
        if !tv.is_object() {
            return false;
        }
        let obj = &mut self.objects[tv.heap_index() as usize];
        match obj {
            HeapObjectData::Bytevector(bytes) => {
                bytes[index] = byte;
                true
            }
            _ => false,
        }
    }

    /// Get mutable reference to native heap bytevector bytes.
    pub fn get_bytevector_mut(&mut self, tv: TaggedValue) -> Option<&mut Vec<u8>> {
        if !tv.is_object() {
            return None;
        }
        let obj = &mut self.objects[tv.heap_index() as usize];
        match obj {
            HeapObjectData::Bytevector(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Copy bytes into a bytevector at a given offset.
    /// Returns true if successful, false if not a bytevector.
    pub fn bytevector_copy_into(&mut self, tv: TaggedValue, at: usize, src: &[u8]) -> bool {
        if !tv.is_object() {
            return false;
        }
        let obj = &mut self.objects[tv.heap_index() as usize];
        match obj {
            HeapObjectData::Bytevector(bytes) => {
                bytes[at..at + src.len()].copy_from_slice(src);
                true
            }
            _ => false,
        }
    }

    // =========================================================================
    // List Operations (convenience methods)
    // =========================================================================

    /// Build a proper list from an iterator of values
    pub fn list_from_iter<I>(&mut self, iter: I) -> TaggedValue
    where
        I: IntoIterator<Item = TaggedValue>,
        I::IntoIter: DoubleEndedIterator,
    {
        self.list_from_iter_with_tail(iter, TaggedValue::NULL)
    }

    /// Build a (possibly improper) list ending in `tail` from the iterator's
    /// elements. `(a b . c)` is `list_from_iter_with_tail([a, b], c)`.
    ///
    /// Conses back to front — hence the `DoubleEndedIterator` bound — with
    /// no staging: the partial list needs no GC root (see `alloc_pair`). A
    /// caller with a forward-only iterator should restructure toward a
    /// reversible source rather than collect into a `Vec` here; the staging
    /// is exactly what this method avoids.
    pub fn list_from_iter_with_tail<I>(&mut self, iter: I, tail: TaggedValue) -> TaggedValue
    where
        I: IntoIterator<Item = TaggedValue>,
        I::IntoIter: DoubleEndedIterator,
    {
        let mut result = tail;
        for item in iter.into_iter().rev() {
            result = self.alloc_pair(item, result);
        }
        result
    }

    /// Check if value is a proper list (ends with null)
    pub fn is_list(&self, tv: TaggedValue) -> bool {
        if tv.is_null() {
            return true;
        }
        if !tv.is_pair() {
            return false;
        }
        let mut current = tv;
        let mut seen = std::collections::HashSet::new();

        loop {
            if current.is_null() {
                return true;
            }
            if !current.is_pair() {
                return false;
            }
            // Cycle detection
            if !seen.insert(current.raw()) {
                return false;
            }
            current = self.cdr(current);
        }
    }

    /// Get list length (None if not a proper list)
    pub fn list_len(&self, tv: TaggedValue) -> Option<usize> {
        if tv.is_null() {
            return Some(0);
        }
        if !tv.is_pair() {
            return None;
        }
        let mut current = tv;
        let mut len = 0;
        let mut seen = std::collections::HashSet::new();

        loop {
            if current.is_null() {
                return Some(len);
            }
            if !current.is_pair() {
                return None;
            }
            // Cycle detection
            if !seen.insert(current.raw()) {
                return None;
            }
            len += 1;
            current = self.cdr(current);
        }
    }

    /// Get nth element of a list (0-indexed)
    /// Returns None if index is out of bounds or not a proper list
    pub fn list_ref(&self, tv: TaggedValue, index: usize) -> Option<TaggedValue> {
        let mut current = tv;
        let mut i = 0;

        loop {
            if current.is_null() {
                return None; // Index out of bounds
            }
            if !current.is_pair() {
                return None; // Not a pair
            }
            if i == index {
                return Some(self.car(current));
            }
            current = self.cdr(current);
            i += 1;
        }
    }

    /// Get tail of list after dropping n elements
    /// Returns None if n > list length or not a proper list
    pub fn list_tail(&self, tv: TaggedValue, n: usize) -> Option<TaggedValue> {
        let mut current = tv;

        for _ in 0..n {
            if current.is_null() {
                return None; // n > list length
            }
            if !current.is_pair() {
                return None; // Not a pair
            }
            current = self.cdr(current);
        }

        Some(current)
    }

    /// Convert list to Vec of TaggedValues
    /// Returns None if not a proper list
    pub fn list_to_vec(&self, tv: TaggedValue) -> Option<Vec<TaggedValue>> {
        let mut result = Vec::new();
        let mut current = tv;

        loop {
            if current.is_null() {
                return Some(result);
            }
            if !current.is_pair() {
                return None; // Not a proper list
            }
            result.push(self.car(current));
            current = self.cdr(current);
        }
    }

    /// Reverse a list
    pub fn list_reverse(&mut self, tv: TaggedValue) -> Option<TaggedValue> {
        let mut result = TaggedValue::NULL;
        let mut current = tv;

        loop {
            if current.is_null() {
                return Some(result);
            }

            if current.is_pair() {
                let car = self.car(current);
                let cdr = self.cdr(current);
                result = self.alloc_pair(car, result);
                current = cdr;
                continue;
            }

            return None; // Not a proper list
        }
    }

    /// Append two lists (creates new pairs for first list)
    pub fn list_append(&mut self, first: TaggedValue, second: TaggedValue) -> Option<TaggedValue> {
        // If first is null, return second
        if first.is_null() {
            return Some(second);
        }

        // Collect elements from first list
        let mut elements = Vec::new();
        let mut current = first;

        loop {
            if current.is_null() {
                break;
            }

            if current.is_pair() {
                elements.push(self.car(current));
                current = self.cdr(current);
                continue;
            }

            return None; // Not a proper list
        }

        Some(self.list_from_iter_with_tail(elements, second))
    }

    // =========================================================================
    // Statistics
    // =========================================================================

    /// Get heap statistics
    pub fn stats(&self) -> HeapStats {
        HeapStats {
            pairs: self.pairs.len(),
            vectors: self.vectors.len(),
            strings: self.strings.len(),
            objects: self.objects.len(),
            symbols: self.symbol_table.len(),
            free_pairs: self.free_pairs.len(),
            free_vectors: self.free_vectors.len(),
            free_strings: self.free_strings.len(),
            free_objects: self.free_objects.len(),
            allocs_since_gc: self.allocs_since_gc,
            gc_collections: self.gc_collections,
            gc_last_swept: self.gc_last_swept,
        }
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

/// Heap statistics
#[derive(Debug, Clone)]
pub struct HeapStats {
    pub pairs: usize,
    pub vectors: usize,
    pub strings: usize,
    pub objects: usize,
    pub symbols: usize,
    pub free_pairs: usize,
    pub free_vectors: usize,
    pub free_strings: usize,
    pub free_objects: usize,
    pub allocs_since_gc: usize,
    pub gc_collections: u64,
    pub gc_last_swept: usize,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heap_new() {
        let heap = Heap::new();
        let stats = heap.stats();
        assert_eq!(stats.pairs, 0);
        assert_eq!(stats.vectors, 0);
        assert_eq!(stats.strings, 0);
    }

    // -------------------------------------------------------------------------
    // Pair tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_pair_alloc() {
        let mut heap = Heap::new();
        let car = TaggedValue::fixnum(1);
        let cdr = TaggedValue::fixnum(2);
        let pair = heap.alloc_pair(car, cdr);

        assert!(pair.is_pair());
        assert_eq!(heap.car(pair), car);
        assert_eq!(heap.cdr(pair), cdr);
    }

    #[test]
    fn test_pair_set() {
        let mut heap = Heap::new();
        let pair = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::fixnum(2));

        heap.set_car(pair, TaggedValue::fixnum(10));
        heap.set_cdr(pair, TaggedValue::fixnum(20));

        assert_eq!(heap.car(pair).as_fixnum_unchecked(), 10);
        assert_eq!(heap.cdr(pair).as_fixnum_unchecked(), 20);
    }

    #[test]
    fn test_list_construction() {
        let mut heap = Heap::new();

        // Build (1 2 3)
        let three = heap.alloc_pair(TaggedValue::fixnum(3), TaggedValue::NULL);
        let two = heap.alloc_pair(TaggedValue::fixnum(2), three);
        let one = heap.alloc_pair(TaggedValue::fixnum(1), two);

        assert!(heap.is_list(one));
        assert_eq!(heap.list_len(one), Some(3));

        assert_eq!(heap.car(one).as_fixnum_unchecked(), 1);
        assert_eq!(heap.car(heap.cdr(one)).as_fixnum_unchecked(), 2);
        assert_eq!(heap.car(heap.cdr(heap.cdr(one))).as_fixnum_unchecked(), 3);
        assert!(heap.cdr(heap.cdr(heap.cdr(one))).is_null());
    }

    #[test]
    fn test_list_from_iter() {
        let mut heap = Heap::new();
        let list = heap.list_from_iter([
            TaggedValue::fixnum(1),
            TaggedValue::fixnum(2),
            TaggedValue::fixnum(3),
        ]);

        assert!(heap.is_list(list));
        assert_eq!(heap.list_len(list), Some(3));
        assert_eq!(heap.car(list).as_fixnum_unchecked(), 1);
    }

    #[test]
    fn test_improper_list() {
        let mut heap = Heap::new();
        // Build (1 . 2) - improper pair
        let pair = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::fixnum(2));

        assert!(!heap.is_list(pair));
        assert_eq!(heap.list_len(pair), None);
    }

    // -------------------------------------------------------------------------
    // Vector tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_vector_alloc() {
        let mut heap = Heap::new();
        let vec = heap.alloc_vector(vec![
            TaggedValue::fixnum(1),
            TaggedValue::fixnum(2),
            TaggedValue::fixnum(3),
        ]);

        assert!(vec.is_vector());
        assert_eq!(heap.vector_len(vec), 3);
        assert_eq!(heap.vector_ref(vec, 0).as_fixnum_unchecked(), 1);
        assert_eq!(heap.vector_ref(vec, 1).as_fixnum_unchecked(), 2);
        assert_eq!(heap.vector_ref(vec, 2).as_fixnum_unchecked(), 3);
    }

    #[test]
    fn test_vector_set() {
        let mut heap = Heap::new();
        let vec = heap.alloc_vector(vec![TaggedValue::fixnum(1), TaggedValue::fixnum(2)]);

        heap.vector_set(vec, 0, TaggedValue::fixnum(10));
        assert_eq!(heap.vector_ref(vec, 0).as_fixnum_unchecked(), 10);
    }

    // -------------------------------------------------------------------------
    // String tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_string_alloc() {
        let mut heap = Heap::new();
        let s = heap.alloc_string("hello".to_string());

        assert!(s.is_string());
        assert_eq!(heap.get_string_chars(s), &['h', 'e', 'l', 'l', 'o']);
        assert_eq!(heap.string_char_len(s), 5);
    }

    // -------------------------------------------------------------------------
    // Symbol tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_symbol_interning() {
        let mut heap = Heap::new();

        let s1 = heap.intern_symbol("foo");
        let s2 = heap.intern_symbol("foo");
        let s3 = heap.intern_symbol("bar");

        // Same symbol should return same object
        assert_eq!(s1.raw(), s2.raw());
        // Different symbols should be different
        assert_ne!(s1.raw(), s3.raw());
    }

    // -------------------------------------------------------------------------
    // Object tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_bigint_alloc() {
        let mut heap = Heap::new();
        let n = heap.alloc_bigint(BigInt::from(12345));

        assert!(n.is_object());
        assert!(heap.is_bigint(n));
        assert!(!heap.is_rational(n));
    }

    #[test]
    fn test_rational_alloc() {
        use num_rational::Ratio;
        let mut heap = Heap::new();
        let r = heap.alloc_rational(Ratio::new(BigInt::from(1), BigInt::from(2)));

        assert!(r.is_object());
        assert!(heap.is_rational(r));
        assert!(!heap.is_bigint(r));
    }

    // -------------------------------------------------------------------------
    // Numeric arithmetic tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_fixnum_add() {
        let mut heap = Heap::new();
        let a = TaggedValue::fixnum(10);
        let b = TaggedValue::fixnum(20);

        let result = heap.numeric_add(a, b).unwrap();
        assert!(result.is_fixnum());
        assert_eq!(result.as_fixnum_unchecked(), 30);
    }

    #[test]
    fn test_fixnum_add_overflow() {
        let mut heap = Heap::new();
        // Use FIXNUM_MAX to ensure we're in range and will overflow when added
        let a = TaggedValue::fixnum(TaggedValue::FIXNUM_MAX);
        let b = TaggedValue::fixnum(100);

        let result = heap.numeric_add(a, b).unwrap();
        // Should overflow to BigInt
        assert!(result.is_object());
        assert!(heap.is_bigint(result));
    }

    #[test]
    fn test_fixnum_sub() {
        let mut heap = Heap::new();
        let a = TaggedValue::fixnum(30);
        let b = TaggedValue::fixnum(20);

        let result = heap.numeric_sub(a, b).unwrap();
        assert!(result.is_fixnum());
        assert_eq!(result.as_fixnum_unchecked(), 10);
    }

    #[test]
    fn test_fixnum_mul() {
        let mut heap = Heap::new();
        let a = TaggedValue::fixnum(6);
        let b = TaggedValue::fixnum(7);

        let result = heap.numeric_mul(a, b).unwrap();
        assert!(result.is_fixnum());
        assert_eq!(result.as_fixnum_unchecked(), 42);
    }

    #[test]
    fn test_fixnum_quotient() {
        let mut heap = Heap::new();
        let a = TaggedValue::fixnum(13);
        let b = TaggedValue::fixnum(4);

        let result = heap.numeric_quotient(a, b).unwrap();
        assert!(result.is_fixnum());
        assert_eq!(result.as_fixnum_unchecked(), 3);
    }

    #[test]
    fn test_fixnum_remainder() {
        let mut heap = Heap::new();
        let a = TaggedValue::fixnum(13);
        let b = TaggedValue::fixnum(4);

        let result = heap.numeric_remainder(a, b).unwrap();
        assert!(result.is_fixnum());
        assert_eq!(result.as_fixnum_unchecked(), 1);
    }

    #[test]
    fn test_fixnum_modulo() {
        let mut heap = Heap::new();
        let a = TaggedValue::fixnum(-13);
        let b = TaggedValue::fixnum(4);

        let result = heap.numeric_modulo(a, b).unwrap();
        assert!(result.is_fixnum());
        assert_eq!(result.as_fixnum_unchecked(), 3);
    }
}
