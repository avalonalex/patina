//! A record of what scopes bindings actually get, and how references resolve
//! against them.
//!
//! Set-of-scopes hygiene is decided by data — a `ScopeSet` on each binding and
//! each reference — and that data was, until this module, invisible. Every
//! wrong turn in triage families 36 to 38 had the same shape: someone read the
//! code that *computes* a scope set, reasoned correctly about it, and was
//! wrong about the value that arrives where it matters. Two examples, both
//! costly:
//!
//! - A one-line change binding parameters through `define_with_scopes` looked
//!   like it fixed family 36. It fixed two repros and broke lambda evaluation
//!   outright, because a parameter's *runtime* references depend on the
//!   by-name path the change removed.
//! - A review concluded family 38 was not blocked on family 36, having traced
//!   the desugarer stamping internal-define binders with a scope set, through
//!   `CoreExprKind::Define`, into `step.rs`. Every step was real; the scope
//!   set arrives **empty**, so the conclusion was not. Settling it took an ad
//!   hoc `eprintln!` in a build nobody kept.
//!
//! That last question — "what does this binding actually get bound at?" —
//! should cost one command, not a code edit. It does now.
//!
//! # Using it
//!
//! `PATINA_SCOPE_TRACE=<file>` appends a record per binding, per scoped
//! resolution, and per completed scoped write. **Use an absolute path**: the
//! compat harness and the Larceny runner both `cd` into a scratch directory
//! they then delete.
//!
//! **Trace one program, not a suite.** Records are written for every binding
//! and every scoped resolution, with no sampling and no deduplication, so the
//! cost is proportional to work done rather than to code size: a 300 000-
//! iteration loop measured **14× slower and 352 MB of trace**. That is the
//! right trade for a diagnostic answering "what happened to this one name",
//! and the wrong one for anything else. Reduce the program first.
//!
//! ```text
//! RUN pid=4711
//! BIND    phase=desugar name="c" scopes={S136} byname=false
//! BIND    phase=run name="c" scopes={} byname=true
//! RESOLVE phase=run name="c" ref={S137} cands=0 picked=- via=byname op=set
//! WROTE   phase=run name="c" ref={S137} landed=byname
//! ```
//!
//! Those four lines are triage family 36. The desugarer stamps the internal
//! define with `{S136}`; the binding that reaches the runtime carries nothing,
//! so a reference at `{S137}` has no candidate, falls back to spelling, and
//! the write lands somewhere the rule never chose. A `let` binder traced the
//! same way keeps its scopes at `phase=run`.
//!
//! # What the fields mean
//!
//! - `phase` — `desugar` while the desugarer is walking, `compile` inside the
//!   VM's renamer, `run` during evaluation. Without it the desugarer's lookups
//!   and the evaluator's interleave indistinguishably, which is the state that
//!   made `macro-debug-mode`'s output hard to read for exactly this question.
//! - `scopes` — what a binding is *filed under*. `{}` means it went to the
//!   plain by-name table and set-of-scopes resolution cannot see it at all.
//! - `byname` — [`crate::environment::ScopedBinding`]'s `visible_by_name`:
//!   whether a name-only lookup may also reach it. The difference between a
//!   parameter and a definition, and the mechanism behind family 36's capture.
//! - `cands` — how many bindings **passed** `is_candidate`, at every site, so
//!   the number means one thing and the two backends can be compared.
//! - `picked` / `via` — which binding the rule chose and how it ended:
//!   `scoped` (the rule decided), `byname` (it declined and the caller fell
//!   back to spelling), `ambiguous` (two candidates, neither more specific —
//!   an error rather than an answer).
//! - `op` — `get`, `set`, or `bind` for a binding occurrence being renamed.
//!   The VM's renamer serves all three through one function, so without this
//!   a VM trace could not be diffed against the tree-walker's for the
//!   write-side split families 36 and 38 turn on.
//! - `landed` on `WROTE` — where a scoped write finally went. A write walks
//!   environment by environment, so its trace is a *sequence* where a read's
//!   is one line; this is its conclusion, and the answer to family 38's
//!   question.
//!
//! `via=byname` is common — every reference to a global from a scoped context
//! is one — so it is a filter rather than a verdict. What is worth grepping is
//! a `via=byname` on a name that *has* a scoped binding two lines above:
//! resolution saw a candidate, rejected it, and spelling answered anyway.
//!
//! Records are **not** deduplicated, unlike [`crate::scope_resolve`]'s
//! ambiguity log: order and repetition are the signal, since the question is
//! usually which of two things happened first.
//!
//! # What it does not see
//!
//! Bindings the VM resolves at compile time and then addresses by register —
//! after `alpha_rename` has renamed them, there is no scope set left to
//! record. `phase=compile` records the renamer's decisions, which is the last
//! point the VM has scopes at all.

