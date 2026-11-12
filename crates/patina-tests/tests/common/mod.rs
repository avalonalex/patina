//! Test helpers for R7RS compliance testing
//!
//! Provides utilities for writing concise tests comparing Patina output
//! against expected values.
//!
//! # Every helper here runs on both backends
//!
//! Patina's central architectural claim is that the tree-walker and the VM
//! implement the same language. These helpers make that a *checked* claim
//! rather than a stated one: each one evaluates the program on both backends
//! and holds both to the same expectation, so a divergence fails a test
//! instead of waiting to be found by hand. The failure message always names
//! which backend produced what, because the culprit is usually the backend
//! you were not thinking about.
//!
//! Note the scope: this covers the tests that *use these helpers*. Several
//! test files still construct an interpreter directly and remain
//! tree-walker-only — porting them is tracked as Q1 in
//! `PRD/TRACK_Q_QUALITY_PRD.md`. Prefer these helpers in new tests.
//!
//! A known divergence between the backends is not declared here. It is a row
//! in a suite file under `tests/scheme/` with a backend-scoped expectation
//! above it —
//!
//!     (cond-expand (patina-tree-walker (test-expect-fail 1)) (else))
//!     (test-equal "the row's name" <the right answer> <the program>)
//!
//! — which asserts the right answer on every implementation and names the
//! backend known to miss it; `scheme_suite.rs` fails the run the day that
//! backend starts passing, so the quarantine retires itself instead of
//! outliving the defect. `rg 'patina-(vm|tree-walker) \(test-expect-fail'
//! crates/patina-tests/tests/scheme` is the inventory, plus the per-backend
//! value pins the grep cannot see (`callability.rs`'s continuation-as-converter)
//! and the two matrix files, which record every backend's answer per row.
//!
//! That spelling needs the wrong backend's answer to be *delivered to the
//! row* — a wrong value, or an error a `guard` can catch. `assert_divergence`
//! was the Rust spelling for the rest, a divergence that took the whole
//! program down or ran the rest of the file from inside a callback; the last
//! of those, the tree-walker's nested-trampoline family, closed 2026-09-10
//! and the helper went with it (`git log -S assert_divergence` has it, should
//! the class reappear).

#![allow(dead_code)]
// `gc_shared_tests!` is used only by the GC test binaries; every other test
// file that includes this module compiles it unused.
#![allow(unused_macros)]

use patina_core::GcMode;
use patina_interpreter::{Interpreter, InterpreterError, TreeWalkInterpreter, TreeWalker};
use patina_runtime::Backend;
use patina_vm::VmBackend;

// ─── Backend selection ───────────────────────────────────────────────────────

/// Which backends one run covers. Private: it is an implementation detail of
/// the helpers below, not a knob tests turn.
///
/// Selecting a single backend is a real need — a pin of one backend's wrong
/// *value* has no both-backend spelling (`callability.rs`'s
/// continuation-as-converter row). Those use the named per-backend helpers
/// (`eval_program_vm`, `try_eval_program_tree_walker`, …), which say in their
/// name what they do; threading this enum out would give the same capability
/// a second, vaguer spelling.
#[derive(Clone, Copy)]
enum Which {
    Both,
    TreeWalker,
    Vm,
}

/// The workspace root, for tests that read repo files (bundled libraries,
/// upstream suites). Encodes the crates/patina-tests → root distance once.
pub fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

/// The library root the test lanes supply with `-A`: third-party libraries
/// Patina needs in order to run other people's code, but does not bundle.
/// See `test-lib/README.md`.
pub fn test_lib_root() -> std::path::PathBuf {
    repo_root().join("test-lib")
}

/// A tree-walker with [`test_lib_root`] on its search path — this lane's
/// spelling of the `-A test-lib` the shell lanes pass.
///
/// Every interpreter the helpers below build goes through here, so a test can
/// import a supplied library without arranging anything. What keeps that from
/// blurring the shipped/supplied line is that the guards enforcing it decide
/// by *path* rather than by what happens to resolve:
/// `primitives_reachable_by_import.rs` walks `lib/` alone, and
/// `upstream_srfi_suites.rs` walks both roots but knows which is which.
/// (`bundled_provenance.rs` deliberately spans both — its subject is upstream
/// drift, not the shipped surface.) The binary's own behaviour, with no `-A`
/// at all, is pinned in `crates/patina-repl/tests/cli_options.rs`.
pub fn tree_walker_interpreter() -> TreeWalkInterpreter {
    tree_walker_interpreter_with(TreeWalkInterpreter::new_tree_walker())
}

/// [`tree_walker_interpreter`] over a caller-supplied filesystem, for tests
/// that need an `OverlayFs`. Shares the wiring so a change to the supplied
/// roots cannot reach one constructor and miss the other.
pub fn tree_walker_interpreter_with_fs(
    fs: std::sync::Arc<dyn patina_core::FileSystem>,
) -> TreeWalkInterpreter {
    tree_walker_interpreter_with(TreeWalkInterpreter::new_tree_walker_with_fs(fs))
}

fn tree_walker_interpreter_with(interp: TreeWalkInterpreter) -> TreeWalkInterpreter {
    interp.backend().add_library_search_path(test_lib_root());
    interp
}

/// Evaluate `code` on both backends with **only the shipped tree** on the
/// search path — no `test-lib/`, no supplied root of any kind.
///
/// For the handful of tests whose subject *is* that something resolves
/// unaided. Every other helper here supplies the root, which is right for
/// them and fatal for these: a test asserting "this chain resolves from
/// `lib/` with no help" passes vacuously the moment the helper hands it help.
pub fn eval_program_shipped_only(code: &str) -> String {
    let tw = run_on(TreeWalkInterpreter::new_tree_walker(), code, Mode::Program);
    let vm = run_on(Interpreter::new(VmBackend::new()), code, Mode::Program);
    match (tw, vm) {
        (Ok(tw), Ok(vm)) => {
            assert_eq!(tw, vm, "backends disagree with only lib/ on the path");
            tw
        }
        (tw, vm) => panic!(
            "expected {code:?} to evaluate with only lib/ on the search path\n\
             tree-walker: {tw:?}\nvm: {vm:?}"
        ),
    }
}

/// The error from evaluating `code` with only the shipped tree on the search
/// path. The negative counterpart of [`eval_program_shipped_only`], for tests
/// asserting that something is *not* reachable without a supplied root.
pub fn eval_program_shipped_only_err(code: &str) -> String {
    let tw = run_on(TreeWalkInterpreter::new_tree_walker(), code, Mode::Program);
    let vm = run_on(Interpreter::new(VmBackend::new()), code, Mode::Program);
    match (tw, vm) {
        // Both must fail. The two resolvers word it differently, so the caller
        // gets both texts to match against rather than one.
        (Err(tw), Err(vm)) => format!("{} | {}", tw.message, vm.message),
        (tw, vm) => panic!(
            "expected {code:?} to fail on BOTH backends with only lib/ on the path — \
             a resolver that leaks a supplied root is exactly what this checks\n\
             tree-walker: {tw:?}\nvm: {vm:?}"
        ),
    }
}

/// A VM interpreter with [`test_lib_root`] on its search path.
pub fn vm_interpreter() -> Interpreter<VmBackend> {
    vm_interpreter_with(VmBackend::new())
}

fn vm_interpreter_with(backend: VmBackend) -> Interpreter<VmBackend> {
    let interp = Interpreter::new(backend);
    interp.backend().add_library_search_path(test_lib_root());
    interp
}

/// [`vm_interpreter`], collecting only when `(gc)` asks, whatever
/// `PATINA_GC_STRESS` or `PATINA_GC_ZEAL` say: the not-collecting side of a
/// test that compares collecting with not collecting. Under a stress lane's
/// variable a backend made from the environment collects too, and the
/// comparison fails without anything being wrong (#626).
pub fn vm_interpreter_gc_off() -> Interpreter<VmBackend> {
    vm_interpreter_with(VmBackend::with_gc_mode(GcMode::Off))
}

