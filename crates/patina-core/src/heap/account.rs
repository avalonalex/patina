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
//! code and environments, which GC_PRD §15 charges as **external bytes**:
//! [`Heap::charge_external_bytes`] is that entry point, for the environment
//! tables of #615, the ports' buffers and whatever else holds memory on a heap
//! object's behalf outside the arenas.
//!
//! The policy that reads the account lives in the collector
//! (`MarkSweepCollector::auto_threshold` in `gc.rs`): the next collection
//! after `max(8 MiB, 2·L)` bytes, L being the bytes the last collection
//! found live. `PATINA_GC_STRESS` still counts allocations, not bytes
//! ([`GcThreshold`]).

use std::mem::size_of;

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

/// The heap's byte totals. Allocation adds to `since_gc`; a sweep moves that
/// into `through_last_gc`, adds what it freed to `reclaimed` and records what
/// marking found live as `live`.
#[derive(Debug, Default)]
pub(super) struct ByteAccount {
    /// Bytes charged since the last collection: what the trigger compares.
    pub(super) since_gc: usize,
    /// Bytes charged before the last collection.
    pub(super) through_last_gc: u64,
    /// Bytes the collections have freed: each dead object's slot and
    /// payload, measured at its death.
    pub(super) reclaimed: u64,
    /// L: the bytes the last collection found live, the external bytes held
    /// at that time included. Zero before the first collection.
    pub(super) live: usize,
    /// Bytes held outside the arenas on behalf of heap objects, now.
    pub(super) external: usize,
}

impl Heap {
    /// Bytes charged since the last collection: what the adaptive trigger
    /// compares against `max(8 MiB, 2·L)`.
    pub fn bytes_since_gc(&self) -> usize {
        self.account.since_gc
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
        self.account.through_last_gc + self.account.since_gc as u64
    }

    /// Every byte the collections have freed since the heap was made: each
    /// dead object's slot and payload. Grows only when a collection frees
    /// something, which is what makes it a reclamation proof's guard.
    pub fn bytes_reclaimed(&self) -> u64 {
        self.account.reclaimed
    }

    /// Bytes held outside the arenas on behalf of heap objects
    /// ([`Heap::charge_external_bytes`]).
    pub fn external_bytes(&self) -> usize {
        self.account.external
    }

    /// The memory the heap holds now, live or not: every arena's capacity in
    /// slots (occupied, free, or reserved by its `Vec`), the payloads of the
    /// occupied slots, and the external bytes. A free slot holds a tombstone
    /// and no payload. It does not count the free lists, the mark bits of a
    /// collection in progress, the symbol table or the check build's stamps.
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
        slots + payloads + self.account.external
    }

    /// Charge `bytes` held outside the arenas on a heap object's behalf: they
    /// count toward the next collection like an allocation, and into L at
    /// every collection until [`Heap::release_external_bytes`] gives them
    /// back. GC_PRD §15's external bytes: environment tables (#615), port
    /// buffers, code, and what an embedder reports.
    pub fn charge_external_bytes(&mut self, bytes: usize) {
        self.account.external += bytes;
        self.account.since_gc += bytes;
        self.refresh_gc_pending();
    }

    /// Give back external bytes charged with
    /// [`Heap::charge_external_bytes`]. Not a reclamation: `bytes-reclaimed`
    /// counts only what collections free.
    pub fn release_external_bytes(&mut self, bytes: usize) {
        debug_assert!(
            bytes <= self.account.external,
            "released {bytes} external bytes, but only {} are charged",
            self.account.external
        );
        self.account.external = self.account.external.saturating_sub(bytes);
    }
}
