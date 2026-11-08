//! The byte account (#606, `PRD/GC_PRD.md` §15): what each object costs the
//! collection trigger, and the totals `(gc-stats)` reports.
//!
//! The trigger used to count objects, so a 100,000-element vector cost it
//! what a pair costs, and a VM continuation's snapshot of a thousand frames
//! cost it one slot. Now every `alloc_*` charges **bytes**: the object's slot
//! in its arena plus its *payload*, the memory it owns outright outside the
//! slot — a vector's elements, a string's characters, a bytevector's bytes, a
//! bignum's limbs, a record's fields, a closure's free variables, a VM
//! continuation's register and frame snapshot, a tree-walker closure's own
//! allocation and an estimate of the frame it captures. One function per
//! arena measures a payload, and allocation, marking and sweep all call it,
//! so what an object was charged at birth is what marking counts while it
//! lives and what sweep credits when it dies. A tree-walker closure's payload
//! is measured by a function of its own, on a path of its own (below).
//!
//! **Payloads do not grow after allocation**, so no growth is charged.
//! Vectors, strings and bytevectors hand out no `&mut` into their payload,
//! and so never their `Vec`: each is written in place (`vector_set`,
//! `string_set_char`, `bytevector_u8_set`, and their range forms), so their
//! capacity is fixed by the type system; a record's field count and a
//! closure's free variables are fixed at construction, and the heap writes
//! them in place; the other payloads charged — a bignum's limbs, an
//! exception's message and irritants, a `values` object, a symbol's name, an
//! identifier's scopes, a continuation's snapshot — are never written after
//! allocation. A parameter's value stack, which `parameterize` grows and
//! shrinks, is not charged: it is as deep as the `parameterize` forms active,
//! which their frames already pay for.
//!
//! **Shared `Rc` payloads are not charged** — a procedure's body and
//! environment (a tree-walker closure is charged an estimate of its frame
//! instead, below), a macro, a library, an environment specifier, a
//! prompt tag, a promise's state (which `promise_update` shares between two
//! promises), a tree-walker continuation (whose frames are `Rc` links shared
//! with every other capture). Attributing them to one slot would charge the
//! same memory once per object that names it, and what they hold belongs to
//! code and environments, which GC_PRD §15 charges as **external bytes**,
//! once per holder rather than once per object that names it. A namespace —
//! an environment made without a parent (the global environment, a
//! library's, and those `environment` and the R5RS constructors build) or a
//! host namespace, whose parent is the global one (#835) — charges its own
//! tables, once when it is made and again as they grow, and gives them back
//! when it drops (#615; `Environment`'s `NamespaceCharge` says what it
//! counts). [`Heap::charge_external_bytes`] is the entry point
//! for a holder with the heap in hand, and an [`ExternalBytes`] handle for
//! one without: the handle needs no heap borrow, because such a holder
//! usually dies when the last `Rc` naming it drops, and that is often inside
//! a sweep, which holds the heap's `RefCell` mutably while it drops the
//! tombstoned slots' payloads, and a namespace's tables grow wherever a
//! definition lands. The sweep settles L after the arenas are swept, so what
//! those drops give back is out of the L it sets.
//!
//! **An open file port** (#607, GC_PRD §9.6 F3) is charged
//! [`FILE_PORT_BYTES`] of external bytes for its buffer, and counted toward
//! **descriptor pressure**: the file ports opened since the last collection
//! and not closed since. When that count reaches the installed
//! [`GcThreshold::descriptors`], `min(128, RLIMIT_NOFILE / 4)`
//! ([`descriptor_pressure_threshold`]), the heap posts a collection, which
//! closes the dead ones: it queues each dead file port's object with the
//! `FinalRegistry`, and the drain after it takes the entry out of the heap's
//! `PortTable`, and a port's file closes as its last `Rc` drops (#785). A
//! port's slot holds only the entry's number, so its payload is charged
//! nothing. A [`FilePortCharge`] that the port's data holds gives both back
//! when the port closes, explicitly or by dropping, or when the collection
//! that queues it takes the charge out first, with no heap borrow. The
//! rule for every kind the registry finalizes: what a queued entry holds
//! outside the arenas leaves the external bytes at classification, before
//! the sweep settles L, so L counts nothing dead. A
//! loop that closes what it opens never reaches the descriptor threshold,
//! though its charges still count toward the byte trigger like the
//! allocations they stand for; one that drops its ports collects every
//! `threshold` opens rather than running out of descriptors, and an open
//! that runs out anyway collects at its call and tries once more
//! (`patina_primitives::Step::Collect`).
//!
//! **A tree-walker closure** (#637) is charged its `Rc<Procedure>`
//! allocation, its parameter vector and the scope sets its parameters own,
//! and [`CAPTURED_FRAME_BYTES`], an estimate of the frame it captures: on
//! that backend every `let` makes a closure, which keeps its frame alive
//! until a sweep drops it, and charged its slot alone, closure-heavy
//! programs peaked at 64–126 MB under the 8 MiB interval. The estimate is
//! one frame binding one parameter; closures made in one frame are each
//! charged for it; a closure made where no frame is — at top level, in a
//! library body, or by `eval` in a namespace — captures a namespace, whose
//! tables are external bytes already, and is charged a frame it does not
//! capture; and frame chains, and frames only a tree-walker continuation
//! holds, are missed. Exact accounting is stage 4f's, which charges the
//! tree-walker's `Rc` payloads (`PRD/GC_PRD.md`).
//!
//! [`cps_lambda_payload`] measures it, and [`HeapObjectData::payload_bytes`]
//! does not: that match runs on every object allocation, the VM's closures
//! and continuations among them, and an arm that looks through the `Rc`
//! made it a function with a stack frame, which measured +0.16–0.40%
//! instructions on the VM's allocation-heavy loops (built with one codegen
//! unit on both sides, #637). So the closure's payload has its own path
//! through the three: [`Heap::alloc_procedure`] charges it ahead of the
//! slot; marking counts it in the closure's trace arm, which looks through
//! the `Rc` already; and sweep credits the dead closures' in one sum, what
//! the account holds for the closures in the arena less what marking found
//! live, with no work per slot. A check build measures each dead closure as
//! the object arena's sweep drops it, and asserts that the two agree.
//!
//! The policy that reads the account lives in the collector
//! (`MarkSweepCollector::auto_threshold` in `gc.rs`): the next collection
//! after `max(8 MiB, 2·L)` bytes, L being the bytes the last collection
//! found live. `PATINA_GC_STRESS` still counts allocations, not bytes
//! ([`GcThreshold`]). Descriptor pressure applies in every mode that
//! collects on its own, and not under `PATINA_GC=0`.

