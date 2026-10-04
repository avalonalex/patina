//! A liveness map that calls a live register dead panics where the register
//! is read, in a check build (#625).
//!
//! Before a collection or a continuation capture, the VM retires every
//! register its per-pc liveness map calls dead (#423). Retirement wrote
//! `UNSPECIFIED`, a legal value, so a wrong map turned a live value into a
//! plausible one: the program failed later, at an unrelated instruction, or
//! not at all. In a check build retirement writes `DEAD_SLOT` instead, and
//! the VM panics where one is read: in `reg_at`, which every instruction's
//! operands go through, and in the closure-call fast path's argument copy,
//! which reads the caller's registers directly (`call_closure_from_regs`).
//!
//! The controls make the maps wrong on purpose, with patina-vm's test-only
//! switch (`test_support::DropHighestLive`): while it is alive, retirement
//! drops the highest live register from every map. Each control asks for a
//! collection with `(gc)` at a point chosen so that the first read of a
//! dropped register is at the site it names, and expects that site's panic.
//! The same programs answer correctly without the switch
//! (`the_programs_answer_without_the_switch`).
//!
//! `(gc)` collects at its call in every mode (#639), with its caller
//! suspended at the call's return pc, so the controls do not depend on the
//! collector's mode as long as it is the default; under `PATINA_GC_STRESS`
//! or `PATINA_GC_ZEAL`, a collection at an earlier pc may drop a register
//! that is read first somewhere else. The register `(gc)`'s value goes to is
//! written after the collection, when the primitive is done, so a control
//! drops another: the argument controls put `(gc)` inside the last argument,
//! whose value is a constant loaded after it, and the switch drops the
//! argument before it.
//!
//! They run in every check build: every debug `cargo test`, and a release one
//! with this crate's `gc-check` feature, which the release GC lane enables.
//! A build without the checks reports them ignored. In such a build the
//! switch still drops the register, which then reads as `UNSPECIFIED`: the
//! operand control's `car` raises a type error, and the argument controls
//! quietly answer with an unspecified value in the list.

mod common;

use common::vm_interpreter;
use patina_core::TaggedValue;
use patina_interpreter::Interpreter;
use patina_vm::VmBackend;
use patina_vm::test_support::DropHighestLive;

/// The first argument of `car`, retired by the collection `(gc)` asks for:
/// at the safe point after the call, the map holds only `x`, which the switch
/// drops, and `car` reads it next.
const OPERAND: &str = "(define (first-of x) (gc) (car x)) (first-of (cons 1 2))";

/// A variadic call's rest argument. The arguments are evaluated into fresh
/// registers in order, and `(gc)`'s own value is dropped as soon as it
/// returns, so the copy of `a` before it is the highest live register when
/// the collection runs, at the call's return pc: the copy that conses the
/// rest list reads it.
const VARIADIC_REST: &str = "(define (gather . xs) xs) \
     (define (call-it a) (gather a (begin (gc) 2)) a) \
     (call-it 1)";

/// A variadic call's fixed argument: the same, with the copy of `a` as the
/// one fixed parameter's value.
const VARIADIC_FIXED: &str = "(define (gather first . rest) first) \
     (define (call-it a) (gather a (begin (gc) 2)) a) \
     (call-it 1)";

/// A fixed-arity call's argument.
const FIXED: &str = "(define (pair-up a b) (cons a b)) \
     (define (call-it a) (pair-up a (begin (gc) 2)) a) \
     (call-it 1)";

fn interpreter() -> Interpreter<VmBackend> {
    let interp = vm_interpreter();
    interp
        .eval_program("(import (scheme base) (patina debug))")
        .expect("import");
    interp
}

/// Run `program` with every liveness map one register short.
fn run_with_wrong_maps(program: &str) {
    let interp = interpreter();
    let _wrong = DropHighestLive::new();
    let _ = interp.eval_program(program);
}

/// A control: a test that must panic with `$expected` in a check build.
macro_rules! control {
    ($name:ident, $expected:literal, $program:expr) => {
        #[test]
        #[cfg_attr(
            not(any(debug_assertions, feature = "gc-check")),
            ignore = "needs a check build"
        )]
        #[should_panic(expected = $expected)]
        fn $name() {
            run_with_wrong_maps($program);
        }
    };
}

control!(
    a_retired_operand_panics_at_reg_at,
    "read of a retired register as an instruction's operand",
    OPERAND
);
control!(
    a_retired_rest_argument_panics_at_the_argument_copy,
    "read of a retired register as a call's argument",
    VARIADIC_REST
);
control!(
    a_retired_fixed_argument_of_a_variadic_call_panics_at_the_argument_copy,
    "read of a retired register as a call's argument",
    VARIADIC_FIXED
);
control!(
    a_retired_argument_of_a_fixed_arity_call_panics_at_the_argument_copy,
    "read of a retired register as a call's argument",
    FIXED
);

/// What the controls see is the switch, not the programs: without it each
/// one collects where its control does and answers correctly.
#[test]
fn the_programs_answer_without_the_switch() {
    let interp = interpreter();
    let car = interp.eval_program(OPERAND).expect("operand program");
    assert_eq!(car, TaggedValue::fixnum(1));
    for program in [VARIADIC_REST, VARIADIC_FIXED, FIXED] {
        let value = interp.eval_program(program).expect(program);
        assert_eq!(value, TaggedValue::fixnum(1), "{program}");
    }
}
