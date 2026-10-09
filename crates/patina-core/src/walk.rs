//! Guards for the recursive walks of code, against what a program can write.
//!
//! - **A cycle.** The reader accepts datum labels anywhere, so a program can
//!   contain a form that contains itself: `#0=(list 1 #0#)`. A walk that
//!   recurses into a form's elements follows that until the stack overflows
//!   and the process aborts (#459). A cycle through a list's *spine* is
//!   [`Heap::spine`]'s to catch; [`OpenNodes`] catches one through an element.
//! - **Depth.** Every stage between the reader and a backend walks code by
//!   recursing on it: the desugarer, the macro expander's marking of what it
//!   substitutes, the quasiquote lowering, the CPS transform, the VM
//!   compiler's passes. Each used a native frame or more per level with no
//!   check, so code nested about 1,000 deep overflowed the main thread's
//!   8 MiB and aborted the process, on both backends, where chibi and Gauche
//!   run it (#617). A walk now calls [`ensure_sufficient_stack`] at its
//!   recursive entry, which moves it to a new segment of stack when this one
//!   runs low, and the desugarer refuses a form nested past
//!   [`MAX_FORM_DEPTH`] with a syntax error a program can catch, as chibi
//!   refuses past its `SEXP_MAX_ANALYZE_DEPTH`. Depth is bounded by that, not
//!   by the stack a thread was given.
//!
//! [`Heap::spine`]: crate::heap::Heap::spine

use crate::tagged_value::TaggedValue;
use std::cell::Cell;

/// The deepest a form may nest, counting the forms macros expand it into: the
/// desugarer refuses one deeper, and so does the macro expander when what it
/// substitutes nests deeper than this (#617).
///
/// A backstop, not a working limit. The walks grow their stack on demand
/// ([`ensure_sufficient_stack`]), so up to here depth costs memory, a few KiB
/// of stack per level, and time: resolving a reference walks the chain of the
/// environments it is inside, and expanding a macro copies what it
/// substitutes, so a walk down `n` nested forms costs `n²`. chibi's own limit is 8,192 analyses
/// deep. Gauche has none, and both abort with a segmentation fault on
/// 100,000 nested forms from a file, where this refuses them.
///
/// A level is a form the desugarer is inside: `(if #t …)` nested 1,000 deep
/// is 1,000 levels, while `(let ((a 1)) …)` is three per `let`, since the
/// macro's expansion `((lambda (a) …) 1)` adds an application and a `lambda`.
pub const MAX_FORM_DEPTH: usize = 10_000;

/// Room a walk must have left on the current stack segment to go on without
/// moving: more than any stage uses between two calls of
/// [`ensure_sufficient_stack`], in an unoptimized build too.
const RED_ZONE: usize = 256 * 1024;

/// The size of each segment [`ensure_sufficient_stack`] moves to. The pages
/// are committed as they are used, and the segment is freed when the walk
/// returns from it.
const STACK_SEGMENT: usize = 4 * 1024 * 1024;

/// Run `f`, on a new segment of native stack if the current one has less than
/// [`RED_ZONE`] left (#617).
///
/// Called at the recursive entry of each walk shaped like the code it walks,
/// so that its depth is limited by [`MAX_FORM_DEPTH`] and memory rather than
/// by the stack the thread was given: the main thread's is 8 MiB, and a
/// thread `std::thread::spawn` starts gets 2 MiB, where nested `let` had
/// aborted from 243 levels. rustc grows its stack the same way, with the same
/// crate, in its own `ensure_sufficient_stack`.
///
/// It is called for every node of every walk, so the check is this thread's
/// [`STACK_FLOOR`] compared with the address of a local, inline; the stack's
/// bounds are read from `stacker` only when that says the walk is near the
/// end, or the floor is not known yet. Nothing on the native stack is part of
/// a Scheme continuation or a GC root, so a walk moving between segments is
/// invisible to both.
#[inline(always)]
pub fn ensure_sufficient_stack<R>(f: impl FnOnce() -> R) -> R {
    if has_stack_room() {
        f()
    } else {
        measure_or_move(f)
    }
}

/// [`ensure_sufficient_stack`] for a node that `nested` says has nodes below
/// it, and `f` as it is for one that has none: a leaf is most of any tree, and
/// a walk goes no deeper from it, so it is not worth the check.
#[inline(always)]
pub fn ensure_sufficient_stack_if<R>(nested: bool, f: impl FnOnce() -> R) -> R {
    if !nested || has_stack_room() {
        f()
    } else {
        measure_or_move(f)
    }
}

/// Whether this thread's stack is known to have more than [`RED_ZONE`] left
/// on the current segment: [`ensure_sufficient_stack`]'s check, for the
/// `Drop` of a code tree, which lets its node's drop glue recurse where this
/// answers yes and hands the node's children to [`drop_deep`] where it does
/// not.
#[inline(always)]
pub fn has_stack_room() -> bool {
    approximate_stack_pointer() > STACK_FLOOR.get()
}