use std::cell::Cell;
use std::mem::size_of;
use std::rc::Rc;
use std::sync::OnceLock;

use num_bigint::BigInt;

use super::mutator::{Carrier, event};
use super::{Heap, HeapObjectData};
use crate::core_expr::ScopedParam;
pub(crate) use crate::environment::CAPTURED_FRAME_BYTES;
use crate::environment::RC_COUNTS;
use crate::procedure::Procedure;
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

/// What an open file port is charged as external bytes (#607): its buffer,
/// at the 8 KiB capacity that `NativeFs` gives the `BufReader` and
/// `BufWriter` under it (`std`'s default), the figure GC_PRD §9.6 F3 sets.
pub const FILE_PORT_BYTES: usize = 8 * 1024;

/// Descriptor pressure's cap: the file ports opened since the last
/// collection, and not closed, that post one however high the descriptor
/// limit is.
const DESCRIPTOR_PRESSURE_CAP: usize = 128;

/// How many file ports opened since the last collection, and not closed
/// since, post a collection (#607): `min(128, RLIMIT_NOFILE / 4)`, at least
/// one. The soft limit is read once per process, when the first collector
/// installs its threshold; a program that changes it later is not followed.
/// Where there is no such limit to read (not a Unix), the cap alone.
pub fn descriptor_pressure_threshold() -> usize {
    static THRESHOLD: OnceLock<usize> = OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        descriptor_limit()
            .map_or(DESCRIPTOR_PRESSURE_CAP, |limit| {
                DESCRIPTOR_PRESSURE_CAP.min(limit / 4)
            })
            .max(1)
    })
}

