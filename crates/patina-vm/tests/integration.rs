//! Integration tests: compile CoreExpr → CodeObject → execute in VmState.
//!
//! These tests exercise the full pipeline (Passes 1–5 + execution loop) for
//! simple Scheme programs expressed directly as CoreExpr trees.
#![allow(
    clippy::disallowed_methods,
    reason = "a test crate: it runs compiled code with `execute` from outside any loop, as the \
              backend's top level does, so it allows the re-entry rule (#622) once, as the \
              test crates do in their Cargo.toml"
)]

use patina_core::Lit;
use patina_core::core_expr::{CoreExpr, CoreExprKind, Formals, ScopedParam};
use patina_core::scope::ScopeSet;
use patina_core::tagged_value::TaggedValue;
use patina_vm::runtime::{LoadedUnit, VmState, execute};
use patina_vm::types::CodeObject;
use patina_vm::types::CodeObjectId;
use std::rc::Rc;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// `patina_vm::compiler::compile`, for these hand-built trees, whose
/// literals are all immediates: none needs a pool entry (#705), so the pool
/// is an empty one on a heap of its own.
fn compile(
    expr: &CoreExpr,
) -> Result<(CodeObject, Vec<CodeObject>), patina_vm::error::CompileError> {
    let heap = patina_core::new_shared_heap();
    let literals = patina_core::LiteralPool::new(&heap.borrow());
    patina_vm::compiler::compile(expr, &literals)
}

fn fresh_state() -> VmState {
    let mut state = VmState::on_a_fresh_heap();
    state.install_primitives();
    state
}

/// Load `top` and `nested` as a unit linked in the machine's globals, where
/// `execute` runs it, as the backend's top level loads a program's form.
fn load(state: &mut VmState, top: CodeObject, nested: Vec<CodeObject>) -> LoadedUnit {
    let globals = Rc::clone(state.global_env());
    state.load_unit(top, nested, &globals)
}

fn run(expr: CoreExpr) -> TaggedValue {
    let (top, nested) = compile(&expr).expect("compile error");
    let mut state = fresh_state();
    let top_id = load(&mut state, top, nested);
    execute(&mut state, top_id).expect("execution error")
}

fn var(name: &str) -> CoreExpr {
    CoreExpr::new(CoreExprKind::Var {
        name: Rc::from(name),
        scopes: ScopeSet::new(),
    })
}

fn lit(n: i64) -> CoreExpr {
    CoreExpr::new(CoreExprKind::Literal(Lit::immediate(TaggedValue::fixnum(
        n,
    ))))
}

