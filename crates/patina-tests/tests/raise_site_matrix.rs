//! Collections and continuation captures while a VM frame is suspended at a
//! raise site or at an inline operation's shadow path, scored against
//! reference implementations (#901).
//!
//! GC stage 4d (#782) changes how the VM's frames are laid out and scanned:
//! one header per frame on a reserved register stack, a dense per-pc map
//! index "with a map at every suspension point: calls, inline operations,
//! raise sites, pc 0", dead slots cleared by the scan, and `return_into` for
//! every write into a suspended frame (`PRD/GC_PRD.md` §11.1, invariants 1–3).
//! The review behind that plan found the frames a "maps only at calls and
//! polls" scan would miss: frames suspended at raise and deoptimization
//! sites, which sit at pcs that are not calls (`PRD/study/gc/design/
//! REVIEW_DISPOSITIONS.md`, SEM-2 and MIG-5). DESIGN's "Other named tests"
//! asks for "a collection and a `call/cc` inside a `guard` handler for errors
//! raised by inline operations: `(car 5)`, `(vector-ref v 99)`, an unbound
//! global, and a shadowed `+`", on both backends. This file is those tests,
//! written before 4d starts, so that they pin today's answers and 4d is
//! measured against them.
//!
//! # The sites
//!
//! What each compiles to on the VM is checked, not assumed:
//! [`each_site_compiles_to_the_instruction_it_names`] compiles each site's
//! procedure as `--dump` and `(vm-compile …)` do and looks for the
//! instruction.
//!
//! - **`car`**: `(car x)` is an inline `Car`. Given 5, it fails inside
//!   `dispatch_one_instruction`, whose error goes to `route_error` and is
//!   raised by `vm_raise_value` (`crates/patina-vm/src/runtime/
//!   vm_state.rs`, `control.rs`): the handler is popped, and
//!   `raise_step_stub`'s frame is pushed over the erroring frame, which
//!   stays at the pc after the `Car`.
//! - **`vector-ref`**: an inline `VectorRef` at index 99 of a three-element
//!   vector. The same path.
//! - **`unbound`**: a `LoadGlobal` of a name nothing defines, whose cold path
//!   answers `UnboundVariable`. The same path. It is a global read, not a
//!   primitive opcode, but like them it raises from a pc that is not a call.
//! - **`plus`**: `(+ x 1)` is an inline `AddImm` (an `Add` whose fixnum
//!   operand the compiler absorbed), compiled while `+` is the primitive.
//!   The program then defines `+`, which marks the primitive shadowed, and
//!   from then on the `AddImm`'s slow path (`inline_primitive!` into
//!   `exec_call_primitive`) calls the program's `+` from the `AddImm`'s pc
//!   (`call_value`), with the site's frame suspended there holding its
//!   parameter `mine`, a fresh list, which it reads as soon as `+` returns.
//!   (A `let` would not do: each compiles to a closure called by a tail
//!   call, which would hold the value instead of the frame.) Only a site
//!   compiled *before* the definition gets there: a site compiled after it
//!   calls `+` with an ordinary `Call`, and a local `let` of `+` never
//!   reaches the opcode at all. (It is not `JumpUnlessShadowed`, which
//!   guards `call-with-values`'s and `dynamic-wind`'s sequences: a shadowed
//!   one jumps to an ordinary call.) Where a handler is under test, the
//!   program's `+` raises with
//!   `raise-continuable` in tail position: its frame is popped before the
//!   raise, so the raise stub sits directly on the site's frame at the
//!   `AddImm`'s pc, two suspension points that are not calls one on the
//!   other, and a handler that returns sends its value through the stub's
//!   `Return` into the `AddImm`'s destination. That raise is a call, whose
//!   return is a poll site, so under zeal a collection also comes with the
//!   stub's frame at its pc 0, before it has called the handler; a declining
//!   `guard`'s re-raise is such a call too.
//!
//! A frame that raised from one of the first three is never resumed: none of
//! the raises is continuable, so control leaves it through an escape or a
//! secondary exception. What a collection or a capture there must keep is the
//! stub's frame and the frames below the erroring one. The `plus` site's
//! frame is resumed, at the `AddImm`'s pc, and reads its registers again.
//!
//! # The handlers
//!
//! - **`guard`**: R7RS 7.3's expansion (`lib/scheme/base/exceptions.scm`).
//!   Its handler captures `handler-k` on top of the raise stub and jumps out
//!   to `guard-k`, so the clauses run in the guard's dynamic environment, and
//!   a collection in a clause finds the frames at the raise site only in
//!   `handler-k`'s snapshot, which the clause thunk's closure keeps.
//!   `collect`: the first clause's test collects and declines, and the next
//!   catches. `reenter`: the inner guard's only clause collects and declines,
//!   so the guard re-enters `handler-k`, the stub and the frames below it
//!   reinstated from the snapshot, and re-raises with `raise-continuable`
//!   from there; an outer guard catches it and collects.
//! - **`escape`**: `with-exception-handler`, whose handler runs on top of the
//!   stub and escapes through a continuation captured outside. `collect`: the
//!   handler collects, then escapes. `reenter`: it captures `saved`, then
//!   escapes; the driver collects and re-enters `saved` once, and the handler
//!   escapes again from the reinstated stack.
//! - **`return`**: the handler returns. From the three errors that is a
//!   return from a non-continuable raise: the stub's `ResumeRaise` reads its
//!   registers (the handler, the raised object, the continuable flag) and
//!   raises a secondary exception, which an outer `guard` catches. Every
//!   oracle does the same (measured; R7RS 6.11's "a secondary exception is
//!   raised in the same dynamic environment as the handler"). From `plus`
//!   the handler's value goes back through the stub into the `AddImm`'s
//!   destination in the site's frame. `collect`: the handler collects, then
//!   returns.
//!   `reenter`: it captures `saved`, then returns; the driver collects and
//!   re-enters `saved`, and the handler returns again into the reinstated
//!   stub.
//! - **`callee`** (`plus` only): no handler. The program's `+` itself
//!   collects (`collect`), or captures `saved` (`reenter`), which the driver
//!   re-enters after a collection: control returns into the `AddImm`'s pc in
//!   the site's reinstated frame, which reads `mine` again.
//!
//! # What is checked
//!
//! Every program answers `(list results keep)`: `results`, each value its
//! transfer produced, two when it re-entered; and `keep`, a fresh list that
//! `run`'s frame holds across the whole transfer and reads afterwards. The
//! `plus` rows' values carry `mine`, which only the site's frame held across
//! the `AddImm`. A collection is a `(gc)`, which collects at its call in every
//! mode (#639), around an allocation loop, which collects too where
//! collections follow allocations (`PATINA_GC_STRESS`).
//!
//! The answer is half the check. A debug build, and release built with
//! `--features patina-core/gc-check`, panic on a reference to a freed or
//! reused slot (#621) and on a read of a register its frame's map retired
//! (#625), so a run there also checks that the scan kept what the frames
//! read afterwards. Each row runs on the backends' default interpreters,
//! which collect as the lane's variable says, and on zeal interpreters,
//! which collect at every outermost safe point whatever it says: on the VM
//! every frame's entry and generic call's return, which puts a collection at
//! the handler's entry with the stub over the erroring frame, and at the
//! program's `+`'s entry with the site's frame at the `AddImm`'s pc
//! ([`rows_score_as_recorded_under_zeal`]).
//!
//! # The oracles
//!
//! Measured 2026-11-22 against **chibi 0.12, Gauche 0.9.15 and Chez Scheme
//! 10.4.1**, external implementations only, and recorded per row in
//! [`Row::oracles`]. Every oracle runs the same program text but for a
//! prelude: the imports (Chez's script runs at its interaction environment's
//! top level), a `gc` that does nothing, and for Chez `error-object?` as
//! `condition?`.
//!
//! **The `plus` rows' order is the one exception.** chibi and Chez bind a
//! reference when they compile it, and Gauche inlines `+` when it compiles
//! the call, so in all three a site compiled before the program's `+`
//! keeps the built-in one. That is R7RS §5.2's latitude, which
//! `tests/scheme/stdlib/rebinding.scm` records with its oracles' answers and
//! `DIVERGENCES.tsv` registers. So the oracles are given the program's `+`
//! *before* the site, where every implementation calls it. Patina follows a
//! binding when the code runs, so the two orders are the same program there,
//! and [`the_rebinding_order_leaves_patinas_answer`] checks it on both
//! backends; the rows run the site first, which is the order that sends the
//! VM down the `AddImm`'s shadow path.
//!
//! One row has two oracles. On `plus/return/reenter` Gauche refuses to return
//! a second time from the handler call it made for `raise-continuable`
//! ("attempt to return from a ghost continuation"; under the row's `guard`,
//! `(secondary #t)`), where chibi and Chez return again, as Patina does.
//! Gauche is left out of that row's `oracles`, not counted as agreement.
//!
//! No row differs from its oracles today on either backend.
//!
//! # Showing the rows can fail
//!
//! Run 2026-11-22 in the debug build, each mutation of `retire_registers`
//! (`crates/patina-vm/src/runtime/vm_state/gc_roots.rs`) applied alone and
//! then removed, as a 4d defect in the maps would look:
//!
//! - **A frame at the `AddImm`'s pc scanned with an empty map.** Both row
//!   tests panic with "read of a retired register as an instruction's
//!   operand", at the read of `mine` right after the `AddImm`. The rows that
//!   fail are the four `plus` rows whose site's frame is resumed after a
//!   collection (`callee` and `return`, both actions), on the VM, on default
//!   and zeal interpreters.
//! - **The raise stub's frame scanned with an empty map.** The same panic, at
//!   `ResumeRaise`'s read of the handler, on the eight `return` rows, on the
//!   VM, on both. Under zeal also the four `guard/reenter` rows and `plus`'s
//!   `guard` and `escape` rows, at the stub's pc 0: the collection on the
//!   raise's return described above.
//! - **The frame under a raise stub scanned with an empty map.** Only
//!   `plus/return/collect` and `plus/return/reenter` fail, where that frame
//!   is the site's, resumed at the `AddImm`'s pc. No row of the three error
//!   sites fails, which is what "never resumed" above says.
//!
//! The tree-walker's rows pass under each: the mutations are the VM's.
//!
//! # Re-measuring
//!
//! [`dump_programs`] writes every program out per implementation, ready to
//! run; it is the only thing guaranteeing that the text an oracle is fed is
//! the text this file runs.
//!
//! # Reading a row
//!
//! As in `control_flow_matrix.rs`: each row records `correct` and what each
//! backend answers today, and a row whose answer moves fails in either
//! direction, `common::describe_move` saying which. A panic in a check build
//! fails the test that ran it, with the check's message.
//!
//! # Adding axes
//!
//! Under a handler, each site is called in tail position from a thunk, so
//! the frame a raise stub sits on reuses the thunk's window (`tail_replace`,
//! #714). A position axis, the site called from a non-tail position so that
//! its frame is pushed fresh, would cover the other way a frame comes to be
//! at a raise site; only the `callee` rows call the site that way today.