/// [`vm_interpreter`], collecting where the adaptive byte threshold says,
/// as a program run with no GC variable set does (`GcMode::On`), whatever
/// `PATINA_GC_STRESS` or `PATINA_GC_ZEAL` say: for a test of the pacing
/// itself, which stress and zeal replace with a count of allocations and
/// with every safe point. Its collections count in the stress lane's record
/// whatever the lane's variable says.
pub fn vm_interpreter_adaptive() -> Interpreter<VmBackend> {
    vm_interpreter_with(VmBackend::with_gc_mode(GcMode::On))
}

/// The tree-walker half of [`vm_interpreter_gc_off`].
pub fn tree_walker_interpreter_gc_off() -> TreeWalkInterpreter {
    tree_walker_interpreter_with(Interpreter::new(TreeWalker::with_gc_mode(GcMode::Off)))
}

/// [`vm_interpreter`], collecting at every outermost safe point
/// (`GcMode::Zeal`, what `PATINA_GC_ZEAL=entry` selects), whatever the
/// environment says: for a test that must hold with a collection at each
/// of them, allocating or not, in every lane, the plain debug run included.
/// On the VM that is a frame's entry or a generic call's return (#700).
/// Stress counts allocations, so a stretch that allocates nothing never
/// collects under it (#625). Its collections count in the stress lane's
/// record whatever the lane's variable says.
pub fn vm_interpreter_zeal() -> Interpreter<VmBackend> {
    vm_interpreter_with(VmBackend::with_gc_mode(GcMode::Zeal))
}

/// The tree-walker half of [`vm_interpreter_zeal`].
pub fn tree_walker_interpreter_zeal() -> TreeWalkInterpreter {
    tree_walker_interpreter_with(Interpreter::new(TreeWalker::with_gc_mode(GcMode::Zeal)))
}

/// A path inside a caller-owned [`tempfile::TempDir`], which deletes the
/// whole directory when it drops. Tests that hand a path to a Scheme program
/// need it as a `String`; the guard stays with the caller so the directory
/// outlives the program.
pub fn scratch_path(dir: &tempfile::TempDir, name: &str) -> String {
    dir.path().join(name).to_string_lossy().into_owned()
}