use crate::scope::ScopeSet;
use std::cell::Cell;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

/// Which part of the pipeline is running.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Desugar,
    Compile,
    Run,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::Desugar => "desugar",
            Phase::Compile => "compile",
            Phase::Run => "run",
        }
    }
}

thread_local! {
    /// `Run` by default: evaluation is the phase with no single entry point to
    /// mark, and marking the other two is enough to tell them apart.
    static PHASE: Cell<Phase> = const { Cell::new(Phase::Run) };
}

/// Restores the previous phase when dropped.
///
/// A guard rather than a set/reset pair because desugaring re-enters itself —
/// a macro expanded during desugaring desugars its output — and a plain reset
/// would report the inner return as leaving the phase entirely.
pub struct PhaseGuard(Phase);

impl Drop for PhaseGuard {
    fn drop(&mut self) {
        PHASE.with(|p| p.set(self.0));
    }
}

/// Enter `phase` until the returned guard drops.
#[must_use]
pub fn enter(phase: Phase) -> PhaseGuard {
    PHASE.with(|p| PhaseGuard(p.replace(phase)))
}

/// What a resolution was for.
///
/// Three, not a `write: bool`, because the VM's renamer answers all three
/// through one function: a reference, an assignment target, and a *binding*
/// occurrence it is renaming. Recording them alike made every VM record read
/// `op=get`, so a trace could not be diffed against the tree-walker's for the
/// write-side split that families 36 and 38 turn on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// A reference being read.
    Get,
    /// A `set!` target.
    Set,
    /// A binding occurrence being resolved so it can be renamed. The VM only;
    /// the tree-walker binds without resolving.
    Bind,
}

impl Op {
    fn as_str(self) -> &'static str {
        match self {
            Op::Get => "get",
            Op::Set => "set",
            Op::Bind => "bind",
        }
    }
}

/// How a resolution ended.
///
/// `via=byname` used to mean everything that was not a scoped hit, which made
/// it 73% of the records on a five-line program and useless as the grep the
/// docs recommend. Separating "the rule declined and spelling answered" from
/// "nothing answered" is what makes it a signal.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Set-of-scopes resolution chose a binding.
    Scoped,
    /// It declined; the caller falls back to a by-name lookup. Whether *that*
    /// succeeds is not known here — see the `BIND`/`RESOLVE` pair around it.
    ByName,
    /// Two candidates and neither more specific: an error, not an answer.
    Ambiguous,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Scoped => "scoped",
            Outcome::ByName => "byname",
            Outcome::Ambiguous => "ambiguous",
        }
    }
}

struct Sink(std::fs::File);

fn sink() -> Option<&'static Mutex<Sink>> {
    static SINK: OnceLock<Option<Mutex<Sink>>> = OnceLock::new();
    SINK.get_or_init(|| {
        // An empty value is what a shell script makes of an unset variable;
        // treating it as a path opens nothing and would make a broken trace
        // look like a clean one. Same guard `gc.rs` and `scope_resolve.rs` use.
        let path = std::env::var("PATINA_SCOPE_TRACE")
            .ok()
            .filter(|v| !v.is_empty() && v != "0")?;
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(mut file) => {
                // Proof the instrument ran: an empty trace is only evidence
                // when a `RUN` line says the process was watched.
                let _ = file.write_all(format!("RUN pid={}\n", std::process::id()).as_bytes());
                Some(Mutex::new(Sink(file)))
            }
            Err(e) => {
                // Loud: a silent failure here reads as "nothing was bound",
                // which is the conclusion this exists to make trustworthy.
                eprintln!("patina: PATINA_SCOPE_TRACE={path}: {e}");
                None
            }
        }
    })
    .as_ref()
}

