//! Smoke tests for the patina-vm crate skeleton.
//!
//! These tests verify that the crate structure compiles and the top-level
//! types are accessible. Real VM execution tests are added in milestones A2+.

use patina_vm::types::{Arity, CallFrame, CodeObject, CodeObjectId, GlobalCacheEntry};

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
        global_cache: GlobalCacheEntry::table(&[]),
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
fn call_frame_is_clone() {
    // CallFrame MUST be Clone — continuations snapshot the entire frame stack.
    let code = std::rc::Rc::new(CodeObject {
        id: CodeObjectId(0),
        name: None,
        instructions: vec![],
        constants: vec![],
        num_regs: 8,
        arity: Arity::Fixed(0),
        source_map: vec![],
        global_cache: GlobalCacheEntry::table(&[]),
    });
    let frame = CallFrame {
        pc: 42,
        register_base: 0,
        num_regs: 8,
        closure: None,
        return_reg: 3,
        code,
    };
    let cloned = frame.clone();
    assert_eq!(cloned.pc, 42);
    assert_eq!(cloned.return_reg, 3);
}

#[test]
fn vm_state_basic() {
    use patina_core::environment::Environment;
    use patina_vm::runtime::VmState;
    use std::rc::Rc;

    let globals = Rc::new(Environment::new());
    let mut state = VmState::new(globals);

    let base = state.alloc_registers(4);
    assert_eq!(base, 0);
    assert_eq!(state.registers.len(), 4);

    state.free_top_registers(0);
    assert_eq!(state.registers.len(), 0);
}