/// The soft `RLIMIT_NOFILE`, or `None` where it is unlimited or cannot be
/// read. Read at each call; [`descriptor_pressure_threshold`] keeps the
/// value it read first.
#[cfg(unix)]
pub fn descriptor_limit() -> Option<usize> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `getrlimit` writes the one `rlimit` it is given, which lives
    // for the call, and reads nothing else.
    let status = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) };
    if status != 0 || limit.rlim_cur == libc::RLIM_INFINITY {
        return None;
    }
    Some(usize::try_from(limit.rlim_cur).unwrap_or(usize::MAX))
}

/// No descriptor limit to read.
#[cfg(not(unix))]
pub fn descriptor_limit() -> Option<usize> {
    None
}

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

/// A tree-walker closure's payload (the module comment says what counts and
/// why it is not in [`HeapObjectData::payload_bytes`]): its `Rc<Procedure>`
/// allocation, its parameter vector at capacity, the scope sets its
/// parameters own, and [`CAPTURED_FRAME_BYTES`] for the frame it captures.
/// The closure is immutable behind its `Rc`, so allocation and marking
/// measure the same bytes.
#[inline]
pub(crate) fn cps_lambda_payload(
    params: &Vec<ScopedParam>,
    variadic: &Option<ScopedParam>,
) -> usize {
    RC_COUNTS
        + size_of::<Procedure>()
        + params.capacity() * size_of::<ScopedParam>()
        + params
            .iter()
            .map(|param| param.scopes.heap_bytes())
            .sum::<usize>()
        + variadic
            .as_ref()
            .map_or(0, |param| param.scopes.heap_bytes())
        + CAPTURED_FRAME_BYTES
}

/// A procedure's payload: a tree-walker closure's ([`cps_lambda_payload`]),
/// and none for a primitive, which is charged its slot alone, as before
/// #637 — a backend makes one per registry entry when it installs them, and
/// they live as long as the backend does. Every field is named, so a new one
/// does not compile here until it says whether it is charged.
pub(crate) fn procedure_payload(procedure: &Procedure) -> usize {
    match procedure {
        // Its slot alone (above).
        Procedure::Primitive {
            // A static name.
            name: _,
            // Argument counts.
            arity: _,
            // A name.
            qualified_name: _,
            // An index.
            registry_index: _,
        } => 0,
        Procedure::CpsLambda {
            params,
            variadic,
            // A name, shared with the code.
            cont_param: _,
            // Code, shared with every closure of this lambda.
            body: _,
            // The frame, estimated: `CAPTURED_FRAME_BYTES`.
            env: _,
            // Shared with the code.
            binding_scopes: _,
        } => cps_lambda_payload(params, variadic),
    }
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
                    | crate::error::ExceptionKind::ReadError
                    | crate::error::ExceptionKind::Interrupt => 0,
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
            // The literals are the object's own; the body is a shared `Rc`.
            HeapObjectData::Macro { body: _, literals } => literals.len() * VALUE_BYTES,
            HeapObjectData::VmContinuationRef { id: _, bytes }
            | HeapObjectData::VmDelimitedContinuationRef { id: _, bytes } => *bytes,
            // Nothing outside the slot. A port's slot holds its `PortTable`
            // entry's number; an open file port is charged as external bytes.
            HeapObjectData::Real(_)
            | HeapObjectData::Port(_)
            | HeapObjectData::Complex { .. }
            | HeapObjectData::LabelPlaceholder(_)
            | HeapObjectData::MutableCell(_)
            | HeapObjectData::Ephemeron(_)
            | HeapObjectData::CoreSyntax(_)
            | HeapObjectData::Free => 0,
            // Shared `Rc` payloads, or ones that change size after
            // allocation: not charged (the module comment says why). A
            // tree-walker closure's payload is charged on a path of its own
            // (`procedure_payload`), which keeps this match a leaf.
            HeapObjectData::Procedure(_)
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
/// collection reaching `allocations`, bytes reaching `bytes`, or file ports
/// opened and not closed reaching `descriptors`, whichever comes first. The
/// mode made concrete (`GcController::current_threshold`): the adaptive
/// default counts bytes, `PATINA_GC_STRESS` allocations, and zeal's
/// threshold of no allocations is already crossed; each of them counts
/// descriptors too, and `PATINA_GC=0` counts nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcThreshold {
    pub allocations: usize,
    pub bytes: usize,
    /// Descriptor pressure's threshold (#607): file ports opened since the
    /// last collection and not closed since. Compared where a file port is
    /// charged ([`Heap::charge_file_port`]).
    pub descriptors: usize,
}

