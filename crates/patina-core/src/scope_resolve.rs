//! One rule for resolving a reference against a set of candidate bindings.
//!
//! Set-of-scopes resolution (Flatt, "Binding as Sets of Scopes", POPL 2016)
//! is used in two places: the tree-walker resolves a reference every time it
//! evaluates one, against a chain of [`Environment`]s; the VM resolves each
//! one once, at compile time, against a stack of frames, and renames it so
//! that the rest of the pipeline needs only names.
//!
//! [`Environment`]: crate::environment::Environment
//!
//! Both used to carry their own copy of the rule. Two copies of one rule must
//! agree, and nothing made them: they already differed on which binding wins
//! a tie — the tree-walker kept the first in its list, the VM's renamer the
//! last in its frame — and the ambiguity check could only see one of them, so
//! a VM run reported nothing however it resolved. This module is now the one
//! copy *for reads*, and it checks itself, so both backends' reads are
//! measured by construction.
//!
//! Two hand-rolled copies remain, both over `Environment`'s tables and
//! neither measured: `set_with_scopes` resolves a write by exact scope-set
//! match, and `has_scoped_binding` by a bare subset test with no
//! most-specific rule. Merging them is triage family 38's work; until then no
//! sweep says anything about `set!`.
//!
//! Unifying the tie-break was a **behaviour change**, not a pure refactor:
//! the tree-walker now answers a within-environment tie the way the VM
//! always did. `tests::a_tie_goes_to_the_most_recent_candidate` pins it.
//!
//! [`resolve_scoped`] is the whole rule. Callers hand it every binding of the
//! name, **most recent first**, and it does the rest.

use crate::scope::ScopeSet;

/// Resolve `reference` against `candidates`, which must be every binding of
/// `name` visible here, ordered **most recent first** — innermost
/// environment or frame first, and within one of those, latest binding
/// first, so that a later binding shadows an earlier one.
///
/// The rule: keep the candidates whose scope set is a subset of the
/// reference's, and answer with the one whose scope set is largest. A tie
/// goes to the first, which is the most recent. `None` means no candidate
/// was a subset, and the caller falls back to its own by-name lookup.
///
/// Every call is checked against Flatt's ambiguity condition — see the
/// `ambiguity` module — so this is also where a resolution the rule does not
/// determine is reported or refused.
pub fn resolve_scoped<T: Clone>(
    name: &str,
    reference: &ScopeSet,
    candidates: &[(ScopeSet, T)],
) -> Option<T> {
    // One pass, no allocation on the release path: the candidate list is
    // materialised only when the check is on, which in release means only
    // when asked for. A debug build always checks, and pays for it there.
    let mut best: Option<usize> = None;
    for (index, (scopes, _)) in candidates.iter().enumerate() {
        if !is_candidate(scopes, reference) {
            continue;
        }
        match best {
            None => best = Some(index),
            // Strictly larger wins; a tie keeps the earlier, most recent one.
            Some(current) if scopes.len() > candidates[current].0.len() => best = Some(index),
            Some(_) => {}
        }
    }
    let best = best?;
    if ambiguity::checking() {
        let matched: Vec<&ScopeSet> = candidates
            .iter()
            .map(|(scopes, _)| scopes)
            .filter(|scopes| is_candidate(scopes, reference))
            .collect();
        let winner = candidates[..best]
            .iter()
            .filter(|(scopes, _)| is_candidate(scopes, reference))
            .count();
        ambiguity::check(name, reference, &matched, winner);
    }
    Some(candidates[best].1.clone())
}

/// Is a binding with these scopes in the running for a reference with those?
///
/// The candidacy half of the rule, exported so a caller can drop a binding
/// before cloning its scope set — the collectors do, since a non-candidate is
/// shown neither to the resolver nor to the check — without keeping a second
/// copy of the test.
#[inline]
pub fn is_candidate(binding: &ScopeSet, reference: &ScopeSet) -> bool {
    binding.is_subset_of(reference)
}

