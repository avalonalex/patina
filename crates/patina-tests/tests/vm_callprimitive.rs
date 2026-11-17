//! Semantic tests for `CallPrimitive` dispatch on the VM backend (Track P P2).
//!
//! These run against `VmBackend` explicitly rather than the both-backends
//! common helpers: the deopt behavior under test only exists in the VM. The
//! host's definitions (#673) run on both backends, since the tree-walker's
//! answer is the one the VM's must match.

// The bare-value `eval_*` forms (#605), deprecated until stage 5e removes them.
#![allow(deprecated)]

mod common;
use common::eval_program_vm as eval;
use patina_interpreter::{Backend, Interpreter};

#[test]
fn fast_path_basics() {
    assert_eq!(eval("(+ 1 2)"), "3");
    assert_eq!(eval("(car '(a b))"), "a");
    assert_eq!(eval("(cons 1 2)"), "(1 . 2)");
    assert_eq!(eval("(vector-ref #(10 20 30) 1)"), "20");
    assert_eq!(eval("(define (f x y) (* (+ x 1) y)) (f 4 2)"), "10");
}

#[test]
fn import_rebind_deoptimizes() {
    // PRD P8.1: rebinding a primitive name via `import` (not define/set!)
    // must deoptimize already-compiled CallPrimitive/inline sites exactly
    // like a top-level redefinition does. Rebinds car to scheme.base's cdr;
    // the tree-walker (name lookup per call) returns (2) here.
    assert_eq!(
        eval(
            "(define (f p) (car p)) \
             (import (rename (only (scheme base) cdr) (cdr car))) \
             (f '(1 2))"
        ),
        "(2)"
    );
}

#[test]
fn tail_deopt_returns_correct_result() {
    // PRD P8.2: a rebound primitive at a tail-position site must dispatch
    // with tail-call semantics. `(car p)` is `count`'s entire body (tail
    // position → compiled to `Car; Return`); after the rebind every call
    // deopts to the closure through the tail path.
    assert_eq!(
        eval(
            "(define (count p) (car p)) \
             (define car (lambda (p) (if (null? (cdr p)) 0 (+ 1 (count (cdr p)))))) \
             (count (list 1 2 3 4))"
        ),
        "3"
    );
}

#[test]
fn tail_deopt_runs_deep_mutual_recursion() {
    // PRD P8.2: 100k-deep mutual tail recursion through a rebound
    // tail-position primitive site. With frame reuse this runs in constant
    // space; the flat-RSS property is verified out-of-band (PRD P8.2
    // acceptance), this test pins the semantics at depth.
    assert_eq!(
        eval(
            "(define (loop n) (car n)) \
             (define car (lambda (n) (if (= n 0) 'done (loop (- n 1))))) \
             (loop 100000)"
        ),
        "done"
    );
}

#[test]
fn define_after_use_deoptimizes() {
    // f is compiled while car is the primitive; rebinding car afterwards must
    // change what f calls (R7RS top-level redefinition semantics).
    assert_eq!(
        eval("(define (f p) (car p)) (define car (lambda (p) 'shadowed)) (f '(1 2))"),
        "shadowed"
    );
}

#[test]
fn set_after_use_deoptimizes() {
    assert_eq!(eval("(define (g) (+ 40 2)) (set! + -) (g)"), "38");
}

/// The sequence a head-position `call-with-values` or `dynamic-wind` compiles
/// to (#442) is guarded like a `CallPrimitive` site: compiled while the name
/// is the form's procedure, it calls what the name holds once that changes.
/// Both positions, because the sequence ends differently in each: in tail
/// position the ordinary call it falls back to is a `TailCall`.
#[test]
fn control_forms_define_after_use_deoptimize() {
    assert_eq!(
        eval(
            "(define (f) (call-with-values (lambda () (values 1 2)) list)) \
             (define (f-inner) (car (list (call-with-values (lambda () (values 1 2)) list)))) \
             (define (g) (dynamic-wind (lambda () #f) (lambda () 'body) (lambda () #f))) \
             (define (g-inner) (car (list (dynamic-wind (lambda () #f) (lambda () 'body) (lambda () #f))))) \
             (define before (list (f) (f-inner) (g) (g-inner))) \
             (define (call-with-values p c) 'mine) \
             (define (dynamic-wind a b c) 'mine) \
             (list before (f) (f-inner) (g) (g-inner))"
        ),
        "(((1 2) (1 2) body body) mine mine mine mine)"
    );
}

