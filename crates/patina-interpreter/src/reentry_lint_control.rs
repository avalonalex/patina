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
//!
//! The rest of the threading-readiness check (C9, #711; `PRD/GC_PRD.md`
//! §18.6) reads the sources the same way: no crate declares a `static mut`,
//! and no `HeapObjectData` payload holds an `Rc`, a `RefCell`, a `Cell` or an
//! `Interior` beyond the ones `SHARED_PAYLOADS` lists. Each scan has a
//! positive control.

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

    // patina-vm
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |s: &mut VmState, id: CodeObjectId| patina_vm::runtime::execute(s, id);
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |s: &mut VmState, id: CodeObjectId, env: &Rc<Environment>| {
        patina_vm::runtime::execute_in(s, id, env)
    };
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
    let _ = |e: &crate::CoreExpr,
             literals: patina_core::LiteralPool,
             env: Rc<Environment>,
             ev: &Evaluator| { patina_tree_walker::eval_cps(e, literals, env, ev) };
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
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |b: &VmBackend| b.call(TaggedValue::NULL, &[]);

    // patina-interpreter: a host's call of a procedure (#710)
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &mut Vm, f: &crate::Owned| i.call(f, &[]);

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
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm, env: &crate::OwnedEnvironment| i.eval_str_in(env, "");
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |i: &Vm, env: &crate::OwnedEnvironment| i.eval_program_in(env, "");

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
    #[expect(clippy::disallowed_methods, reason = "positive control")]
    let _ = |parent: Rc<Environment>| Environment::for_temporaries(parent);
}

/// Every crate's `thread_local!` statics are the ones its crate root's
/// `#![expect(clippy::disallowed_macros, reason = …)]` names, no more and no
/// fewer: the list is what the threading-readiness check (C9) reads, and the
/// lint cannot keep it whole by itself.
#[test]
fn every_thread_local_is_listed_at_its_crate_root() {
    let mut declared_anywhere = 0;
    for dir in crate_dirs() {
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

/// No crate declares a `static mut` (C9): it is state no carrier owns, which
/// a build with more than one carrier could not share. Nothing is listed.
#[test]
fn no_crate_declares_a_static_mut() {
    let mut found = Vec::new();
    let mut scanned = 0;
    for dir in crate_dirs() {
        for file in rust_files(&dir) {
            let text = std::fs::read_to_string(&file).expect("read a source file");
            scanned += 1;
            found.extend(
                static_muts(&text)
                    .into_iter()
                    .map(|line| format!("{}: {}", file.display(), line.trim())),
            );
        }
    }
    assert!(
        found.is_empty(),
        "a `static mut` is state no carrier owns (C9, #711): make process-wide state an \
         atomic, a `OnceLock` or a `Mutex`, and keep a heap's or a carrier's state in it: \
         {found:#?}"
    );
    // The scan itself is under test: it must read the workspace.
    assert!(scanned >= 300, "scanned {scanned} files");
}

/// The `HeapObjectData` payloads that hold an `Rc`, a `RefCell`, a `Cell` or
/// an `Interior` (a `RefCell` inside), as `Variant.field`, or `Variant.0` for
/// a tuple variant's. Rule 3 (`PRD/GC_PRD.md` §18.2) takes them out of the
/// heap by stage 5 (§18.3), and an entry comes off as its payload goes. A new
/// one is added on purpose: the test below fails until it is listed (C9).
const SHARED_PAYLOADS: &[&str] = &[
    "Continuation.0",
    "Ephemeron.0",
    "EnvironmentSpecifier.env",
    "Identifier.name",
    "Library.0",
    "Macro.body",
    "MutableCell.0",
    "Parameter.values",
    "Procedure.0",
    "Promise.0",
    "PromptTag.0",
    "Record.fields",
    "Record.record_type",
    "RecordType.0",
    "Symbol.0",
    "VmClosure.globals",
];

/// The payloads of `HeapObjectData` holding an `Rc`, a `RefCell`, a `Cell`
/// or an `Interior` are the ones `SHARED_PAYLOADS` lists (C9). The scan reads
/// each payload's type as written, so a type that wraps one inside a struct
/// of its own is beyond it.
#[test]
fn heap_payloads_hold_no_new_shared_state() {
    let heap = Path::new(env!("CARGO_MANIFEST_DIR")).join("../patina-core/src/heap/mod.rs");
    let text = std::fs::read_to_string(heap).expect("read the heap's source");
    let found: BTreeSet<String> = shared_payloads(&text).into_iter().collect();
    let listed: BTreeSet<String> = SHARED_PAYLOADS.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        found, listed,
        "the payloads of `HeapObjectData` holding an `Rc`, `RefCell`, `Cell` or `Interior` \
         (left) and `SHARED_PAYLOADS` (right) differ. A new one is shared state that rule 3 \
         (`PRD/GC_PRD.md` §18.2) takes out by stage 5 (C9, #711): keep it out of the payload \
         where it can be, and list it where not. One that has gone comes off the list"
    );
}

/// Positive control: the scan finds a `static mut` declared outside a
/// comment, and not one in a comment, a `static` or a `&'static mut`. The
/// declaration is put together here, so that the workspace scan does not
/// find this file.
#[test]
fn the_static_scan_finds_a_static_mut() {
    let declaration = format!("pub {} mut COUNTER: u32 = 0;", "static");
    let text = format!(
        "// {declaration}\n\
         static COUNT: AtomicU32 = AtomicU32::new(0);\n\
         fn leak() -> &'static mut u32 {{ Box::leak(Box::new(0)) }}\n\
         {declaration}\n"
    );
    assert_eq!(static_muts(&text), [declaration]);
}

/// Positive control: the payload scan finds a struct variant's `RefCell`
/// field, a tuple variant's `Rc` and one that rustfmt split over lines, and
/// passes the rest, comments and attributes among them.
#[test]
fn the_payload_scan_finds_new_shared_state() {
    let text = "pub enum HeapObjectData {
    Pair(TaggedValue, TaggedValue),
    /// A counter. An `Rc<str>` in a comment is not a payload.
    #[allow(dead_code)]
    Counter {
        hits: RefCell<u32>,
        name: String, // nor is a Cell<u8> here
    },
    Shared(TaggedValue, Rc<str>),
    Split(
        std::collections::HashMap<String, std::rc::Rc<crate::port::Port>>,
    ),
    Callback(Box<dyn Fn(TaggedValue) -> TaggedValue>),
    Counted(Arc<RefCellCount>),
    Free,
}
";
    assert_eq!(
        shared_payloads(text),
        ["Counter.hits", "Shared.1", "Split.0"]
    );
}