/// Is the trace on? A `OnceLock` read and a branch.
///
/// Callers check this *before* building any string, so a run without the
/// variable pays only this on the tree-walker's per-read path.
#[inline]
pub fn enabled() -> bool {
    sink().is_some()
}

fn emit(record: String) {
    let Some(sink) = sink() else {
        return;
    };
    // Never poison-panic: this is a diagnostic, and a `File` behind a `Mutex`
    // has no invariant a panic elsewhere can have broken.
    let mut sink = sink.lock().unwrap_or_else(|e| e.into_inner());
    // One `write_all`, not `writeln!`: `O_APPEND` makes each syscall atomic,
    // never each `write!` fragment, and the corpus harness runs up to eight
    // patina processes at once. Written a fragment at a time their records
    // shred each other — measured at six mangled lines per corpus sweep on the
    // ambiguity log before it was fixed the same way.
    let _ = sink.0.write_all(record.as_bytes());
}

/// Render a scope set as one space-free field, so a record stays parseable.
fn render(scopes: &ScopeSet) -> String {
    if scopes.is_empty() {
        // The interesting value, and the one an empty string would hide.
        return "{}".to_string();
    }
    let mut out = String::from("{");
    for (i, scope) in scopes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("{scope}"));
    }
    out.push('}');
    out
}

fn phase_str() -> &'static str {
    PHASE.with(|p| p.get()).as_str()
}

/// Record a binding being created.
///
/// `scopes` is what it is actually filed under — `{}` meaning it went to the
/// plain by-name table, which is the fact that took an ad hoc `eprintln!` to
/// establish for internal defines.
pub fn bind(name: &str, scopes: &ScopeSet, visible_by_name: bool) {
    if !enabled() {
        return;
    }
    emit(bind_record(phase_str(), name, scopes, visible_by_name));
}

/// The `BIND` grammar, separated from the sink so it can be tested.
///
/// The sink is a process-global `OnceLock` over an environment variable read
/// once, which a unit test cannot set deterministically. The format is the
/// part that drifts — two documents describe it — so the format is the part
/// under test.
fn bind_record(phase: &str, name: &str, scopes: &ScopeSet, visible_by_name: bool) -> String {
    // `{:?}` quotes and escapes: Patina accepts `|a b|` as an identifier, and
    // an unescaped name could forge a field or a whole record.
    format!(
        "BIND    phase={phase} name={name:?} scopes={} byname={visible_by_name}\n",
        render(scopes)
    )
}

/// Record a scoped resolution and how it was answered.
///
/// `candidates` is always the number that **passed** `is_candidate` — the
/// field means one thing at every site. Counting all bindings of the name at
/// one site and the visible ones at another made `cands` incomparable across
/// backends, which is the comparison the field exists for.
pub fn resolve(
    name: &str,
    reference: &ScopeSet,
    candidates: usize,
    picked: Option<&ScopeSet>,
    op: Op,
    outcome: Outcome,
) {
    if !enabled() {
        return;
    }
    emit(resolve_record(
        phase_str(),
        name,
        reference,
        candidates,
        picked,
        op,
        outcome,
    ));
}

/// The `RESOLVE` grammar. See [`bind_record`] for why it is split out.
fn resolve_record(
    phase: &str,
    name: &str,
    reference: &ScopeSet,
    candidates: usize,
    picked: Option<&ScopeSet>,
    op: Op,
    outcome: Outcome,
) -> String {
    format!(
        "RESOLVE phase={phase} name={name:?} ref={} cands={candidates} picked={} via={} op={}\n",
        render(reference),
        picked.map(render).unwrap_or_else(|| "-".to_string()),
        outcome.as_str(),
        op.as_str()
    )
}

/// Record where a scoped write finally landed.
///
/// The write walks environment by environment, so its trace is a *sequence*
/// where a read's is one line, and without a terminal record the sequence has
/// no conclusion — which is exactly the question triage family 38 asks. `where`
/// is `scoped`, `byname`, or `undefined`.
pub fn wrote(name: &str, reference: &ScopeSet, landed: &str) {
    if !enabled() {
        return;
    }
    emit(wrote_record(phase_str(), name, reference, landed));
}

