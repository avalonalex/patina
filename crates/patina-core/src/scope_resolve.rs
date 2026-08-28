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
//! copy *for reads*, and it enforces Flatt's ambiguity condition itself, so
//! neither backend can answer a reference the rule does not determine.
//!
//! Two hand-rolled copies remain, both over `Environment`'s tables and
//! neither measured. `set_with_scopes` resolves a write by the same subset
//! rule since triage family 38, but inline and one environment at a time
//! rather than through this module; `has_scoped_binding` tests subset with no
//! most-specific rule at all. So a `set!` still resolves without being
//! checked for ambiguity, and no sweep says anything about one.
//!
//! Unifying the tie-break was a **behaviour change**, not a pure refactor:
//! the tree-walker now answers a within-environment tie the way the VM
//! always did. `tests::a_tie_goes_to_the_most_recent_candidate` pins it.
//!
//! [`resolve_scoped`] is the whole rule. Callers hand it every binding of the
//! name, **most recent first**, and it does the rest.

use crate::scope::ScopeSet;

/// A reference the rule does not determine.
///
/// Flatt's rule requires the winning candidate to be a superset of every
/// other; when two are not ordered by subset, neither is more specific and
/// the reference is ambiguous. Racket reports this at expansion time, and so
/// does Patina: the desugarer resolves every head and every value-position
/// name and turns this into a `DesugarError` before either backend runs. The
/// VM's renamer raises a `CompileError` and the tree-walker's runtime lookup
/// an `EvalError`, but those are backstops — they fire only where their own
/// tables disagree with the one the desugarer built.
///
/// It used to be answered by scope-set size instead, which is how Larceny
/// triage family 39 produced a correct answer for the wrong reason and
/// family 37's first attempts produced wrong ones. Nothing in the repo or in
/// any measured workload reaches it now: 249 processes, zero ambiguous
/// references. That is the claim this type exists to keep true.
#[derive(Debug, Clone)]
pub struct AmbiguousReference {
    /// The identifier that could not be resolved.
    pub name: String,
    /// The scopes the reference carried.
    pub reference: ScopeSet,
    /// The candidate the size rule would have picked.
    pub picked: ScopeSet,
    /// The candidates it does not contain — the reason it is not decisive.
    pub rivals: Vec<ScopeSet>,
}

impl std::fmt::Display for AmbiguousReference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ambiguous reference: `{}` at {} is bound at {}",
            self.name, self.reference, self.picked
        )?;
        for rival in &self.rivals {
            write!(f, " and at {rival}")?;
        }
        // Two calls rather than one continued literal: a `\` continuation is
        // fragile to reflow and this message is the whole diagnostic.
        f.write_str(", and neither binding contains the other")?;
        f.write_str(" — set-of-scopes resolution does not determine which it names")
    }
}

impl std::error::Error for AmbiguousReference {}

/// Resolve `reference` against `candidates`, which must be every binding of
/// `name` visible here, ordered **most recent first** — innermost
/// environment or frame first, and within one of those, latest binding
/// first, so that a later binding shadows an earlier one.
///
/// The rule: keep the candidates whose scope set is a subset of the
/// reference's, and answer with the one whose scope set is largest. A tie
/// goes to the first, which is the most recent. `Ok(None)` means no
/// candidate was a subset, and the caller falls back to its own by-name
/// lookup.
///
/// `Err` is Flatt's ambiguity condition failing: the largest is not a
/// superset of every other candidate, so it is not the most specific one and
/// there is no answer to give. Checked on every call, not behind a switch —
/// see [`AmbiguousReference`] and the `ambiguity` module.
pub fn resolve_scoped<T: Clone>(
    name: &str,
    reference: &ScopeSet,
    candidates: &[(ScopeSet, T)],
) -> Result<Option<T>, Box<AmbiguousReference>> {
    Ok(resolve_index(name, reference, candidates)?.map(|i| candidates[i].1.clone()))
}

