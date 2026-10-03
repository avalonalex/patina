//! The byte account (#606, `PRD/GC_PRD.md` §15): what each object costs the
//! collection trigger, and the totals `(gc-stats)` reports.
//!
//! The trigger used to count objects, so a 100,000-element vector cost it
//! what a pair costs, and a VM continuation's snapshot of a thousand frames
//! cost it one slot. Now every `alloc_*` charges **bytes**: the object's slot
//! in its arena plus its *payload*, the memory it owns outright outside the
//! slot — a vector's elements, a string's characters, a bytevector's bytes, a
//! bignum's limbs, a record's fields, a closure's free variables, a VM
//! continuation's register and frame snapshot. One function per arena
//! measures a payload, and allocation, marking and sweep all call it, so what
//! an object was charged at birth is what marking counts while it lives and
//! what sweep credits when it dies.
//!
//! **Payloads do not grow after allocation**, so no growth is charged. Vectors,
//! strings and bytevectors hand out slices only, never their `Vec`
//! (`vector_slice_mut`, `get_string_chars_mut`, `get_bytevector_mut`), so
//! their capacity is fixed by the type system; a record's field count and a
//! closure's free variables are fixed at construction, and the heap writes
//! them in place; the other payloads charged — a bignum's limbs, an
//! exception's message and irritants, a `values` object, a symbol's name, an
//! identifier's scopes, a continuation's snapshot — are never written after
//! allocation. A parameter's value stack, which `parameterize` grows and
//! shrinks, is not charged: it is as deep as the `parameterize` forms active,
//! which their frames already pay for.
//!
//! **Shared `Rc` payloads are not charged** — a procedure's body and
//! environment, a macro, a library, an environment specifier, a port, a
//! prompt tag, a promise's state (which `promise_update` shares between two
//! promises), a tree-walker continuation (whose frames are `Rc` links shared
//! with every other capture). Attributing them to one slot would charge the
//! same memory once per object that names it, and what they hold belongs to
//! code and environments, which GC_PRD §15 charges as **external bytes**,
//! once per holder rather than once per object that names it. A namespace —
//! an environment made without a parent: the global environment, a
//! library's, and those `environment` and the R5RS constructors build —
//! charges its own tables, once when it is made and again as they grow, and
//! gives them back when it drops (#615; `Environment`'s `NamespaceCharge`
//! says what it counts). [`Heap::charge_external_bytes`] is the entry point
//! for a holder with the heap in hand, and an [`ExternalBytes`] handle for
//! one without: the handle needs no heap borrow, because such a holder
//! usually dies when the last `Rc` naming it drops, and that is often inside
//! a sweep, which holds the heap's `RefCell` mutably while it drops the
//! tombstoned slots' payloads, and a namespace's tables grow wherever a
//! definition lands. The sweep settles L after the arenas are swept, so what
//! those drops give back is out of the L it sets.
//!
//! The policy that reads the account lives in the collector
//! (`MarkSweepCollector::auto_threshold` in `gc.rs`): the next collection
//! after `max(8 MiB, 2·L)` bytes, L being the bytes the last collection
//! found live. `PATINA_GC_STRESS` still counts allocations, not bytes
//! ([`GcThreshold`]).

use std::cell::Cell;
use std::mem::size_of;
use std::rc::Rc;

use num_bigint::BigInt;

use super::{Heap, HeapObjectData};
use crate::tagged_value::TaggedValue;

/// Bytes of one pair slot: the whole of a pair.
pub const PAIR_SLOT_BYTES: usize = size_of::<(TaggedValue, TaggedValue)>();
/// Bytes of one vector slot, before the elements it owns.
pub const VECTOR_SLOT_BYTES: usize = size_of::<Vec<TaggedValue>>();
/// Bytes of one string slot, before the characters it owns.
pub const STRING_SLOT_BYTES: usize = size_of::<Vec<char>>();
/// Bytes of one object slot, before its payload.
pub const OBJECT_SLOT_BYTES: usize = size_of::<HeapObjectData>();

const VALUE_BYTES: usize = size_of::<TaggedValue>();

/// A vector's payload: its element buffer, at capacity.
#[inline]
pub(crate) fn vector_payload(elements: &Vec<TaggedValue>) -> usize {
    elements.capacity() * VALUE_BYTES
}

