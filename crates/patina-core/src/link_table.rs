//! A VM code unit's global operands (#836; `PRD/GC_PRD.md` §11.6).
//!
//! The VM's compiler (pass 5) gives each distinct global spelling a unit
//! names an index, a [`LinkIdx`], in the order it first emits one, and the
//! instructions that read, write or define a global, or call a primitive
//! through one, carry that index in place of the name. The spellings sit in
//! the unit's [`LinkTable`], one per unit, shared by every member of the unit
//! through one `Rc`. Anything that needs an operand's name reads it there:
//! the global lookups on a cache miss, a definition, a shadowed primitive's
//! deoptimization, the errors, the disassembler and the tracer.

use std::rc::Rc;

/// An index into a unit's [`LinkTable`]: the global operand of `LoadGlobal`,
/// `StoreGlobal`, `Define`, `CallPrimitive` and the 20 inline opcodes.
pub type LinkIdx = u32;

/// A code unit's global spellings, by [`LinkIdx`]: made by the VM's pass 5
/// for the unit it compiles, and held by every member of the unit through
/// one `Rc`. It holds names only, no heap value, so nothing traces it; once
/// the VM links its units at load (#836), it will also hold the binding each
/// operand names.
#[derive(Debug)]
pub struct LinkTable {
    /// The spellings, each once, in the order pass 5 first emitted an
    /// operand naming it.
    names: Box<[Rc<str>]>,
}

impl LinkTable {
    /// The table of a unit whose operands name `names`, operand `k` naming
    /// `names[k]`.
    pub fn new(names: Box<[Rc<str>]>) -> Self {
        LinkTable { names }
    }

    /// A table with no names, for code with no global operand: the runtime's
    /// stubs and code built by hand.
    pub fn empty() -> Rc<Self> {
        Rc::new(LinkTable::new(Box::default()))
    }

    /// Every spelling, by index.
    #[inline]
    pub fn names(&self) -> &[Rc<str>] {
        &self.names
    }

    /// The spelling operand `k` names. Panics if the table has no entry `k`,
    /// which pass 5 never emits.
    #[inline]
    pub fn name(&self, k: LinkIdx) -> &Rc<str> {
        &self.names[k as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::LinkTable;
    use std::rc::Rc;

    #[test]
    fn an_operand_names_its_entry() {
        let names: Vec<Rc<str>> = vec!["car".into(), "x".into()];
        let table = LinkTable::new(names.into_boxed_slice());
        assert_eq!((&**table.name(0), &**table.name(1)), ("car", "x"));
        assert_eq!(table.names().len(), 2);
        assert!(LinkTable::empty().names().is_empty());
    }
}