/// [`resolve_scoped`], returning *which* candidate won rather than its value.
///
/// The same rule; the index is what a caller needs when it has to act on the
/// binding rather than read it — `set_with_scopes` writes through it — and
/// what `scope_trace` needs to name the winner.
///
/// Deriving the index from the value instead does not work, and shipped
/// broken once: two bindings of a name can hold the *same* value, and at
/// `phase=desugar` every binder is a placeholder, so a search by value named
/// whichever came first regardless of which the rule chose. The rule knows the
/// answer; handing it back beats re-deriving it.
pub fn resolve_index<T>(
    name: &str,
    reference: &ScopeSet,
    candidates: &[(ScopeSet, T)],
) -> Result<Option<usize>, Box<AmbiguousReference>> {
    // One pass, and it allocates nothing unless the reference is actually
    // ambiguous — which is why the check can run always rather than behind a
    // switch. Only *reporting* needs the candidate list.
    let mut best: Option<usize> = None;
    let mut matching = 0usize;
    for (index, (scopes, _)) in candidates.iter().enumerate() {
        if !is_candidate(scopes, reference) {
            continue;
        }
        matching += 1;
        match best {
            None => best = Some(index),
            // Strictly larger wins; a tie keeps the earlier, most recent one.
            Some(current) if scopes.len() > candidates[current].0.len() => best = Some(index),
            Some(_) => {}
        }
    }
    let Some(best) = best else {
        return Ok(None);
    };
    let winner = &candidates[best].0;

    // Flatt's condition: the winner must contain every other candidate.
    // A scan over sets already in hand, and skipped outright for a lone
    // candidate, which is a superset of the empty rest of the field — the
    // case nearly every variable read in a program takes, and one the
    // tree-walker takes per read rather than once at compile time.
    let decisive = matching < 2
        || candidates
            .iter()
            .enumerate()
            .filter(|(index, (scopes, _))| *index != best && is_candidate(scopes, reference))
            .all(|(_, (scopes, _))| is_candidate(scopes, winner));
    if !decisive {
        let rivals: Vec<ScopeSet> = candidates
            .iter()
            .enumerate()
            .filter(|(index, (scopes, _))| {
                *index != best && is_candidate(scopes, reference) && !is_candidate(scopes, winner)
            })
            .map(|(_, (scopes, _))| scopes.clone())
            .collect();
        ambiguity::log(name, reference, winner, &rivals);
        // Boxed: `get_with_scopes` returns this `Result` from the
        // tree-walker's per-read path, where a by-value error four times the
        // size of the success payload is paid for on every read that never
        // fails. `-D clippy::result_large_err` catches it.
        return Err(Box::new(AmbiguousReference {
            name: name.to_string(),
            reference: reference.clone(),
            picked: winner.clone(),
            rivals,
        }));
    }
    if matching > 1 {
        ambiguity::log_ties(name, reference, winner, candidates, best);
    }
    Ok(Some(best))
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

/// Leaves a trail of resolutions the rule in this module does not determine.
///
/// The ambiguous ones are *raised*, by [`resolve_scoped`] itself — this only
/// writes them down, plus a second kind the rule cannot see. The two are kept
/// apart because they are different phenomena:
///
/// - `AMBIG` — Flatt-ambiguous: the winner is not a superset of some rival,
///   so neither is more specific and no answer is justified. Raised as an
///   [`AmbiguousReference`], and logged on its way out.
/// - `TIE` — a rival with the **identical** scope set. The rule calls this
///   decisive (a set is a subset of itself) but it is not: the answer comes
///   from the caller's ordering alone. Racket cannot reach this — its binding
///   table is keyed by scope set, one binding per key — while Patina can,
///   because `Environment::insert_scoped` dedups within one environment and
///   the walk covers the whole chain. Reported only; see [`log_ties`].
///
/// One environment variable, off by default:
///
/// - `PATINA_AMBIGUITY_LOG=<file>` appends a record per distinct site.
///   **Use an absolute path.** The compat harness and the Larceny runner both
///   `cd` into a scratch directory that they then delete, so a relative path
///   writes 249 logs into directories that no longer exist.
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
/// - The fallback. [`resolve_scoped`] returns `Ok(None)` when no candidate is
///   a subset, and the caller then answers by name with no scope reasoning at
///   all. That path is neither reported nor raised.
/// - Writes. `Environment::set_with_scopes` resolves by the same subset rule
///   since triage family 38, but inline and one environment at a time rather
///   than through [`resolve_scoped`], so a `set!` is logged by neither arm and
///   an ambiguous one still picks by size instead of raising.
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
    ///
    /// The log is a diagnostic only. Whether an ambiguous reference is
    /// *refused* is not a switch: [`super::resolve_scoped`] returns an error
    /// for one either way, and the caller reports it. Refusal was briefly
    /// opt-in, from when `main` could not yet pass it — one test, triage
    /// family 39 — and once that was fixed the variable only offered a way to
    /// ask for the wrong answer.
    fn logging() -> bool {
        sink().is_some()
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

    /// Append one record, once per distinct site.
    fn emit(record: String) {
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

    /// Record a reference the rule does not determine, on its way to becoming
    /// a [`super::AmbiguousReference`].
    ///
    /// The error is what stops the program; this only leaves the trail, so a
    /// sweep over many workloads can say where such references arise without
    /// reading every failure by hand. It is how Larceny triage family 37 was
    /// traced to `(match '(a . 1) ((x . y) (list x y)))` answering `(a a)`.
    pub(super) fn log(name: &str, reference: &ScopeSet, picked: &ScopeSet, rivals: &[ScopeSet]) {
        if !logging() {
            return;
        }
        // `{:?}` quotes and escapes: Patina accepts `|a b|` as an identifier,
        // and an unescaped name could forge a field or a whole record.
        emit(format!(
            "AMBIG name={:?} ref={} picked={} rivals={}\n",
            name,
            render(reference),
            render(picked),
            rivals.iter().map(render).collect::<Vec<_>>().join("|")
        ));
    }

    /// Record two bindings with the *same* scope set, where the caller's
    /// ordering decided which won.
    ///
    /// A different phenomenon from ambiguity, and not an error. Flatt's model
    /// cannot express a tie — an identical set is a subset of itself, so the
    /// rule calls the pick decisive — and ties are common: 371 across the
    /// sweep. Not all are defects. A VM `is_simple` binding carries an empty
    /// scope set, so nested internal defines of one name tie legitimately.
    /// Refusing them would fail ordinary programs; what is worth fixing is
    /// whatever creates a duplicate that is not of that kind, which is why
    /// this is reported and never raised.
    pub(super) fn log_ties<T>(
        name: &str,
        reference: &ScopeSet,
        picked: &ScopeSet,
        candidates: &[(ScopeSet, T)],
        best: usize,
    ) {
        if !logging() {
            return;
        }
        let equal: Vec<&ScopeSet> = candidates
            .iter()
            .enumerate()
            .filter(|(index, (scopes, _))| *index != best && scopes == picked)
            .map(|(_, (scopes, _))| scopes)
            .collect();
        if equal.is_empty() {
            return;
        }
        emit(format!(
            "TIE name={:?} ref={} picked={} equal={}\n",
            name,
            render(reference),
            render(picked),
            equal
                .iter()
                .map(|s| render(s))
                .collect::<Vec<_>>()
                .join("|")
        ));
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
            resolve_scoped("x", &set(&[1, 2]), &candidates).unwrap(),
            Some("inner")
        );
    }

    /// A candidate that is not a subset is not a candidate, however large.
    #[test]
    fn a_candidate_that_is_not_a_subset_is_ignored() {
        let candidates = vec![(set(&[2, 3]), "unrelated"), (set(&[1]), "visible")];
        assert_eq!(
            resolve_scoped("x", &set(&[1, 2]), &candidates).unwrap(),
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
            resolve_scoped("x", &set(&[1, 2]), &candidates).unwrap(),
            Some("recent")
        );
    }

    /// The empty set is a subset of everything, so an unscoped binding is a
    /// candidate — the least specific one.
    #[test]
    fn an_unscoped_candidate_loses_to_any_scoped_one() {
        let candidates = vec![(set(&[]), "unscoped"), (set(&[1]), "scoped")];
        assert_eq!(
            resolve_scoped("x", &set(&[1]), &candidates).unwrap(),
            Some("scoped")
        );
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
            resolve_scoped("x", &set(&[1, 9]), &candidates).unwrap(),
            Some("outer")
        );
    }

    /// No candidate matching means the caller falls back to its own by-name
    /// lookup, so the answer is `None` rather than a guess.
    #[test]
    fn nothing_matching_resolves_to_nothing() {
        let candidates = vec![(set(&[2]), "elsewhere")];
        assert_eq!(resolve_scoped("x", &set(&[1]), &candidates).unwrap(), None);
        let empty: Vec<(ScopeSet, &str)> = Vec::new();
        assert_eq!(resolve_scoped("x", &set(&[1]), &empty).unwrap(), None);
    }

    /// Two candidates, neither containing the other: the rule does not
    /// determine this reference, so nothing is returned. Size would have
    /// picked `{1,2}` for being larger, which is the guess this replaces.
    #[test]
    fn two_unordered_candidates_are_ambiguous() {
        let candidates = vec![(set(&[1, 2]), "left"), (set(&[3]), "right")];
        let err = resolve_scoped("x", &set(&[1, 2, 3]), &candidates)
            .expect_err("neither candidate contains the other");
        assert_eq!(err.name, "x");
        assert_eq!(err.picked, set(&[1, 2]));
        assert_eq!(err.rivals, vec![set(&[3])]);
        // The message names both, so a report identifies the two binders.
        let shown = err.to_string();
        assert!(shown.contains("`x`"), "{shown}");
        assert!(
            shown.contains("is bound at {S1, S2} and at {S3}"),
            "{shown}"
        );
        assert!(
            shown.contains("neither binding contains the other"),
            "{shown}"
        );
    }

    /// A rival the winner *does* contain is not a rival. Ambiguity is about
    /// the winner failing to be a superset, not about having company: the
    /// chain `{1} ⊂ {1,2} ⊂ {1,2,3}` has three matching candidates and is
    /// decided, which is exactly what accumulating binders buys.
    #[test]
    fn a_contained_rival_is_not_ambiguity() {
        let candidates = vec![
            (set(&[1, 2, 3]), "inner"),
            (set(&[1, 2]), "middle"),
            (set(&[1]), "outer"),
        ];
        assert_eq!(
            resolve_scoped("x", &set(&[1, 2, 3]), &candidates).unwrap(),
            Some("inner")
        );
    }

    /// A non-candidate cannot make a reference ambiguous. `{4}` is unordered
    /// against the winner, but it is not visible from the reference at all,
    /// so it is not in the comparison.
    #[test]
    fn an_invisible_binding_is_not_a_rival() {
        let candidates = vec![(set(&[1, 2]), "visible"), (set(&[4]), "elsewhere")];
        assert_eq!(
            resolve_scoped("x", &set(&[1, 2]), &candidates).unwrap(),
            Some("visible")
        );
    }
}