impl GcThreshold {
    /// Never crossed: only `(gc)` raises the flag.
    pub const NEVER: Self = Self {
        allocations: usize::MAX,
        bytes: usize::MAX,
        descriptors: usize::MAX,
    };

    /// Crossed after `n` allocations, whatever their size.
    pub const fn allocations(n: usize) -> Self {
        Self {
            allocations: n,
            ..Self::NEVER
        }
    }

    /// Crossed after `n` bytes of allocation.
    pub const fn bytes(n: usize) -> Self {
        Self {
            bytes: n,
            ..Self::NEVER
        }
    }

    /// This threshold, crossed also when `n` file ports opened since the last
    /// collection are still open.
    pub const fn with_descriptors(self, n: usize) -> Self {
        Self {
            descriptors: n,
            ..self
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
    /// The payloads of the tree-walker closures in the object arena, live or
    /// not yet swept ([`cps_lambda_payload`]): `alloc_procedure` adds each
    /// one's, and sweep credits the dead ones' as this less what marking
    /// found live, which it then becomes.
    pub(super) closures: usize,
}

impl ByteAccount {
    /// An empty account that posts a collection to `mutator`, the heap's
    /// carrier, when an external charge crosses the byte threshold.
    pub(super) fn new(mutator: Rc<Carrier>) -> Self {
        Self {
            shared: Rc::new(SharedCounts {
                since_gc: Cell::new(0),
                external: Cell::new(0),
                threshold: Cell::new(usize::MAX),
                mutator,
                descriptors: Cell::new(0),
                open_file_ports: Cell::new(0),
                cycle: Cell::new(0),
                posted: Cell::new(None),
                open_window: Cell::new(None),
                wait_site: Cell::new(None),
                namespaces: Cell::new(0),
                binding_records: Cell::new(0),
                cells: Cell::new(0),
                code_units: Cell::new(0),
            }),
            through_last_gc: 0,
            reclaimed: 0,
            live: 0,
            closures: 0,
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
    /// The heap's carrier, where a charge that crosses the threshold posts
    /// the collection (`Heap::mutator`, #699).
    pub(super) mutator: Rc<Carrier>,
    /// Descriptor pressure (#607): the file ports opened since the last
    /// collection and not closed since. Here rather than on the heap because
    /// a port's charge is given back inside a collection, at classification
    /// (#785), or wherever its last `Rc` drops, and its [`FilePortCharge`]
    /// takes itself off with no heap borrow.
    pub(super) descriptors: Cell<usize>,
    /// The file ports open now, opened in any cycle: the use of the
    /// descriptor limit, `RLIMIT_NOFILE`, as far as the heap's ports go.
    /// Here for the reason `descriptors` is.
    pub(super) open_file_ports: Cell<usize>,
    /// Collections so far, as the sweep counts them when it resets
    /// `descriptors`: a [`FilePortCharge`] records the cycle it was made in,
    /// so that closing a port an earlier collection found live does not take
    /// off one opened since.
    pub(super) cycle: Cell<u64>,
    /// When the pending flag last rose from down, and the bytes charged
    /// then (#648): the start of the wait that K16's first high-water mark
    /// and the time to safepoint measure. Taken by the collection.
    pub(super) posted: Cell<Option<(usize, std::time::Instant)>>,
    /// The site of the deferral window open now (`NoGcScope::enter`),
    /// mirrored here so that a post with no heap to hand sees it.
    pub(super) open_window: Cell<Option<&'static std::panic::Location<'static>>>,
    /// The first deferral window that held the collection pending now:
    /// open when it was posted, or opened while it waited.
    pub(super) wait_site: Cell<Option<&'static std::panic::Location<'static>>>,
    /// The live namespaces (`Environment`'s `NamespaceCharge`), counted
    /// when one is made and when it drops: here for the reason the external
    /// bytes are, since a namespace usually drops inside a sweep.
    pub(super) namespaces: Cell<usize>,
    /// The binding records the live namespaces hold (#834): a running sum,
    /// raised as a namespace makes one and lowered by its records when it
    /// drops.
    pub(super) binding_records: Cell<usize>,
    /// The cells of their own the live namespaces' records name now (#834):
    /// a running sum, raised as a namespace makes one, lowered when one of
    /// its records is re-pointed away from its own cell, and by its cells
    /// when it drops.
    pub(super) cells: Cell<usize>,
    /// The code units the VM has loaded and not released (#836): its count,
    /// written whenever its table of units changes ([`Heap::set_code_units`]).
    /// Here rather than a field of `Heap`, where one more field moves the
    /// allocator's hot fields (`HandleTable`'s `root_set` has the
    /// measurement).
    pub(super) code_units: Cell<usize>,
}

impl SharedCounts {
    /// Post a collection to the carrier. When none was posted, note when and
    /// where it was (#648): once per collection cycle, so allocation does no
    /// new work while the post stands.
    #[inline]
    pub(super) fn raise_pending(&self) {
        if !self.mutator.gc_posted() {
            self.note_posted();
            self.mutator.post(event::GC_MAJOR);
        }
    }

    #[cold]
    #[inline(never)]
    fn note_posted(&self) {
        self.posted
            .set(Some((self.since_gc.get(), std::time::Instant::now())));
        if self.wait_site.get().is_none() {
            self.wait_site.set(self.open_window.get());
        }
    }

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
            self.raise_pending();
        }
    }

    /// Hold `bytes` fewer outside the arenas ([`ExternalBytes::release`]).
    fn release_external(&self, bytes: usize) {
        let held = self.external.get();
        debug_assert!(
            bytes <= held,
            "released {bytes} external bytes, but only {held} are charged"
        );
        self.external.set(held.saturating_sub(bytes));
    }

    /// The sweep's part of descriptor pressure: the ports opened before this
    /// collection that it found live count toward no later one.
    pub(super) fn start_cycle(&self) {
        self.descriptors.set(0);
        self.cycle.set(self.cycle.get().wrapping_add(1));
    }
}

/// What an open file port holds against the collector (#607): the
/// [`FILE_PORT_BYTES`] of its buffer, charged as external bytes, and its
/// place in descriptor pressure's count. [`Heap::charge_file_port`] makes
/// one when the port gets its heap object, and the port's data holds it
/// beside the file, so both go together: when the port is closed, or when
/// its last `Rc` drops. A collection that finds the port's objects dead,
/// with its every `Rc` held by their queued entries, takes the charge out at
/// classification, before it settles L, while the heap is borrowed mutably,
/// so giving it back needs no heap borrow; the drain after the collection
/// closes the file (#785).
#[derive(Debug)]
pub(crate) struct FilePortCharge {
    counts: Rc<SharedCounts>,
    /// The collection cycle the port was opened in ([`SharedCounts::cycle`]).
    cycle: u64,
}

impl Drop for FilePortCharge {
    fn drop(&mut self) {
        let counts = &self.counts;
        counts.release_external(FILE_PORT_BYTES);
        counts
            .open_file_ports
            .set(counts.open_file_ports.get().saturating_sub(1));
        // A port an earlier collection found live was counted toward that
        // one, and is not among the ones opened since.
        if counts.cycle.get() == self.cycle {
            counts
                .descriptors
                .set(counts.descriptors.get().saturating_sub(1));
        }
    }
}

/// A handle to the heap's external bytes ([`Heap::external_bytes_handle`]),
/// through which a holder charges what it holds and gives it back when it
/// dies, with no heap borrow either way.
///
/// A holder of external bytes, such as an environment's tables, usually dies
/// when the last `Rc` naming it drops, and that is often inside a sweep:
/// tombstoning a dead slot drops its `Rc` payload while the sweep holds the
/// heap mutably, so a `Drop` that borrowed the heap would panic. The handle
/// shares the total with the heap instead, and the sweep reads it after the
/// arenas are swept, so bytes given back by the drops a sweep causes are out
/// of the L that collection sets. A file port's buffer shares the total
/// through its [`FilePortCharge`], which a collection that finds the port
/// dead takes out at classification, under the same borrow and before the
/// sweep settles L (#785). A namespace charges its
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
        self.0.release_external(bytes);
    }

