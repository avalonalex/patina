//! `CodeObject` — the compiled representation of a single Scheme procedure.
//!
//! See VM_ISA.md §2.3.

use super::instruction::Instruction;
use patina_core::core_expr::Symbol;
use patina_core::environment::Environment;
use patina_core::error::SourceLocation;
use patina_core::tagged_value::TaggedValue;
use std::cell::Cell;
use std::sync::atomic::{AtomicU32, Ordering};

/// Uniquely identifies a `CodeObject`.
///
/// Ids are minted by [`CodeObjectId::fresh`] from a process-wide sequential
/// counter — dense and never reused — so id-indexed stores can be plain
/// `Vec`s indexed by [`CodeObjectId::index`] (see `VmState::code_store`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CodeObjectId(pub u32);

impl CodeObjectId {
    /// Mint the next process-wide id.
    pub fn fresh() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        CodeObjectId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// The dense index this id occupies in id-indexed stores.
    #[inline(always)]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Index into `CodeObject::constants`.
pub type ConstIdx = u16;

/// Arity descriptor for a compiled procedure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    /// Exactly `n` arguments required.
    Fixed(u16),
    /// At least `n` arguments; excess are collected into a rest list.
    /// `Variadic(0)` accepts any number of arguments.
    Variadic(u16),
}

impl Arity {
    pub fn accepts(&self, n: usize) -> bool {
        match self {
            Arity::Fixed(k) => n == *k as usize,
            Arity::Variadic(k) => n >= *k as usize,
        }
    }
}

/// The compiled representation of a Scheme procedure (top-level or lambda).
///
/// A `CodeObject` is immutable after compilation and may be shared between
/// closures that share the same code but different captured environments.
#[derive(Debug, Clone)]
pub struct CodeObject {
    /// Process-unique id (see `CodeObjectId`).
    pub id: CodeObjectId,

    /// Inferred or declared name (for stack traces and error messages).
    pub name: Option<Symbol>,

    /// The instruction stream.
    pub instructions: Vec<Instruction>,

    /// Constant pool: literals too large to inline in `LoadImmediate`
    /// (heap-allocated strings, exact rationals, etc.).
    pub constants: Vec<TaggedValue>,

    /// Total registers needed by this frame (assigned by Pass 4).
    pub num_regs: u16,

    /// Arity (fixed or variadic).
    pub arity: Arity,

    /// Maps instruction indices to source locations for error reporting.
    /// Sorted by pc; binary-search to resolve.
    pub source_map: Vec<(usize, SourceLocation)>,

    /// Per-site inline caches for `LoadGlobal`/`StoreGlobal`, indexed by pc
    /// (Track P P4). Built by [`GlobalCacheEntry::table`]: the same length
    /// as `instructions` (entries for non-global instructions stay empty),
    /// or empty when the code has no global-access instructions at all.
    /// See [`GlobalCacheEntry`] for the soundness argument.
    pub global_cache: Vec<Cell<GlobalCacheEntry>>,
}

/// One `LoadGlobal`/`StoreGlobal` site's resolved binding, and the canonical
/// statement of why hits are sound:
///
/// - Entries key on `Environment::env_id`, which is process-unique and
///   **never reused** — unlike an address — so an entry left over from a
///   dead environment can only miss, never falsely hit.
/// - A binding's slot in an environment's local table is **stable for the
///   environment's life**: redefinition overwrites the slot in place
///   (see `Bindings` in patina-core), so a hit always reads the current
///   value of the same name.
/// - Only names that resolve in the queried environment's **own** table are
///   cached; parent-resolved names always take the full lookup, so a later
///   local (re)definition that would change resolution can never be masked.
#[derive(Debug, Clone, Copy)]
pub struct GlobalCacheEntry {
    /// `Environment::env_id` of the resolved environment; 0 = empty entry.
    pub env_id: u64,
    /// Slot index in that environment's local binding table.
    pub slot: u32,
}

impl GlobalCacheEntry {
    pub const EMPTY: GlobalCacheEntry = GlobalCacheEntry { env_id: 0, slot: 0 };

    /// The cache table for a freshly compiled instruction stream: pc-indexed,
    /// or empty (never touched at runtime) when no instruction accesses a
    /// global. Sizing from the stream itself keeps the length invariant in
    /// one place.
    pub fn table(instructions: &[Instruction]) -> Vec<Cell<GlobalCacheEntry>> {
        let has_global_ops = instructions.iter().any(|i| {
            matches!(
                i,
                Instruction::LoadGlobal { .. } | Instruction::StoreGlobal { .. }
            )
        });
        if has_global_ops {
            vec![Cell::new(Self::EMPTY); instructions.len()]
        } else {
            Vec::new()
        }
    }

    /// Probe — and on a local resolution, fill — a per-site cache entry.
    /// Returns the binding's slot when `name` lives in `globals`' own table
    /// (a hit costs one id compare); `None` for parent-resolved or unbound
    /// names, which are never cached.
    #[inline(always)]
    pub fn probe(cell: &Cell<GlobalCacheEntry>, globals: &Environment, name: &str) -> Option<u32> {
        let entry = cell.get();
        let env_id = globals.env_id();
        if entry.env_id == env_id {
            return Some(entry.slot);
        }
        let slot = globals.local_slot(name)?;
        cell.set(GlobalCacheEntry { env_id, slot });
        Some(slot)
    }
}

impl CodeObject {
    /// Look up the source location for a given program counter.
    pub fn source_location(&self, pc: usize) -> Option<&SourceLocation> {
        let idx = self
            .source_map
            .partition_point(|(entry_pc, _)| *entry_pc <= pc);
        if idx == 0 {
            None
        } else {
            Some(&self.source_map[idx - 1].1)
        }
    }
}
