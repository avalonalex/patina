//! `CodeObject` — the compiled representation of a single Scheme procedure.
//!
//! See VM_ISA.md §2.3.

use super::instruction::Instruction;
use patina_core::core_expr::Symbol;
use patina_core::environment::Environment;
use patina_core::error::SourceLocation;
use patina_core::tagged_value::TaggedValue;
use std::cell::Cell;
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

/// Identifies a `CodeObject`: a slot in one `VmState`'s code store, and the
/// generation of the code in it.
///
/// The VM gives code a slot when it loads it (`VmState::load_unit`), and a
/// slot whose code has been let go is given to later code, with the next
/// generation. The store therefore grows with the code loaded at once rather
/// than with all the code ever compiled (#352), and an id naming code that
/// has gone finds nothing rather than whatever took its slot: a lookup checks
/// the generation. A slot that has used every generation is not given out
/// again, so ids never repeat.
///
/// Before it is loaded, code is named by a [`CodeObjectId::label`], which
/// `MakeClosure` in the code around it uses too; loading replaces both. A
/// label is never in a store.
///
/// The slot is the low 32 bits and the generation the high 32, so a
/// first-generation id is its slot. A heap `VmClosure` holds the id as a
/// plain `u64`, and `patina_core::debug_format` prints it the way `Display`
/// does here.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CodeObjectId(pub u64);

impl CodeObjectId {
    /// The generation labels are made in, which no loaded code has.
    const LABEL_GENERATION: u32 = u32::MAX;

    /// The id of generation `generation` of slot `slot`.
    #[inline(always)]
    pub fn new(slot: u32, generation: u32) -> Self {
        CodeObjectId((u64::from(generation) << 32) | u64::from(slot))
    }

    /// A name for code the compiler has not handed to a VM yet, distinct from
    /// every other label made in this compilation.
    pub fn label() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        // Wrapping is harmless: a label only has to differ from the others in
        // its own compilation.
        CodeObjectId::new(
            COUNTER.fetch_add(1, Ordering::Relaxed),
            Self::LABEL_GENERATION,
        )
    }

    /// The slot this id names in its `VmState`'s code store.
    #[inline(always)]
    pub fn index(self) -> usize {
        self.0 as u32 as usize
    }

    /// Which of the code objects its slot has held this one is.
    #[inline(always)]
    pub fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }

    /// The id of the code its slot holds next, or `None` when the slot has
    /// used every generation and must not be given out again.
    pub fn next_generation(self) -> Option<Self> {
        let generation = self.generation().checked_add(1)?;
        (generation != Self::LABEL_GENERATION)
            .then(|| CodeObjectId::new(self.index() as u32, generation))
    }
}

/// `12` for the first code in slot 12, `12.3` for its fourth, and a label as
/// its number.
impl fmt::Display for CodeObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.generation() {
            0 | Self::LABEL_GENERATION => write!(f, "{}", self.index()),
            generation => write!(f, "{}.{}", self.index(), generation),
        }
    }
}

impl fmt::Debug for CodeObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.generation() == Self::LABEL_GENERATION {
            write!(f, "CodeObjectId(label {self})")
        } else {
            write!(f, "CodeObjectId({self})")
        }
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
    /// Its slot and generation once loaded, a label before (see
    /// `CodeObjectId`).
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

    /// How many live VM closures run this code (#338). A frame holds its code
    /// itself, so the `Rc` count says whether a frame needs it; a closure names
    /// its code by id, so this is what says a closure does. Counted up where
    /// `MakeClosure` makes one and down when the collector frees one. It
    /// changes through a shared code object, as `global_cache` does, and like
    /// that it has no bearing on what the code does.
    pub live_closures: Cell<u32>,
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

#[cfg(test)]
mod tests {
    use super::CodeObjectId;

    #[test]
    fn an_id_is_its_slot_and_generation() {
        let id = CodeObjectId::new(7, 3);
        assert_eq!((id.index(), id.generation()), (7, 3));
        assert_eq!(CodeObjectId::new(7, 0).0, 7);
        assert_eq!(id.to_string(), "7.3");
        assert_eq!(CodeObjectId::new(7, 0).to_string(), "7");
    }

    #[test]
    fn a_slot_is_given_out_until_its_generations_run_out() {
        let id = CodeObjectId::new(7, 0);
        assert_eq!(id.next_generation(), Some(CodeObjectId::new(7, 1)));
        // The last generation before the one labels are made in.
        let last = CodeObjectId::new(7, u32::MAX - 1);
        assert_eq!(last.next_generation(), None);
        assert_eq!(
            CodeObjectId::new(7, u32::MAX - 2).next_generation(),
            Some(last)
        );
    }

    #[test]
    fn labels_are_distinct_and_never_a_loaded_generation() {
        let (a, b) = (CodeObjectId::label(), CodeObjectId::label());
        assert_ne!(a, b);
        assert_eq!(a.generation(), CodeObjectId::LABEL_GENERATION);
        assert_eq!(a.next_generation(), None);
    }
}
