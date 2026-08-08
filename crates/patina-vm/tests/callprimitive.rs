//! Compile-time `CallPrimitive` emission tests (Track P P2).
//!
//! Verifies that pass 5 emits `CallPrimitive` exactly when the callee is a
//! `GlobalRef` that resolves to a registry primitive in the compile
//! environment — and keeps the generic `Call` path everywhere else.

use patina_core::core_expr::{CoreExpr, CoreExprKind, Formals, ScopedParam};
use patina_core::environment::Environment;
use patina_core::procedure::Arity;
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
                .map(|p| ScopedParam::simple(Rc::from(p)))
                .collect(),
        ),
        body,
        binding_scope: None,
    })
}

/// Compile `expr` against `state`'s globals and return every instruction
/// from the top-level and nested code objects.
fn compile_all_in(state: &VmState, expr: &CoreExpr) -> Vec<Instruction> {
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

/// Compile `expr` against a fresh VM's globals (primitives installed).
fn compile_all_instructions(expr: &CoreExpr) -> Vec<Instruction> {
    let mut state = VmState::new(Rc::new(Environment::new()));
    state.install_primitives();
    compile_all_in(&state, expr)
}

/// Fresh VM state where `name` is bound the way the library loader binds it:
/// a `Procedure::Primitive` whose qualified name comes from `library`, not
/// from the registry entry.
fn state_with_library_binding(name: &'static str, arity: Arity, library: &[&str]) -> VmState {
    let mut state = VmState::new(Rc::new(Environment::new()));
    state.install_primitives();
    state
        .globals
        .define_primitive(name, arity, library.iter().map(|s| s.to_string()).collect());
    state
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
    // The P2 property under test: no generic Call and no callee LoadGlobal.
    // (Which inline opcode the site gets is pinned by
    // two_arg_add_emits_inline_opcode.)
    let instrs = compile_all_instructions(&app(var("+"), vec![lit(1), lit(2)]));
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
    // (lambda (p) (car p)) — body in tail position. Since P3 the body is the
    // inline Car opcode followed by Return; no generic TailCall.
    let expr = lambda(vec!["p"], vec![app(var("car"), vec![var("p")])]);
    let instrs = compile_all_instructions(&expr);
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::Car { .. })),
        1,
        "{instrs:?}"
    );
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
    let state =
        state_with_library_binding("values", Arity::Min(0), &["patina", "internal", "control"]);
    let instrs = compile_all_in(&state, &app(var("values"), vec![lit(1), lit(2)]));
    assert_eq!(count_call_prims(&instrs), 0, "{instrs:?}");
    assert_eq!(count_generic_calls(&instrs), 1, "{instrs:?}");
}

#[test]
fn lexically_bound_callee_keeps_generic_call() {
    // ((lambda (car) (car 5)) some-proc) — inner car is a parameter, not a
    // global; the call must go through the generic path.
    let inner = lambda(vec!["car"], vec![app(var("car"), vec![lit(5)])]);
    let instrs = compile_all_instructions(&app(inner, vec![var("some-user-proc")]));
    assert_eq!(count_call_prims(&instrs), 0, "{instrs:?}");
}

// ── Inline opcode emission (Track P P3) ──────────────────────────────────────

fn count_matching(instrs: &[Instruction], pred: fn(&Instruction) -> bool) -> usize {
    instrs.iter().filter(|i| pred(i)).count()
}

#[test]
fn two_arg_add_emits_inline_opcode() {
    let instrs = compile_all_instructions(&app(var("+"), vec![lit(1), lit(2)]));
    assert_eq!(
        count_matching(&instrs, |i| matches!(
            i,
            Instruction::Add { .. } | Instruction::AddImm { .. }
        )),
        1,
        "{instrs:?}"
    );
    assert_eq!(count_call_prims(&instrs), 0, "{instrs:?}");
}

