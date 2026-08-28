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
//! `PATINA_SCOPE_TRACE=<file>` appends a record per binding and per scoped
//! resolution. **Use an absolute path**: the compat harness and the Larceny
//! runner both `cd` into a scratch directory they then delete.
//!
//! ```text
//! RUN pid=4711
//! BIND    phase=desugar name="x" scopes={S136} byname=true
//! BIND    phase=run     name="c" scopes={}     byname=true
//! RESOLVE phase=run     name="c" ref={S137} cands=0 picked=-      via=byname
//! RESOLVE phase=run     name="x" ref={S136} cands=1 picked={S136} via=scoped
//! ```
//!
//! The second and third lines are the internal-define finding: bound at `{}`,
//! so a reference carrying `{S137}` has nothing to resolve against and falls
//! back by name. That is family 36 in two lines.
//!
//! # What the fields mean
//!
//! - `phase` — `desugar` while the desugarer is walking, `compile` inside the
//!   VM's renamer, `run` during evaluation. Without it the desugarer's
//!   lookups and the evaluator's are interleaved and indistinguishable, which
//!   is the state that made `macro-debug-mode`'s output hard to read for
//!   exactly this question.
//! - `byname` — whether a name-only lookup may also reach the binding
//!   (`visible_by_name`). The difference between a parameter and a definition,
//!   and the mechanism behind the family 36 capture.
//! - `cands` / `picked` / `via` — how many bindings the rule considered, which
//!   it chose, and whether the answer came from scopes at all. `via=byname`
//!   means set-of-scopes resolution declined and the caller fell back to
//!   spelling; a hygiene defect is usually a `via=byname` that should have
//!   been `via=scoped`.
//!
//! Unlike [`crate::scope_resolve`]'s ambiguity log, records are **not**
//! deduplicated: order and repetition are the signal here, since the question
//! is usually "which of these two happened first".
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
    // `{:?}` quotes and escapes: Patina accepts `|a b|` as an identifier, and
    // an unescaped name could forge a field or a whole record.
    emit(format!(
        "BIND    phase={} name={:?} scopes={} byname={}\n",
        phase_str(),
        name,
        render(scopes),
        visible_by_name
    ));
}

/// Record a scoped resolution and how it was answered.
///
/// `picked` is `None` when no candidate was a subset, which the caller then
/// answers by name — the `via=byname` case, and the one worth grepping for.
pub fn resolve(
    name: &str,
    reference: &ScopeSet,
    candidates: usize,
    picked: Option<&ScopeSet>,
    write: bool,
) {
    if !enabled() {
        return;
    }
    emit(format!(
        "RESOLVE phase={} name={:?} ref={} cands={} picked={} via={} op={}\n",
        phase_str(),
        name,
        render(reference),
        candidates,
        picked.map(render).unwrap_or_else(|| "-".to_string()),
        if picked.is_some() { "scoped" } else { "byname" },
        if write { "set" } else { "get" }
    ));
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
}