thread_local! {
    /// The lowest the stack pointer may go on the current segment before a
    /// walk moves to a new one: the segment's end plus [`RED_ZONE`].
    /// `usize::MAX` until a walk on this thread first asks.
    static STACK_FLOOR: Cell<usize> = const { Cell::new(usize::MAX) };
}

/// [`ensure_sufficient_stack`] past its [`STACK_FLOOR`]: on a thread that has
/// not measured its stack yet, measure it and go on; on one that is near the
/// end of a segment, move to a new one for `f`.
#[cold]
#[inline(never)]
fn measure_or_move<R>(f: impl FnOnce() -> R) -> R {
    match stacker::remaining_stack() {
        Some(remaining) if remaining > RED_ZONE => {
            set_floor(remaining);
            f()
        }
        _ => {
            // Restored when `f` returns or unwinds: the floor is the segment's,
            // and the segment is freed.
            let _floor = RestoreFloor(STACK_FLOOR.get());
            stacker::grow(STACK_SEGMENT, || {
                set_floor(stacker::remaining_stack().expect("stacker knows the segment it made"));
                f()
            })
        }
    }
}

/// Set [`STACK_FLOOR`] for the segment this runs on, `remaining` bytes from
/// its end.
fn set_floor(remaining: usize) {
    STACK_FLOOR.set(approximate_stack_pointer() - remaining + RED_ZONE);
}

/// The address of a local: within a frame of the stack pointer, which is all
/// a red zone of [`RED_ZONE`] needs.
#[inline(always)]
fn approximate_stack_pointer() -> usize {
    let local = 0u8;
    std::hint::black_box(std::ptr::addr_of!(local)) as usize
}

/// Puts [`STACK_FLOOR`] back when dropped.
struct RestoreFloor(usize);

impl Drop for RestoreFloor {
    fn drop(&mut self) {
        STACK_FLOOR.set(self.0);
    }
}

/// Drop `children`, what a node of a code tree held, on a stack grown for
/// them (#617).
///
/// For the `Drop` of each tree the stages build — `CoreExpr`, its quasiquote
/// templates, `CpsExpr`, the VM compiler's trees. A tree is as deep as the
/// code it came from, and its drop glue recursed once per level unchecked, a
/// few frames each: in an unoptimized build, dropping a tree
/// [`MAX_FORM_DEPTH`] deep overflowed a 2 MiB stack. So each node's `Drop`
/// asks [`has_stack_room`], and where there is none takes its children out
/// and hands them here.
#[cold]
#[inline(never)]
pub fn drop_deep<T>(children: T) {
    ensure_sufficient_stack(move || drop(children));
}

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

    /// How many nodes are open: the depth of the walk, which a walk compares
    /// with [`MAX_FORM_DEPTH`].
    pub fn depth(&self) -> usize {
        self.nodes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenNodes, STACK_FLOOR, ensure_sufficient_stack, has_stack_room};
    use crate::TaggedValue;
    use crate::heap::Heap;

    /// Run `test` on a thread with a 512 KiB stack: room for the test, and
    /// far less than the walks below use.
    fn on_small_stack<R: Send + 'static>(test: impl FnOnce() -> R + Send + 'static) -> R {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(test)
            .expect("spawn")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    }

    /// A walk 20,000 deep with a kibibyte of locals a level: about 20 MiB of
    /// frames.
    fn walk(depth: usize) -> usize {
        ensure_sufficient_stack(|| {
            let locals = std::hint::black_box([1u8; 1024]);
            match depth {
                0 => 0,
                _ => walk(depth - 1) + usize::from(locals[depth % 1024]),
            }
        })
    }

    #[test]
    fn a_walk_goes_deeper_than_its_thread_has_stack() {
        assert_eq!(on_small_stack(|| walk(20_000)), 20_000);
    }

    #[test]
    fn a_walk_that_panics_on_a_new_segment_leaves_the_stack_measured_right() {
        on_small_stack(|| {
            fn panicking(depth: usize) {
                ensure_sufficient_stack(|| {
                    let _locals = std::hint::black_box([0u8; 1024]);
                    if depth == 0 {
                        std::panic::panic_any("the bottom");
                    }
                    panicking(depth - 1);
                })
            }
            assert_eq!(walk(1), 1);
            let floor = STACK_FLOOR.get();
            let unwound = std::panic::catch_unwind(|| panicking(20_000));
            assert!(unwound.is_err());
            // The floor is this stack's again, not that of a segment freed
            // under it, and a walk deeper than the thread still grows.
            assert_eq!(STACK_FLOOR.get(), floor);
            assert!(has_stack_room());
            assert_eq!(walk(20_000), 20_000);
        });
    }

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
