//! A guard for recursive walks of code that a datum label can make circular.
//!
//! The reader accepts datum labels anywhere, so a program can contain a form
//! that contains itself: `#0=(list 1 #0#)`. A walk that recurses into a
//! form's elements follows that until the stack overflows and the process
//! aborts (#459). A cycle through a list's *spine* is [`Heap::spine`]'s to
//! catch; this catches one through an element.
//!
//! [`Heap::spine`]: crate::heap::Heap::spine

use crate::tagged_value::TaggedValue;

/// The compound nodes a recursive walk is inside, innermost last.
///
/// A node entered again while it is still open is a cycle: in a finite datum
/// without one, a node cannot contain itself, and a macro expansion is built
/// of new pairs and pieces of its input, so it cannot contain the form that
/// holds it either.
#[derive(Debug, Default)]
pub struct OpenNodes {
    nodes: Vec<u64>,
}

impl OpenNodes {
    /// Below this depth a node is entered without looking for it. Code is
    /// rarely nested this deep, so an ordinary walk pays a push and a pop per
    /// node and nothing more; a cycle nests without end, so it gets here, and
    /// from here on the node it comes back to is found within one lap.
    const UNCHECKED_DEPTH: usize = 32;

    /// Enter `node`. `false`, entering nothing, when it is already open.
    pub fn enter(&mut self, node: TaggedValue) -> bool {
        let bits = node.raw_bits();
        if self.nodes.len() >= Self::UNCHECKED_DEPTH && self.nodes.contains(&bits) {
            return false;
        }
        self.nodes.push(bits);
        true
    }

    /// Leave the node most recently entered.
    pub fn leave(&mut self) {
        self.nodes.pop();
    }

    /// Whether no node is open — what a walk finds when it starts.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::OpenNodes;
    use crate::TaggedValue;
    use crate::heap::Heap;

    #[test]
    fn a_node_entered_again_while_open_is_refused_once_the_walk_is_deep() {
        let mut heap = Heap::new();
        let pairs: Vec<TaggedValue> = (0..40)
            .map(|i| heap.alloc_pair(TaggedValue::fixnum(i), TaggedValue::NULL))
            .collect();
        let mut open = OpenNodes::default();
        for &p in &pairs {
            assert!(open.enter(p));
        }
        // Deep enough that entering looks: an open node is refused, and a
        // node that was left may be entered again.
        assert!(!open.enter(pairs[3]));
        open.leave();
        assert!(!open.enter(pairs[3]));
        assert!(open.enter(pairs[39]));
    }
}
