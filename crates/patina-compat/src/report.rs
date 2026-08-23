//! Results serialization and the report.
//!
//! Results are written as an s-expression — the one format this repository
//! is guaranteed to read forever, with the parser it already ships. The
//! report renders the headline number, the per-status breakdown, and the
//! failure histograms that constitute the L1/L2 work queue.

use crate::exclusions::{Exclusion, Reason};
use crate::run::{PackageResult, Status};
use crate::sexp;
use patina_core::SharedHeap;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// The detail list a status carries, with its serialization key.
fn status_detail(status: &Status) -> Option<(&'static str, &[String])> {
    match status {
        Status::MissingLibrary(l) | Status::OutOfScope(l) => Some(("missing", l)),
        Status::UnboundIdentifier(l) => Some(("unbound", l)),
        Status::ParseError(l) | Status::LoadError(l) => Some(("errors", l)),
        _ => None,
    }
}

/// Serialize results to the s-expression snapshot format.
pub fn to_sexp(results: &[PackageResult], backend: &str) -> String {
    let mut out = String::new();
    out.push_str(";; patina-compat results — regenerate with: cargo run -p patina-compat -- run\n");
    out.push_str("(patina-compat-results\n");
    out.push_str(" (version 1)\n");
    let _ = writeln!(out, " (backend \"{}\")", sexp::escape_string(backend));
    out.push_str(" (results\n");
    for r in results {
        let _ = write!(
            out,
            "  ((slug \"{}\") (mode {}) (status {})",
            sexp::escape_string(&r.slug),
            r.mode,
            r.status.key()
        );
        if let Some((key, items)) = status_detail(&r.status) {
            let _ = write!(out, " ({}", key);
            for item in items {
                let _ = write!(out, " \"{}\"", sexp::escape_string(item));
            }
            let _ = write!(out, ")");
        }
        out.push_str(")\n");
    }
    out.push_str(" ))\n");
    out
}

/// Parse a results snapshot back into `PackageResult`s and the backend it
/// was measured on (for `report`).
pub fn from_sexp(source: &str, heap: &SharedHeap) -> Result<(Vec<PackageResult>, String), String> {
    let forms = sexp::parse_all(source, heap)?;
    let top = forms
        .first()
        .and_then(|f| sexp::tagged_form(*f, "patina-compat-results", heap))
        .ok_or("not a patina-compat-results file")?;

    let mut backend = "vm".to_string();
    let mut results = Vec::new();
    for section in top {
        if let Some(rest) = sexp::tagged_form(section, "backend", heap) {
            if let Some(name) = rest.first().and_then(|tv| sexp::string_value(*tv, heap)) {
                backend = name;
            }
            continue;
        }
        let Some(rows) = sexp::tagged_form(section, "results", heap) else {
            continue;
        };
        for row in rows {
            results.push(parse_result_row(row, heap)?);
        }
    }
    Ok((results, backend))
}

fn parse_result_row(
    row: patina_core::TaggedValue,
    heap: &SharedHeap,
) -> Result<PackageResult, String> {
    let fields = sexp::list_elements(row, heap).ok_or("malformed result row")?;
    let field = |key: &str| fields.iter().find_map(|f| sexp::tagged_form(*f, key, heap));
    let field_symbol = |key: &str| {
        field(key).and_then(|rest| rest.first().and_then(|tv| sexp::symbol_name(*tv, heap)))
    };

    let slug = field("slug")
        .and_then(|rest| rest.first().and_then(|tv| sexp::string_value(*tv, heap)))
        .ok_or("result row without slug")?;
    let mode = if field_symbol("mode").as_deref() == Some("test") {
        "test"
    } else {
        "probe"
    };
    let names: Vec<String> = ["missing", "unbound", "errors"]
        .iter()
        .find_map(|key| field(key))
        .map(|rest| {
            rest.iter()
                .filter_map(|tv| sexp::string_value(*tv, heap))
                .collect()
        })
        .unwrap_or_default();

    let status = match field_symbol("status").as_deref() {
        Some("pass") => Status::Pass,
        Some("missing-library") => Status::MissingLibrary(names),
        Some("parse-error") => Status::ParseError(names),
        Some("load-error") => Status::LoadError(names),
        Some("unbound-identifier") => Status::UnboundIdentifier(names),
        Some("wrong-result") => Status::WrongResult,
        Some("runtime-error") => Status::RuntimeError,
        Some("timeout") => Status::Timeout,
        Some("out-of-scope") => Status::OutOfScope(names),
        other => return Err(format!("unknown status {:?}", other)),
    };
    Ok(PackageResult { slug, mode, status })
}