/// The same through `set!`, and back: restoring the procedure restores the
/// answer, though not the sequence — the shadow mark stays set, and the site
/// calls the procedure through its value form from then on.
#[test]
fn control_forms_set_after_use_deoptimize() {
    assert_eq!(
        eval(
            "(define saved-cwv call-with-values) \
             (define saved-dw dynamic-wind) \
             (define (f) (call-with-values (lambda () (values 1 2)) list)) \
             (define (g) (dynamic-wind (lambda () #f) (lambda () 'body) (lambda () #f))) \
             (define before (list (f) (g))) \
             (set! call-with-values (lambda (p c) 'assigned)) \
             (set! dynamic-wind (lambda (a b c) 'assigned)) \
             (define during (list (f) (g))) \
             (set! call-with-values saved-cwv) \
             (set! dynamic-wind saved-dw) \
             (list before during (f) (g))"
        ),
        "(((1 2) body) (assigned assigned) (1 2) body)"
    );
}

/// How [`host_definitions`] binds a name in the global environment.
#[derive(Clone, Copy, Debug)]
enum GlobalDefiner {
    /// `Interpreter::define`, through `Backend::define_in`.
    Interpreter,
    /// The deprecated `Backend::define`, promised until stage 5e (#839).
    Deprecated,
}

/// A host's definition deoptimizes as a program's does (#673). Through
/// `Interpreter::define`, or the deprecated `Backend::define` that S1's
/// `define_in` replaced (#839), code compiled while the name was a
/// primitive's calls the new value, as on the tree-walker, which looks the
/// name up at each call. `Environment::define` and `set` write the binding
/// and nothing else, and left that code calling the primitive on the VM.
fn host_definitions<B: Backend>(interp: Interpreter<B>, definer: GlobalDefiner) -> Vec<String> {
    interp
        .eval_program_owned(
            "(define (f x) (car x)) \
             (define (k) (call-with-values (lambda () (values 1 2)) list)) \
             (define (h) 'old) \
             (define (g) (h))",
        )
        .unwrap();
    let replaced = interp.eval_str_owned("(lambda args 'replaced)").unwrap();
    let new = interp.eval_str_owned("(lambda () 'new)").unwrap();
    let define = |name: &str, value: &patina_core::Owned| match definer {
        GlobalDefiner::Interpreter => interp.define(name, value),
        GlobalDefiner::Deprecated => {
            let backend = interp.backend();
            backend.define(backend.global_env(), name, interp.raw_value(value));
        }
    };
    define("car", &replaced);
    define("call-with-values", &replaced);
    define("h", &new);
    ["(f '(1 2))", "(car '(1 2))", "(k)", "(g)"]
        .into_iter()
        .map(|form| interp.display_tagged(interp.eval_str_owned(form).unwrap()))
        .collect()
}

#[test]
fn a_host_definition_deoptimizes() {
    let expected = ["replaced", "replaced", "replaced", "new"];
    for definer in [GlobalDefiner::Interpreter, GlobalDefiner::Deprecated] {
        assert_eq!(
            host_definitions(common::vm_interpreter(), definer),
            expected,
            "VM, {definer:?}"
        );
        assert_eq!(
            host_definitions(common::tree_walker_interpreter(), definer),
            expected,
            "tree-walker, {definer:?}"
        );
    }
}

/// How [`host_environment_definitions`] defines `car` in the host
/// environment.
#[derive(Clone, Copy, Debug)]
enum Definer {
    /// `Interpreter::define_in`, through `Backend::define_in`.
    Host,
    /// A `define` form evaluated there: on the VM a compiled `Define`.
    Compiled,
}