/// A string's payload: its character buffer, at capacity (4 bytes a
/// character: strings are `Vec<char>` for O(1) indexing).
#[inline]
pub(crate) fn string_payload(chars: &Vec<char>) -> usize {
    chars.capacity() * size_of::<char>()
}

/// A bignum's limbs, from its bit length: what `num-bigint` stores, in
/// 64-bit digits, less any spare capacity it keeps, which it does not expose.
fn bigint_payload(n: &BigInt) -> usize {
    (n.bits().div_ceil(64) as usize) * size_of::<u64>()
}

impl HeapObjectData {
    /// The bytes this object owns outside its slot (the module comment says
    /// what counts). Every variant is matched, so a new one does not compile
    /// until it says what it owns.
    #[inline]
    pub fn payload_bytes(&self) -> usize {
        match self {
            HeapObjectData::BigInt(n) => bigint_payload(n),
            HeapObjectData::Rational(r) => bigint_payload(r.numer()) + bigint_payload(r.denom()),
            HeapObjectData::Symbol(name) => name.len(),
            HeapObjectData::Bytevector(bytes) => bytes.capacity(),
            HeapObjectData::Exception {
                kind,
                message,
                irritants,
            } => {
                let custom = match kind {
                    crate::error::ExceptionKind::Custom(name) => name.capacity(),
                    crate::error::ExceptionKind::Error
                    | crate::error::ExceptionKind::FileError
                    | crate::error::ExceptionKind::ReadError => 0,
                };
                custom + message.capacity() + irritants.capacity() * VALUE_BYTES
            }
            HeapObjectData::Record {
                record_type: _,
                fields,
            } => fields.borrow().capacity() * VALUE_BYTES,
            HeapObjectData::Identifier {
                name: _,
                scopes,
                written: _,
            } => scopes.heap_bytes(),
            HeapObjectData::Values(values) => values.capacity() * VALUE_BYTES,
            HeapObjectData::VmClosure {
                code_id: _,
                free_vars,
                globals: _,
            } => free_vars.capacity() * VALUE_BYTES,
            HeapObjectData::VmContinuationRef { id: _, bytes }
            | HeapObjectData::VmDelimitedContinuationRef { id: _, bytes } => *bytes,
            // Nothing outside the slot.
            HeapObjectData::Real(_)
            | HeapObjectData::Complex { .. }
            | HeapObjectData::LabelPlaceholder(_)
            | HeapObjectData::MutableCell(_)
            | HeapObjectData::Ephemeron(_)
            | HeapObjectData::CoreSyntax(_)
            | HeapObjectData::Free => 0,
            // Shared `Rc` payloads, or ones that change size after
            // allocation: not charged (the module comment says why).
            HeapObjectData::Procedure(_)
            | HeapObjectData::Port(_)
            | HeapObjectData::Macro(_)
            | HeapObjectData::RecordType(_)
            | HeapObjectData::Continuation(_)
            | HeapObjectData::Parameter { .. }
            | HeapObjectData::Promise(_)
            | HeapObjectData::Library(_)
            | HeapObjectData::EnvironmentSpecifier { .. }
            | HeapObjectData::PromptTag(_) => 0,
        }
    }
}

/// What raises the collection-pending flag: allocations since the last
/// collection reaching `allocations`, or bytes reaching `bytes`, whichever
/// comes first. The mode made concrete (`GcController::current_threshold`):
/// the adaptive default counts bytes, `PATINA_GC_STRESS` allocations, and
/// zeal's threshold of no allocations is already crossed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcThreshold {
    pub allocations: usize,
    pub bytes: usize,
}

impl GcThreshold {
    /// Never crossed: only `(gc)` raises the flag.
    pub const NEVER: Self = Self {
        allocations: usize::MAX,
        bytes: usize::MAX,
    };

    /// Crossed after `n` allocations, whatever their size.
    pub const fn allocations(n: usize) -> Self {
        Self {
            allocations: n,
            bytes: usize::MAX,
        }
    }

    /// Crossed after `n` bytes of allocation.
    pub const fn bytes(n: usize) -> Self {
        Self {
            allocations: usize::MAX,
            bytes: n,
        }
    }
}