/// Render the markdown report.
///
/// `exclusions` is the committed opt-out list. It never changes what was
/// measured — only which rows the scoped number counts and which rows reach
/// the work queues. Both numbers are printed, in that order, so the scoped
/// one can never be read without the raw one beside it.
/// `full_corpus` says whether `results` covers every package. Only then can
/// an exclusion that matched nothing be called stale rather than merely
/// out of view, so only then is it reported.
pub fn render(
    results: &[PackageResult],
    backend: &str,
    exclusions: &[Exclusion],
    full_corpus: bool,
) -> String {
    let total = results.len();
    let count = |key: &str| results.iter().filter(|r| r.status.key() == key).count();
    let pass = count("pass");

    let excluded: BTreeMap<&str, &Exclusion> =
        exclusions.iter().map(|e| (e.slug.as_str(), e)).collect();
    // Only exclusions that matched a package in *this* run are subtracted; a
    // filtered run must not shrink its own denominator by rows it never ran.
    let applied: Vec<(&PackageResult, &Exclusion)> = results
        .iter()
        .filter_map(|r| excluded.get(r.slug.as_str()).map(|e| (r, *e)))
        .collect();
    let drifted: Vec<(&PackageResult, &Exclusion)> = applied
        .iter()
        .copied()
        .filter(|(r, e)| r.status.key() != e.expect)
        .collect();
    let counted: Vec<&PackageResult> = results
        .iter()
        .filter(|r| !excluded.contains_key(r.slug.as_str()))
        .collect();
    let counted_pass = counted.iter().filter(|r| r.status.key() == "pass").count();
    let measured: BTreeMap<&str, ()> = results.iter().map(|r| (r.slug.as_str(), ())).collect();
    let unmatched: Vec<&Exclusion> = if full_corpus {
        exclusions
            .iter()
            .filter(|e| !measured.contains_key(e.slug.as_str()))
            .collect()
    } else {
        Vec::new()
    };

    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Patina third-party compatibility ({} backend)\n",
        backend
    );
    let _ = writeln!(out, "**{} of {} packages pass.**\n", pass, total);
    if !applied.is_empty() {
        let _ = writeln!(
            out,
            "**{} of {} in scope** — {} packages are excluded from the score by \
             `compat/EXCLUSIONS.scm`, each for a reason that is not a measurement of \
             Patina. The raw number above never moves because of that file.\n",
            counted_pass,
            counted.len(),
            applied.len()
        );
    }
    if !drifted.is_empty() {
        let _ = writeln!(
            out,
            "> ⚠️ **{} exclusion(s) no longer match what the package does.** See \
             *Exclusions that have drifted* below — an entry whose reason has stopped \
             being true is due for retirement, not for a new expectation.\n",
            drifted.len()
        );
    }

    if !unmatched.is_empty() {
        let _ = writeln!(
            out,
            "> ⚠️ **{} exclusion(s) name a package the corpus no longer has:** {}. \
             An entry that matches nothing excludes nothing, so it is dead weight rather \
             than a silent subtraction — delete it, or fix the slug.\n",
            unmatched.len(),
            unmatched
                .iter()
                .map(|e| format!("`{}`", e.slug))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    out.push_str("| Status | Packages | In scope |\n|---|---|---|\n");
    for key in Status::KEYS {
        let in_scope = counted.iter().filter(|r| r.status.key() == key).count();
        let _ = writeln!(out, "| {} | {} | {} |", key, count(key), in_scope);
    }

    // The histograms are work queues, so they read the in-scope packages
    // only. A missing library named solely by an excluded package is not
    // something bundling can fix — `(srfi 160 base)` and `(chibi io)` each
    // sat in this table with one requester, and both requesters are C-shim
    // packages that would still fail with the library in hand.
    histogram_section(
        &mut out,
        &counted,
        "Missing libraries — the bundling work queue",
        "Library",
        |s| match s {
            Status::MissingLibrary(l) => Some(l),
            _ => None,
        },
        |name| format!("({})", name),
    );
    histogram_section(
        &mut out,
        &counted,
        "Parse errors",
        "Error",
        |s| match s {
            Status::ParseError(l) => Some(l),
            _ => None,
        },
        |name| format!("`{}`", name),
    );
    histogram_section(
        &mut out,
        &counted,
        "Load errors",
        "Error",
        |s| match s {
            Status::LoadError(l) => Some(l),
            _ => None,
        },
        |name| format!("`{}`", name),
    );
    histogram_section(
        &mut out,
        &counted,
        "Unbound identifiers",
        "Identifier",
        |s| match s {
            Status::UnboundIdentifier(l) => Some(l),
            _ => None,
        },
        |name| name.to_string(),
    );

    if !drifted.is_empty() {
        out.push_str("\n## Exclusions that have drifted\n\n");
        out.push_str(
            "Each row was excluded on the understanding that it produces the status in \
             *Expected*. It does not any more, so the recorded reason needs re-checking \
             — and if the package now passes, the entry should go.\n\n",
        );
        out.push_str("| Package | Expected | Actual | Reason | Note |\n|---|---|---|---|---|\n");
        for (r, e) in &drifted {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                r.slug,
                e.expect,
                r.status.key(),
                e.reason.key(),
                e.note
            );
        }
    }

    if !applied.is_empty() {
        out.push_str("\n## Excluded from the score\n\n");
        out.push_str(
            "These packages still run on every pass — exclusion decides whether a result \
             counts, never whether it is measured, and `results.scm` records them exactly \
             as it records everything else.\n",
        );
        for reason in Reason::ALL {
            let rows: Vec<&(&PackageResult, &Exclusion)> =
                applied.iter().filter(|(_, e)| e.reason == reason).collect();
            if rows.is_empty() {
                continue;
            }
            let _ = writeln!(out, "\n### {} ({})\n", reason.heading(), rows.len());
            out.push_str("| Package | Status | Why |\n|---|---|---|\n");
            for (r, e) in rows {
                let _ = writeln!(out, "| {} | {} | {} |", r.slug, r.status.key(), e.note);
            }
        }
    }

    out.push_str("\n## Per-package matrix\n\n");
    out.push_str("| Package | Mode | Status | Scope |\n|---|---|---|---|\n");
    for r in results {
        let scope = match excluded.get(r.slug.as_str()) {
            Some(e) => e.reason.key(),
            None => "in scope",
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} |",
            r.slug,
            r.mode,
            r.status.key(),
            scope
        );
    }
    out
}

