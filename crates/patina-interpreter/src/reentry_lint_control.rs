//! Positive control for the workspace `clippy.toml`'s re-entry rule (#622).
//!
//! Every entry of `disallowed-methods` that this crate can name is called
//! here once, under an `expect` of its own. Deleting or misspelling an entry
//! leaves its `expect` unfulfilled, and clippy fails. That covers a misspelled
//! crate name too, which clippy otherwise ignores without a word: a path into
//! a crate the linted crate does not depend on is skipped, and this crate
//! depends on every crate the list names.
//!
//! The private entries (`across_reentry`, `run_loop_until`, `run_trampoline`,
//! `run_synchronously` and the rest) cannot be named from here. Their own
//! call sites carry `expect`s, and a private entry left with no call site is
//! dead code, which rustc reports.
//!
//! Nothing here runs: each probe is a closure the test builds and drops.
//! `std::thread_local` (disallowed-macros) is controlled by the crate-root
//! `expect` of each crate that has one.

use crate::{Backend, Environment, Evaluator, Interpreter, TaggedValue, VmBackend};
use patina_primitives::ApplyContext;
use patina_tree_walker::CpsEvaluator;
use patina_vm::runtime::VmState;
use patina_vm::types::CodeObjectId;
use std::cell::RefCell;
use std::rc::Rc;

type Vm = Interpreter<VmBackend>;

#[test]
fn every_nameable_entry_is_matched() {
    // patina-primitives
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |c: &dyn ApplyContext| c.apply_proc(TaggedValue::NULL, Vec::new());
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |c: &dyn ApplyContext, env: &Rc<Environment>| c.eval_expr(TaggedValue::NULL, env);
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |c: &dyn ApplyContext| c.load_scheme_library(&[]);

    // patina-vm
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |s: &mut VmState, id: CodeObjectId| patina_vm::runtime::execute(s, id);
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |b: &VmBackend| b.load_library(&[]);

    // patina-tree-walker
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |c: &CpsEvaluator<'_>, e: &patina_core::CpsExpr| c.eval(e);
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |c: &CpsEvaluator<'_>, e: Rc<patina_core::CpsExpr>, env: Rc<Environment>| {
        c.eval_in_env(e, env)
    };
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |c: &CpsEvaluator<'_>| c.apply_from_direct_tagged(TaggedValue::NULL, Vec::new());
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |e: &crate::CoreExpr, env: Rc<Environment>, ev: &Evaluator| {
        patina_tree_walker::eval_cps(e, env, ev)
    };
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |ev: &Evaluator| ev.load_library(&[]);
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |ev: &Evaluator| ev.eval_inline_define_library(TaggedValue::NULL);
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |ev: &Evaluator, set: &patina_frontend::ImportSet, env: &Rc<Environment>| {
        ev.process_import_for_eval(set, env)
    };

    // patina-runtime
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |b: &VmBackend, env: &Rc<Environment>| b.eval(TaggedValue::NULL, env);
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |b: &VmBackend| b.eval_global(TaggedValue::NULL);
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |b: &VmBackend, env: &Rc<Environment>, map: &Rc<RefCell<crate::SourceMap>>| {
        b.eval_with_source_map(TaggedValue::NULL, env, map)
    };

    // patina-interpreter
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_str("");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_str_with_source_name("", "");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm, env: &Rc<Environment>| i.eval_str_in_env("", "", env);
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_str_tracked("");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program("");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_resilient("");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_with_source_name("", "");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm, fold_case: &mut bool| i.eval_program_with_fold_case("", "", fold_case);
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm, fold_case: &mut bool, env: &Rc<Environment>| {
        i.eval_program_in_env("", "", fold_case, env)
    };
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_tracked("");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_resilient_tracked("");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_resilient_with_source_name("", "");

    // patina-core: outside the core, which is exempt (its Cargo.toml)
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |parent: Rc<Environment>| Environment::with_parent(parent);
}