/// Reports a resolution that the rule in this module does not determine.
///
/// Flatt's rule ("Binding as Sets of Scopes", POPL 2016 §3) resolves a
/// reference to the candidate whose scope set is the largest subset of the
/// reference's — *and requires that candidate to be a superset of every other
/// candidate*. When two candidates are not ordered by subset, neither is more
/// specific, the reference is **ambiguous**, and Racket raises an error.
///
/// [`resolve_scoped`] takes the largest by size and, on a tie, the first —
/// which callers order most recent first. This module reports every place
/// those two disagree, in two kinds, kept apart because they are different
/// phenomena:
///
/// - `AMBIG` — Flatt-ambiguous: the winner is not a superset of some rival,
///   so the *rule* does not determine the answer and size decides it.
/// - `TIE` — a rival with the **identical** scope set, so even size does not
///   decide and the answer comes from the caller's ordering alone. Racket
///   cannot reach this (its binding table is keyed by scope set, one binding
///   per key); Patina can, because `Environment::insert_scoped` dedups only
///   within one environment while the walk covers the whole chain.
///
/// Two environment variables, both off by default:
///
/// - `PATINA_AMBIGUITY_LOG=<file>` appends a record per distinct site.
///   **Use an absolute path.** The compat harness and the Larceny runner both
///   `cd` into a scratch directory that they then delete, so a relative path
///   writes 249 logs into directories that no longer exist.
/// - `PATINA_AMBIGUITY_STRICT=1` panics on `AMBIG` instead of accepting it.
///
/// The record grammar — fields never contain spaces, and an empty scope set
/// renders as `{}` so every field is present:
///
/// ```text
/// RUN pid=4711
/// AMBIG name="x" ref=S1,S2,S3 picked=S1,S2 rivals=S3
/// TIE name="ls" ref=S1,S2 picked=S1 equal=S1
/// ```
///
/// **What it does not see**, so that a silent log is read for what it is:
///
/// - The fallback. [`resolve_scoped`] returns `None` when no candidate is a
///   subset, and the caller then answers by name with no scope reasoning at
///   all. That path is neither reported nor refused.
/// - Writes. `Environment::set_with_scopes` still resolves by exact scope-set
///   match and `has_scoped_binding` by a bare subset test; neither calls this
///   module, so every `set!` is excluded (triage family 38).
/// - The VM's `Define` arm resolves *binding* occurrences through
///   [`resolve_scoped`], where the tree-walker does not, so VM records include
///   definitions and the two backends' counts are not like for like.
mod ambiguity {
    use super::ScopeSet;
    use std::collections::HashSet;
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};

    struct Sink {
        file: std::fs::File,
        /// One record per distinct site. A reference inside a hot loop
        /// resolves millions of times and says nothing new after the first.
        seen: HashSet<String>,
    }

    fn sink() -> Option<&'static Mutex<Sink>> {
        static SINK: OnceLock<Option<Mutex<Sink>>> = OnceLock::new();
        SINK.get_or_init(|| {
            // An empty value is what a shell script produces from an unset
            // variable; treating it as a path opens nothing and would make a
            // broken sink look like a clean result. Same guard `gc.rs` uses.
            let path = std::env::var("PATINA_AMBIGUITY_LOG")
                .ok()
                .filter(|v| !v.is_empty() && v != "0")?;
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                Ok(mut file) => {
                    // Proof the instrument ran: silence in the log is only
                    // evidence when a `RUN` line says the process was watched.
                    let _ = file.write_all(format!("RUN pid={}\n", std::process::id()).as_bytes());
                    Some(Mutex::new(Sink {
                        file,
                        seen: HashSet::new(),
                    }))
                }
                Err(e) => {
                    // Loud, because a silent failure here reads as "resolution
                    // is not guessing", which is the conclusion this exists to
                    // make trustworthy.
                    eprintln!("patina: PATINA_AMBIGUITY_LOG={path}: {e}");
                    None
                }
            }
        })
        .as_ref()
    }

    /// Is the log on? A `OnceLock` read and a branch.
    fn logging() -> bool {
        sink().is_some()
    }

    /// Is strict mode on (`PATINA_AMBIGUITY_STRICT`)? It adds the refusal to
    /// a *release* build; a debug build refuses anyway.
    ///
    /// It was once the other way round. Enforcing Flatt's rule looked free —
    /// no workload logged an ambiguous reference — until the check ran over
    /// the Rust suite, where `test_let_syntax_nested_lexical_scoping` tripped
    /// it: a passing, R7RS-required program whose answer came from
    /// environment nesting rather than from scopes, because each `let` bound
    /// its variable at one fresh scope and sibling binders were therefore
    /// unordered. Larceny triage family 39. Fixed 2026-08-27 by scoping a
    /// binder at the scopes it *stands in*, so nested binders form a chain
    /// and a chain is always decidable; the suite passes the check now, and
    /// so does every workload — 249 processes, zero ambiguous references.
    fn strict() -> bool {
        static STRICT: OnceLock<bool> = OnceLock::new();
        *STRICT.get_or_init(|| {
            std::env::var("PATINA_AMBIGUITY_STRICT")
                .ok()
                .is_some_and(|v| !v.is_empty() && v != "0")
        })
    }

    /// Should the check run at all?
    ///
    /// Always in a debug build, where an ambiguous reference is a panic. In a
    /// release build only when one of the two variables asks for it, so a
    /// release interpreter pays a `OnceLock` read and a branch.
    pub(super) fn checking() -> bool {
        refusing() || logging()
    }

    /// Is an ambiguous reference refused rather than reported?
    ///
    /// In a debug build, always — so every `cargo test` and both CI debug
    /// lanes hold Flatt's rule without anyone opting in. A release build
    /// refuses only when `PATINA_AMBIGUITY_STRICT` asks it to.
    fn refusing() -> bool {
        cfg!(debug_assertions) || strict()
    }

    /// Render a scope set without spaces, so a record stays one field.
    fn render(scopes: &ScopeSet) -> String {
        if scopes.is_empty() {
            // A VM `is_simple` binding has none. Rendered as the empty string
            // it left two blank fields and an unparseable record.
            return "{}".to_string();
        }
        let mut out = String::new();
        for (i, scope) in scopes.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("{scope}"));
        }
        out
    }

    /// Judge one resolution: `candidates[winner]` was chosen.
    ///
    /// An `AMBIG` verdict panics under `PATINA_AMBIGUITY_STRICT` and is
    /// otherwise reported. Patina is pre-alpha and unversioned, so the target
    /// is the specification rather than compatibility with its own past
    /// answers, and a reference the rule does not determine is a defect: it
    /// is how Larceny triage family 37 produced `(a a)` for
    /// `(match '(a . 1) ((x . y) (list x y)))` instead of failing. It is not
    /// the default only because `main` cannot pass it yet — see
    /// [`strict`] for the case that fails and why it matters.
    ///
    /// A `TIE` is only reported. It is a different phenomenon: two bindings
    /// with the *same* scope set, which Flatt's model cannot express and
    /// which the caller's ordering therefore decides. They are common — 371
    /// across the sweep once both backends are measured — and not all are
    /// defects: a VM `is_simple` binding carries an *empty* scope set, so
    /// nested internal defines of one name tie legitimately. Asserting on a
    /// `TIE` would fail ordinary programs; what is worth fixing is whatever
    /// creates a duplicate that is not of that kind.
    pub(super) fn check(name: &str, reference: &ScopeSet, candidates: &[&ScopeSet], winner: usize) {
        let best = candidates[winner];
        let mut rivals: Vec<&ScopeSet> = Vec::new();
        let mut equal: Vec<&ScopeSet> = Vec::new();
        for (index, scopes) in candidates.iter().copied().enumerate() {
            if index == winner {
                continue;
            }
            // Equality first: `is_subset_of` is reflexive, so an identical
            // set would otherwise be filtered out as "contained" — and that
            // is the one pick nothing in the rule justifies.
            if scopes == best {
                equal.push(scopes);
            } else if !scopes.is_subset_of(best) {
                rivals.push(scopes);
            }
        }
        if rivals.is_empty() && equal.is_empty() {
            // Nothing to report, and nothing to refuse: no rivals means the
            // rule decided this reference on its own.
            return;
        }
        let ambiguous = !rivals.is_empty();
        let refuse = || {
            assert!(
                !ambiguous || !refusing(),
                "ambiguous reference: `{}` with scopes {} resolves to {} and to {}, \
             and neither contains the other — Flatt's rule does not determine \
             this reference, so the answer came from scope-set size alone. \
             See the `ambiguity` module and Larceny triage family 39.",
                name,
                reference,
                best,
                rivals
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(" and to ")
            )
        };
        // `{:?}` quotes and escapes: Patina accepts `|a b|` as an identifier,
        // and an unescaped name could forge a field or a whole record.
        let record = if rivals.is_empty() {
            format!(
                "TIE name={:?} ref={} picked={} equal={}\n",
                name,
                render(reference),
                render(best),
                equal
                    .iter()
                    .map(|s| render(s))
                    .collect::<Vec<_>>()
                    .join("|")
            )
        } else {
            format!(
                "AMBIG name={:?} ref={} picked={} rivals={}\n",
                name,
                render(reference),
                render(best),
                rivals
                    .iter()
                    .map(|s| render(s))
                    .collect::<Vec<_>>()
                    .join("|")
            )
        };
        let Some(sink) = sink() else {
            // Strict without a log: nothing to write, but still refuse.
            refuse();
            return;
        };
        // Never poison-panic: this is a diagnostic, and a `File` behind a
        // `Mutex` has no invariant a panic elsewhere can have broken.
        let mut sink = sink.lock().unwrap_or_else(|e| e.into_inner());
        if !sink.seen.insert(record.clone()) {
            drop(sink);
            refuse();
            return;
        }
        // One `write_all`, not `writeln!`: `O_APPEND` makes each syscall
        // atomic, never each `write!` fragment, and the corpus harness runs
        // up to eight patina processes appending to one file. Written a
        // fragment at a time, their records shred each other — measured at
        // six mangled lines per corpus sweep before this.
        let _ = sink.file.write_all(record.as_bytes());
        drop(sink);

        // Refused only after the record is on disk. Panicking first left a
        // strict run's log holding just its `RUN` line — which a sweep reads
        // as "this process was watched and found nothing", the exact
        // conclusion the `RUN` line exists to license.
        refuse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::ScopeId;

    /// Fixed ids, not `ScopeId::fresh()`: that bumps a process-global
    /// counter, and `scope::tests::test_fresh_scope_ids` resets it and then
    /// asserts the next three values. Cargo runs both on threads of one
    /// binary, so a fresh id here is a flake there.
    fn set(scopes: &[usize]) -> ScopeSet {
        let mut s = ScopeSet::new();
        for &scope in scopes {
            s.add_scope(ScopeId(scope));
        }
        s
    }

    /// The largest scope set that is a subset of the reference's wins.
    #[test]
    fn the_most_specific_candidate_wins() {
        let candidates = vec![(set(&[1]), "outer"), (set(&[1, 2]), "inner")];
        assert_eq!(
            resolve_scoped("x", &set(&[1, 2]), &candidates),
            Some("inner")
        );
    }

    /// A candidate that is not a subset is not a candidate, however large.
    #[test]
    fn a_candidate_that_is_not_a_subset_is_ignored() {
        let candidates = vec![(set(&[2, 3]), "unrelated"), (set(&[1]), "visible")];
        assert_eq!(
            resolve_scoped("x", &set(&[1, 2]), &candidates),
            Some("visible")
        );
    }

    /// Two bindings with the *same* scope set — a `TIE` — resolve to the
    /// first, and callers order most recent first, so a later binding
    /// shadows an earlier one. This is the case nothing in the rule decides,
    /// and the tree-walker's within-environment order changed to match the
    /// VM's when the two copies merged, so it is pinned here rather than
    /// left to whichever caller is read first.
    #[test]
    fn a_tie_goes_to_the_most_recent_candidate() {
        let candidates = vec![(set(&[1]), "recent"), (set(&[1]), "older")];
        assert_eq!(
            resolve_scoped("x", &set(&[1, 2]), &candidates),
            Some("recent")
        );
    }

    /// The empty set is a subset of everything, so an unscoped binding is a
    /// candidate — the least specific one.
    #[test]
    fn an_unscoped_candidate_loses_to_any_scoped_one() {
        let candidates = vec![(set(&[]), "unscoped"), (set(&[1]), "scoped")];
        assert_eq!(resolve_scoped("x", &set(&[1]), &candidates), Some("scoped"));
    }

    /// A macro-introduced reference reaches the binding it was written
    /// against, not a nearer one — the property Larceny triage family 39
    /// turns on. `m1` is defined inside the outer `let`, so its template's
    /// `x` carries that binder's scope plus its own expansion scope, and of
    /// the three nested binders only the outer one is a subset of it.
    ///
    /// This is decidable *because* binders accumulate: `{1} ⊂ {1,2} ⊂
    /// {1,2,3}` is a chain. Scoped at one fresh scope each, as they were,
    /// the three would be mutually unordered and the rule could not choose.
    #[test]
    fn a_template_reference_reaches_its_own_definition_sites_binder() {
        let candidates = vec![
            (set(&[1, 2, 3]), "inner"),
            (set(&[1, 2]), "middle"),
            (set(&[1]), "outer"),
        ];
        assert_eq!(
            resolve_scoped("x", &set(&[1, 9]), &candidates),
            Some("outer")
        );
    }

    /// No candidate matching means the caller falls back to its own by-name
    /// lookup, so the answer is `None` rather than a guess.
    #[test]
    fn nothing_matching_resolves_to_nothing() {
        let candidates = vec![(set(&[2]), "elsewhere")];
        assert_eq!(resolve_scoped("x", &set(&[1]), &candidates), None);
        let empty: Vec<(ScopeSet, &str)> = Vec::new();
        assert_eq!(resolve_scoped("x", &set(&[1]), &empty), None);
    }
}