/// The same in a host environment (#620), through `define_in` or a
/// compiled `define`. One that imports `(scheme base)` binds `car` itself,
/// so the code compiled there takes the fast path until the definition
/// marks it; one that reaches `car` through its parent compiles an ordinary
/// call, which the VM links to a read-through record over the global `car`
/// (#836), and the definition adopts that record, so the call reads the new
/// value. Either way the global `car` stays the primitive: a definition in
/// a host namespace never writes through its read-through record into the
/// global cell (#852).
fn host_environment_definitions<B: Backend>(
    interp: Interpreter<B>,
    import: &str,
    definer: Definer,
) -> Vec<String> {
    let env = interp.new_environment();
    interp
        .eval_program_in(&env, &format!("{import} (define (f x) (car x))"))
        .unwrap();
    match definer {
        Definer::Host => {
            let replaced = interp.eval_str_owned("(lambda (x) 'replaced)").unwrap();
            interp.define_in(&env, "car", &replaced);
        }
        Definer::Compiled => {
            interp
                .eval_str_in(&env, "(define car (lambda (x) 'replaced))")
                .unwrap();
        }
    }
    vec![
        interp.display_tagged(interp.eval_str_in(&env, "(f '(1 2))").unwrap()),
        interp.display_tagged(interp.eval_str_in(&env, "(car '(1 2))").unwrap()),
        interp.display_tagged(interp.eval_str_owned("(car '(1 2))").unwrap()),
    ]
}

#[test]
fn a_host_definition_in_a_host_environment_deoptimizes() {
    let expected = ["replaced", "replaced", "1"];
    for definer in [Definer::Host, Definer::Compiled] {
        for import in ["(import (scheme base))", ""] {
            assert_eq!(
                host_environment_definitions(common::vm_interpreter(), import, definer),
                expected,
                "VM, {import:?}, {definer:?}"
            );
            assert_eq!(
                host_environment_definitions(common::tree_walker_interpreter(), import, definer),
                expected,
                "tree-walker, {import:?}, {definer:?}"
            );
        }
    }
}

/// A `set!` in a host namespace of a name only the global namespace binds
/// assigns the global's binding (#835): the host namespace does not bind it,
/// and the store marks the primitive the binding held, so code the global
/// namespace compiled while the name held it calls the new value. 4b.9's
/// linked store, through a read-through record, must keep this.
fn host_namespace_set<B: Backend>(interp: Interpreter<B>) -> Vec<String> {
    let warm = interp
        .eval_program_owned("(define first car) (define (g x) (first x)) (g '(1 2))")
        .unwrap();
    let env = interp.new_environment();
    interp
        .eval_program_in(&env, "(set! first (lambda (x) 'replaced))")
        .unwrap();
    let binds = interp
        .raw_environment(&env)
        .local_names(interp.backend().heap())
        .iter()
        .any(|name| name == "first");
    vec![
        interp.display_tagged(warm),
        interp.display_tagged(interp.eval_str_owned("(g '(1 2))").unwrap()),
        interp.display_tagged(interp.eval_str_in(&env, "(first '(1 2))").unwrap()),
        format!("host binds first: {binds}"),
    ]
}

#[test]
fn a_set_through_a_host_namespace_marks_the_primitive_it_replaces() {
    let expected = ["1", "replaced", "replaced", "host binds first: false"];
    assert_eq!(host_namespace_set(common::vm_interpreter()), expected);
    assert_eq!(
        host_namespace_set(common::tree_walker_interpreter()),
        expected
    );
}