    /// A namespace was made: one more in [`Heap::namespaces`]. Crate-private,
    /// as the four below are, so that a host holding the raw handle cannot
    /// forge the counts.
    pub(crate) fn namespace_made(&self) {
        let namespaces = &self.0.namespaces;
        namespaces.set(namespaces.get() + 1);
    }

    /// A namespace dropped, holding `records` binding records and `cells`
    /// cells of its own: it leaves [`Heap::namespaces`], and they leave
    /// [`Heap::binding_records`] and [`Heap::cells`]. Borrow-free, since a
    /// namespace usually drops inside a sweep.
    pub(crate) fn namespace_dropped(&self, records: usize, cells: usize) {
        let counts = &self.0;
        debug_assert!(
            counts.namespaces.get() > 0,
            "a namespace dropped, but none is counted"
        );
        counts
            .namespaces
            .set(counts.namespaces.get().saturating_sub(1));
        lower(&counts.binding_records, records, "binding records");
        lower(&counts.cells, cells, "cells");
    }

    /// A namespace made `n` binding records (#834).
    pub(crate) fn records_made(&self, n: usize) {
        let records = &self.0.binding_records;
        records.set(records.get() + n);
    }

    /// A namespace made `n` cells of its own (#834).
    pub(crate) fn cells_made(&self, n: usize) {
        let cells = &self.0.cells;
        cells.set(cells.get() + n);
    }