mod common;

use patina_interpreter::Interpreter;
use patina_runtime::Backend;
use patina_vm::types::Instruction;

/// How a rejection is rendered, so a row that starts failing says which.
/// The message is kept but never compared, as in `control_flow_matrix.rs`.
const ERROR: &str = "<error>";

/// Where the error is raised, or for `plus`, where the program's procedure
/// is called from.
#[derive(Clone, Copy, PartialEq)]
enum Site {
    /// An inline `Car` of 5.
    Car,
    /// An inline `VectorRef` past the vector's end.
    VectorRef,
    /// A `LoadGlobal` of a name nothing defines.
    Unbound,
    /// An inline `AddImm` compiled while `+` was the primitive, run after the
    /// program defined `+`.
    Plus,
}

/// What catches the raise.
#[derive(Clone, Copy, PartialEq)]
enum Handler {
    /// `guard`.
    Guard,
    /// `with-exception-handler`, whose handler escapes through a continuation.
    Escape,
    /// `with-exception-handler`, whose handler returns.
    Return,
    /// No handler: the action runs in the program's `+`. `plus` rows only.
    None,
}

/// What runs while the frames at the site are suspended.
#[derive(Clone, Copy, PartialEq)]
enum Action {
    /// A collection.
    Collect,
    /// A continuation captured there, re-entered after a collection.
    Reenter,
}

