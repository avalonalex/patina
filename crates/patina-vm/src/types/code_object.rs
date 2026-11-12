//! `CodeObject` — the compiled representation of a single Scheme procedure.
//!
//! See VM_ISA.md §2.3.

use super::instruction::Instruction;
use patina_core::LinkTable;
use patina_core::core_expr::Symbol;
use patina_core::error::SourceLocation;
use patina_core::tagged_value::TaggedValue;
use patina_core::{Trace, TraceThroughRc};
use std::cell::Cell;
use std::fmt;
use std::rc::Rc;
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
///
/// Its trace (derived, #686) reports its constants, every heap value its
/// instructions can load: pinned, since a code object is shared through `Rc`
/// and never written, until stage 4e makes its constants a descriptor's. The
/// VM traces each loaded one from its code store ([`TraceThroughRc`]): a
/// form's code stays there only while a frame, a captured continuation or a
/// live closure can still run it (#338). Its link table, `links`, is not
/// traced from the store, but by code id (#836).
#[derive(Debug, Clone, Trace)]
pub struct CodeObject {
    /// Its slot and generation once loaded, a label before (see
    /// `CodeObjectId`).
    #[trace(skip, reason = "a code id")]
    pub id: CodeObjectId,

    /// Inferred or declared name (for stack traces and error messages).
    pub name: Option<Symbol>,

    /// The instruction stream.
    #[trace(
        skip,
        reason = "opcodes and register numbers; an immediate operand is a value that fits in the word, never a heap reference, which goes in `constants`"
    )]
    pub instructions: Vec<Instruction>,

    /// Constant pool: literals too large to inline in `LoadImmediate`
    /// (heap-allocated strings, exact rationals, etc.).
    pub constants: Vec<TaggedValue>,

    /// Total registers needed by this frame (assigned by Pass 4).
    #[trace(skip, reason = "a count")]
    pub num_regs: u16,

    /// Arity (fixed or variadic).
    #[trace(skip, reason = "argument counts")]
    pub arity: Arity,

    /// Maps instruction indices to source locations for error reporting.
    /// Sorted by pc; binary-search to resolve.
    #[trace(skip, reason = "source positions")]
    pub source_map: Vec<(usize, SourceLocation)>,

    /// Bitsets of possible register roots before each instruction, after completed
    /// expression temporaries have been retired. Locals stay conservative.
    /// Register `r` uses bit `r % 64` in word `r / 64`.
    /// The collector and continuation capture clear excluded slots before
    /// tracing; normal dispatch does no work for these maps (#423).
    /// Runtime stubs and hand-built code use `None` to retain every slot.
    #[trace(skip, reason = "liveness bitsets")]
    pub register_roots: Option<Vec<Vec<u64>>>,

    /// The unit's link table (#836): the spelling of each global operand
    /// (`LoadGlobal`, `StoreGlobal`, `Define`, `CallPrimitive` and the
    /// inline opcodes carry an index into it), made by pass 5 and shared by
    /// every member of the unit through this one `Rc`, and an entry beside
    /// each for the record it links, linked by `VmState::load_unit` in the
    /// namespace the unit was compiled in, before any read goes through it.
    /// Code with no global operand, the runtime's stubs among it, has an
    /// empty one; a stub's is never linked, and a table compiled and never
    /// loaded (`--dump`, the disassembler) serves its names alone.
    #[trace(
        skip,
        reason = "traced by its unit's code id (`VmState::trace_code_ids`), when a frame, a marked closure or the running code names the unit, never from the code store (4b's start decision 3)"
    )]
    pub links: Rc<LinkTable>,

    /// How many live VM closures run this code (#338). A frame holds its code
    /// itself, so the `Rc` count says whether a frame needs it; a closure names
    /// its code by id, so this is what says a closure does. Counted up where
    /// `MakeClosure` makes one and down when the collector frees one. It
    /// changes through a shared code object, and has no bearing on what the
    /// code does.
    #[trace(skip, reason = "a count")]
    pub live_closures: Cell<u32>,
}

impl TraceThroughRc for CodeObject {}

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