/// The heap's byte totals. Allocation adds to the shared `since_gc`; a sweep
/// moves that into `through_last_gc`, adds what it freed to `reclaimed` and
/// records what marking found live as `live`.
#[derive(Debug)]
pub(super) struct ByteAccount {
    /// The counts a holder of external bytes writes, shared with every
    /// [`ExternalBytes`] handle.
    pub(super) shared: Rc<SharedCounts>,
    /// Bytes charged before the last collection.
    pub(super) through_last_gc: u64,
    /// Bytes the collections have freed: each dead object's slot and
    /// payload, measured at its death.
    pub(super) reclaimed: u64,
    /// L: the bytes the last collection found live, the external bytes held
    /// at that time included. Zero before the first collection.
    pub(super) live: usize,
}

impl ByteAccount {
    /// An empty account that raises `pending`, the heap's collection-pending
    /// flag, when an external charge crosses the byte threshold.
    pub(super) fn new(pending: Rc<Cell<bool>>) -> Self {
        Self {
            shared: Rc::new(SharedCounts {
                since_gc: Cell::new(0),
                external: Cell::new(0),
                threshold: Cell::new(usize::MAX),
                pending,
            }),
            through_last_gc: 0,
            reclaimed: 0,
            live: 0,
        }
    }
}

/// The part of the byte account that a holder outside the heap writes, so
/// that charging or giving back external bytes needs no heap borrow (#615).
///
/// The trigger's count is here and not on the heap because a namespace
/// charges as its tables grow, and a table grows wherever a definition
/// lands, some of them under a heap borrow; the byte threshold and the
/// pending flag are here so that such a charge raises the flag the moment
/// it crosses, as an allocation does, rather than at the next allocation.
#[derive(Debug)]
pub(super) struct SharedCounts {
    /// Bytes charged since the last collection, by allocations and external
    /// charges both: what the trigger compares.
    pub(super) since_gc: Cell<usize>,
    /// Bytes held outside the arenas on behalf of heap objects, now.
    pub(super) external: Cell<usize>,
    /// The installed [`GcThreshold::bytes`]: `Heap::set_gc_threshold` writes
    /// it, and an external charge compares `since_gc` against it.
    pub(super) threshold: Cell<usize>,
    /// The heap's collection-pending flag (`Heap::gc_pending`).
    pub(super) pending: Rc<Cell<bool>>,
}

impl SharedCounts {
    /// Count `bytes` toward the next collection, and answer the count for
    /// the caller to compare. Saturating, so a size an embedder got wrong
    /// cannot wrap the count past the threshold and put the collection off.
    #[inline]
    pub(super) fn count(&self, bytes: usize) -> usize {
        let since_gc = self.since_gc.get().saturating_add(bytes);
        self.since_gc.set(since_gc);
        since_gc
    }

    /// Hold `bytes` more outside the arenas and count them like an
    /// allocation, raising the pending flag if they cross the byte threshold.
    fn charge_external(&self, bytes: usize) {
        self.external.set(self.external.get().saturating_add(bytes));
        if self.count(bytes) >= self.threshold.get() {
            self.pending.set(true);
        }
    }
}

/// A handle to the heap's external bytes ([`Heap::external_bytes_handle`]),
/// through which a holder charges what it holds and gives it back when it
/// dies, with no heap borrow either way.
///
/// A holder of external bytes — an environment's tables, a port's buffer —
/// usually dies when the last `Rc` naming it drops, and that is often inside
/// a sweep: tombstoning a dead slot drops its `Rc` payload while the sweep
/// holds the heap mutably, so a `Drop` that borrowed the heap would panic.
/// The handle shares the total with the heap instead, and the sweep reads it
/// after the arenas are swept, so bytes given back by the drops a sweep
/// causes are out of the L that collection sets. A namespace charges its
/// tables as they grow (`Environment`'s `NamespaceCharge`), which can be
/// under a heap borrow too, so a charge goes through the handle as well.
#[derive(Debug, Clone)]
pub struct ExternalBytes(Rc<SharedCounts>);

impl ExternalBytes {
    /// The external bytes held now.
    pub fn held(&self) -> usize {
        self.0.external.get()
    }

    /// Charge `bytes` held outside the arenas: [`Heap::charge_external_bytes`]
    /// without the heap. They count toward the next collection like an
    /// allocation, raising the pending flag if they cross the threshold, and
    /// into L at every collection until they are given back.
    pub fn charge(&self, bytes: usize) {
        self.0.charge_external(bytes);
    }