const SITES: [Site; 4] = [Site::Car, Site::VectorRef, Site::Unbound, Site::Plus];
const HANDLERS: [Handler; 4] = [
    Handler::Guard,
    Handler::Escape,
    Handler::Return,
    Handler::None,
];
const ACTIONS: [Action; 2] = [Action::Collect, Action::Reenter];

impl Site {
    fn name(self) -> &'static str {
        match self {
            Site::Car => "car",
            Site::VectorRef => "vector-ref",
            Site::Unbound => "unbound",
            Site::Plus => "plus",
        }
    }

    /// The procedure that raises, or that calls `+` from the `AddImm`.
    fn definition(self) -> &'static str {
        match self {
            Site::Car => "(define (site x) (car x))",
            Site::VectorRef => "(define (site v) (vector-ref v 99))",
            Site::Unbound => "(define (site) no-such-variable-4d)",
            Site::Plus => "(define (site x mine) (list (+ x 1) mine))",
        }
    }

    fn call(self) -> &'static str {
        match self {
            Site::Car => "(site 5)",
            Site::Plus => "(site 5 (list 'site 5))",
            Site::VectorRef => "(site (vector 'a 'b 'c))",
            Site::Unbound => "(site)",
        }
    }
}

impl Handler {
    fn name(self) -> &'static str {
        match self {
            Handler::Guard => "guard",
            Handler::Escape => "escape",
            Handler::Return => "return",
            Handler::None => "callee",
        }
    }

    /// Whether `site` can be paired with this handler: the action runs in
    /// the callee only where there is a program's procedure to run it in.
    fn applies_to(self, site: Site) -> bool {
        self != Handler::None || site == Site::Plus
    }
}

impl Action {
    fn name(self) -> &'static str {
        match self {
            Action::Collect => "collect",
            Action::Reenter => "reenter",
        }
    }
}

/// Where a `plus` program defines its `+`: after the site, as the rows run
/// it, or before, as the oracles run it (see the header).
#[derive(Clone, Copy, PartialEq)]
enum Order {
    SiteFirst,
    RebindingFirst,
}