/// The `WROTE` grammar. See [`bind_record`] for why it is split out.
fn wrote_record(phase: &str, name: &str, reference: &ScopeSet, landed: &str) -> String {
    format!(
        "WROTE   phase={phase} name={name:?} ref={} landed={landed}\n",
        render(reference)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The phase is restored on drop, including through nesting — the case a
    /// set/reset pair gets wrong when a macro expanded during desugaring
    /// desugars its own output.
    #[test]
    fn nested_phases_restore_in_order() {
        assert_eq!(phase_str(), "run");
        {
            let _outer = enter(Phase::Desugar);
            assert_eq!(phase_str(), "desugar");
            {
                let _inner = enter(Phase::Desugar);
                assert_eq!(phase_str(), "desugar");
            }
            // The inner guard restored `desugar`, not `run`.
            assert_eq!(phase_str(), "desugar");
        }
        assert_eq!(phase_str(), "run");
    }

    /// An empty scope set renders as a visible token rather than nothing, so
    /// the record stays parseable and the interesting case stays legible.
    #[test]
    fn an_empty_scope_set_is_visible() {
        assert_eq!(render(&ScopeSet::new()), "{}");
    }

    fn set(ids: &[usize]) -> ScopeSet {
        let mut s = ScopeSet::new();
        for &id in ids {
            s.add_scope(crate::scope::ScopeId(id));
        }
        s
    }

    /// The exact lines two documents promise.
    ///
    /// Asserted literally rather than by round-tripping a parser: the contract
    /// is a format a person greps and `awk`s, so a test that only proves it is
    /// self-consistent would let field order, spacing or the `{}` spelling
    /// drift away from what the module doc and `docs/MACRO_SYSTEM.md` teach.
    /// Every field this file documents appears in one of the three.
    #[test]
    fn the_record_grammar_is_what_the_docs_say() {
        assert_eq!(
            bind_record("desugar", "c", &set(&[136]), false),
            "BIND    phase=desugar name=\"c\" scopes={S136} byname=false\n"
        );
        // The finding the trace exists for: bound under nothing at all.
        assert_eq!(
            bind_record("run", "c", &ScopeSet::new(), true),
            "BIND    phase=run name=\"c\" scopes={} byname=true\n"
        );
        assert_eq!(
            resolve_record("run", "c", &set(&[137]), 0, None, Op::Set, Outcome::ByName),
            "RESOLVE phase=run name=\"c\" ref={S137} cands=0 picked=- via=byname op=set\n"
        );
        assert_eq!(
            resolve_record(
                "x",
                "x",
                &set(&[136, 138]),
                2,
                Some(&set(&[136])),
                Op::Get,
                Outcome::Scoped
            ),
            "RESOLVE phase=x name=\"x\" ref={S136,S138} cands=2 picked={S136} via=scoped op=get\n"
        );
        assert_eq!(
            wrote_record("run", "c", &set(&[137]), "byname"),
            "WROTE   phase=run name=\"c\" ref={S137} landed=byname\n"
        );
    }

    /// A name that could otherwise forge a field or a whole record is escaped.
    ///
    /// Patina accepts `|a b|` as an identifier, so a name may contain spaces,
    /// quotes or newlines — and the format is space-separated.
    #[test]
    fn a_name_cannot_forge_a_record() {
        let line = bind_record("run", "a b\nBIND fake", &ScopeSet::new(), true);
        assert_eq!(line.lines().count(), 1, "{line:?}");
        assert!(line.contains(r#"name="a b\nBIND fake""#), "{line:?}");
    }

    /// Every `Op` and `Outcome` renders as the token the docs list — a new
    /// variant added without a rendering would otherwise reach the file as
    /// whatever `Debug` prints.
    #[test]
    fn every_op_and_outcome_has_its_documented_token() {
        assert_eq!(
            [Op::Get, Op::Set, Op::Bind].map(Op::as_str),
            ["get", "set", "bind"]
        );
        assert_eq!(
            [Outcome::Scoped, Outcome::ByName, Outcome::Ambiguous].map(Outcome::as_str),
            ["scoped", "byname", "ambiguous"]
        );
    }
}