    /// Give back `bytes` charged with [`ExternalBytes::charge`] or
    /// [`Heap::charge_external_bytes`]. Not a reclamation: `bytes-reclaimed`
    /// counts only what collections free. Giving back more than is held is a
    /// bug in the holder: it panics in debug builds and gives back what is
    /// held in release.
    pub fn release(&self, bytes: usize) {
        let held = self.0.external.get();
        debug_assert!(
            bytes <= held,
            "released {bytes} external bytes, but only {held} are charged"
        );
        self.0.external.set(held.saturating_sub(bytes));
    }
}

impl Heap {
    /// Bytes charged since the last collection: what the adaptive trigger
    /// compares against `max(8 MiB, 2·L)`.
    pub fn bytes_since_gc(&self) -> usize {
        self.account.shared.since_gc.get()
    }

    /// L: the bytes the last collection found live — the slots of the
    /// objects it marked, their payloads (a live VM continuation's snapshot
    /// among them) and the external bytes held then. Zero before the first
    /// collection.
    pub fn live_bytes(&self) -> usize {
        self.account.live
    }

    /// Every byte charged since the heap was made: each allocation's slot and
    /// payload, and the external bytes charged.
    pub fn bytes_allocated(&self) -> u64 {
        self.account
            .through_last_gc
            .saturating_add(self.bytes_since_gc() as u64)
    }

    /// Every byte the collections have freed since the heap was made: each
    /// dead object's slot and payload. Grows only when a collection frees
    /// something, which is what makes it a reclamation proof's guard.
    pub fn bytes_reclaimed(&self) -> u64 {
        self.account.reclaimed
    }

    /// Bytes held outside the arenas on behalf of heap objects
    /// ([`Heap::charge_external_bytes`]), now.
    pub fn external_bytes(&self) -> usize {
        self.account.shared.external.get()
    }

    /// A handle through which a holder of external bytes gives them back
    /// without borrowing the heap ([`ExternalBytes`]).
    pub fn external_bytes_handle(&self) -> ExternalBytes {
        ExternalBytes(Rc::clone(&self.account.shared))
    }

    /// The memory the arenas hold now, live or not: every arena's capacity in
    /// slots (occupied, free, or reserved by its `Vec`) and the payloads of
    /// the occupied slots. A free slot holds a tombstone and no payload. It
    /// does not count the external bytes, which [`Heap::external_bytes`]
    /// reports apart (GC_PRD's footprint is the two together), the free
    /// lists, the mark bits of a collection in progress, the symbol table or
    /// the check build's stamps.
    ///
    /// A walk of the arenas, not a running total: for `(gc-stats)`, not for a
    /// hot path.
    pub fn committed_bytes(&self) -> usize {
        let slots = self.pairs.capacity() * PAIR_SLOT_BYTES
            + self.vectors.capacity() * VECTOR_SLOT_BYTES
            + self.strings.capacity() * STRING_SLOT_BYTES
            + self.objects.capacity() * OBJECT_SLOT_BYTES;
        let payloads = self.vectors.iter().map(vector_payload).sum::<usize>()
            + self.strings.iter().map(string_payload).sum::<usize>()
            + self
                .objects
                .iter()
                .map(HeapObjectData::payload_bytes)
                .sum::<usize>();
        slots + payloads
    }

    /// Charge `bytes` held outside the arenas on a heap object's behalf: they
    /// count toward the next collection like an allocation, and into L at
    /// every collection until they are given back, through
    /// [`Heap::release_external_bytes`] or, from a holder's `Drop`, an
    /// [`ExternalBytes`] handle. GC_PRD §15's external bytes: environment
    /// tables (#615, which charge through a handle as they grow), port
    /// buffers, code, and what an embedder reports.
    ///
    /// Saturating, so a size an embedder got wrong cannot wrap the count the
    /// trigger compares and put off the next collection.
    pub fn charge_external_bytes(&mut self, bytes: usize) {
        self.account.shared.charge_external(bytes);
    }

    /// Give back external bytes charged with
    /// [`Heap::charge_external_bytes`]: [`ExternalBytes::release`], for a
    /// caller that holds the heap.
    pub fn release_external_bytes(&mut self, bytes: usize) {
        self.external_bytes_handle().release(bytes);
    }
}