fn lit_bool(b: bool) -> CoreExpr {
    CoreExpr::new(CoreExprKind::Literal(Lit::immediate(if b {
        TaggedValue::TRUE
    } else {
        TaggedValue::FALSE
    })))
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
        binding_scopes: std::rc::Rc::new(patina_core::ScopeSet::new()),
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
    let mut state = fresh_state();
    let top_id = load(&mut state, top, nested);
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
        CoreExpr::new(CoreExprKind::Literal(Lit::immediate(TaggedValue::FALSE))),
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
        binding_scopes: std::rc::Rc::new(patina_core::ScopeSet::new()),
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
        binding_scopes: std::rc::Rc::new(patina_core::ScopeSet::new()),
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

// ─────────────────────────────────────────────────────────────────────────────
// Code store slots (#352)
// ─────────────────────────────────────────────────────────────────────────────

/// Load `expr` as a unit and run it, as the backend runs a top-level form.
fn load_and_run(state: &mut VmState, expr: &CoreExpr) -> (CodeObjectId, TaggedValue) {
    let (top, nested) = compile(expr).expect("compile error");
    let unit = load(state, top, nested);
    let id = unit.id();
    let value = execute(state, unit).expect("execution error");
    (id, value)
}

#[test]
fn a_slot_let_go_is_given_to_the_next_code_loaded() {
    let mut state = fresh_state();
    let (first, _) = load_and_run(&mut state, &lit(1));
    state.release_unit_if_unused(first);
    let (second, value) = load_and_run(&mut state, &app(var("+"), vec![lit(20), lit(22)]));
    assert_eq!(value.as_fixnum(), Some(42));
    assert_eq!(second.index(), first.index());
    assert_eq!(second.generation(), first.generation() + 1);
}

/// Loading gives a unit's code whichever slots are free, and a closure is
/// made of the code its `MakeClosure` names once loading has replaced the
/// label the compiler named it by.
#[test]
fn a_closure_made_by_code_in_reused_slots_runs_its_own_code() {
    let mut state = fresh_state();
    let loaded: Vec<CodeObjectId> = (0..4)
        .map(|n| load_and_run(&mut state, &lit(n)).0)
        .collect();
    for &id in &loaded {
        state.release_unit_if_unused(id);
    }
    // (define (add x) ((lambda (y) (+ x y)) 2)) (add 40)
    let add = lambda(
        vec!["x"],
        vec![app(
            lambda(vec!["y"], vec![app(var("+"), vec![var("x"), var("y")])]),
            vec![lit(2)],
        )],
    );
    let program = begin(vec![define("add", add), app(var("add"), vec![lit(40)])]);
    let (id, value) = load_and_run(&mut state, &program);
    assert_eq!(value.as_fixnum(), Some(42));
    assert_eq!(id.generation(), 1, "{id:?} was not given a slot let go");
}

/// Run `expr`, which must fail, and answer the error.
fn run_err(expr: CoreExpr) -> patina_vm::error::VmError {
    let (top, nested) = compile(&expr).expect("compile error");
    let mut state = fresh_state();
    let top_id = load(&mut state, top, nested);
    execute(&mut state, top_id).expect_err("ran without an error")
}

/// The name an unbound-variable error reports, read from the unit's link
/// table (#836): each program names other globals first, so an operand that
/// read the wrong entry would report another name.
#[test]
fn an_unbound_global_is_reported_by_its_operands_name() {
    use patina_vm::error::VmError;
    let unbound = |error: VmError| match error {
        VmError::UnboundVariable { name } => name.to_string(),
        other => panic!("not an unbound variable: {other:?}"),
    };
    // (begin (define x 1) (define y x) never-read)
    let read = begin(vec![
        define("x", lit(1)),
        define("y", var("x")),
        var("never-read"),
    ]);
    assert_eq!(unbound(run_err(read)), "never-read");
    // (begin (define x 1) (set! never-set x))
    let write = begin(vec![define("x", lit(1)), set("never-set", var("x"))]);
    assert_eq!(unbound(run_err(write)), "never-set");
}

/// A shadowed primitive's site looks up the name its own operand names:
/// here a name nothing binds, so the deoptimization reports that name, not
/// the primitive's (#836). Each of the three kinds of site that hand their
/// operand to the deoptimization is run: a `CallPrimitive`, an inline
/// opcode and a fused test. Built by hand, since compiled code resolves a
/// site's primitive from the binding its operand names, which a rebinding
/// never removes.
#[test]
fn a_shadowed_primitives_site_looks_up_its_operands_name() {
    use patina_core::LinkTable;
    use patina_vm::error::VmError;
    use patina_vm::types::Arity;
    use patina_vm::types::instruction::{Instruction, PrimitiveFnId, TestOp};

    // Runs `(define <primitive> 1)`, which shadows the primitive, then the
    // site `site` builds for it, whose operand is link 1, then `Return r1`,
    // in a unit whose table is `[<primitive>, never-bound]`.
    let run_site = |primitive: &str, site: &dyn Fn(PrimitiveFnId) -> Vec<Instruction>| {
        let mut state = fresh_state();
        let func_id = state
            .primitive_registry()
            .resolve_index(&format!("scheme.base/{primitive}"))
            .unwrap_or_else(|| panic!("`{primitive}` is registered"));
        let mut instructions = vec![
            Instruction::LoadImmediate {
                dst: 0,
                val: TaggedValue::fixnum(1),
            },
            // (define <primitive> 1): shadowed from here on.
            Instruction::Define { link: 0, src: 0 },
        ];
        instructions.extend(site(PrimitiveFnId(func_id as u32)));
        instructions.push(Instruction::Return { val: 1 });
        let names: Vec<Rc<str>> = vec![Rc::from(primitive), Rc::from("never-bound")];
        let code = CodeObject {
            id: CodeObjectId::label(),
            name: None,
            links: Rc::new(LinkTable::new(names.into_boxed_slice())),
            instructions,
            constants: Vec::new(),
            num_regs: 2,
            arity: Arity::Fixed(0),
            source_map: Vec::new(),
            register_roots: None,
            live_closures: std::cell::Cell::new(0),
        };
        let id = load(&mut state, code, Vec::new());
        match execute(&mut state, id) {
            Err(VmError::UnboundVariable { name }) => {
                assert_eq!(&*name, "never-bound", "at a site of `{primitive}`")
            }
            other => panic!("at a site of `{primitive}`, not an unbound variable: {other:?}"),
        }
    };
    run_site("car", &|func_id| {
        vec![Instruction::CallPrimitive {
            func_id,
            link: 1,
            args: vec![0],
            dst: 1,
        }]
    });
    run_site("car", &|func_id| {
        vec![Instruction::Car {
            src: 0,
            dst: 1,
            func_id,
            link: 1,
        }]
    });
    // The fused test at pc 2, its kept `JumpUnless` at 3, both to the
    // `Return` at 4.
    run_site("null?", &|func_id| {
        vec![
            Instruction::TestJumpUnless {
                test: TestOp::NullP,
                a: 0,
                b: 0,
                dst: 1,
                target: 4,
                func_id,
                link: 1,
            },
            Instruction::JumpUnless { cond: 1, target: 4 },
        ]
    });
}

/// A shadowed primitive's site reads its operand as `LoadGlobal` reads a
/// global (#836): where the linked read finds no binding in the operand's
/// record, by the cold read, which reads the name in the unit's namespace.
/// Here the operand is a name the namespace reaches by an alias, whose entry
/// is the heap's by-name record, so the linked read always misses and the
/// site must call what the alias names. Built by hand: pass 5 emits a
/// primitive site only for a name its namespace binds in a slot of its own,
/// whose record the linked read answers, so no compiled site reads cold.
#[test]
fn a_shadowed_primitives_site_reads_an_aliased_operand_by_name() {
    use patina_core::LinkTable;
    use patina_vm::types::Arity;
    use patina_vm::types::instruction::{Instruction, PrimitiveFnId};

    let mut state = fresh_state();
    let globals = Rc::clone(state.global_env());
    // `negate` names the namespace's own `-`, by an alias.
    globals.define_alias("negate", Rc::clone(&globals), "-".into());
    let car = state
        .primitive_registry()
        .resolve_index("scheme.base/car")
        .expect("`car` is registered");
    let names: Vec<Rc<str>> = vec![Rc::from("car"), Rc::from("negate")];
    let code = CodeObject {
        id: CodeObjectId::label(),
        name: None,
        links: Rc::new(LinkTable::new(names.into_boxed_slice())),
        instructions: vec![
            Instruction::LoadImmediate {
                dst: 0,
                val: TaggedValue::fixnum(1),
            },
            // (define car 1): `car`'s sites are shadowed from here on.
            Instruction::Define { link: 0, src: 0 },
            // A site of `car` whose operand is `negate`: (negate 1).
            Instruction::CallPrimitive {
                func_id: PrimitiveFnId(car as u32),
                link: 1,
                args: vec![0],
                dst: 1,
            },
            Instruction::Return { val: 1 },
        ],
        constants: Vec::new(),
        num_regs: 2,
        arity: Arity::Fixed(0),
        source_map: Vec::new(),
        register_roots: None,
        live_closures: std::cell::Cell::new(0),
    };
    let id = load(&mut state, code, Vec::new());
    let answer = execute(&mut state, id).expect("the site calls what `negate` names");
    assert_eq!(answer.as_fixnum(), Some(-1));
}