#[test]
fn three_arg_add_stays_on_call_primitive() {
    let instrs = compile_all_instructions(&app(var("+"), vec![lit(1), lit(2), lit(3)]));
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::Add { .. })),
        0,
        "{instrs:?}"
    );
    assert_eq!(count_call_prims(&instrs), 1, "{instrs:?}");
}

#[test]
fn unary_car_emits_inline_opcode() {
    let expr = lambda(vec!["p"], vec![app(var("car"), vec![var("p")])]);
    let instrs = compile_all_instructions(&expr);
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::Car { .. })),
        1,
        "{instrs:?}"
    );
}

#[test]
fn vector_set_emits_inline_opcode() {
    let expr = app(var("vector-set!"), vec![var("some-vec"), lit(0), lit(9)]);
    let instrs = compile_all_instructions(&expr);
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::VectorSet { .. })),
        1,
        "{instrs:?}"
    );
}

#[test]
fn internal_library_alias_binding_emits_inline_opcode() {
    // In the real system the stdlib binds `<` via the library loader as
    // patina.internal.numbers/< (see internal_numbers.rs), which resolves to
    // the registry's scheme.base/< entry only through the short-name
    // fallback. The inline opcode must key on the registry's canonical name,
    // not the binding's alias — this is the exact shape every program that
    // imports (scheme base) sees.
    let state = state_with_library_binding("<", Arity::Min(2), &["patina", "internal", "numbers"]);
    let instrs = compile_all_in(&state, &app(var("<"), vec![lit(1), lit(2)]));
    assert_eq!(
        count_matching(&instrs, |i| matches!(
            i,
            Instruction::Lt { .. } | Instruction::LtImm { .. }
        )),
        1,
        "{instrs:?}"
    );
    assert_eq!(count_call_prims(&instrs), 0, "{instrs:?}");
}

#[test]
fn not_emits_inline_opcode_via_library_binding() {
    // `not` moved from a Scheme definition (base/lists.scm) into the
    // registry; the stdlib binds it via the library loader as
    // patina.internal.predicates/not. It must compile to the inline `Not`
    // opcode, not a generic closure call — this was 40% of tak's runtime
    // while `not` was Scheme-defined.
    let state = state_with_library_binding(
        "not",
        Arity::Exact(1),
        &["patina", "internal", "predicates"],
    );
    let instrs = compile_all_in(&state, &app(var("not"), vec![lit(1)]));
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::Not { .. })),
        1,
        "{instrs:?}"
    );
    assert_eq!(count_call_prims(&instrs), 0, "{instrs:?}");
    assert_eq!(count_generic_calls(&instrs), 0, "{instrs:?}");
}

#[test]
fn cxr_composition_emits_call_primitive_via_library_binding() {
    // The car/cdr compositions (cadr, cddr, ...) also moved into the
    // registry; they have no inline opcode but must at least compile to
    // CallPrimitive (no frame push) instead of a generic closure call.
    let state =
        state_with_library_binding("cadr", Arity::Exact(1), &["patina", "internal", "lists"]);
    let instrs = compile_all_in(&state, &app(var("cadr"), vec![lit(1)]));
    assert_eq!(count_call_prims(&instrs), 1, "{instrs:?}");
    assert_eq!(count_generic_calls(&instrs), 0, "{instrs:?}");
}

#[test]
fn wrong_arity_car_stays_on_call_primitive() {
    // (car x y) is an arity error — it must reach the handler to raise it.
    let instrs = compile_all_instructions(&app(var("car"), vec![lit(1), lit(2)]));
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::Car { .. })),
        0,
        "{instrs:?}"
    );
    assert_eq!(count_call_prims(&instrs), 1, "{instrs:?}");
}

// ── Operand and branch fusion (Track P P5) ───────────────────────────────────

fn if_(test: CoreExpr, then: CoreExpr, else_: CoreExpr) -> CoreExpr {
    CoreExpr::new(CoreExprKind::If {
        test: Rc::new(test),
        then: Rc::new(then),
        else_: Rc::new(else_),
    })
}

