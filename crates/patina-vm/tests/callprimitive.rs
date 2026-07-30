//! Compile-time `CallPrimitive` emission tests (Track P P2).
//!
//! Verifies that pass 5 emits `CallPrimitive` exactly when the callee is a
//! `GlobalRef` that resolves to a registry primitive in the compile
//! environment — and keeps the generic `Call` path everywhere else.

use patina_core::core_expr::{CoreExpr, CoreExprKind, Formals, ScopedParam};
use patina_core::environment::Environment;
use patina_core::tagged_value::TaggedValue;
use patina_vm::compiler::compile_with_qq_resolving;
use patina_vm::runtime::VmState;
use patina_vm::types::instruction::Instruction;
use std::rc::Rc;

fn var(name: &str) -> CoreExpr {
    CoreExpr::new(CoreExprKind::Var {
        name: Rc::from(name),
        scopes: Default::default(),
    })
}

fn lit(n: i64) -> CoreExpr {
    CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(n)))
}

fn app(func: CoreExpr, args: Vec<CoreExpr>) -> CoreExpr {
    CoreExpr::new(CoreExprKind::App {
        func: Rc::new(func),
        args,
    })
}

fn lambda(params: Vec<&str>, body: Vec<CoreExpr>) -> CoreExpr {
    CoreExpr::new(CoreExprKind::Lambda {
        params: Formals::Fixed(
            params
                .into_iter()
                .map(|p| ScopedParam {
                    name: Rc::from(p),
                    scopes: Default::default(),
                })
                .collect(),
        ),
        body,
        binding_scope: None,
    })
}

/// Compile `expr` against a fresh VM's globals (primitives installed) and
/// return every instruction from the top-level and nested code objects.
fn compile_all_instructions(expr: &CoreExpr) -> Vec<Instruction> {
    let mut state = VmState::new(Rc::new(Environment::new()));
    state.install_primitives();
    let env = state.globals.clone();
    let heap = env.heap().clone();
    let (top, nested) = compile_with_qq_resolving(expr, &heap, &env, &state.primitive_registry)
        .expect("compile error");
    let mut instrs = top.instructions;
    for co in nested {
        instrs.extend(co.instructions);
    }
    instrs
}

fn count_call_prims(instrs: &[Instruction]) -> usize {
    instrs
        .iter()
        .filter(|i| matches!(i, Instruction::CallPrimitive { .. }))
        .count()
}

fn count_generic_calls(instrs: &[Instruction]) -> usize {
    instrs
        .iter()
        .filter(|i| matches!(i, Instruction::Call { .. } | Instruction::TailCall { .. }))
        .count()
}

#[test]
fn add_emits_call_primitive() {
    let instrs = compile_all_instructions(&app(var("+"), vec![lit(1), lit(2)]));
    assert_eq!(count_call_prims(&instrs), 1, "{instrs:?}");
    assert_eq!(count_generic_calls(&instrs), 0, "{instrs:?}");
    // The callee is never loaded — no LoadGlobal for "+".
    assert!(
        !instrs
            .iter()
            .any(|i| matches!(i, Instruction::LoadGlobal { name, .. } if &**name == "+")),
        "{instrs:?}"
    );
}

#[test]
fn tail_primitive_emits_call_primitive_and_return() {
    // (lambda (p) (car p)) — body in tail position.
    let expr = lambda(vec!["p"], vec![app(var("car"), vec![var("p")])]);
    let instrs = compile_all_instructions(&expr);
    assert_eq!(count_call_prims(&instrs), 1, "{instrs:?}");
    assert_eq!(count_generic_calls(&instrs), 0, "{instrs:?}");
}

#[test]
fn unknown_callee_keeps_generic_call() {
    let instrs = compile_all_instructions(&app(var("some-user-proc"), vec![lit(1)]));
    assert_eq!(count_call_prims(&instrs), 0, "{instrs:?}");
    assert_eq!(count_generic_calls(&instrs), 1, "{instrs:?}");
}

#[test]
fn control_primitives_are_excluded() {
    // In the real system `values` binds via the library loader as
    // patina.internal.control/values (see internal_control.rs), and the VM
    // intercepts that qualified name at call time. Bind it the same way here
    // and check the resolver refuses to emit CallPrimitive for it.
    let mut state = VmState::new(Rc::new(Environment::new()));
    state.install_primitives();
    let env = state.globals.clone();
    env.define_primitive(
        "values",
        patina_core::procedure::Arity::Min(0),
        vec![
            "patina".to_string(),
            "internal".to_string(),
            "control".to_string(),
        ],
    );
    let heap = env.heap().clone();
    let expr = app(var("values"), vec![lit(1), lit(2)]);
    let (top, _) = compile_with_qq_resolving(&expr, &heap, &env, &state.primitive_registry)
        .expect("compile error");
    assert_eq!(
        count_call_prims(&top.instructions),
        0,
        "{:?}",
        top.instructions
    );
    assert_eq!(
        count_generic_calls(&top.instructions),
        1,
        "{:?}",
        top.instructions
    );
}

#[test]
fn lexically_bound_callee_keeps_generic_call() {
    // ((lambda (car) (car 5)) some-proc) — inner car is a parameter, not a
    // global; the call must go through the generic path.
    let inner = lambda(vec!["car"], vec![app(var("car"), vec![lit(5)])]);
    let instrs = compile_all_instructions(&app(inner, vec![var("some-user-proc")]));
    assert_eq!(count_call_prims(&instrs), 0, "{instrs:?}");
}