/// Append one histogram section (skipped when empty): name occurrences
/// across packages, most-blocked first.
fn histogram_section<'a>(
    out: &mut String,
    results: &[&'a PackageResult],
    title: &str,
    column: &str,
    select: impl Fn(&'a Status) -> Option<&'a Vec<String>>,
    decorate: impl Fn(&str) -> String,
) {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for r in results {
        if let Some(names) = select(&r.status) {
            for name in names {
                *counts.entry(name).or_insert(0) += 1;
            }
        }
    }
    if counts.is_empty() {
        return;
    }
    let mut rows: Vec<(&str, usize)> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));

    let _ = writeln!(out, "\n## {}\n", title);
    let _ = writeln!(out, "| {} | Packages |\n|---|---|", column);
    for (name, n) in rows {
        let _ = writeln!(out, "| {} | {} |", decorate(name), n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_round_trip_through_sexp() {
        let results = vec![
            PackageResult {
                slug: "a".into(),
                mode: "test",
                status: Status::Pass,
            },
            PackageResult {
                slug: "b".into(),
                mode: "probe",
                status: Status::MissingLibrary(vec!["srfi 19".into()]),
            },
            PackageResult {
                slug: "c".into(),
                mode: "probe",
                status: Status::UnboundIdentifier(vec!["string-index".into()]),
            },
            PackageResult {
                slug: "d".into(),
                mode: "probe",
                status: Status::OutOfScope(vec!["chibi ast".into()]),
            },
            PackageResult {
                slug: "e".into(),
                mode: "probe",
                status: Status::LoadError(vec!["Exported identifier 'f' not defined".into()]),
            },
        ];

        let text = to_sexp(&results, "tree-walker");
        let heap = patina_core::new_shared_heap();
        let (parsed, backend) = from_sexp(&text, &heap).unwrap();

        assert_eq!(backend, "tree-walker");
        assert_eq!(parsed.len(), 5);
        assert_eq!(parsed[0].slug, "a");
        assert_eq!(parsed[0].mode, "test");
        assert_eq!(parsed[0].status, Status::Pass);
        assert_eq!(
            parsed[1].status,
            Status::MissingLibrary(vec!["srfi 19".into()])
        );
        assert_eq!(
            parsed[2].status,
            Status::UnboundIdentifier(vec!["string-index".into()])
        );
        assert_eq!(
            parsed[3].status,
            Status::OutOfScope(vec!["chibi ast".into()])
        );
        assert_eq!(
            parsed[4].status,
            Status::LoadError(vec!["Exported identifier 'f' not defined".into()])
        );
    }

    fn two_results() -> Vec<PackageResult> {
        vec![
            PackageResult {
                slug: "a".into(),
                mode: "test",
                status: Status::Pass,
            },
            PackageResult {
                slug: "b".into(),
                mode: "probe",
                status: Status::OutOfScope(vec!["chibi ast".into()]),
            },
        ]
    }

    fn excluding(slug: &str, expect: &str) -> Vec<Exclusion> {
        vec![Exclusion {
            slug: slug.into(),
            reason: Reason::Ffi,
            expect: expect.into(),
            note: "needs libfoo".into(),
        }]
    }

    /// The raw number is the measurement and does not move; the scoped one
    /// appears beside it only when something was actually excluded.
    #[test]
    fn report_headline_keeps_the_raw_number() {
        let report = render(&two_results(), "vm", &[], true);
        assert!(report.contains("**1 of 2 packages pass.**"), "{}", report);
        assert!(!report.contains("in scope**"), "{}", report);
    }

    #[test]
    fn an_exclusion_narrows_only_the_scoped_number() {
        let report = render(&two_results(), "vm", &excluding("b", "out-of-scope"), true);
        assert!(report.contains("**1 of 2 packages pass.**"), "{}", report);
        assert!(report.contains("**1 of 1 in scope**"), "{}", report);
        assert!(
            report.contains("Needs a foreign-function interface (1)"),
            "{}",
            report
        );
    }

    /// The point of `expect`: an entry that no longer describes the package
    /// is reported, not silently applied.
    #[test]
    fn an_exclusion_that_no_longer_matches_is_reported_as_drift() {
        let report = render(&two_results(), "vm", &excluding("a", "out-of-scope"), true);
        assert!(
            report.contains("Exclusions that have drifted"),
            "{}",
            report
        );
        assert!(report.contains("no longer match"), "{}", report);
    }

    /// A work queue must not list a library only an excluded package wants:
    /// bundling it would not move that package.
    #[test]
    fn the_bundling_queue_ignores_excluded_packages() {
        let results = vec![PackageResult {
            slug: "b".into(),
            mode: "probe",
            status: Status::MissingLibrary(vec!["srfi 160 base".into()]),
        }];
        let queued = render(&results, "vm", &[], true);
        assert!(queued.contains("(srfi 160 base)"), "{}", queued);
        let dropped = render(&results, "vm", &excluding("b", "missing-library"), true);
        assert!(!dropped.contains("| (srfi 160 base) |"), "{}", dropped);
    }

    /// A full run can tell a stale entry from an unrun one, and says so:
    /// an entry matching nothing subtracts nothing, and reads as if it did.
    #[test]
    fn a_full_run_reports_an_exclusion_that_matches_nothing() {
        let report = render(
            &two_results(),
            "vm",
            &excluding("gone-from-corpus", "pass"),
            true,
        );
        assert!(report.contains("no longer has"), "{}", report);
        assert!(report.contains("gone-from-corpus"), "{}", report);
    }

    /// An exclusion for a package this run did not measure must not shrink
    /// the denominator — otherwise a filtered run reports a better ratio than
    /// it earned.
    #[test]
    fn an_exclusion_for_an_unrun_package_changes_nothing() {
        let report = render(
            &two_results(),
            "vm",
            &excluding("not-in-this-run", "pass"),
            false,
        );
        assert!(report.contains("**1 of 2 packages pass.**"), "{}", report);
        assert!(!report.contains("in scope**"), "{}", report);
    }
}
