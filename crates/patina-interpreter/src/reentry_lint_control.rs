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
//!
//! `std::thread_local` (disallowed-macros) is the other half. Clippy takes
//! that lint only at a crate root, so a crate that has one lists its statics
//! in one crate-root `expect`, which every later invocation in the crate also
//! fulfils: the lint alone would let a new one through. The second test here
//! reads each crate's sources and holds the statics they declare to the list
//! that `expect`'s reason gives, so a static added or removed without its
//! line in the reason fails.

use crate::{Backend, Environment, Evaluator, Interpreter, TaggedValue, VmBackend};
use patina_primitives::ApplyContext;
use patina_tree_walker::CpsEvaluator;
use patina_vm::runtime::VmState;
use patina_vm::types::CodeObjectId;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
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

    // patina-interpreter: the twins that answer a handle (#605)
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_str_owned("");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_str_with_source_name_owned("", "");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm, env: &Rc<Environment>| i.eval_str_in_env("", "", env);
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_owned("");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_resilient_owned("");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_with_source_name_owned("", "");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm, fold_case: &mut bool| i.eval_program_with_fold_case_owned("", "", fold_case);
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm, fold_case: &mut bool, env: &Rc<Environment>| {
        i.eval_program_in_env("", "", fold_case, env)
    };
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_resilient_with_source_name_owned("", "");

    // patina-interpreter: the bare-value forms, deprecated until stage 5e
    // removes them (#605)
    #[allow(deprecated)]
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_str("");
    #[allow(deprecated)]
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_str_with_source_name("", "");
    #[allow(deprecated)]
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_str_tracked("");
    #[allow(deprecated)]
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program("");
    #[allow(deprecated)]
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_resilient("");
    #[allow(deprecated)]
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_with_source_name("", "");
    #[allow(deprecated)]
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm, fold_case: &mut bool| i.eval_program_with_fold_case("", "", fold_case);
    #[allow(deprecated)]
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_tracked("");
    #[allow(deprecated)]
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_resilient_tracked("");
    #[allow(deprecated)]
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm| i.eval_program_resilient_with_source_name("", "");

    // patina-interpreter's deprecated adapters
    #[cfg(feature = "legacy-pipeline")]
    {
        use crate::legacy::Pipeline;
        #[expect(clippy::disallowed_methods, reason = "positive control")]
        let _ = |p: &dyn Pipeline, env: &Rc<Environment>| p.eval("", env);
        #[expect(clippy::disallowed_methods, reason = "positive control")]
        let _ = |p: &dyn Pipeline, env: &Rc<Environment>| p.eval_program("", env);
        #[allow(deprecated)]
        #[expect(clippy::disallowed_methods, reason = "positive control")]
        let _ = |i: &crate::SimpleInterpreter| i.eval_str("");
        #[allow(deprecated)]
        #[expect(clippy::disallowed_methods, reason = "positive control")]
        let _ = |i: &crate::SimpleInterpreter| i.eval_program("");
    }

    // patina-core
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |parent: Rc<Environment>| Environment::with_parent(parent);
}

/// Every crate's `thread_local!` statics are the ones its crate root's
/// `#![expect(clippy::disallowed_macros, reason = …)]` names, no more and no
/// fewer: the list is what the threading-readiness check (C9) reads, and the
/// lint cannot keep it whole by itself.
#[test]
fn every_thread_local_is_listed_at_its_crate_root() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory");
    let mut crate_dirs: Vec<PathBuf> = std::fs::read_dir(crates)
        .expect("read the crates directory")
        .map(|entry| entry.expect("a crate directory").path())
        .filter(|dir| dir.join("src").is_dir())
        .collect();
    crate_dirs.sort();
    let mut declared_anywhere = 0;
    for dir in crate_dirs {
        let src = dir.join("src");
        let mut declared = BTreeSet::new();
        for file in rust_files(&src) {
            let text = std::fs::read_to_string(&file).expect("read a source file");
            declared.extend(thread_local_statics(&text));
        }
        let listed: BTreeSet<String> = ["lib.rs", "main.rs"]
            .iter()
            .filter_map(|root| std::fs::read_to_string(src.join(root)).ok())
            .flat_map(|text| listed_statics(&text))
            .collect();
        assert_eq!(
            declared,
            listed,
            "{}: the `thread_local!` statics in src/ (left) and the ones the crate root's \
             `disallowed_macros` expect names (right) differ; name each static there with what \
             it holds (#622)",
            dir.display()
        );
        declared_anywhere += declared.len();
    }
    // The scan itself is under test: it must find the statics that exist.
    assert!(declared_anywhere >= 10, "found {declared_anywhere} statics");
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read a source directory") {
        let path = entry.expect("a source entry").path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files
}

/// The statics declared by each `thread_local!` invocation in `text`. An
/// invocation is the macro name followed by its opening delimiter, outside a
/// line comment; a mention in prose or in a string is not one.
fn thread_local_statics(text: &str) -> Vec<String> {
    const MACRO: &str = "thread_local!";
    let mut statics = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(MACRO) {
        let at = from + found;
        from = at + MACRO.len();
        let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        if text[line_start..at].trim_start().starts_with("//") {
            continue;
        }
        let rest = text[from..].trim_start();
        let Some(open) = rest.chars().next().filter(|c| matches!(c, '{' | '(')) else {
            continue;
        };
        let close = if open == '{' { '}' } else { ')' };
        let mut depth = 0usize;
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
        }
        for line in rest[..end].lines().map(str::trim) {
            if line.starts_with("//") {
                continue;
            }
            if let Some(after) = line
                .split_once("static ")
                .filter(|(before, _)| before.is_empty() || before.ends_with(' '))
                .map(|(_, after)| after)
            {
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                statics.push(name);
            }
        }
    }
    statics
}

/// The statics named in a crate root's `disallowed_macros` reason: each
/// backquoted span whose last path segment is a SCREAMING_CASE identifier.
fn listed_statics(root: &str) -> Vec<String> {
    let Some(at) = root.find("clippy::disallowed_macros") else {
        return Vec::new();
    };
    let reason = &root[at..];
    let start = reason.find("reason = \"").expect("the expect's reason") + "reason = \"".len();
    let end = reason[start..].find('"').expect("the reason's end");
    reason[start..start + end]
        .split('`')
        .skip(1)
        .step_by(2)
        .filter_map(|span| span.rsplit("::").next())
        .filter(|name| {
            name.len() > 1
                && name.chars().any(|c| c.is_ascii_uppercase())
                && name
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        })
        .map(str::to_string)
        .collect()
}