/// Each crate's directory under `crates/`, in order.
fn crate_dirs() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(crates)
        .expect("read the crates directory")
        .map(|entry| entry.expect("a crate directory").path())
        .filter(|dir| dir.join("src").is_dir())
        .collect();
    dirs.sort();
    dirs
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

/// The lines of `text` that declare a `static mut` outside a line comment.
fn static_muts(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| {
            let code = line.split("//").next().unwrap_or("");
            let words: Vec<&str> = code.split_whitespace().collect();
            words.windows(2).any(|pair| pair == ["static", "mut"])
        })
        .map(str::to_string)
        .collect()
}

/// Each payload of the `HeapObjectData` enum in `text` whose type names
/// `Rc`, `RefCell`, `Cell` or `Interior`, as `Variant.field`, or `Variant.0`
/// and on for a tuple variant's, in order.
fn shared_payloads(text: &str) -> Vec<String> {
    const HEAD: &str = "pub enum HeapObjectData {";
    let start = text.find(HEAD).expect("the HeapObjectData enum") + HEAD.len();
    // The rest of the text on one line, without comments or attributes, and
    // with no `->` to unbalance the brackets.
    let body = text[start..]
        .lines()
        .map(|line| line.split("//").next().unwrap_or("").trim())
        .filter(|line| !line.starts_with("#["))
        .collect::<Vec<_>>()
        .join(" ")
        .replace("->", "  ");
    // The enum's body ends at the brace that closes it.
    let mut depth = 0usize;
    let mut end = body.len();
    for (i, c) in body.char_indices() {
        match c {
            '{' | '(' | '[' | '<' => depth += 1,
            '}' if depth == 0 => {
                end = i;
                break;
            }
            '}' | ')' | ']' | '>' => depth -= 1,
            _ => {}
        }
    }
    let mut found = Vec::new();
    for variant in top_level_pieces(&body[..end]) {
        let name: String = variant
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let payload = variant[name.len()..].trim();
        if let Some(fields) = payload.strip_prefix('(').and_then(|p| p.strip_suffix(')')) {
            for (index, ty) in top_level_pieces(fields).into_iter().enumerate() {
                if names_shared_state(ty) {
                    found.push(format!("{name}.{index}"));
                }
            }
        } else if let Some(fields) = payload.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
            for field in top_level_pieces(fields) {
                let (field, ty) = field.split_once(':').expect("a named field and its type");
                if names_shared_state(ty) {
                    found.push(format!("{name}.{}", field.trim()));
                }
            }
        }
    }
    found
}

/// The pieces of `text` between its commas outside any bracket, trimmed,
/// with the empty one after a trailing comma left out.
fn top_level_pieces(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut depth = 0usize;
    let mut from = 0;
    for (i, c) in text.char_indices() {
        match c {
            '{' | '(' | '[' | '<' => depth += 1,
            '}' | ')' | ']' | '>' => depth -= 1,
            ',' if depth == 0 => {
                pieces.push(text[from..i].trim());
                from = i + 1;
            }
            _ => {}
        }
    }
    pieces.push(text[from..].trim());
    pieces.retain(|piece| !piece.is_empty());
    pieces
}

/// Whether a type, as written, names `Rc`, `RefCell`, `Cell` or `Interior`:
/// a whole word, so neither `Arc` nor `RefCellCount` does.
fn names_shared_state(ty: &str) -> bool {
    ty.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| matches!(word, "Rc" | "RefCell" | "Cell" | "Interior"))
}