    /// A namespace's records stopped naming `n` cells of its own: each was
    /// re-pointed away from its cell, which an importer's record or an
    /// export may still keep (#834).
    pub(crate) fn cells_let_go(&self, n: usize) {
        lower(&self.0.cells, n, "cells");
    }
}

/// Take `n` off the running sum `count` of `what`. Taking off more than is
/// counted is a bug in the counting: it panics in debug builds, and floors
/// at 0 in release.
fn lower(count: &Cell<usize>, n: usize, what: &str) {
    let held = count.get();
    debug_assert!(n <= held, "{n} {what} let go, but only {held} are counted");
    count.set(held.saturating_sub(n));
}

/// A host's account of the bytes it holds outside the heap on its values'
/// behalf (#710, `PRD/GC_PRD.md` §11.5's external-bytes accounter, after
/// V8's `ExternalMemoryAccounter`): a buffer a host primitive keeps for an
/// object, say. What it charges counts toward the next collection like an
/// allocation, and into L until it is given back; whatever it still holds
/// when it is dropped is given back then.
#[derive(Debug)]
pub struct ExternalAccounter {
    bytes: ExternalBytes,
    held: usize,
}

impl ExternalAccounter {
    /// An accounter holding nothing, charging through `bytes`.
    ///
    /// Raw (#831): `ExternalBytes` is reached only through the raw
    /// `Heap::external_bytes_handle`. A host gets an accounter from
    /// `Interpreter::external_bytes`, which is inside the promise.
    pub fn new(bytes: ExternalBytes) -> Self {
        ExternalAccounter { bytes, held: 0 }
    }

    /// Charge `bytes` more.
    pub fn increase(&mut self, bytes: usize) {
        self.held += bytes;
        self.bytes.charge(bytes);
    }