/// One point of the space, with its expected and recorded answers.
struct Row {
    site: Site,
    handler: Handler,
    action: Action,
    /// The answer the implementations in `oracles` give.
    correct: &'static str,
    /// What the VM answers today.
    vm: &'static str,
    /// What the tree-walker answers today.
    tw: &'static str,
    /// The external implementations behind `correct`.
    oracles: &'static str,
    /// The issue that tracks a backend's wrong answer; empty where both are
    /// right.
    issue: &'static str,
}

impl Row {
    fn name(&self) -> String {
        format!(
            "{}/{}/{}",
            self.site.name(),
            self.handler.name(),
            self.action.name()
        )
    }
}

/// What every program defines first: the driver's state, a collection, a
/// capture, and a way to print a raised object that every implementation
/// prints alike.
///
/// The allocation loop is 100 long: 200 allocations, a dozen collections at
/// `PATINA_GC_STRESS=16`. Under zeal the tree-walker collects at every step
/// of it: at 1,000 the zeal test took 40 s in the debug build, where it
/// takes about 5.
const HELPERS: &str = "\
(define saved #f)
(define results '())
(define (churn n)
  (let loop ((i n) (acc '()))
    (if (= i 0) (length acc) (loop (- i 1) (cons (vector i) acc)))))
(define (collect!) (gc) (churn 100) (gc))
(define (capture!)
  (call-with-current-continuation (lambda (c) (set! saved c) 'first)))
(define (describe e) (if (error-object? e) 'error-object e))
";

/// The program's `+` for a `plus` row: one that raises where a handler is
/// under test, and otherwise one that collects or captures and adds without
/// calling `+`.
fn plus_definition(handler: Handler, action: Action) -> &'static str {
    match (handler, action) {
        (Handler::None, Action::Collect) => "(define (+ a b) (collect!) (- a (- 0 b)))",
        (Handler::None, Action::Reenter) => {
            "(define (+ a b)\n  (let ((v (capture!)))\n    (if (eq? v 'first) (- a (- 0 b)) v)))"
        }
        _ => "(define (+ a b) (raise-continuable (list 'plus a b)))",
    }
}

/// The expression whose value `run` binds: the site's call under the row's
/// handler.
fn transfer(row: &Row) -> String {
    let call = row.site.call();
    match (row.handler, row.action) {
        (Handler::Guard, Action::Collect) => format!(
            "(guard (e ((begin (collect!) #f) 'never)\n                   (#t (list 'caught (describe e))))\n             {call})"
        ),
        (Handler::Guard, Action::Reenter) => format!(
            "(guard (o (#t (collect!) (list 'outer (describe o))))\n             (guard (e ((begin (collect!) #f) 'never))\n               {call}))"
        ),
        (Handler::Escape, Action::Collect) => format!(
            "(call-with-current-continuation\n              (lambda (k)\n                (with-exception-handler\n                  (lambda (e) (collect!) (k (list 'escaped (describe e))))\n                  (lambda () {call}))))"
        ),
        (Handler::Escape, Action::Reenter) => format!(
            "(call-with-current-continuation\n              (lambda (k)\n                (with-exception-handler\n                  (lambda (e)\n                    (let ((v (capture!)))\n                      (k (list 'escaped v (describe e)))))\n                  (lambda () {call}))))"
        ),
        (Handler::Return, Action::Collect) => format!(
            "(guard (o (#t (list 'secondary (error-object? o))))\n             (with-exception-handler\n               (lambda (e) (collect!) (describe e))\n               (lambda () {call})))"
        ),
        (Handler::Return, Action::Reenter) => format!(
            "(guard (o (#t (list 'secondary (error-object? o))))\n             (with-exception-handler\n               (lambda (e) (let ((v (capture!))) (list v (describe e))))\n               (lambda () {call})))"
        ),
        (Handler::None, _) => call.to_string(),
    }
}

/// The program for `row`, without a prelude. Its last line is the
/// expression whose value is the answer.
fn program(row: &Row, order: Order) -> String {
    let mut code = String::from(HELPERS);
    let site = row.site.definition();
    let definitions = if row.site == Site::Plus {
        let plus = plus_definition(row.handler, row.action);
        match order {
            Order::SiteFirst => [site, plus],
            Order::RebindingFirst => [plus, site],
        }
    } else {
        [site, ""]
    };
    for definition in definitions.into_iter().filter(|d| !d.is_empty()) {
        code.push_str(definition);
        code.push('\n');
    }
    code.push_str(&format!(
        "(define (run)\n  (let ((keep (list 'keep (vector 'k))))\n    (let ((r {}))\n      (set! results (cons r results))\n      (when saved\n        (let ((c saved))\n          (set! saved #f)\n          (collect!)\n          (c 'again)))\n      (list (reverse results) keep))))\n(run)",
        transfer(row)
    ));
    code
}

/// What Patina's runs put in front of a program: `gc` is `(patina debug)`'s.
const PATINA_PRELUDE: &str = "(import (scheme base) (patina debug))\n";

/// Run `code` on `interp`, rendering a rejection as [`ERROR`].
fn answer<B: Backend>(interp: Interpreter<B>, code: &str) -> String
where
    B::Error: std::fmt::Display,
{
    match interp.eval_program_owned(&format!("{PATINA_PRELUDE}{code}")) {
        Ok(value) => interp.display_tagged(&value),
        Err(e) => {
            let message = e.to_string();
            let first = message.lines().next().unwrap_or("").trim();
            format!("{ERROR}: {first}")
        }
    }
}

/// The interpreters a row runs on.
#[derive(Clone, Copy)]
enum Mode {
    /// The backends' default interpreters, collecting as the lane's
    /// variable says.
    Default,
    /// Zeal interpreters, collecting at every outermost safe point.
    Zeal,
}

/// `code`'s answer on the VM and on the tree-walker.
fn answers(code: &str, mode: Mode) -> [String; 2] {
    match mode {
        Mode::Default => [
            answer(common::vm_interpreter(), code),
            answer(common::tree_walker_interpreter(), code),
        ],
        Mode::Zeal => [
            answer(common::vm_interpreter_zeal(), code),
            answer(common::tree_walker_interpreter_zeal(), code),
        ],
    }
}

/// Every row, as it answers on 2026-11-22. All 26 agree with their oracles
/// on both backends.
#[rustfmt::skip]
const ROWS: &[Row] = &[
    // ---- car: an inline `Car` of 5 ---------------------------------------
    Row { site: Site::Car, handler: Handler::Guard, action: Action::Collect,
          correct: "(((caught error-object)) (keep #(k)))", vm: "(((caught error-object)) (keep #(k)))", tw: "(((caught error-object)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Car, handler: Handler::Guard, action: Action::Reenter,
          correct: "(((outer error-object)) (keep #(k)))", vm: "(((outer error-object)) (keep #(k)))", tw: "(((outer error-object)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Car, handler: Handler::Escape, action: Action::Collect,
          correct: "(((escaped error-object)) (keep #(k)))", vm: "(((escaped error-object)) (keep #(k)))", tw: "(((escaped error-object)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Car, handler: Handler::Escape, action: Action::Reenter,
          correct: "(((escaped first error-object) (escaped again error-object)) (keep #(k)))", vm: "(((escaped first error-object) (escaped again error-object)) (keep #(k)))", tw: "(((escaped first error-object) (escaped again error-object)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Car, handler: Handler::Return, action: Action::Collect,
          correct: "(((secondary #t)) (keep #(k)))", vm: "(((secondary #t)) (keep #(k)))", tw: "(((secondary #t)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Car, handler: Handler::Return, action: Action::Reenter,
          correct: "(((secondary #t) (secondary #t)) (keep #(k)))", vm: "(((secondary #t) (secondary #t)) (keep #(k)))", tw: "(((secondary #t) (secondary #t)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },

    // ---- vector-ref: an inline `VectorRef` past the end ------------------
    Row { site: Site::VectorRef, handler: Handler::Guard, action: Action::Collect,
          correct: "(((caught error-object)) (keep #(k)))", vm: "(((caught error-object)) (keep #(k)))", tw: "(((caught error-object)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::VectorRef, handler: Handler::Guard, action: Action::Reenter,
          correct: "(((outer error-object)) (keep #(k)))", vm: "(((outer error-object)) (keep #(k)))", tw: "(((outer error-object)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::VectorRef, handler: Handler::Escape, action: Action::Collect,
          correct: "(((escaped error-object)) (keep #(k)))", vm: "(((escaped error-object)) (keep #(k)))", tw: "(((escaped error-object)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::VectorRef, handler: Handler::Escape, action: Action::Reenter,
          correct: "(((escaped first error-object) (escaped again error-object)) (keep #(k)))", vm: "(((escaped first error-object) (escaped again error-object)) (keep #(k)))", tw: "(((escaped first error-object) (escaped again error-object)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::VectorRef, handler: Handler::Return, action: Action::Collect,
          correct: "(((secondary #t)) (keep #(k)))", vm: "(((secondary #t)) (keep #(k)))", tw: "(((secondary #t)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::VectorRef, handler: Handler::Return, action: Action::Reenter,
          correct: "(((secondary #t) (secondary #t)) (keep #(k)))", vm: "(((secondary #t) (secondary #t)) (keep #(k)))", tw: "(((secondary #t) (secondary #t)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },

    // ---- unbound: a `LoadGlobal` of a name nothing defines ---------------
    // chibi warns on stderr when it compiles the reference, and raises when
    // it runs, as the others do.
    Row { site: Site::Unbound, handler: Handler::Guard, action: Action::Collect,
          correct: "(((caught error-object)) (keep #(k)))", vm: "(((caught error-object)) (keep #(k)))", tw: "(((caught error-object)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Unbound, handler: Handler::Guard, action: Action::Reenter,
          correct: "(((outer error-object)) (keep #(k)))", vm: "(((outer error-object)) (keep #(k)))", tw: "(((outer error-object)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Unbound, handler: Handler::Escape, action: Action::Collect,
          correct: "(((escaped error-object)) (keep #(k)))", vm: "(((escaped error-object)) (keep #(k)))", tw: "(((escaped error-object)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Unbound, handler: Handler::Escape, action: Action::Reenter,
          correct: "(((escaped first error-object) (escaped again error-object)) (keep #(k)))", vm: "(((escaped first error-object) (escaped again error-object)) (keep #(k)))", tw: "(((escaped first error-object) (escaped again error-object)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Unbound, handler: Handler::Return, action: Action::Collect,
          correct: "(((secondary #t)) (keep #(k)))", vm: "(((secondary #t)) (keep #(k)))", tw: "(((secondary #t)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Unbound, handler: Handler::Return, action: Action::Reenter,
          correct: "(((secondary #t) (secondary #t)) (keep #(k)))", vm: "(((secondary #t) (secondary #t)) (keep #(k)))", tw: "(((secondary #t) (secondary #t)) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },

    // ---- plus: the program's `+`, called from an inline `AddImm`'s pc -------
    // The oracles ran each with the program's `+` defined first (see the
    // header). `(site 5)` in each value is `mine`, which only the site's
    // frame held across the `AddImm`.
    Row { site: Site::Plus, handler: Handler::Guard, action: Action::Collect,
          correct: "(((caught (plus 5 1))) (keep #(k)))", vm: "(((caught (plus 5 1))) (keep #(k)))", tw: "(((caught (plus 5 1))) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Plus, handler: Handler::Guard, action: Action::Reenter,
          correct: "(((outer (plus 5 1))) (keep #(k)))", vm: "(((outer (plus 5 1))) (keep #(k)))", tw: "(((outer (plus 5 1))) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Plus, handler: Handler::Escape, action: Action::Collect,
          correct: "(((escaped (plus 5 1))) (keep #(k)))", vm: "(((escaped (plus 5 1))) (keep #(k)))", tw: "(((escaped (plus 5 1))) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    Row { site: Site::Plus, handler: Handler::Escape, action: Action::Reenter,
          correct: "(((escaped first (plus 5 1)) (escaped again (plus 5 1))) (keep #(k)))", vm: "(((escaped first (plus 5 1)) (escaped again (plus 5 1))) (keep #(k)))", tw: "(((escaped first (plus 5 1)) (escaped again (plus 5 1))) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    // The handler's value goes back through `+` into the `AddImm`'s
    // destination, after the collection.
    Row { site: Site::Plus, handler: Handler::Return, action: Action::Collect,
          correct: "((((plus 5 1) (site 5))) (keep #(k)))", vm: "((((plus 5 1) (site 5))) (keep #(k)))", tw: "((((plus 5 1) (site 5))) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    // Gauche refuses the second return: see the header.
    Row { site: Site::Plus, handler: Handler::Return, action: Action::Reenter,
          correct: "((((first (plus 5 1)) (site 5)) ((again (plus 5 1)) (site 5))) (keep #(k)))", vm: "((((first (plus 5 1)) (site 5)) ((again (plus 5 1)) (site 5))) (keep #(k)))", tw: "((((first (plus 5 1)) (site 5)) ((again (plus 5 1)) (site 5))) (keep #(k)))",
          oracles: "chibi, chez", issue: "" },
    Row { site: Site::Plus, handler: Handler::None, action: Action::Collect,
          correct: "(((6 (site 5))) (keep #(k)))", vm: "(((6 (site 5))) (keep #(k)))", tw: "(((6 (site 5))) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
    // The re-entry into the `AddImm`'s pc: `again` is the value `+` returns the
    // second time, delivered into the site's reinstated frame.
    Row { site: Site::Plus, handler: Handler::None, action: Action::Reenter,
          correct: "(((6 (site 5)) (again (site 5))) (keep #(k)))", vm: "(((6 (site 5)) (again (site 5))) (keep #(k)))", tw: "(((6 (site 5)) (again (site 5))) (keep #(k)))",
          oracles: "chibi, gauche, chez", issue: "" },
];

/// Score every row on `mode`'s interpreters against what the table records.
fn score(mode: Mode) {
    let mut moved = Vec::new();
    for row in ROWS {
        let code = program(row, Order::SiteFirst);
        let [vm, tw] = answers(&code, mode);
        for (backend, recorded, got) in [("vm", row.vm, vm), ("tree-walker", row.tw, tw)] {
            if got != recorded {
                moved.push(common::describe_move(
                    &row.name(),
                    backend,
                    recorded,
                    &got,
                    row.correct,
                    &code,
                ));
            }
        }
    }
    assert!(
        moved.is_empty(),
        "{} raise-site row(s) no longer answer what the table records.\n\n{}\n\n\
         Update the row(s) in ROWS and the count in `the_defect_count_is_what_it_says`.",
        moved.len(),
        moved.join("\n\n")
    );
}

/// Every row answers what the table says, on both backends, on interpreters
/// that collect as the lane's variable says.
///
/// It fails in both directions, as `control_flow_matrix.rs`'s does: a fixed
/// defect turns its row red just as a broken shape does.
#[test]
fn rows_score_as_recorded() {
    score(Mode::Default);
}

/// The same, with a collection at every outermost safe point, whatever the
/// lane's variable says, so that every lane collects with the stub over the
/// erroring frame and with the site's frame at the `AddImm`'s pc.
#[test]
fn rows_score_as_recorded_under_zeal() {
    score(Mode::Zeal);
}

/// The `plus` programs in the oracles' order, the program's `+` defined
/// before the site, answer as the rows do, on both backends: Patina follows
/// a binding when the code runs, so the order the rows run, which is the one
/// that reaches the `AddImm`'s shadow path on the VM, is the program the
/// oracles answered.
#[test]
fn the_rebinding_order_leaves_patinas_answer() {
    let mut moved = Vec::new();
    for row in ROWS.iter().filter(|row| row.site == Site::Plus) {
        let code = program(row, Order::RebindingFirst);
        let [vm, tw] = answers(&code, Mode::Default);
        for (backend, got) in [("vm", vm), ("tree-walker", tw)] {
            if got != row.correct {
                moved.push(format!(
                    "  {} [{backend}] with the program's + defined first\n    got      {got}\n    \
                     correct  {}",
                    row.name(),
                    row.correct
                ));
            }
        }
    }
    assert!(moved.is_empty(), "{}", moved.join("\n"));
}

/// What the VM compiles `site`'s procedure to after `setup` has run, as
/// `--dump` prints it, kept to the instructions the rows depend on: the
/// three inline operations, each global read with its operand, and each
/// call, ordinary or of a primitive by its id.
fn compiled(setup: &str, site: Site) -> Vec<String> {
    let interp = common::vm_interpreter();
    interp.eval_program_owned(setup).expect("the setup runs");
    let inspected = interp
        .backend()
        .compile_for_inspection(site.definition())
        .expect("the site compiles");
    let mut found = Vec::new();
    for (top, nested) in inspected.units() {
        for code in std::iter::once(top).chain(nested) {
            for instr in &code.instructions {
                let shown = match instr {
                    Instruction::Car { .. } => "Car".to_string(),
                    Instruction::VectorRef { .. } => "VectorRef".to_string(),
                    Instruction::Add { .. } => "Add".to_string(),
                    Instruction::AddImm { .. } => "AddImm".to_string(),
                    Instruction::LoadGlobal { link, .. } => {
                        format!("LoadGlobal {}", code.links.name(*link))
                    }
                    Instruction::Call { .. } | Instruction::TailCall { .. } => "Call".to_string(),
                    Instruction::CallPrimitive { .. } => "CallPrimitive".to_string(),
                    _ => continue,
                };
                found.push(shown);
            }
        }
    }
    found
}

/// Each site compiles to the instruction the header names, in the state
/// its row compiles it in, so a compiler change that stopped inlining one
/// would fail here rather than leave its rows testing an ordinary call.
#[test]
fn each_site_compiles_to_the_instruction_it_names() {
    let imports = "(import (scheme base))";
    assert_eq!(compiled(imports, Site::Car), ["Car"], "car");
    assert_eq!(
        compiled(imports, Site::VectorRef),
        ["VectorRef"],
        "vector-ref"
    );
    assert_eq!(
        compiled(imports, Site::Unbound),
        ["LoadGlobal no-such-variable-4d"],
        "unbound"
    );

    // The rows' order: the site compiled while `+` is the primitive. The
    // `list` is a call of a primitive by its id; `+` is the `AddImm`, with
    // no global read of `+` and no ordinary call.
    let plus = compiled(imports, Site::Plus);
    assert_eq!(
        plus.iter().filter(|shown| *shown == "AddImm").count(),
        1,
        "plus, site first: {plus:?}"
    );
    assert!(
        !plus
            .iter()
            .any(|shown| shown == "Call" || shown == "LoadGlobal +"),
        "plus, site first: {plus:?}"
    );

    // The oracles' order: compiled after the program defines `+`, the site
    // calls it by an ordinary call, which is why the rows run the other one.
    let rebinding = format!(
        "{imports} {}",
        plus_definition(Handler::Guard, Action::Collect)
    );
    let plus = compiled(&rebinding, Site::Plus);
    assert!(
        !plus.iter().any(|shown| shown == "Add" || shown == "AddImm")
            && plus.iter().any(|shown| shown == "LoadGlobal +")
            && plus.iter().any(|shown| shown == "Call"),
        "plus, rebinding first: {plus:?}"
    );
}

/// How many rows each backend gets wrong, asserted as a number, and the
/// table's own consistency.
#[test]
fn the_defect_count_is_what_it_says() {
    let wrong = |pick: fn(&Row) -> &'static str| {
        ROWS.iter()
            .filter(|row| pick(row) != row.correct)
            .map(Row::name)
            .collect::<Vec<_>>()
    };
    // Zero on both backends. A row going red is a regression on a shape the
    // reference implementations agree about, whichever direction it moved.
    let vm = wrong(|row| row.vm);
    assert_eq!(vm.len(), 0, "VM rows wrong: {vm:?}");
    let tw = wrong(|row| row.tw);
    assert_eq!(tw.len(), 0, "tree-walker rows wrong: {tw:?}");

    for row in ROWS {
        let wrong_somewhere = row.vm != row.correct || row.tw != row.correct;
        assert_eq!(
            wrong_somewhere,
            !row.issue.is_empty(),
            "{}: `issue` must be set exactly on rows a backend gets wrong",
            row.name()
        );
        assert!(
            !row.oracles.is_empty(),
            "{}: every row records which implementations answer its `correct`",
            row.name()
        );
    }
}

/// The table is the space's every point, sites by handlers by actions, less
/// the callee rows of the sites with no program's procedure, with no row
/// written twice.
#[test]
fn the_matrix_is_a_complete_cross_product() {
    let mut expected = Vec::new();
    for site in SITES {
        for handler in HANDLERS.into_iter().filter(|h| h.applies_to(site)) {
            for action in ACTIONS {
                expected.push(format!(
                    "{}/{}/{}",
                    site.name(),
                    handler.name(),
                    action.name()
                ));
            }
        }
    }
    let mut present: Vec<String> = ROWS.iter().map(Row::name).collect();
    let total = present.len();
    present.sort();
    present.dedup();
    assert_eq!(present.len(), total, "duplicate rows in ROWS");
    let missing: Vec<&String> = expected.iter().filter(|n| !present.contains(n)).collect();
    let extra: Vec<&String> = present.iter().filter(|n| !expected.contains(n)).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "ROWS is not the space.\n  missing: {missing:?}\n  extra:   {extra:?}"
    );
}

/// Write every program out, once per implementation, ready to run.
///
/// Ignored by default: it asserts nothing, and exists so the table can be
/// re-measured rather than re-derived. Each file is the program, its
/// implementation's prelude, and a `write` around the last expression. The
/// oracles get the `plus` programs in their order, the program's `+`
/// defined first; Patina gets the rows' order and, as `*.rebinding-first.scm`,
/// the oracles'. The program text is otherwise never edited.
///
/// ```text
/// RAISE_SITE_MATRIX_DUMP=/tmp/rsm cargo test -p patina-tests \
///     --test raise_site_matrix -- --ignored dump_programs
/// for f in /tmp/rsm/chibi/*.scm;  do chibi-scheme "$f"; done
/// for f in /tmp/rsm/gauche/*.scm; do gosh -r7 "$f"; done
/// for f in /tmp/rsm/chez/*.scm;   do chez --script "$f"; done
/// for f in /tmp/rsm/patina/*.scm; do patina "$f"; patina --tree-walker "$f"; done
/// ```
#[test]
#[ignore]
fn dump_programs() {
    const PRELUDES: &[(&str, &str)] = &[
        (
            "patina",
            "(import (scheme base) (scheme write) (patina debug))\n",
        ),
        (
            "chibi",
            "(import (scheme base) (scheme write))\n(define (gc) #f)\n",
        ),
        (
            "gauche",
            "(import (scheme base) (scheme write))\n(define (gc) #f)\n",
        ),
        (
            "chez",
            "(define (gc) #f)\n(define (error-object? x) (condition? x))\n",
        ),
    ];
    let dir =
        std::env::var("RAISE_SITE_MATRIX_DUMP").expect("set RAISE_SITE_MATRIX_DUMP to a directory");
    for (implementation, prelude) in PRELUDES {
        let sub = format!("{dir}/{implementation}");
        std::fs::create_dir_all(&sub).expect("create dump directory");
        for row in ROWS {
            let orders: &[(Order, &str)] = match (row.site, *implementation) {
                (Site::Plus, "patina") => &[
                    (Order::SiteFirst, ""),
                    (Order::RebindingFirst, ".rebinding-first"),
                ],
                (Site::Plus, _) => &[(Order::RebindingFirst, "")],
                _ => &[(Order::SiteFirst, "")],
            };
            for (order, suffix) in orders {
                let code = program(row, *order);
                let (body, last) = code
                    .rsplit_once('\n')
                    .expect("every program has more than one line");
                let path = format!("{sub}/{}{suffix}.scm", row.name().replace('/', "_"));
                std::fs::write(&path, format!("{prelude}{body}\n(write {last})(newline)\n"))
                    .expect("write program");
            }
        }
    }
}