/// Every file under `root`, recursively, in unspecified order. Callers filter
/// by extension. Panics on an unreadable directory — in a test, an IO failure
/// should be loud, not an empty listing that lets an assertion pass vacuously.
pub fn files_under(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("readable dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

/// Library names for every `.sld` under `root`, e.g. `lib/srfi/130.sld` ->
/// `["srfi", "130"]` (for `root` = `lib/`). The one home for the ".sld path
/// is the library name" convention — three tests had grown three copies,
/// disagreeing on extension matching and path-separator handling.
pub fn shipped_libraries(root: &std::path::Path) -> Vec<Vec<String>> {
    files_under(root)
        .into_iter()
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("sld"))
        .map(|path| {
            path.strip_prefix(root)
                .expect("under the given root")
                .with_extension("")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .collect()
}

// ─── Matrix reporting ────────────────────────────────────────────────────────

/// Describe one row of a scoring matrix that stopped answering what it
/// recorded.
///
/// Shared by `hygiene_matrix.rs` and `control_flow_matrix.rs`, which record
/// the same four things per row — what was recorded, what came back, what the
/// oracles say, and the program — and had the same three-arm reading of the
/// difference written out twice. A third matrix would have made it three. The
/// *reading* is the part worth having in one place: whether a row moved
/// **toward** the oracles or away from them is what tells the reader if they
/// are looking at a fix or a regression, and getting that backwards sends
/// someone hunting the wrong direction.
pub fn describe_move(
    name: &str,
    backend: &str,
    recorded: &str,
    got: &str,
    correct: &str,
    program: &str,
) -> String {
    let direction = if got == correct {
        "FIXED — it now matches the reference implementations"
    } else if recorded == correct {
        "REGRESSED — it used to match the reference implementations"
    } else {
        "changed, and is still wrong"
    };
    let indented = program
        .lines()
        .map(|l| format!("      {l}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "  {name} [{backend}] {direction}\n    recorded {recorded}\n    got      {got}\n\
         \x20   correct  {correct}\n    program:\n{indented}"
    )
}

// ─── Error classes ───────────────────────────────────────────────────────────

/// The coarse class of a failure, compared between backends (audit D3).
///
/// Error *text* is deliberately not compared: the backends legitimately word
/// the same failure differently, so pinning prose would turn every message
/// improvement into a test break. The *stage* is compared — one backend
/// rejecting a program before it runs while the other fails it at run time is
/// a real divergence, not a wording difference.
///
/// Two buckets is all the VM's `Backend` boundary preserves today — see
/// `VmBackendError`, whose `Runtime` variant carries only a rendered string.
/// Refining this split means carrying a class across that boundary first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorClass {
    /// The pipeline rejected the program before it ran: lex, parse, desugar,
    /// or VM compile.
    BeforeRun,
    /// The program was accepted and failed during evaluation.
    AtRuntime,
}

/// How a backend's error maps onto [`ErrorClass`]. Implemented per concrete
/// error type because the class is no longer recoverable from the rendered
/// message.
trait ClassifyError {
    fn class(&self) -> ErrorClass;
}

impl ClassifyError for patina_runtime::EvalError {
    // Desugar failures arrive here rather than via `InterpreterError::Desugar`
    // — see `EvalError::DesugarError`. Matched exhaustively so adding a
    // variant forces a classification decision instead of silently defaulting
    // to AtRuntime.
    fn class(&self) -> ErrorClass {
        use patina_runtime::EvalError::*;
        match self {
            WithLocation { error, .. } | WithDiagnostic { error, .. } => error.class(),
            DesugarError(_) => ErrorClass::BeforeRun,
            UndefinedVariable(_)
            | NotAProcedure(_)
            | WrongArity { .. }
            | InvalidSyntax(_)
            | TypeError(_)
            | DivisionByZero
            | IndexOutOfBounds(_)
            | IOError(_)
            | InternalError(_)
            | ContinuationEscape
            | Interrupt
            | SchemeException { .. } => ErrorClass::AtRuntime,
        }
    }
}

impl ClassifyError for patina_vm::VmBackendError {
    fn class(&self) -> ErrorClass {
        match self {
            patina_vm::VmBackendError::Compile(_) | patina_vm::VmBackendError::Desugar { .. } => {
                ErrorClass::BeforeRun
            }
            patina_vm::VmBackendError::Runtime { .. } => ErrorClass::AtRuntime,
            patina_vm::VmBackendError::WithDiagnostic { error, .. } => error.class(),
        }
    }
}

/// A failure with its coarse class kept next to the rendered message. The
/// class is asserted; the message is only ever shown.
#[derive(Debug)]
struct TestError {
    class: ErrorClass,
    message: String,
}

/// What one backend did with a program: the `write`-formatted value, or the
/// classified error.
type Outcome = Result<String, TestError>;

/// Whether the code is a whole program (`eval_program`) or a single
/// expression (`eval_str`). The two entry points differ in how they treat
/// multiple top-level forms, so tests must keep using the one they chose.
/// `HostProgram` is a whole program run in a host environment: the
/// interpreter's fresh `new_environment()`, through `eval_program_in`.
#[derive(Clone, Copy)]
enum Mode {
    Expr,
    Program,
    HostProgram,
}

/// Run `code` on one backend, capturing either its value or its error.
///
/// Generic over `B: Backend` — `global_env()` is on the trait and
/// `display_tagged` is the same writer for both backends, so one body serves
/// the tree-walker and the VM.
fn run_on<B: Backend>(interp: Interpreter<B>, code: &str, mode: Mode) -> Outcome
where
    B::Error: ClassifyError,
{
    let result = match mode {
        Mode::Expr => interp.eval_str_owned(code),
        Mode::Program => interp.eval_program_owned(code),
        Mode::HostProgram => {
            let env = interp.new_environment();
            interp.eval_program_in(&env, code)
        }
    };
    match result {
        Ok(value) => Ok(interp.display_tagged(&value)),
        Err(e) => {
            let class = match &e {
                InterpreterError::Lex(_)
                | InterpreterError::Parse(_)
                | InterpreterError::Desugar(_) => ErrorClass::BeforeRun,
                InterpreterError::Backend(backend_error) => backend_error.class(),
            };
            Err(TestError {
                class,
                message: e.to_string(),
            })
        }
    }
}

/// Run `code` on each backend `which` selects, labelled for failure messages.
///
/// Matched exhaustively rather than tested with `!=`, so adding a backend is a
/// compile error here instead of silently defaulting to running both.
fn outcomes(which: Which, code: &str, mode: Mode) -> Vec<(&'static str, Outcome)> {
    let tree_walker = || ("tree-walker", run_on(tree_walker_interpreter(), code, mode));
    let vm = || ("vm", run_on(vm_interpreter(), code, mode));
    match which {
        Which::Both => vec![tree_walker(), vm()],
        Which::TreeWalker => vec![tree_walker()],
        Which::Vm => vec![vm()],
    }
}

/// Assert every selected backend evaluates `code` to `expected`.
fn expect_value(which: Which, code: &str, expected: &str, mode: Mode) {
    for (backend, outcome) in outcomes(which, code, mode) {
        match outcome {
            Ok(actual) => assert_eq!(
                actual, expected,
                "\n[{backend}] wrong result\nProgram:\n{code}\nExpected: {expected}\nGot: {actual}"
            ),
            Err(e) => panic!(
                "\n[{backend}] failed to evaluate\nProgram:\n{code}\nError: {}",
                e.message
            ),
        }
    }
}

/// Assert every selected backend rejects `code` — at the same [`ErrorClass`].
///
/// One backend erroring where the other succeeds is itself a divergence — the
/// exact shape of the control-operator cluster in Track Q §1.2 — so this is a
/// stronger check than the single-backend version it replaces.
fn expect_error(which: Which, code: &str, mode: Mode) {
    let mut failures = Vec::new();
    for (backend, outcome) in outcomes(which, code, mode) {
        match outcome {
            Ok(value) => panic!("\n[{backend}] expected an error\nProgram:\n{code}\nGot: {value}"),
            Err(e) => failures.push((backend, e)),
        }
    }
    // Every backend must reject at the same stage as the first — written over
    // the rest of the list, not a two-element pattern, so a third backend
    // joins the check instead of silently disabling it.
    if let Some(((b1, e1), rest)) = failures.split_first() {
        for (b2, e2) in rest {
            assert_eq!(
                e1.class, e2.class,
                "\nbackends both fail, but at different stages\nProgram:\n{code}\n\
                 [{b1}] {:?}: {}\n[{b2}] {:?}: {}",
                e1.class, e1.message, e2.class, e2.message
            );
        }
    }
}

// ─── Public helpers ──────────────────────────────────────────────────────────

/// `interp` with every safe point's collection turned off.
///
/// The switch is on the interpreter's heap (`Heap::set_skip_safe_points`, a
/// `test-support` hook), so the only collections are the ones a primitive
/// runs at its call: what shows that `(gc)` collects there (#639), since a
/// safe point after the call (the tree-walker's next step, a VM poll site)
/// could otherwise collect what it posts before anything after the call
/// could look.
fn without_safe_points<B: Backend>(interp: Interpreter<B>) -> Interpreter<B> {
    interp
        .backend()
        .heap()
        .borrow_mut()
        .set_skip_safe_points(true);
    interp
}

/// Assert that `code`, run as a program on both backends with every safe
/// point's collection turned off ([`without_safe_points`]), produces
/// `expected`.
pub fn assert_program_eval_to_without_safe_points(code: &str, expected: &str) {
    let runs = [
        (
            "tree-walker",
            run_on(
                without_safe_points(tree_walker_interpreter()),
                code,
                Mode::Program,
            ),
        ),
        (
            "vm",
            run_on(without_safe_points(vm_interpreter()), code, Mode::Program),
        ),
    ];
    for (backend, outcome) in runs {
        match outcome {
            Ok(actual) => assert_eq!(
                actual, expected,
                "\n[{backend}] wrong result with safe points skipped\nProgram:\n{code}\n\
                 Expected: {expected}\nGot: {actual}"
            ),
            Err(e) => panic!(
                "\n[{backend}] failed to evaluate with safe points skipped\nProgram:\n{code}\n\
                 Error: {}",
                e.message
            ),
        }
    }
}

/// [`eval_program_vm`] with every safe point's collection turned off
/// ([`without_safe_points`]): the VM half of
/// [`assert_program_eval_to_without_safe_points`], for a test of the VM's own
/// machinery around a collection at a call.
pub fn eval_program_vm_without_safe_points(code: &str) -> String {
    match run_on(without_safe_points(vm_interpreter()), code, Mode::Program) {
        Ok(v) => v,
        Err(e) => panic!(
            "Failed to evaluate program with safe points skipped: {}\n{code}",
            e.message
        ),
    }
}

/// Assert that evaluating a Scheme expression produces the expected result
/// on both backends.
pub fn assert_eval_to(expr: &str, expected: &str) {
    expect_value(Which::Both, expr, expected, Mode::Expr);
}

/// Assert that evaluating a Scheme expression produces an error on both backends
pub fn assert_eval_error(expr: &str) {
    expect_error(Which::Both, expr, Mode::Expr);
}

/// Pin a program **both** backends reject but at different [`ErrorClass`]es,
/// asserting each one's stage and that both diagnostics contain `message`.
///
/// Deliberately narrower than [`assert_program_eval_error`], which asserts no
/// more than "this failed". Two shapes want it:
///
/// - the stages legitimately *disagree* — the VM expands quasiquote templates
///   during compilation while the tree-walker evaluates them, so a template
///   whose *unquote* is bad is rejected before the run on one backend and
///   during it on the other;
/// - the stages agree and *which stage* is the claim, because a program
///   rejected before it runs is a different fact from one that raises. Pass
///   the same class twice.
///
/// Pinning the message keeps the diagnostic itself under test. Pin the part
/// the backends share: they word the stage prefix differently (`desugar error`
/// against `Desugar error`), so a substring spanning it would fail one.
pub fn assert_program_eval_error_at(
    code: &str,
    tree_walker: ErrorClass,
    vm: ErrorClass,
    message: &str,
) {
    for (backend, expected) in [("tree-walker", tree_walker), ("vm", vm)] {
        let which = if backend == "vm" {
            Which::Vm
        } else {
            Which::TreeWalker
        };
        for (_, outcome) in outcomes(which, code, Mode::Program) {
            match outcome {
                Ok(value) => {
                    panic!("\n[{backend}] expected an error\nProgram:\n{code}\nGot: {value}")
                }
                Err(e) => {
                    assert_eq!(
                        e.class, expected,
                        "\n[{backend}] rejected at the wrong stage\nProgram:\n{code}\n{}",
                        e.message
                    );
                    assert!(
                        e.message.contains(message),
                        "\n[{backend}] wrong diagnostic\nProgram:\n{code}\n\
                         expected it to contain: {message}\ngot: {}",
                        e.message
                    );
                }
            }
        }
    }
}

/// Evaluate multiple expressions in sequence on both backends, assert the two
/// agree, and return the shared result.
///
/// The agreement check is what makes callers that compare the return value
/// against their own expectation differential for free.
pub fn eval_program(code: &str) -> String {
    let tw = run_on(tree_walker_interpreter(), code, Mode::Program);
    let vm = run_on(vm_interpreter(), code, Mode::Program);

    // Both failing is a shared bug in the program under test, not a
    // divergence — report both errors, since the two backends can fail for
    // different reasons and showing only one hides half the evidence.
    if let (Err(a), Err(b)) = (&tw, &vm) {
        panic!(
            "\nFailed to evaluate program:\n{code}\ntree-walker: {}\nvm: {}",
            a.message, b.message
        );
    }

    // Agreement is on *values*: error text is deliberately not compared (see
    // `ErrorClass`), and the class needs no check here because both-failing
    // already panicked above.
    let show = |o: &Outcome| match o {
        Ok(v) => v.clone(),
        Err(e) => format!("errored: {}", e.message),
    };
    assert!(
        tw.as_ref().ok() == vm.as_ref().ok(),
        "\nbackends disagree\nProgram:\n{code}\ntree-walker: {}\nvm: {}",
        show(&tw),
        show(&vm)
    );

    tw.expect("checked non-error above")
}

/// Evaluate on the VM, returning `Err` with the message instead of panicking.
///
/// The panicking `eval_program_vm` cannot express "this program should have
/// been *rejected* and was not", which the hygiene matrix pins on two shapes:
/// Patina accepts a `define-syntax` in a `do` result clause where chibi and
/// Racket both refuse it. Named per backend to match the panicking pair
/// beside it, so one idiom covers both.
pub fn try_eval_program_vm(code: &str) -> Result<String, String> {
    run_on(vm_interpreter(), code, Mode::Program).map_err(|e| e.message)
}

/// The tree-walker half of [`try_eval_program_vm`].
pub fn try_eval_program_tree_walker(code: &str) -> Result<String, String> {
    run_on(tree_walker_interpreter(), code, Mode::Program).map_err(|e| e.message)
}

/// [`try_eval_program_vm`] in a host environment: the program runs through
/// `Interpreter::eval_program_in` on the interpreter's fresh
/// `new_environment()`, whose parent is the global environment, rather than
/// at top level. What it defines stays in that environment, and what it
/// imports is bound there. For the hygiene matrix's host slice (#835).
pub fn try_eval_program_in_host_vm(code: &str) -> Result<String, String> {
    run_on(vm_interpreter(), code, Mode::HostProgram).map_err(|e| e.message)
}

/// The tree-walker half of [`try_eval_program_in_host_vm`].
pub fn try_eval_program_in_host_tree_walker(code: &str) -> Result<String, String> {
    run_on(tree_walker_interpreter(), code, Mode::HostProgram).map_err(|e| e.message)
}

/// Evaluate a program on the tree-walker only and `write` the result. For
/// tests that target tree-walker-specific machinery.
pub fn eval_program_tree_walker(code: &str) -> String {
    match run_on(tree_walker_interpreter(), code, Mode::Program) {
        Ok(v) => v,
        Err(e) => panic!("Failed to evaluate program: {}\n{code}", e.message),
    }
}

/// Evaluate a program on the VM only and `write` the result. For tests that
/// exercise VM-only machinery (CallPrimitive deopt, inline opcodes).
pub fn eval_program_vm(code: &str) -> String {
    match run_on(vm_interpreter(), code, Mode::Program) {
        Ok(v) => v,
        Err(e) => panic!("Failed to evaluate program: {}\n{code}", e.message),
    }
}

/// [`eval_program_vm`] on [`vm_interpreter_gc_off`]: only `(gc)` collects.
pub fn eval_program_vm_gc_off(code: &str) -> String {
    match run_on(vm_interpreter_gc_off(), code, Mode::Program) {
        Ok(v) => v,
        Err(e) => panic!("Failed to evaluate program: {}\n{code}", e.message),
    }
}

/// [`eval_program_vm`] on [`vm_interpreter_adaptive`]: collections come
/// where the adaptive byte threshold puts them, in every lane.
pub fn eval_program_vm_adaptive(code: &str) -> String {
    match run_on(vm_interpreter_adaptive(), code, Mode::Program) {
        Ok(v) => v,
        Err(e) => panic!("Failed to evaluate program: {}\n{code}", e.message),
    }
}

/// [`eval_program_tree_walker`] on [`tree_walker_interpreter_gc_off`]: only
/// `(gc)` collects.
pub fn eval_program_tree_walker_gc_off(code: &str) -> String {
    match run_on(tree_walker_interpreter_gc_off(), code, Mode::Program) {
        Ok(v) => v,
        Err(e) => panic!("Failed to evaluate program: {}\n{code}", e.message),
    }
}

/// [`eval_program_vm`] on [`vm_interpreter_zeal`]: a collection at every
/// outermost safe point.
pub fn eval_program_vm_zeal(code: &str) -> String {
    match run_on(vm_interpreter_zeal(), code, Mode::Program) {
        Ok(v) => v,
        Err(e) => panic!("Failed to evaluate program: {}\n{code}", e.message),
    }
}

/// [`eval_program_tree_walker`] on [`tree_walker_interpreter_zeal`]: a
/// collection at every outermost safe point.
pub fn eval_program_tree_walker_zeal(code: &str) -> String {
    match run_on(tree_walker_interpreter_zeal(), code, Mode::Program) {
        Ok(v) => v,
        Err(e) => panic!("Failed to evaluate program: {}\n{code}", e.message),
    }
}

/// Assert that a multi-expression program produces expected result on both backends
pub fn assert_program_eval_to(code: &str, expected: &str) {
    expect_value(Which::Both, code, expected, Mode::Program);
}

/// Assert that evaluating a multi-expression program produces an error on both backends
pub fn assert_program_eval_error(code: &str) {
    expect_error(Which::Both, code, Mode::Program);
}

/// Assert that evaluating an expression with (scheme char) imported produces
/// expected result on both backends
pub fn assert_eval_with_scheme_char(expr: &str, expected: &str) {
    let code = format!("(import (scheme char)) {}", expr);
    expect_value(Which::Both, &code, expected, Mode::Program);
}

// ─── Shared GC test suite ────────────────────────────────────────────────────

/// Backend-independent garbage-collection tests.
///
/// `(gc)` records a request that the next safe point services, so these
/// exercise the whole path — root providers, safe point, the driver, mark,
/// and sweep — for whichever backend `$eval` drives. Invoked once per backend
/// so both are covered in a single test lane; each backend's own file keeps
/// only the tests that target machinery unique to it.
///
/// `$eval` is a `fn(&str) -> String` that evaluates a program and `write`s the
/// result: `eval_program_tree_walker` or `eval_program_vm`. `$eval_gc_off` is
/// the same backend collecting only when `(gc)` asks
/// (`eval_program_tree_walker_gc_off`, `eval_program_vm_gc_off`), for the three
/// tests that need to know when collections run: they must pass under the
/// stress lanes' `PATINA_GC_STRESS` too, which a backend made from the
/// environment obeys (#626). `$eval_zeal` is the same backend collecting at
/// every outermost safe point (`eval_program_tree_walker_zeal`,
/// `eval_program_vm_zeal`), for the tests that must hold with a collection
/// at each of them, allocating or not, in every lane: the plain run's
/// adaptive trigger seldom collects in these tests' small programs, and stress
/// only after allocations. `collection_is_recorded_in_stats` checks that it
/// collects where nothing allocates, and `assert_gc_and_zeal_eval_to` that it
/// collected in each program it runs beyond the `(gc)`s the program asks for.
///
/// These take a *single-backend* evaluator on purpose. Several cases read
/// `(gc-stats)` counters, which legitimately differ between the backends —
/// the same workload frees 12295 pairs on the VM and 12294 on the
/// tree-walker — so routing them through the agreement-asserting
/// `eval_program` would fail on a difference that is not a divergence.
macro_rules! gc_shared_tests {
    ($eval:path, $eval_gc_off:path, $eval_zeal:path) => {
        /// Pull one `(gc-stats)` field out of the alist the primitive returns.
        fn stat(code_before: &str, field: &str) -> i64 {
            stat_on($eval, code_before, field)
        }

        /// [`stat`], evaluated by `eval`.
        fn stat_on(eval: fn(&str) -> String, code_before: &str, field: &str) -> i64 {
            let code = format!(
                r#"(import (patina debug))
                   {code_before}
                   (cdr (assq '{field} (gc-stats)))"#
            );
            eval(&code)
                .parse()
                .unwrap_or_else(|_| panic!("expected a number for {field}"))
        }

        /// The bytes of one pair (`patina_core::heap::PAIR_SLOT_BYTES`),
        /// what the byte account charges a cons.
        const PAIR_BYTES: i64 = patina_core::heap::PAIR_SLOT_BYTES as i64;

        /// Evaluate `code` with `eval` and parse the list of integers it
        /// writes.
        fn integers(eval: fn(&str) -> String, code: &str) -> Vec<i64> {
            let written = eval(code);
            written
                .trim_start_matches('(')
                .trim_end_matches(')')
                .split_whitespace()
                .map(|n| {
                    n.parse()
                        .unwrap_or_else(|_| panic!("not a list of integers: {written}"))
                })
                .collect()
        }

        fn assert_gc_eval_to(code: &str, expected: &str) {
            let result = $eval(code);
            assert_eq!(
                result, expected,
                "\nProgram:\n{code}\nExpected: {expected}\nGot: {result}"
            );
        }

        /// [`assert_gc_eval_to`], and the same under `$eval_zeal`, which must
        /// have collected beyond the `(gc)`s `code` asks for: run again with
        /// a last form that reads `collections`, it counts more than them. A
        /// zeal helper that collected only when asked, or as the adaptive
        /// trigger does, would pass the answer and test nothing.
        fn assert_gc_and_zeal_eval_to(code: &str, expected: &str) {
            assert_gc_eval_to(code, expected);
            let result = $eval_zeal(code);
            assert_eq!(
                result, expected,
                "\nUnder zeal\nProgram:\n{code}\nExpected: {expected}\nGot: {result}"
            );
            let asked = code.matches("(gc)").count() as i64;
            let collections = stat_on($eval_zeal, code, "collections");
            assert!(
                collections > asked,
                "{collections} collections under zeal, and the program asks for {asked}\
                 \nProgram:\n{code}"
            );
        }

        #[test]
        fn gc_runs_and_reclaims_unreachable_pairs() {
            // Allocate 5000 conses, each garbage at once, and collect: the
            // bytes the collections reclaim across the churn include the
            // pairs'. A delta of `bytes-reclaimed`, which grows only when a
            // collection frees something, so it holds however often the
            // mode collected along the way (#606).
            //
            // The first `(gc)` takes the garbage the program made before the
            // churn. Without it the default mode, which does not collect
            // while the libraries load, frees about 940 KB of their garbage
            // at the last `(gc)`, and the bound below would hold with no
            // pair churned at all; with it, a run without the churn reclaims
            // about 3 KB.
            let [reclaimed] = integers(
                $eval,
                r#"(import (patina debug))
                   (define (reclaimed) (cdr (assq 'bytes-reclaimed (gc-stats))))
                   (define (churn n) (if (> n 0) (begin (cons n n) (churn (- n 1)))))
                   (gc)
                   (define before (reclaimed))
                   (churn 5000)
                   (gc)
                   (list (- (reclaimed) before))"#,
            )[..] else {
                panic!("expected one count")
            };
            assert!(
                reclaimed >= 4000 * PAIR_BYTES,
                "expected the collector to reclaim 5000 pairs, it reclaimed {reclaimed} bytes"
            );
        }

        #[test]
        fn collection_is_recorded_in_stats() {
            let collections = stat("(gc)", "collections");
            assert!(
                collections > 0,
                "no collection was recorded after (gc): {collections}"
            );
            // `$eval_zeal` collects with no `(gc)` asked, allocation or not:
            // a loop of 100 iterations that allocates nothing passes a safe
            // point each iteration, and collects at each
            // (`crates/patina-repl/tests/gc_zeal.rs`'s control, at a tenth of
            // its 1000: the plain mode collects 0 times across it and stress
            // at 1 a few, and the tree-walker's 8 collections an iteration
            // cost it 0.66 s at 1000). Were it collecting as the environment
            // says, or as stress does, only after allocations, the tests'
            // zeal halves would test nothing.
            let [spun] = integers(
                $eval_zeal,
                r#"(import (patina debug))
                   (define (spin n) (if (> n 0) (spin (- n 1))))
                   (define (collections) (cdr (assq 'collections (gc-stats))))
                   (define before (collections))
                   (spin 100)
                   (list (- (collections) before))"#,
            )[..] else {
                panic!("expected one count")
            };
            assert!(
                spun >= 100,
                "{spun} collections under zeal across 100 iterations that allocate nothing"
            );
        }

        #[test]
        fn per_owner_counts_are_each_reported_once_in_the_lanes_order() {
            // `(gc-stats)`'s counts per owner, which the steady-state lane
            // holds flat (its SS5) and reads by `assq`: each once, so that
            // no reading finds a stale entry ahead of its own, each a count,
            // and in the order the lane lists them (`driver.scm`'s
            // `steady-per-owner-keys`, the runner's `PER_OWNER_KEYS`), where
            // a key joins last in the PR that adds it to `(gc-stats)`:
            // `code-units` after `binding-records` (#836). Every one answers
            // a count on both backends, `code-units` 0 on the tree-walker.
            assert_gc_eval_to(
                r#"
                (import (scheme base) (patina debug))
                (define per-owner '(symbols ports namespaces cells binding-records code-units))
                (define stats (gc-stats))
                (define (owned entries)
                  (cond ((null? entries) '())
                        ((memq (caar entries) per-owner)
                         (cons (caar entries) (owned (cdr entries))))
                        (else (owned (cdr entries)))))
                (define (counts? keys)
                  (or (null? keys)
                      (let ((n (cdr (assq (car keys) stats))))
                        (and (exact-integer? n) (>= n 0) (counts? (cdr keys))))))
                (list (owned stats) (counts? per-owner))
                "#,
                "((symbols ports namespaces cells binding-records code-units) #t)",
            );
        }

        #[test]
        fn live_data_survives_collection() {
            // The list is still bound when the collection runs; every element
            // must survive and stay readable.
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define keep (list 1 2 3 4 5))
                (gc)
                (apply + keep)
                "#,
                "15",
            );
        }

        #[test]
        fn deep_live_structure_survives_collection() {
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define (build n) (if (= n 0) '() (cons n (build (- n 1)))))
                (define keep (build 2000))
                (gc)
                (length keep)
                "#,
                "2000",
            );
        }

        #[test]
        fn unreachable_cycles_are_reclaimed() {
            // A thousand self-referential pairs, each garbage the moment the
            // next iteration starts. Reference counting could never reclaim
            // any of them, so a collection freeing their bytes is only
            // possible if cycles are collected.
            //
            // With collection off but for `(gc)`, so that exactly one
            // collection runs across the cycles: a first `(gc)` takes the
            // garbage the program made before them, and the second must
            // free them. Its count says it ran once, `bytes-reclaimed` that
            // it freed something (#606), and the pairs in use that what it
            // freed was the cycles: bytes alone would not, since each
            // iteration's other garbage is several times a pair's size.
            // The pairs in use grow only by `before`'s alist, allocated
            // after its counts were taken, which is subtracted; were the
            // cycles kept, they would grow by 1000 more.
            let code = r#"
                (import (patina debug))
                (define (make-cycles n)
                  (if (> n 0)
                      (let ((x (cons n '())))
                        (set-cdr! x x)
                        (make-cycles (- n 1)))))
                (define (stat stats key) (cdr (assq key stats)))
                (define (in-use stats) (- (stat stats 'pairs) (stat stats 'free-pairs)))
                (gc)
                (define before (gc-stats))
                (make-cycles 1000)
                (gc)
                (define after (gc-stats))
                (list (stat before 'collections)
                      (- (stat after 'collections) (stat before 'collections))
                      (- (stat after 'bytes-reclaimed) (stat before 'bytes-reclaimed))
                      (- (in-use after) (in-use before) (* 2 (length before))))
                "#;
            let [before, ran, freed, grown] = integers($eval_gc_off, code)[..] else {
                panic!("expected four counts from\n{code}")
            };
            assert!(before >= 1, "the first (gc) had not run: {before}");
            assert_eq!(ran, 1, "collections across the cycles");
            assert!(
                freed >= 1000 * PAIR_BYTES,
                "the collection freed {freed} bytes, under the 1000 cycles' pairs"
            );
            assert!(
                grown < 100,
                "the pairs in use grew by {grown} across 1000 cycles and a collection"
            );
        }

        #[test]
        fn captured_continuation_survives_and_escapes() {
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define (find-first pred lst)
                  (call/cc
                    (lambda (return)
                      (for-each (lambda (x) (gc) (if (pred x) (return x))) lst)
                      #f)))
                (find-first even? '(1 3 5 6 7))
                "#,
                "6",
            );
        }

        #[test]
        fn continuation_invoked_after_collection() {
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (let ((k (call/cc (lambda (c) c))))
                  (gc)
                  (if (procedure? k) (k 42) k))
                "#,
                "42",
            );
        }

        #[test]
        fn collection_during_dynamic_wind_preserves_thunks() {
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define trace '())
                (dynamic-wind
                  (lambda () (set! trace (cons 'before trace)))
                  (lambda () (gc) (set! trace (cons 'during trace)))
                  (lambda () (set! trace (cons 'after trace))))
                (reverse trace)
                "#,
                "(before during after)",
            );
        }

        #[test]
        fn collection_inside_a_comparator_the_internal_member_calls() {
            // A Rust primitive that calls a procedure back runs it on a
            // nested loop (`ApplyContext::apply_in`) and keeps its own state
            // in Rust locals meanwhile: here `(patina internal lists)`'s
            // `member` walks, with the program's comparator, a fresh list
            // that no root provider sees once the call's arguments have been
            // taken apart (the VM retires their registers at the call). The
            // comparator's `(gc)` must be deferred, not run: each counts a
            // deferred collection and collects nothing, in every mode, and
            // the walk's next step reads the list whole. With the nested
            // loops' deferral gone, the collection runs with `member`'s
            // context live, and a check build panics there, before it frees
            // the list. The internal library's `member` is called
            // directly: `(scheme base)`'s is Scheme and calls its comparator
            // as the machine calls any procedure (#471), and since #784 so
            // does `parameterize` over a procedure a program wrote, which was
            // this test's other half. This was a tree-walker test until #784.
            assert_gc_eval_to(
                r#"
                (import (patina debug) (only (patina internal lists) member))
                (define (stat key) (cdr (assq key (gc-stats))))
                (define inside '())
                (define found
                  (member 3 (list 1 2 3 4)
                    (lambda (x y)
                      (let ((c (stat 'collections)) (d (stat 'deferred-collections)))
                        (gc)
                        (set! inside
                              (cons (list (- (stat 'collections) c)
                                          (- (stat 'deferred-collections) d))
                                    inside)))
                      (= x y))))
                (list found (reverse inside))
                "#,
                "((3 4) ((0 1) (0 1) (0 1)))",
            );
        }

        #[test]
        fn a_collection_in_a_parameter_like_setter_runs_at_its_call() {
            // `parameterize` over procedures a program wrote, which read with
            // no argument and set with one: since #784 it sets them from
            // Scheme (`%parameterize-each`), so the setter is a frame of the
            // machine, and its `(gc)` collects at its call, deferring
            // nothing: on the install, while the old value `p` held is a
            // value only Scheme holds, and on the restore. It used to be
            // called from Rust, on a nested loop, while `%parameterize-swap!`
            // held that value in a Rust vector, so the `(gc)` was deferred
            // and counted. Each call's counts are kept, the install's first.
            // Collecting only where `(gc)` asks, so that no other collection
            // falls between the reads.
            assert_eq!(
                $eval_gc_off(
                    r#"
                    (import (patina debug))
                    (define (stat key) (cdr (assq key (gc-stats))))
                    (define held (list 'old 1 2))
                    (define (p . v) (if (null? v) held (set! held (car v))))
                    (define counts '())
                    (define (q . v)
                      (if (null? v)
                          'q-old
                          (let ((c (stat 'collections)) (d (stat 'deferred-collections)))
                            (gc)
                            (set! counts (cons (list (- (stat 'collections) c)
                                                     (- (stat 'deferred-collections) d))
                                               counts)))))
                    (define inside (parameterize ((p 'new) (q 'q-new)) held))
                    (list inside held (reverse counts))
                    "#
                ),
                "(new (old 1 2) ((1 0) (1 0)))",
            );
        }

        #[test]
        fn a_current_port_whose_slot_was_reused_gets_its_own_writes() {
            // #608's hazard: a port redirected into, dropped and collected
            // frees its slot, a new port can take the slot, and a cache of
            // the current port keyed on its word would then answer the dead
            // one. The cache holds the port itself, refreshed at every
            // assignment of the parameter, so each write reaches the port it
            // was meant for. A one-argument set makes no wind closures, so
            // the next port is the likeliest to take the freed slot; a
            // `parameterize` round follows each. The check builds stamp a
            // slot's generation into its words, which would hide a word-keyed
            // cache's mistake, so a plain release run of this test is the one
            // that could see it.
            assert_gc_eval_to(
                r#"
                (import (scheme base) (scheme write) (patina debug))
                (define saved (current-output-port))
                (define (set-round i)
                  (let ((p (open-output-string)))
                    (current-output-port p)
                    (display i)
                    (current-output-port saved)
                    (get-output-string p)))
                (define (parameterize-round i)
                  (let ((q (open-output-string)))
                    (parameterize ((current-output-port q)) (display i))
                    (get-output-string q)))
                (let loop ((i 0) (missed '()))
                  (if (= i 300)
                      (reverse missed)
                      (begin
                        (gc)
                        (let* ((a (set-round i))
                               (_ (gc))
                               (b (parameterize-round i))
                               (want (number->string i)))
                          (loop (+ i 1)
                                (if (and (string=? a want) (string=? b want))
                                    missed
                                    (cons (list i a b) missed)))))))
                "#,
                "()",
            );
        }

        #[test]
        fn records_and_exceptions_survive_collection() {
            // The record's fields live behind an `Rc<RefCell<Vec<_>>>`, and
            // the guard clause closes over `p` — both must survive.
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define-record-type point (make-point x y) point? (x point-x) (y point-y))
                (define p (make-point 3 4))
                (gc)
                (guard (e (#t (+ (point-x p) (point-y p))))
                  (raise 'boom))
                "#,
                "7",
            );
        }

        #[test]
        fn strings_and_vectors_survive_collection() {
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define s (string-append "hello" " " "world"))
                (define v (vector 1 2 3))
                (gc)
                (string-append s (number->string (vector-ref v 2)))
                "#,
                "\"hello world3\"",
            );
        }

        #[test]
        fn collecting_keeps_the_arena_smaller_than_not_collecting() {
            // Compared against the same workload with the collections removed,
            // so it can only pass if slots are actually reclaimed and reused —
            // a fixed bound would pass vacuously with zero collections. The
            // side without them runs with collection off but for `(gc)`, so
            // that a stress lane does not collect there either; the side with
            // them runs in whatever mode the environment selects.
            let workload = |collect: &str| {
                format!(
                    r#"(define (round n)
                         (if (> n 0)
                             (begin
                               (let loop ((i 0) (acc '()))
                                 (if (< i 200) (loop (+ i 1) (cons i acc)) acc))
                               {collect}
                               (round (- n 1)))))
                       (round 10)"#
                )
            };
            // The side with them also reports what its collections freed
            // across the workload: only a collection that freed something
            // reclaims a byte (#606).
            let [with_gc, reclaimed] = integers(
                $eval,
                &format!(
                    r#"(import (patina debug))
                       (define (stat key) (cdr (assq key (gc-stats))))
                       (define reclaimed (stat 'bytes-reclaimed))
                       {}
                       (list (stat 'pairs) (- (stat 'bytes-reclaimed) reclaimed))"#,
                    workload("(gc)")
                ),
            )[..] else {
                panic!("expected an arena size and a byte count")
            };
            let without_gc = stat_on($eval_gc_off, &workload(""), "pairs");
            assert!(
                with_gc < without_gc,
                "collecting did not shrink the arena: {with_gc} with gc vs {without_gc} without"
            );
            assert!(reclaimed > 0, "the side with (gc) reclaimed nothing");
        }

        #[test]
        fn liveness_stress_large_list_survives_collection() {
            // The liveness stress from #10's GC groundwork: a 100k-element
            // list stays fully intact across a collection that runs while it
            // is the only thing keeping its cells alive.
            assert_gc_eval_to(
                r#"
                (import (patina debug))
                (define (build n acc) (if (= n 0) acc (build (- n 1) (cons n acc))))
                (define big (build 100000 '()))
                (gc)
                (define (sum lst acc) (if (null? lst) acc (sum (cdr lst) (+ acc (car lst)))))
                (sum big 0)
                "#,
                "5000050000",
            );
        }

        #[test]
        fn arena_plateaus_under_repeated_churn_with_gc() {
            // The arena-reuse proof from #10's GC groundwork: across 20 churn
            // rounds with a collection each, the pairs arena must reuse freed
            // slots rather than grow — the delta after 100k churned conses
            // stays under one round's worth — and the collections must have
            // reclaimed the rounds' bytes (#606).
            let [delta, reclaimed] = integers(
                $eval,
                r#"(import (patina debug))
                   (define (churn n) (if (> n 0) (begin (cons n n) (churn (- n 1)))))
                   (define (rounds k)
                     (if (> k 0) (begin (churn 5000) (gc) (rounds (- k 1)))))
                   (churn 5000)
                   (gc)
                   (define (stat key) (cdr (assq key (gc-stats))))
                   (define baseline (stat 'pairs))
                   (define reclaimed (stat 'bytes-reclaimed))
                   (rounds 20)
                   (list (- (stat 'pairs) baseline) (- (stat 'bytes-reclaimed) reclaimed))"#,
            )[..] else {
                panic!("expected an arena delta and a byte count")
            };
            assert!(
                delta < 5000,
                "pairs arena grew by {delta} slots across 20 collected churn rounds"
            );
            assert!(
                reclaimed >= 19 * 5000 * PAIR_BYTES,
                "20 rounds of 5000 conses, and {reclaimed} bytes reclaimed"
            );
        }

        #[test]
        fn a_forward_reference_links_the_definition_made_after_a_collection() {
            // `g`'s code reads `h` before anything binds it, and `h` is
            // defined only after two collections. The tree-walker reads it
            // by name, and finds the definition whenever it is made. The
            // VM links a unit's names when it loads it (#836), so `g`'s
            // unit links `h` to a placeholder: a record nothing binds,
            // which a collection keeps only while live code's link table
            // holds it. `g`'s closure is live, so the collections must keep
            // it: `(g)` reads through the link, and a read of a record they
            // freed is what a check build's stale-reference check refuses
            // (#621), where a release build may find the slot reused and
            // answer 9.
            //
            // The answer cannot show that the definition adopts the
            // placeholder: where a linked record reads unbound, the VM's
            // cold read goes by name, and finds a record the definition
            // made beside it. `binding-records` shows it. Between its
            // readings the program defines `g` and `h`, two records on both
            // backends, the VM's definitions adopting what the load linked;
            // a definition that made a record of its own would leave its
            // placeholder counted beside it, one more. `(h)` in
            // the last form is linked when that form loads, after the
            // definition, so finds `h` by name: a definition that took the
            // placeholder out of the namespace's store without giving the
            // namespace its slot would leave it unbound. The readings'
            // variables are defined before the first and then `set!`, so
            // that no definition but `g`'s and `h`'s falls between them.
            // Under zeal too, with a collection at every outermost safe
            // point between the forms.
            assert_gc_and_zeal_eval_to(
                r#"
                (import (patina debug))
                (define (records) (cdr (assq 'binding-records (gc-stats))))
                (define r0 #f) (define r1 #f)
                (set! r0 (records))
                (define (g) (h))
                (gc) (gc)
                (define (h) 9)
                (set! r1 (records))
                (list (g) (h) (- r1 r0))
                "#,
                "(9 9 2)",
            );
        }

        #[test]
        fn a_placeholder_only_a_captured_continuation_links_survives_a_collection() {
            // `later` is read only on a re-entry of the continuation `k`,
            // captured inside a top-level form, and once that form has
            // returned nothing holds the code that reads it but `k`.
            // `(k #f)` re-enters it after the collections and the
            // definition: the rest of the form runs again, with `n` at 2,
            // so `seen` is set only on the re-entry. On both backends a
            // continuation a top-level form captured ends with that form,
            // and the program goes on after the form that invoked it, so
            // the forms between do not run again. The VM links names at
            // load (#836), so its placeholder for `later` is held only
            // through `k`, whose code a collection must trace; a read of a
            // record freed under it is what a check build's stale-reference
            // check refuses (#621).
            //
            // Two programs, since `k` holds code two ways. In the first the
            // read is in an anonymous procedure's code, and `k`'s frames
            // hold that procedure's closure in a slot as well as running its
            // code. In the second it is in the form's own code, whose frame
            // makes no closure, so only the frame's code reaches the unit
            // that links `later`: `(if #t ...)` keeps the form one
            // expression, where a top-level `begin` would be spliced into
            // forms of their own. Each counts `binding-records` across the
            // definition, one record on both backends, the VM's definition
            // adopting the placeholder the load linked, as in
            // `a_forward_reference_links_the_definition_made_after_a_collection`.
            // Under zeal too.
            assert_gc_and_zeal_eval_to(
                r#"
                (import (patina debug))
                (define (records) (cdr (assq 'binding-records (gc-stats))))
                (define r0 #f) (define r1 #f)
                (define k #f) (define n 0) (define seen 'none)
                (set! r0 (records))
                ((lambda ()
                   (call/cc (lambda (c) (set! k c)))
                   (set! n (+ n 1))
                   (if (> n 1) (set! seen later))))
                (gc) (gc)
                (define later 7)
                (set! r1 (records))
                (if (= n 1) (k #f))
                (list seen n (- r1 r0))
                "#,
                "(7 2 1)",
            );
            assert_gc_and_zeal_eval_to(
                r#"
                (import (patina debug))
                (define (records) (cdr (assq 'binding-records (gc-stats))))
                (define r0 #f) (define r1 #f)
                (define k #f) (define n 0) (define seen 'none)
                (define (capture!) (call/cc (lambda (c) (set! k c))))
                (set! r0 (records))
                (if #t
                    (begin
                      (capture!)
                      (set! n (+ n 1))
                      (if (> n 1) (set! seen later))))
                (gc) (gc)
                (define later 7)
                (set! r1 (records))
                (if (= n 1) (k #f))
                (list seen n (- r1 r0))
                "#,
                "(7 2 1)",
            );
        }

        // The routes by which code compiled in a namespace that nothing
        // else roots keeps running after a collection (#836). Each program
        // compiles a procedure in a namespace that, at the collections, the
        // machine's globals do not hold, nor a library's registry entry, a
        // host's handle or a live environment specifier, whose trace would
        // walk the namespace whole. The code then reads a binding of that
        // namespace through its link: `map`'s import record, a binding
        // `LoadGlobal` reads through its entry, or a library's private
        // binding, or by name, through an alias of the namespace. On the VM
        // the namespace and the records it reads are held by the code's
        // unit, whose link table a marked closure, a frame or the running
        // code reports by its code id, and nothing else: a closure holds no
        // environment. A collection that kept the unit but not what its
        // table links, or not the namespace it holds, would free the record,
        // and the read is what a check build's stale-reference check refuses
        // (#621). On the tree-walker a procedure holds its environment.
        // Under zeal too.

        #[test]
        fn code_eval_compiled_runs_after_its_specifier_is_garbage() {
            // `k` is a closure of a unit linked in the specifier's
            // namespace, and the specifier is garbage once the `define`
            // has returned: `k`'s code id is all that reports the unit.
            assert_gc_and_zeal_eval_to(
                r#"
                (import (scheme base) (scheme eval) (patina debug))
                (define k (eval '(lambda () map) (environment '(scheme base))))
                (gc) (gc)
                (eq? (k) map)
                "#,
                "#t",
            );
        }

        #[test]
        fn a_replaced_library_s_procedure_reads_its_own_namespace_after_a_collection() {
            // `old-get` is the first `(r836 lib)`'s `get`. The second
            // replaces the library, and its registry entry with it, so the
            // first's namespace, where `secret` is bound to `(old)`, is
            // held only by the units of the code compiled in it, which
            // `old-get` reports.
            assert_gc_and_zeal_eval_to(
                r#"
                (import (scheme base) (patina debug))
                (define-library (r836 lib)
                  (export get)
                  (import (scheme base))
                  (begin (define secret (list 'old)) (define (get) secret)))
                (import (r836 lib))
                (define old-get get)
                (define-library (r836 lib)
                  (export get)
                  (import (scheme base))
                  (begin (define secret (list 'new)) (define (get) secret)))
                (import (r836 lib))
                (gc) (gc)
                (list (old-get) (get))
                "#,
                "((old) (new))",
            );
        }

        #[test]
        fn a_parameter_s_converter_reads_its_namespace_after_a_collection() {
            // The converter is a closure compiled in the specifier's
            // namespace, held by the parameter object only, and called
            // again by `parameterize` after the collections.
            assert_gc_and_zeal_eval_to(
                r#"
                (import (scheme base) (scheme eval) (patina debug))
                (define p
                  (eval '(make-parameter 1 (lambda (x) (cons x map)))
                        (environment '(scheme base))))
                (gc) (gc)
                (list (car (p)) (parameterize ((p 2)) (eq? (cdr (p)) map)))
                "#,
                "(1 #t)",
            );
        }

        #[test]
        fn a_continuation_s_frame_reads_its_namespace_after_a_collection() {
            // `r` is a continuation captured in `f`'s frame, and `f` is
            // then dropped: at the collections only the continuation's
            // frames hold `f`'s code, and no `eval` is among them. Its
            // re-entry runs the rest of `f`, which reads `map`, and the
            // `define` it returns into, after which the program goes on
            // after `(r 5)` on both backends.
            assert_gc_and_zeal_eval_to(
                r#"
                (import (scheme base) (scheme eval) (patina debug))
                (define f
                  (eval '(lambda ()
                           (let ((v (call/cc (lambda (c) c))))
                             (if (procedure? v) v (list v map))))
                        (environment '(scheme base))))
                (define r (f))
                (set! f #f)
                (gc) (gc)
                (if (procedure? r) (r 5))
                (list (car r) (eq? (cadr r) map))
                "#,
                "(5 #t)",
            );
        }

        #[test]
        fn a_promise_s_thunk_reads_its_namespace_after_a_collection() {
            // The promise's thunk is a closure compiled in the specifier's
            // namespace, held by the promise only until `force` runs it.
            assert_gc_and_zeal_eval_to(
                r#"
                (import (scheme base) (scheme eval) (scheme lazy) (patina debug))
                (define d
                  (eval '(delay (list map)) (environment '(scheme base) '(scheme lazy))))
                (gc) (gc)
                (eq? (car (force d)) map)
                "#,
                "#t",
            );
        }

        #[test]
        fn a_closure_only_an_ephemeron_holds_reads_its_namespace_after_a_collection() {
            // The closure, compiled in the specifier's namespace, is held
            // only as the datum of an ephemeron whose key the program's
            // `key` holds. The collector marks it in the ephemeron pass,
            // after the roots, so on the VM its code id reaches
            // `trace_code_ids` only in a later round of the mark phase's
            // fixpoint, which must take it there.
            assert_gc_and_zeal_eval_to(
                r#"
                (import (scheme base) (scheme eval) (srfi 124) (patina debug))
                (define key (list 'key))
                (define eph
                  (make-ephemeron key
                                  (eval '(lambda () (list map)) (environment '(scheme base)))))
                (gc) (gc)
                (list (eq? (car ((ephemeron-datum eph))) map) (ephemeron-broken? eph))
                "#,
                "(#t #f)",
            );
        }

        #[test]
        fn a_replaced_library_s_procedure_reads_by_name_through_its_namespace_after_a_collection(
        ) {
            // `old-step` is the first `(r836 stepper)`'s `step`, whose body
            // the first `(r836 counter)`'s macro expands into a read and an
            // assignment of that library's private `counter`: a name
            // `(r836 stepper)`'s namespace reaches by an alias, which the
            // code reads by name, through the namespace its unit's table
            // was linked in, not through an entry the table links. Both
            // libraries are then replaced, so the first `(r836 stepper)`'s
            // namespace is held only by the units of the code compiled in
            // it, and the first `(r836 counter)`'s only through that
            // namespace's alias. On the VM it is the table's namespace that
            // keeps `counter`'s record and its value.
            assert_gc_and_zeal_eval_to(
                r#"
                (import (scheme base) (patina debug))
                (define-library (r836 counter)
                  (export bump!)
                  (import (scheme base))
                  (begin
                    (define counter (list 0))
                    (define-syntax bump!
                      (syntax-rules ()
                        ((_) (begin (set! counter (list (+ (car counter) 1))) counter))))))
                (define-library (r836 stepper)
                  (export step)
                  (import (scheme base) (r836 counter))
                  (begin (define (step) (bump!))))
                (import (r836 stepper))
                (define old-step step)
                (define-library (r836 counter)
                  (export bump!)
                  (import (scheme base))
                  (begin (define-syntax bump! (syntax-rules () ((_) 'new-counter)))))
                (define-library (r836 stepper)
                  (export step)
                  (import (scheme base))
                  (begin (define (step) 'new-stepper)))
                (import (r836 stepper))
                (gc) (gc)
                (list (old-step) (old-step) (step))
                "#,
                "((1) (2) new-stepper)",
            );
        }

        /// The program of
        /// `a_placeholder_no_live_code_links_is_dropped`, with batches of
        /// `batch` `eval`s.
        fn placeholders_dropped(batch: usize) -> String {
            format!(
                r#"
                (import (scheme base) (scheme eval) (scheme repl) (patina debug))
                (define env (interaction-environment))
                (define (count key) (cdr (assq key (gc-stats))))
                (define r0 #f) (define c0 #f) (define x0 #f)
                (define r1 #f) (define c1 #f) (define x1 #f) (define x2 #f)
                (define (batch)
                  (do ((i 0 (+ i 1))) ((= i {batch}))
                    (eval (list 'lambda '()
                                (string->symbol
                                  (string-append "unbound-" (number->string i))))
                          env)))
                (gc) (gc)
                (set! r0 (count 'binding-records))
                (set! c0 (count 'cells))
                (set! x0 (count 'external-bytes))
                (batch) (gc) (gc)
                (set! r1 (count 'binding-records))
                (set! c1 (count 'cells))
                (set! x1 (count 'external-bytes))
                (batch) (gc) (gc)
                (set! x2 (count 'external-bytes))
                (define unbound-0 1)
                (list (<= (abs (- r1 r0)) 16) (<= (abs (- c1 c0)) 16)
                      (< (- x1 x0) 65536) (= x2 x1) unbound-0)
                "#
            )
        }

        #[test]
        fn a_placeholder_no_live_code_links_is_dropped() {
            // Each `eval` compiles a procedure that reads a name nothing
            // binds, `unbound-0` to `unbound-999`, in the global namespace,
            // and drops it. The VM links each name to a placeholder in the
            // namespace's store when it loads the unit (#836): a binding
            // record and a cell, counted. Once the procedure is dead nothing
            // traces its unit's table, so the pass at the sweep's start drops
            // every one of them, and two collections later `binding-records`
            // and `cells` read as they did before the batch. A pass that did
            // not run, or a store traced as a root, would leave them a batch
            // above. The store's table keeps the capacity its first batch
            // grew it to, under 64 KiB, and the second batch, of the same
            // names, leaves `external-bytes` where the first did: a store
            // that kept something for every name it ever held would not.
            // The last form shows that a name whose placeholder was dropped
            // can still be defined and read. The tree-walker links nothing,
            // and nothing moves. Every variable is defined before the first
            // reading and then `set!`, since a definition's operand is
            // linked at load. 1,000 a batch, and under zeal 100, which
            // collects at every outermost safe point and must have collected
            // beyond the program's `(gc)`s; still past SS5's floor of 16.
            let expected = "(#t #t #t #t 1)";
            assert_gc_eval_to(&placeholders_dropped(1000), expected);
            let zeal = placeholders_dropped(100);
            let result = $eval_zeal(&zeal);
            assert_eq!(result, expected, "\nUnder zeal\nProgram:\n{zeal}");
            let asked = zeal.matches("(gc)").count() as i64;
            let collections = stat_on($eval_zeal, &zeal, "collections");
            assert!(
                collections > asked,
                "{collections} collections under zeal, and the program asks for {asked}"
            );
        }

        #[test]
        fn a_read_that_missed_is_not_remembered_across_collections() {
            // `g` reads `later-836m` before anything binds it: the call
            // raises an unbound-variable error, which the program catches.
            // Two collections later the name is defined, and the same
            // closure reads it. A miss the read remembered, in the unit's
            // link table or anywhere else, would raise again; an error path
            // that dropped or replaced the placeholder `g`'s unit links
            // would leave it reading a record the collections freed, which a
            // check build refuses (#621). On the VM the read goes through
            // the placeholder the load linked (#836); on the tree-walker by
            // name. Under zeal too.
            assert_gc_and_zeal_eval_to(
                r#"
                (import (scheme base) (patina debug))
                (define (g) later-836m)
                (define r (guard (e (#t 'caught)) (g)))
                (gc) (gc)
                (define later-836m 5)
                (list r (g))
                "#,
                "(caught 5)",
            );
        }
    };
}