/// A private binding that holds a primitive is never a fast path, before
/// its library re-points it or after (#837; found in review of #442 for
/// `call-with-values` and ordinary primitives, when the shape was an
/// alias a test installed by hand). `(t repoint)` keeps `car` under the
/// private name `h` and `call-with-values` under `k`, and its macros call
/// each; the program has procedures of its own by those names, so each
/// template call is relinked. The operand is the library's variable,
/// whatever it holds: an ordinary load and call, with no inline `Car`, no
/// `CallPrimitive` and no control sequence, so that the library's
/// assignment is what the next call runs.
#[test]
fn a_re_pointed_private_binding_is_never_a_fast_path() {
    const REPOINT: &str = "(define-library (t repoint)
  (export call-h call-k repoint!)
  (import (scheme base))
  (begin
    (define h car)
    (define k call-with-values)
    (define (repoint!) (set! h cdr) (set! k (lambda (p c) 'second)))
    (define-syntax call-h (syntax-rules () ((_ p) (h p))))
    (define-syntax call-k (syntax-rules () ((_ p c) (k p c))))))
(import (scheme base) (t repoint))
(define (h p) 'the-programs)
(define (k p c) 'the-programs)
";
    let interp = Interpreter::new(patina_vm::VmBackend::new());
    interp
        .eval_program_owned(REPOINT)
        .expect("the library and the program's own h and k");
    let g = operands(
        &interp
            .backend()
            .compile_for_inspection("(define (g) (call-h '(1 2)))")
            .expect("compiles"),
    );
    assert_eq!(g.loads, ["h@linked"], "the callee's operand");
    assert_eq!(
        (g.calls, g.inline_cars, g.primitive_calls, g.control),
        (1, 0, 0, 0),
        "a private name for car is an ordinary call"
    );
    let f = operands(
        &interp
            .backend()
            .compile_for_inspection("(define (f) (call-k (lambda () 1) (lambda (x) x)))")
            .expect("compiles"),
    );
    assert_eq!(f.loads, ["k@linked"], "the callee's operand");
    assert_eq!(
        (f.calls, f.control),
        (1, 0),
        "a private name for call-with-values is an ordinary call"
    );

    let program = format!(
        "{REPOINT}(define (f) (call-k (lambda () 1) (lambda (x) (list 'cwv x))))
(define (g) (call-h '(1 2)))
(define before (list (f) (g)))
(repoint!)
(list before (f) (g) (h 0) (k 0 0))"
    );
    let expected = "(((cwv 1) 1) second (2) the-programs the-programs)";
    assert_eq!(eval(&program), expected, "VM");
    assert_eq!(
        common::eval_program_tree_walker(&program),
        expected,
        "tree-walker"
    );
}

#[test]
fn deopt_is_per_primitive() {
    // Rebinding car must not disturb cdr's fast path.
    assert_eq!(
        eval("(define (g p) (cdr p)) (define car (lambda (p) 'x)) (g '(1 2))"),
        "(2)"
    );
}

#[test]
fn lexical_shadowing_never_uses_fast_path() {
    assert_eq!(
        eval("(let ((cdr (lambda (x) 'lexical))) (cdr '(1 2)))"),
        "lexical"
    );
}

#[test]
fn control_primitives_still_intercepted() {
    assert_eq!(
        eval("(call-with-current-continuation (lambda (k) (k 'ok)))"),
        "ok"
    );
    assert_eq!(eval("(call-with-values (lambda () (values 1 2)) +)"), "3");
    assert_eq!(
        eval("(dynamic-wind (lambda () #f) (lambda () 'body) (lambda () #f))"),
        "body"
    );
}

#[test]
fn higher_order_primitive_through_fast_path() {
    // member's predicate form re-enters the VM from a CallPrimitive dispatch.
    // (vector-map used to be the example here, but it is an ordinary Scheme
    // closure now — the continuation-broken Rust version was deleted.) So is
    // `(scheme base)`'s `member` since #471, so this calls the primitive under
    // it, from `(patina internal lists)`.
    assert_eq!(
        eval(
            "(import (rename (only (patina internal lists) member) (member prim-member))) \
             (prim-member 2.0 '(1 2 3) =)"
        ),
        "(2 3)"
    );
    // `force` rode the same path until #476. It is a VM control primitive
    // now, excluded from `CallPrimitive` (`primitive_calls::is_excluded`),
    // so this row only pins that the exclusion leaves the call working.
    assert_eq!(
        eval("(import (scheme lazy)) (force (delay (+ 20 22)))"),
        "42"
    );
}

#[test]
fn errors_still_catchable() {
    assert_eq!(eval("(guard (e (#t 'caught)) (car 5))"), "caught");
    assert_eq!(
        eval("(guard (e (#t 'caught)) (vector-ref (vector 1) 99))"),
        "caught"
    );
}

#[test]
fn arity_error_message_parity() {
    let interp = patina_interpreter::Interpreter::new(patina_vm::VmBackend::new());
    let err = interp
        .eval_program("(car '(1) '(2))")
        .expect_err("arity error expected")
        .to_string();
    assert!(err.contains("car"), "unexpected message: {err}");
}

/// A library whose macro's template calls `car`, which the library imports
/// from `(scheme base)` as the program does.
const CARLIB: &str = "(define-library (t carlib)
  (export first-of)
  (import (scheme base))
  (begin (define-syntax first-of (syntax-rules () ((_ x) (car x))))))
(import (scheme base) (t carlib))
";

/// Each inline `Car`'s operand among `code`'s and its lambdas'
/// instructions, as the disassembler prints it: its link-table spelling,
/// marked `@linked` where the entry is pinned to a record the desugarer
/// linked (#837).
fn car_operands(inspected: &patina_vm::backend::Inspected) -> Vec<String> {
    let mut operands = Vec::new();
    for (top, nested) in inspected.units() {
        for code in std::iter::once(top).chain(nested) {
            for instr in &code.instructions {
                if let patina_vm::types::Instruction::Car { link, .. } = instr {
                    let spelling = code.links.name(*link);
                    operands.push(if code.links.is_pinned(*link) {
                        format!("{spelling}@linked")
                    } else {
                        spelling.to_string()
                    });
                }
            }
        }
    }
    operands
}

/// The operand of each `LoadGlobal` and `StoreGlobal` among `code`'s and
/// its lambdas' instructions, as the disassembler prints it
/// ([`car_operands`]), and whether the code calls, inlines `car`, calls a
/// primitive by its id, applies (`Apply`, `TailApply`), or runs a control
/// form's sequence (`call-with-values`'s or `dynamic-wind`'s instructions,
/// or the guard in front of them).
struct Operands {
    loads: Vec<String>,
    stores: Vec<String>,
    /// The link-table entry of each pinned operand, load or store, beside
    /// what it prints.
    pinned: Vec<(String, String)>,
    calls: usize,
    inline_cars: usize,
    primitive_calls: usize,
    applies: usize,
    control: usize,
}

fn operands(inspected: &patina_vm::backend::Inspected) -> Operands {
    use patina_vm::types::Instruction;
    let mut found = Operands {
        loads: Vec::new(),
        stores: Vec::new(),
        pinned: Vec::new(),
        calls: 0,
        inline_cars: 0,
        primitive_calls: 0,
        applies: 0,
        control: 0,
    };
    for (top, nested) in inspected.units() {
        for code in std::iter::once(top).chain(nested) {
            let printed = |link| {
                let spelling = code.links.name(link);
                if code.links.is_pinned(link) {
                    format!("{spelling}@linked")
                } else {
                    spelling.to_string()
                }
            };
            for instr in &code.instructions {
                if let Instruction::LoadGlobal { link, .. } | Instruction::StoreGlobal { link, .. } =
                    instr
                    && code.links.is_pinned(*link)
                {
                    found.pinned.push((printed(*link), format!("{link:?}")));
                }
                match instr {
                    Instruction::LoadGlobal { link, .. } => found.loads.push(printed(*link)),
                    Instruction::StoreGlobal { link, .. } => found.stores.push(printed(*link)),
                    Instruction::Call { .. } | Instruction::TailCall { .. } => found.calls += 1,
                    Instruction::Car { .. } => found.inline_cars += 1,
                    Instruction::CallPrimitive { .. } => found.primitive_calls += 1,
                    Instruction::Apply { .. } | Instruction::TailApply { .. } => {
                        found.applies += 1;
                    }
                    Instruction::CallWithValues { .. }
                    | Instruction::TailCallWithValues { .. }
                    | Instruction::PushWind { .. }
                    | Instruction::JumpUnlessShadowed { .. } => found.control += 1,
                    _ => {}
                }
            }
        }
    }
    found
}

/// A relinked reference is a linked operand under the spelling its
/// template wrote (#837), where it was an ordinary entry under the
/// spelling relinking gave it (`car.N`, `secret.N`): the code
/// `compile_for_inspection` answers, the path `(vm-compile …)` and
/// `--dump` print, reads `car@linked` and `secret@linked`.
///
/// `g2` is compiled after the program defines `car`, so the template's
/// `car` is relinked, not bound early: its call is an ordinary `Call` of
/// the loaded operand, with no inline `Car` and no `CallPrimitive`,
/// whatever the record's cell holds. `w` assigns the library's private
/// `secret` through a template: a `StoreGlobal` of the linked operand.
#[test]
fn a_relinked_operand_is_printed_as_its_template_wrote_it() {
    let interp = Interpreter::new(patina_vm::VmBackend::new());
    interp
        .eval_program_owned(&format!(
            "{CARLIB}(define-library (t secretlib)
  (export set-secret! get-secret)
  (import (scheme base))
  (begin
    (define secret 42)
    (define-syntax get-secret (syntax-rules () ((_) secret)))
    (define-syntax set-secret! (syntax-rules () ((_ v) (set! secret v))))))
(import (t secretlib))
(define (car x) 'mine)"
        ))
        .expect("the libraries, the imports and the program's car");
    let call = operands(
        &interp
            .backend()
            .compile_for_inspection("(define (g2 p) (first-of p))")
            .expect("compiles"),
    );
    assert_eq!(call.loads, ["car@linked"], "the callee's operand");
    assert_eq!(
        (call.calls, call.inline_cars, call.primitive_calls),
        (1, 0, 0),
        "a relinked callee is an ordinary call"
    );
    let store = operands(
        &interp
            .backend()
            .compile_for_inspection("(define (w v) (set-secret! v) (get-secret))")
            .expect("compiles"),
    );
    assert_eq!(store.stores, ["secret@linked"], "the assignment's operand");
    assert_eq!(store.loads, ["secret@linked"], "the read's operand");

    let program = format!(
        "{CARLIB}(define-library (t secretlib)
  (export set-secret! get-secret)
  (import (scheme base))
  (begin
    (define secret 42)
    (define-syntax get-secret (syntax-rules () ((_) secret)))
    (define-syntax set-secret! (syntax-rules () ((_ v) (set! secret v))))))
(import (t secretlib))
(define (car x) 'mine)
(define secret 'program)
(define (g2 p) (first-of p))
(define (w v) (set-secret! v) (get-secret))
(list (g2 '(1 2)) (w 7) (car '(1 2)) secret)"
    );
    let expected = "(1 7 mine program)";
    assert_eq!(eval(&program), expected, "VM");
    assert_eq!(
        common::eval_program_tree_walker(&program),
        expected,
        "tree-walker"
    );
}

/// The code of three relinked references where a call has a path of its
/// own (#837; the answers are rows of `expansion/relinked-names.scm`):
///
/// - **`apply`.** `(apply f xs)` is lowered to an `Apply` where its head is
///   the `apply` primitive. A template's `apply`, used by a program that
///   has defined an `apply` of its own, is relinked: an ordinary `Call` of
///   the loaded operand, and no `Apply` or `TailApply`. Bound early, in a
///   program that has not, it is lowered.
/// - **`call-with-values`.** The same for the control form's sequence:
///   none for a relinked callee, and the sequence where it is bound early.
/// - **An assignment beside a call of one binding.** In one form, a call
///   bound early (the program imports the name), then the program's
///   definition of the spelling, which is only declared when the rest is
///   desugared, and after it a relinked assignment and a relinked call.
///   The early-bound and the relinked operands are two pinned entries of
///   the one record, and the relinked assignment and call share one.
#[test]
fn a_relinked_reference_takes_no_call_path_of_its_own() {
    const LIBRARIES: &str = "(define-library (t paths)
  (export apply-of values-of sc-proc call-sc set-sc!)
  (import (scheme base))
  (begin
    (define (sc-proc x) (list 'lib x))
    (define-syntax call-sc (syntax-rules () ((_ x) (sc-proc x))))
    (define-syntax set-sc! (syntax-rules () ((_ v) (set! sc-proc v))))
    (define-syntax apply-of (syntax-rules () ((_ f xs) (apply f xs))))
    (define-syntax values-of (syntax-rules () ((_ p c) (call-with-values p c))))))
(import (scheme base) (t paths))
";
    let inspect = |interp: &Interpreter<patina_vm::VmBackend>, form: &str| {
        operands(
            &interp
                .backend()
                .compile_for_inspection(form)
                .expect("compiles"),
        )
    };
    let apply_form = "(define (a) (apply-of + '(1 2)))";
    let values_form = "(define (v) (values-of (lambda () 1) (lambda (x) x)))";

    // Bound early: the program shares the library's `apply` and
    // `call-with-values`.
    let early = Interpreter::new(patina_vm::VmBackend::new());
    early.eval_program_owned(LIBRARIES).expect("the library");
    let applied = inspect(&early, apply_form);
    assert_eq!((applied.applies, applied.calls), (1, 0), "an early apply");
    assert!(
        inspect(&early, values_form).control > 0,
        "an early sequence"
    );

    // Relinked: the program has its own.
    let relinked = Interpreter::new(patina_vm::VmBackend::new());
    relinked
        .eval_program_owned(&format!(
            "{LIBRARIES}(define (apply f xs) 'program)
(define (call-with-values p c) 'program)"
        ))
        .expect("the library and the program's own procedures");
    let applied = inspect(&relinked, apply_form);
    assert!(
        applied.loads.contains(&"apply@linked".to_string()),
        "{:?}",
        applied.loads
    );
    assert_eq!(
        (applied.applies, applied.calls),
        (0, 1),
        "a relinked apply is an ordinary call"
    );
    let values = inspect(&relinked, values_form);
    assert_eq!(values.loads, ["call-with-values@linked"]);
    assert_eq!(
        (values.control, values.calls),
        (0, 1),
        "a relinked call-with-values has no sequence"
    );

    // An early-bound call, the program's definition, and a relinked
    // assignment and call, in one form.
    let both = inspect(
        &early,
        "(begin (define early (call-sc 1))
                (define (sc-proc x) 'mine)
                (define later (begin (set-sc! car) (call-sc '(2)))))",
    );
    let of_the_binding: Vec<&(String, String)> = both
        .pinned
        .iter()
        .filter(|(printed, _)| printed == "sc-proc@linked")
        .collect();
    assert_eq!(
        of_the_binding.len(),
        3,
        "an early call, a relinked assignment and a relinked call: {:?}",
        both.pinned
    );
    assert_ne!(
        of_the_binding[0].1, of_the_binding[1].1,
        "the early-bound and the relinked operand are two entries"
    );
    assert_eq!(
        of_the_binding[1].1, of_the_binding[2].1,
        "the relinked assignment and call share one"
    );
    assert_eq!(both.stores, ["sc-proc@linked"], "the relinked assignment");

    let program = format!(
        "{LIBRARIES}(define (apply f xs) 'program)
(define (call-with-values p c) 'program)
(define (a) (apply-of + '(1 2)))
(define (v) (values-of (lambda () 1) (lambda (x) (list 'cwv x))))
(begin (define early (call-sc 1))
       (define (sc-proc x) 'mine)
       (define later (begin (set-sc! car) (call-sc '(2)))))
(list (a) (v) early later (sc-proc 0) (apply + '(1)) (call-with-values 1 2))"
    );
    let expected = "(3 (cwv 1) (lib 1) 2 mine program program)";
    assert_eq!(eval(&program), expected, "VM");
    assert_eq!(
        common::eval_program_tree_walker(&program),
        expected,
        "tree-walker"
    );
}

/// A generated macro carries the records its generator's expansion
/// relinked (#837): `get`, which a library's `def-getter` made in one
/// form, is expanded in later ones, and its template's reference to the
/// library's private `secret` compiles to a load of the linked operand
/// there too, `secret@linked`, where it was read by the name relinking
/// gave it. The library assigns the variable in between, and the
/// program has a `secret` of its own.
#[test]
fn a_generated_macros_carried_reference_is_a_linked_operand() {
    let library = "(define-library (t carried)
  (export def-getter bump!)
  (import (scheme base))
  (begin
    (define secret 42)
    (define (bump!) (set! secret (+ secret 1)))
    (define-syntax def-getter
      (syntax-rules ()
        ((_ name) (define-syntax name (syntax-rules () ((_) secret))))))))
(import (scheme base) (t carried))
(define secret 'program)
(def-getter get)
";
    let interp = Interpreter::new(patina_vm::VmBackend::new());
    interp
        .eval_program_owned(library)
        .expect("the library, the import and the generated macro");
    let later = operands(
        &interp
            .backend()
            .compile_for_inspection("(define (h) (get))")
            .expect("compiles"),
    );
    assert_eq!(later.loads, ["secret@linked"], "the carried reference");

    let program = format!("{library}(define (h) (get))\n(bump!)\n(list (h) (get) secret)");
    let expected = "(43 43 program)";
    assert_eq!(eval(&program), expected, "VM");
    assert_eq!(
        common::eval_program_tree_walker(&program),
        expected,
        "tree-walker"
    );
}

/// A template's `car` and the program's own `car` in one procedure stay two
/// operands (#837's `OperandKey`): once the program defines `car`, the
/// template's still reads the library's binding and the program's reads
/// the program's, `(1 mine)` on both backends. chibi 0.12 and Gauche
/// 0.9.15 answer `(1 1)`, inlining the program's `car` too: that is #603's
/// latitude (`DIVERGENCES.tsv`'s `stdlib/rebinding.scm` rows), where
/// Patina's own `car` follows the name. An implementation that gave both
/// operands the template's binding would also answer `(1 1)`, so this is
/// not to be "corrected" to the oracles' answer.
///
/// On the VM the two are two inline `Car`s on two link-table entries of
/// one spelling, the template's pinned to the library's record
/// (`car@linked`) and the program's its own (`car`); before the switch
/// the template's was an ordinary entry spelled `car.N`.
#[test]
fn a_linked_and_an_own_operand_of_one_spelling_stay_apart() {
    let interp = Interpreter::new(patina_vm::VmBackend::new());
    interp
        .eval_program_owned(CARLIB)
        .expect("the library and the import");
    let inspected = interp
        .backend()
        .compile_for_inspection("(define (g p) (list (first-of p) (car p)))")
        .expect("compiles");
    assert_eq!(car_operands(&inspected), ["car@linked", "car"]);

    let program = format!(
        "{CARLIB}(define (g p) (list (first-of p) (car p)))
(define before (g '(1 2)))
(define (car x) 'mine)
(list before (g '(1 2)))"
    );
    let expected = "((1 1) (1 mine))";
    assert_eq!(eval(&program), expected, "VM");
    assert_eq!(
        common::eval_program_tree_walker(&program),
        expected,
        "tree-walker"
    );
}

/// A template's call of `car`, compiled before the program defines `car`,
/// takes `car`'s fast path, and after the definition still answers the
/// library's `car` (deoptimized, through what the reference is bound to).
/// A template expanded after the definition reaches the library's `car`
/// too, by an ordinary call. The fast path is asserted on the code
/// `compile_for_inspection` answers for `g`, the path `(vm-compile …)`
/// prints: an inline `Car` on the linked record's operand, `car@linked`
/// (#837; `car.N`, an ordinary entry, before the switch).
#[test]
fn an_early_bound_primitive_keeps_its_fast_path() {
    let interp = Interpreter::new(patina_vm::VmBackend::new());
    interp
        .eval_program_owned(CARLIB)
        .expect("the library and the import");
    let inspected = interp
        .backend()
        .compile_for_inspection("(define (g p) (first-of p))")
        .expect("compiles");
    assert_eq!(
        car_operands(&inspected),
        ["car@linked"],
        "the template's car is inlined, on its linked operand"
    );

    let program = format!(
        "{CARLIB}(define (g p) (first-of p))
(define before (g '(1 2)))
(define (car x) 'mine)
(define (g2 p) (first-of p))
(list before (g '(1 2)) (g2 '(1 2)) (car '(1 2)))"
    );
    let expected = "(1 1 1 mine)";
    assert_eq!(eval(&program), expected, "VM");
    assert_eq!(
        common::eval_program_tree_walker(&program),
        expected,
        "tree-walker"
    );
}