    /// Give back `bytes` of what this accounter charged. Giving back more is
    /// a bug in the host: it panics in debug builds, and gives back what is
    /// held in release.
    pub fn decrease(&mut self, bytes: usize) {
        debug_assert!(
            bytes <= self.held,
            "an external-bytes accounter gave back {bytes} of the {} it held",
            self.held
        );
        let bytes = bytes.min(self.held);
        self.held -= bytes;
        self.bytes.release(bytes);
    }

    /// What this accounter holds now.
    pub fn held(&self) -> usize {
        self.held
    }
}

impl Drop for ExternalAccounter {
    fn drop(&mut self) {
        if self.held > 0 {
            self.bytes.release(self.held);
        }
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

    /// The live namespaces, each of which charges its tables
    /// (`Environment`'s `NamespaceCharge`): the global environment, each
    /// loaded library's, each live environment specifier's,
    /// `%parameterize-swap!`'s compile namespace, and each live host
    /// namespace, the environment `Interpreter::new_environment` answers
    /// (#835). A frame is not counted. `(gc-stats)`'s `namespaces`.
    pub fn namespaces(&self) -> usize {
        self.account.shared.namespaces.get()
    }

    /// The binding records the live namespaces hold (#834): each slot's,
    /// each scoped entry's and each import alias's, and each of a host
    /// namespace's read-through records (#835). A frame's imports are not
    /// counted. A running sum, exact once the dead namespaces have dropped.
    /// `(gc-stats)`'s `binding-records`.
    pub fn binding_records(&self) -> usize {
        self.account.shared.binding_records.get()
    }

    /// The cells of their own the live namespaces' records name now (#834),
    /// a host namespace's among them (#835). Not counted: a free cell, the
    /// one a host namespace's read-through records name among them, and a
    /// cell only an importer's record or an export keeps once its namespace
    /// dropped or re-pointed away from it. A running sum, exact once the
    /// dead namespaces have dropped. `(gc-stats)`'s `cells`.
    pub fn cells(&self) -> usize {
        self.account.shared.cells.get()
    }

    /// The code units the VM has loaded and not released (#836). A unit is
    /// what one compilation loaded: a form's top code object (a program's
    /// form, a library body form, a datum `eval` compiled) with the code of
    /// the lambdas nested in it, kept and let go whole. The VM writes the
    /// count ([`Heap::set_code_units`]) as it loads a unit and as it lets
    /// units go, when a form returns and after a collection, so it is exact
    /// once a collection has run. The tree-walker loads no code, and it
    /// reads 0 there. `(gc-stats)`'s `code-units`.
    pub fn code_units(&self) -> usize {
        self.account.shared.code_units.get()
    }

    /// Record `n` as the VM's count of loaded code units
    /// ([`Heap::code_units`]), which the VM writes as its table of units
    /// changes. A diagnostic that nothing in the heap reads: a host that
    /// writes it corrupts only what `(gc-stats)` reports.
    #[doc(hidden)]
    pub fn set_code_units(&self, n: usize) {
        self.account.shared.code_units.set(n);
    }

    /// The memory the arenas hold now, live or not: every arena's capacity in
    /// slots (occupied, free, or reserved by its `Vec`) and the payloads of
    /// the occupied slots. A free slot holds a tombstone and no payload. It
    /// does not count the external bytes, which [`Heap::external_bytes`]
    /// reports apart (GC_PRD's footprint is the two together), the free
    /// lists, the mark bits of a collection in progress, the symbol table or
    /// the check build's stamps.
    ///
    /// A tree-walker closure's payload is what it was charged
    /// (`cps_lambda_payload`), which counts an estimate of the frame it
    /// captures, not a measurement: once per closure, so a frame several
    /// closures share is counted for each, and a closure that captures a
    /// namespace is counted a frame it does not hold.
    ///
    /// A walk of the arenas, not a running total, but for the tree-walker
    /// closures' payloads, whose total the account keeps: for `(gc-stats)`,
    /// not for a hot path.
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
                .sum::<usize>()
            // The occupied slots' tree-walker closures, which the walk above
            // does not measure: the account keeps their sum.
            + self.account.closures;
        slots + payloads
    }

