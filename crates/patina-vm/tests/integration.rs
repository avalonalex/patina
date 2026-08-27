//! Integration tests: compile CoreExpr → CodeObject → execute in VmState.
//!
//! These tests exercise the full pipeline (Passes 1–5 + execution loop) for
//! simple Scheme programs expressed directly as CoreExpr trees.

use patina_core::core_expr::{CoreExpr, CoreExprKind, Formals, ScopedParam};
use patina_core::environment::Environment;
use patina_core::scope::ScopeSet;
use patina_core::tagged_value::TaggedValue;
use patina_vm::compiler::compile;
use patina_vm::runtime::{VmState, execute};
use std::rc::Rc;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn fresh_state() -> VmState {
    let mut state = VmState::new(Rc::new(Environment::new()));
    state.install_primitives();
    state
}

fn run(expr: CoreExpr) -> TaggedValue {
    let (top, nested) = compile(&expr).expect("compile error");
    let top_id = top.id;
    let mut state = fresh_state();
    state.load(top);
    state.load_all(nested);
    execute(&mut state, top_id).expect("execution error")
}

fn var(name: &str) -> CoreExpr {
    CoreExpr::new(CoreExprKind::Var {
        name: Rc::from(name),
        scopes: ScopeSet::new(),
    })
}

fn lit(n: i64) -> CoreExpr {
    CoreExpr::new(CoreExprKind::Literal(TaggedValue::fixnum(n)))
}

fn lit_bool(b: bool) -> CoreExpr {
    CoreExpr::new(CoreExprKind::Literal(if b {
        TaggedValue::TRUE
    } else {
        TaggedValue::FALSE
    }))
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
                .iter()
                .map(|n| ScopedParam::simple(Rc::from(*n)))
                .collect(),
        ),
        body,
        binding_scopes: Default::default(),
    })
}

fn begin(exprs: Vec<CoreExpr>) -> CoreExpr {
    CoreExpr::new(CoreExprKind::Begin(exprs))
}

fn if_(test: CoreExpr, then: CoreExpr, else_: CoreExpr) -> CoreExpr {
    CoreExpr::new(CoreExprKind::If {
        test: Rc::new(test),
        then: Rc::new(then),
        else_: Rc::new(else_),
    })
}

fn define(name: &str, value: CoreExpr) -> CoreExpr {
    CoreExpr::new(CoreExprKind::Define {
        scopes: ScopeSet::new(),
        name: Rc::from(name),
        value: Rc::new(value),
    })
}

