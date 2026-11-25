//! Smoke tests for the patina-vm crate skeleton.
//!
//! These tests verify that the crate structure compiles and the top-level
//! types are accessible. Real VM execution tests are added in milestones A2+.

use patina_core::LinkTable;
use patina_vm::types::{Arity, CodeObject, CodeObjectId, FrameStack};

#[test]
fn code_object_constructs() {
    let code = CodeObject {
        id: CodeObjectId(0),
        name: None,
        instructions: vec![],
        constants: vec![],
        num_regs: 4,
        arity: Arity::Fixed(2),
        source_map: vec![],
        register_roots: None,
        links: LinkTable::empty(),
        live_closures: std::cell::Cell::new(0),
    };
    assert!(code.source_location(0).is_none());
    assert!(matches!(code.arity, Arity::Fixed(2)));
    assert!(code.arity.accepts(2));
    assert!(!code.arity.accepts(1));
}

#[test]
fn variadic_arity_accepts() {
    let a = Arity::Variadic(1);
    assert!(a.accepts(1));
    assert!(a.accepts(5));
    assert!(!a.accepts(0));
}

#[test]
fn a_register_stack_is_clone() {
    // A continuation's snapshot is a clone of the register stack: the
    // frames' headers and registers in one buffer, with the frames' code
    // beside it (#908). Until then a `CallFrame` was the stored record, and
    // its `Clone` was what this pinned.
    let stack = FrameStack::default();
    let _copy: FrameStack = stack.clone();
}