#[test]
fn local_operands_read_in_place() {
    // `(lambda (x y) (< y x))`: both operands are locals, so the inline Lt
    // reads the parameter registers directly — no staging Moves at all.
    let instrs = compile_all_instructions(&lambda(
        vec!["x", "y"],
        vec![app(var("<"), vec![var("y"), var("x")])],
    ));
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::Lt { .. })),
        1,
        "{instrs:?}"
    );
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::Move { .. })),
        0,
        "{instrs:?}"
    );
}

#[test]
fn literal_right_operand_absorbed() {
    // `(- x 1)` absorbs the literal: SubImm, and no LoadImmediate remains.
    let instrs = compile_all_instructions(&lambda(
        vec!["x"],
        vec![app(var("-"), vec![var("x"), lit(1)])],
    ));
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::SubImm { .. })),
        1,
        "{instrs:?}"
    );
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::LoadImmediate { .. })),
        0,
        "{instrs:?}"
    );
}

#[test]
fn literal_left_operand_stays_registered() {
    // `(- 1 x)`: subtraction order must survive the deopt path exactly, so
    // a left literal is materialized and the register form is emitted.
    let instrs = compile_all_instructions(&lambda(
        vec!["x"],
        vec![app(var("-"), vec![lit(1), var("x")])],
    ));
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::Sub { .. })),
        1,
        "{instrs:?}"
    );
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::SubImm { .. })),
        0,
        "{instrs:?}"
    );
}

#[test]
fn not_condition_fuses_into_branch() {
    // `(if (not (< y x)) x y)` fuses the Not into NotJumpUnless; the plain
    // JumpUnless stays right behind it as the shadowed-`not` deopt landing.
    let instrs = compile_all_instructions(&lambda(
        vec!["x", "y"],
        vec![if_(
            app(var("not"), vec![app(var("<"), vec![var("y"), var("x")])]),
            var("x"),
            var("y"),
        )],
    ));
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::NotJumpUnless { .. })),
        1,
        "{instrs:?}"
    );
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::Not { .. })),
        0,
        "{instrs:?}"
    );
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::JumpUnless { .. })),
        1,
        "{instrs:?}"
    );
}

#[test]
fn effectful_arg_falls_back_to_staged_temps() {
    // `(+ x (begin (set! x 99) 1))`: a non-atomic argument forces the whole
    // call back onto staged temps so left-to-right evaluation order (and
    // the value of x the add sees) is unchanged from the unoptimized form.
    let set_x = CoreExpr::new(CoreExprKind::Set {
        var: Rc::from("x"),
        scopes: Default::default(),
        value: Rc::new(lit(99)),
    });
    let instrs = compile_all_instructions(&lambda(
        vec!["x"],
        vec![app(
            var("+"),
            vec![
                var("x"),
                CoreExpr::new(CoreExprKind::Begin(vec![set_x, lit(1)])),
            ],
        )],
    ));
    // x is a mutated local, so it lives in a cell: its staged read must
    // come BEFORE the begin's cell write, and the Add must consume that
    // staged temp (register form — the literal is not absorbed either).
    let read_at = instrs
        .iter()
        .position(|i| matches!(i, Instruction::ReadCell { .. }))
        .expect("staged ReadCell");
    let write_at = instrs
        .iter()
        .position(|i| matches!(i, Instruction::WriteCell { .. }))
        .expect("WriteCell for set!");
    assert!(read_at < write_at, "{instrs:?}");
    let Some(Instruction::ReadCell { dst: staged, .. }) = instrs.get(read_at) else {
        unreachable!()
    };
    assert!(
        instrs
            .iter()
            .any(|i| matches!(i, Instruction::Add { a, .. } if a == staged)),
        "{instrs:?}"
    );
    assert_eq!(
        count_matching(&instrs, |i| matches!(i, Instruction::AddImm { .. })),
        0,
        "{instrs:?}"
    );
}