fn set(name: &str, value: CoreExpr) -> CoreExpr {
    CoreExpr::new(CoreExprKind::Set {
        var: Rc::from(name),
        scopes: ScopeSet::new(),
        value: Rc::new(value),
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn literal_fixnum() {
    let result = run(lit(42));
    assert_eq!(result.as_fixnum(), Some(42));
}

#[test]
fn literal_true() {
    let result = run(lit_bool(true));
    assert_eq!(result, TaggedValue::TRUE);
}

#[test]
fn literal_false() {
    let result = run(lit_bool(false));
    assert_eq!(result, TaggedValue::FALSE);
}

#[test]
fn begin_returns_last() {
    // (begin 1 2 3)  →  3
    let result = run(begin(vec![lit(1), lit(2), lit(3)]));
    assert_eq!(result.as_fixnum(), Some(3));
}

#[test]
fn if_true_branch() {
    // (if #t 1 2)  →  1
    let result = run(if_(lit_bool(true), lit(1), lit(2)));
    assert_eq!(result.as_fixnum(), Some(1));
}

#[test]
fn if_false_branch() {
    // (if #f 1 2)  →  2
    let result = run(if_(lit_bool(false), lit(1), lit(2)));
    assert_eq!(result.as_fixnum(), Some(2));
}

#[test]
fn lambda_creates_closure() {
    // (lambda () 42) — should compile and return a closure object
    let expr = lambda(vec![], vec![lit(42)]);
    let (top, nested) = compile(&expr).expect("compile");
    let top_id = top.id;
    let mut state = fresh_state();
    state.load(top);
    state.load_all(nested);
    let val = execute(&mut state, top_id).expect("execute");
    // Result should be a procedure (VM closure).
    assert!(val.is_object(), "expected closure object, got {:?}", val);
}

#[test]
fn call_lambda() {
    // ((lambda () 99))  →  99
    let thunk = lambda(vec![], vec![lit(99)]);
    let expr = app(thunk, vec![]);
    let result = run(expr);
    assert_eq!(result.as_fixnum(), Some(99));
}

#[test]
fn lambda_with_arg() {
    // ((lambda (x) x) 7)  →  7
    let lam = lambda(vec!["x"], vec![var("x")]);
    let expr = app(lam, vec![lit(7)]);
    let result = run(expr);
    assert_eq!(result.as_fixnum(), Some(7));
}

#[test]
fn nested_call() {
    // ((lambda (x) ((lambda (y) y) x)) 5)  →  5
    let inner = lambda(vec!["y"], vec![var("y")]);
    let outer = lambda(vec!["x"], vec![app(inner, vec![var("x")])]);
    let expr = app(outer, vec![lit(5)]);
    let result = run(expr);
    assert_eq!(result.as_fixnum(), Some(5));
}

#[test]
fn closure_captures_free_var() {
    // ((lambda (x) ((lambda () x))) 42)  →  42
    // The inner thunk captures x from the outer lambda.
    let inner = lambda(vec![], vec![var("x")]);
    let outer = lambda(vec!["x"], vec![app(inner, vec![])]);
    let expr = app(outer, vec![lit(42)]);
    let result = run(expr);
    assert_eq!(result.as_fixnum(), Some(42));
}

#[test]
fn global_define_and_load() {
    // (begin (define x 10) x)  →  10
    let expr = begin(vec![define("x", lit(10)), var("x")]);
    let result = run(expr);
    assert_eq!(result.as_fixnum(), Some(10));
}

#[test]
fn if_with_var_condition() {
    // (begin (define b #t) (if b 1 2))  →  1
    let expr = begin(vec![
        define("b", lit_bool(true)),
        if_(var("b"), lit(1), lit(2)),
    ]);
    let result = run(expr);
    assert_eq!(result.as_fixnum(), Some(1));
}

#[test]
fn tail_call_self_recursion() {
    // Tail-recursive countdown using a global:
    //
    // (define (loop n) (if (= n 0) 99 (loop (- n 1))))
    // but since we don't have primitives yet, use a manual countdown:
    //
    // ((lambda (f) (f f 5)) (lambda (self n) (if n (self self n) 99)))
    //
    // Without tail calls this overflows; with TCO it stays O(1) stack.
    //
    // We can't express `(- n 1)` without primitives, so use a simpler
    // convergent test: the lambda always returns #f (base case always fires).
    //
    // (if #f (self self 0) 42)  →  42  (no recursion, just validates the path)
    let self_sym = Rc::<str>::from("self");
    let n_sym = Rc::<str>::from("n");

    let body = if_(
        CoreExpr::new(CoreExprKind::Literal(TaggedValue::FALSE)),
        app(
            CoreExpr::new(CoreExprKind::Var {
                name: self_sym.clone(),
                scopes: ScopeSet::new(),
            }),
            vec![
                CoreExpr::new(CoreExprKind::Var {
                    name: self_sym.clone(),
                    scopes: ScopeSet::new(),
                }),
                CoreExpr::new(CoreExprKind::Var {
                    name: n_sym.clone(),
                    scopes: ScopeSet::new(),
                }),
            ],
        ),
        lit(42),
    );
    let inner = CoreExpr::new(CoreExprKind::Lambda {
        params: Formals::Fixed(vec![
            ScopedParam::simple(self_sym.clone()),
            ScopedParam::simple(n_sym.clone()),
        ]),
        body: vec![body],
        binding_scopes: Default::default(),
    });

    // ((lambda (f) (f f 0)) inner)
    let f_sym = Rc::<str>::from("f");
    let outer_body = app(
        CoreExpr::new(CoreExprKind::Var {
            name: f_sym.clone(),
            scopes: ScopeSet::new(),
        }),
        vec![
            CoreExpr::new(CoreExprKind::Var {
                name: f_sym.clone(),
                scopes: ScopeSet::new(),
            }),
            lit(0),
        ],
    );
    let outer = CoreExpr::new(CoreExprKind::Lambda {
        params: Formals::Fixed(vec![ScopedParam::simple(f_sym)]),
        body: vec![outer_body],
        binding_scopes: Default::default(),
    });
    let expr = app(outer, vec![inner]);
    let result = run(expr);
    assert_eq!(result.as_fixnum(), Some(42));
}

#[test]
fn set_global() {
    // (begin (define x 1) (set! x 99) x)  →  99
    let expr = begin(vec![define("x", lit(1)), set("x", lit(99)), var("x")]);
    let result = run(expr);
    assert_eq!(result.as_fixnum(), Some(99));
}

#[test]
fn mutated_captured_var() {
    // (lambda (x) (set! x 42) (lambda () x))
    // ((outer 0))  →  42
    //
    // x is mutated AND captured → must be MutableCell-boxed.
    let inner = lambda(vec![], vec![var("x")]);
    let outer = lambda(vec!["x"], vec![set("x", lit(42)), inner]);
    // ((outer 0)) — call inner thunk returned by outer
    let call_outer = app(outer, vec![lit(0)]);
    let call_inner = app(call_outer, vec![]);
    let result = run(call_inner);
    assert_eq!(result.as_fixnum(), Some(42));
}

// ─────────────────────────────────────────────────────────────────────────────
// Primitive wiring tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn primitive_addition() {
    // (+ 3 4)  →  7
    let expr = app(var("+"), vec![lit(3), lit(4)]);
    let result = run(expr);
    assert_eq!(result.as_fixnum(), Some(7));
}

#[test]
fn primitive_in_lambda() {
    // ((lambda (x) (+ x 1)) 41)  →  42
    let lam = lambda(vec!["x"], vec![app(var("+"), vec![var("x"), lit(1)])]);
    let expr = app(lam, vec![lit(41)]);
    let result = run(expr);
    assert_eq!(result.as_fixnum(), Some(42));
}

#[test]
fn primitive_conditional() {
    // (if (= 1 1) 42 0)  →  42
    let cond = app(var("="), vec![lit(1), lit(1)]);
    let expr = if_(cond, lit(42), lit(0));
    let result = run(expr);
    assert_eq!(result.as_fixnum(), Some(42));
}

#[test]
fn tail_recursive_countdown() {
    // (define (loop n acc) (if (= n 0) acc (loop (- n 1) (+ acc 1))))
    // (loop 100 0)  →  100
    //
    // Uses proper tail calls + primitives; blows up if TCO is broken.
    let loop_body = if_(
        app(var("="), vec![var("n"), lit(0)]),
        var("acc"),
        app(
            var("loop"),
            vec![
                app(var("-"), vec![var("n"), lit(1)]),
                app(var("+"), vec![var("acc"), lit(1)]),
            ],
        ),
    );
    let loop_fn = lambda(vec!["n", "acc"], vec![loop_body]);
    let expr = begin(vec![
        define("loop", loop_fn),
        app(var("loop"), vec![lit(100), lit(0)]),
    ]);
    let result = run(expr);
    assert_eq!(result.as_fixnum(), Some(100));
}
