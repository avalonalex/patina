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
//! a VM run reported nothing however it resolved. This module is the one
//! copy, and it checks itself, so both backends are measured by construction.
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
    let matching: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, (scopes, _))| scopes.is_subset_of(reference))
        .map(|(index, _)| index)
        .collect();
    let mut best: Option<usize> = None;
    for &index in &matching {
        match best {
            None => best = Some(index),
            // Strictly larger wins; a tie keeps the earlier, most recent one.
            Some(current) if candidates[index].0.len() > candidates[current].0.len() => {
                best = Some(index)
            }
            Some(_) => {}
        }
    }
    let best = best?;
    if ambiguity::checking() {
        let scopes: Vec<&ScopeSet> = matching.iter().map(|&i| &candidates[i].0).collect();
        let winner = matching
            .iter()
            .position(|&i| i == best)
            .expect("winner matched");
        ambiguity::check(name, reference, &scopes, winner);
    }
    Some(candidates[best].1.clone())
}

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

    /// Is strict mode on (`PATINA_AMBIGUITY_STRICT`)? An ambiguous reference
    /// then panics instead of being reported.
    ///
    /// **Not the default, and that is a finding rather than a compromise.**
    /// Enforcing Flatt's rule looked free — no workload logs an ambiguous
    /// reference — until the check ran over the Rust suite, where
    /// `test_let_syntax_nested_lexical_scoping` trips it:
    ///
    /// ```scheme
    /// (let ((x 'outer))
    ///   (let-syntax ((m1 (syntax-rules () ((m1) x))))
    ///     (let ((x 'middle))
    ///       (let-syntax ((m2 (syntax-rules () ((m2) x))))
    ///         (let ((x 'inner))
    ///           (list (m1) (m2)))))))   ; R7RS: (outer middle)
    /// ```
    ///
    /// That answer is correct today, and it is *not* the scope rule that
    /// produces it: the two candidate bindings are both single scopes,
    /// neither containing the other, and the winner comes from environment
    /// nesting. So Patina's scope sets do not carry enough information to
    /// decide a distinction R7RS requires, and an extra-model rule is
    /// covering for it. Asserting would fail a passing, correct test.
    ///
    /// Larceny triage family 39 owns that; until it is fixed, strict mode is
    /// the tool for measuring a change against the rule rather than a gate.
    fn strict() -> bool {
        static STRICT: OnceLock<bool> = OnceLock::new();
        *STRICT.get_or_init(|| {
            std::env::var("PATINA_AMBIGUITY_STRICT")
                .ok()
                .is_some_and(|v| !v.is_empty() && v != "0")
        })
    }

    /// Should the check run at all? Only when something will come of it.
    pub(super) fn checking() -> bool {
        logging() || strict()
    }

    /// Render a scope set without spaces, so a record stays one field.
    fn render(scopes: &ScopeSet) -> String {
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
    /// which chain order therefore decides. `main` has 33 of them (all `ls`,
    /// in chibi-match), so asserting would fail today — they are a defect one
    /// layer down, in whatever creates the duplicate, and that is the thing
    /// to fix rather than to assert about here.
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
            return;
        }
        assert!(
            rivals.is_empty() || !strict(),
            "ambiguous reference: `{}` with scopes {} resolves to {} and to {}, \
             and neither contains the other — Flatt's rule does not determine \
             this reference, so the answer came from scope-set size alone. \
             See the `ambiguity` module and Larceny triage family 37.",
            name,
            reference,
            best,
            rivals
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(" and to ")
        );
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
            return;
        };
        // Never poison-panic: this is a diagnostic, and a `File` behind a
        // `Mutex` has no invariant a panic elsewhere can have broken.
        let mut sink = sink.lock().unwrap_or_else(|e| e.into_inner());
        if !sink.seen.insert(record.clone()) {
            return;
        }
        // One `write_all`, not `writeln!`: `O_APPEND` makes each syscall
        // atomic, never each `write!` fragment, and the corpus harness runs
        // up to eight patina processes appending to one file. Written a
        // fragment at a time, their records shred each other — measured at
        // six mangled lines per corpus sweep before this.
        let _ = sink.file.write_all(record.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::ScopeId;

    fn set(scopes: &[ScopeId]) -> ScopeSet {
        let mut s = ScopeSet::new();
        for &scope in scopes {
            s.add_scope(scope);
        }
        s
    }

    /// The largest scope set that is a subset of the reference's wins.
    #[test]
    fn the_most_specific_candidate_wins() {
        let (a, b) = (ScopeId::fresh(), ScopeId::fresh());
        let candidates = vec![(set(&[a]), "outer"), (set(&[a, b]), "inner")];
        assert_eq!(
            resolve_scoped("x", &set(&[a, b]), &candidates),
            Some("inner")
        );
    }

    /// A candidate that is not a subset is not a candidate, however large.
    #[test]
    fn a_candidate_that_is_not_a_subset_is_ignored() {
        let (a, b, c) = (ScopeId::fresh(), ScopeId::fresh(), ScopeId::fresh());
        let candidates = vec![(set(&[b, c]), "unrelated"), (set(&[a]), "visible")];
        assert_eq!(
            resolve_scoped("x", &set(&[a, b]), &candidates),
            Some("visible")
        );
    }

    /// Equal-sized sets tie, and the tie goes to the first — which callers
    /// order most-recent-first, so a later binding shadows an earlier one.
    #[test]
    fn a_tie_goes_to_the_most_recent_candidate() {
        let (a, b) = (ScopeId::fresh(), ScopeId::fresh());
        let candidates = vec![(set(&[a]), "recent"), (set(&[b]), "older")];
        assert_eq!(
            resolve_scoped("x", &set(&[a, b]), &candidates),
            Some("recent")
        );
    }

    /// The empty set is a subset of everything, so an unscoped binding is a
    /// candidate — the least specific one.
    #[test]
    fn an_unscoped_candidate_loses_to_any_scoped_one() {
        let a = ScopeId::fresh();
        let candidates = vec![(set(&[]), "unscoped"), (set(&[a]), "scoped")];
        assert_eq!(resolve_scoped("x", &set(&[a]), &candidates), Some("scoped"));
    }

    /// No candidate matching means the caller falls back to its own by-name
    /// lookup, so the answer is `None` rather than a guess.
    #[test]
    fn nothing_matching_resolves_to_nothing() {
        let (a, b) = (ScopeId::fresh(), ScopeId::fresh());
        let candidates = vec![(set(&[b]), "elsewhere")];
        assert_eq!(resolve_scoped("x", &set(&[a]), &candidates), None);
        let empty: Vec<(ScopeSet, &str)> = Vec::new();
        assert_eq!(resolve_scoped("x", &set(&[a]), &empty), None);
    }
}
