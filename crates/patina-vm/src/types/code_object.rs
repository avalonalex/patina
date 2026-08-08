//! `CodeObject` — the compiled representation of a single Scheme procedure.
//!
//! See VM_ISA.md §2.3.

use super::instruction::Instruction;
use patina_core::core_expr::Symbol;
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
    /// (Track P P4). Always the same length as `instructions`; entries for
    /// non-global instructions simply stay empty. See
    /// [`GlobalCacheEntry`] for the soundness argument.
    pub global_cache: Vec<Cell<GlobalCacheEntry>>,
}

/// One `LoadGlobal`/`StoreGlobal` site's resolved binding: the globals
/// environment it was resolved against (by never-reused `Environment::env_id`,
/// so a stale entry can only miss, never falsely hit) and the binding's slot
/// in that environment's local table (stable for the environment's life —
/// redefinition overwrites the slot in place).
///
/// Only names that resolve in the queried environment's *own* table are
/// cached; parent-resolved names always take the full lookup, so a later
/// local (re)definition that would change resolution can never be masked.
#[derive(Debug, Clone, Copy)]
pub struct GlobalCacheEntry {
    /// `Environment::env_id` of the resolved environment; 0 = empty entry.
    pub env_id: u64,
    /// Slot index in that environment's local binding table.
    pub slot: u32,
}

impl GlobalCacheEntry {
    pub const EMPTY: GlobalCacheEntry = GlobalCacheEntry { env_id: 0, slot: 0 };

    /// A fresh all-empty cache table for a code object with `len` instructions.
    pub fn table(len: usize) -> Vec<Cell<GlobalCacheEntry>> {
        vec![Cell::new(Self::EMPTY); len]
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