    /// Charge a procedure's payload ([`procedure_payload`]) ahead of its
    /// slot: `alloc_procedure`'s part of a tree-walker closure's own path
    /// through the account (the module comment). Counted toward the trigger
    /// before the slot is, so that the slot's allocation compares the two
    /// together against the threshold; and not an allocation of its own, so
    /// `PATINA_GC_STRESS`, which counts allocations, is unchanged.
    #[inline]
    pub(super) fn charge_procedure_payload(&mut self, procedure: &Procedure) {
        let bytes = procedure_payload(procedure);
        self.account.closures += bytes;
        self.account.shared.count(bytes);
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

    /// Charge a file port that has just opened (#607): [`FILE_PORT_BYTES`]
    /// of external bytes, and one more port in descriptor pressure's count.
    /// When the count reaches the installed [`GcThreshold::descriptors`] and
    /// no collection is pending already, post one for the next safe point
    /// and count it in [`Heap::descriptor_collections`]. The port holds what
    /// this returns until it closes. `alloc_port` calls it, once per port.
    pub(crate) fn charge_file_port(&mut self) -> FilePortCharge {
        let counts = &self.account.shared;
        counts.charge_external(FILE_PORT_BYTES);
        let open = counts.descriptors.get().saturating_add(1);
        counts.descriptors.set(open);
        counts
            .open_file_ports
            .set(counts.open_file_ports.get().saturating_add(1));
        if open >= self.gc_threshold.descriptors && !self.mutator.gc_posted() {
            counts.raise_pending();
            self.descriptor_collections += 1;
        }
        FilePortCharge {
            counts: Rc::clone(counts),
            cycle: counts.cycle.get(),
        }
    }

    /// File ports opened since the last collection and not closed since:
    /// what descriptor pressure compares with [`GcThreshold::descriptors`].
    pub fn descriptors_since_gc(&self) -> usize {
        self.account.shared.descriptors.get()
    }

    /// File ports open now, whenever they opened: the use of the descriptor
    /// limit ([`descriptor_limit`]), as far as the heap's own ports go.
    pub fn open_file_ports(&self) -> usize {
        self.account.shared.open_file_ports.get()
    }

    /// Collections that descriptor pressure posted: each time the file ports
    /// opened since the last collection, and not closed, reached the
    /// threshold with no collection pending already.
    pub fn descriptor_collections(&self) -> u64 {
        self.descriptor_collections
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The per-owner counts (#834) move only with the events that name
    /// them, through a handle and with no heap borrow, and a namespace's
    /// drop gives back what it holds.
    #[test]
    fn the_namespace_counts_move_with_their_events() {
        let heap = Heap::new();
        let counts = || (heap.namespaces(), heap.binding_records(), heap.cells());
        assert_eq!(counts(), (0, 0, 0));
        let handle = heap.external_bytes_handle();
        handle.namespace_made();
        handle.namespace_made();
        handle.records_made(3);
        handle.cells_made(2);
        assert_eq!(counts(), (2, 3, 2));
        handle.cells_let_go(1);
        assert_eq!(counts(), (2, 3, 1));
        handle.namespace_dropped(2, 1);
        assert_eq!(counts(), (1, 1, 0));
        handle.namespace_dropped(1, 0);
        assert_eq!(counts(), (0, 0, 0));
    }

    /// The code units' count is what the VM last wrote, 0 on a heap no VM
    /// runs on (#836).
    #[test]
    fn the_code_units_count_is_what_was_last_written() {
        let heap = Heap::new();
        assert_eq!(heap.code_units(), 0);
        heap.set_code_units(3);
        assert_eq!(heap.code_units(), 3);
        heap.set_code_units(1);
        assert_eq!(heap.code_units(), 1);
    }

    /// Letting go of more than is counted is a bug in the counting.
    #[test]
    #[cfg_attr(not(debug_assertions), ignore = "a debug assertion")]
    #[should_panic(expected = "2 cells let go, but only 1 are counted")]
    fn letting_go_of_more_cells_than_are_counted_is_refused() {
        let heap = Heap::new();
        let handle = heap.external_bytes_handle();
        handle.cells_made(1);
        handle.cells_let_go(2);
    }
}
