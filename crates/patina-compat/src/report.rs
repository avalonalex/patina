//! Results serialization and the report.
//!
//! Results are written as an s-expression — the one format this repository
//! is guaranteed to read forever, with the parser it already ships. The
//! report renders the headline number, the per-status breakdown, and the
//! failure histograms that constitute the L1/L2 work queue.

use crate::run::{PackageResult, Status};
use crate::sexp;
use patina_core::SharedHeap;
use std::collections::BTreeMap;
use std::fmt::Write as _;

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
        match &r.status {
            Status::MissingLibrary(libs) | Status::OutOfScope(libs) => {
                let _ = write!(out, " (missing");
                for lib in libs {
                    let _ = write!(out, " \"{}\"", sexp::escape_string(lib));
                }
                let _ = write!(out, ")");
            }
            Status::UnboundIdentifier(ids) => {
                let _ = write!(out, " (unbound");
                for id in ids {
                    let _ = write!(out, " \"{}\"", sexp::escape_string(id));
                }
                let _ = write!(out, ")");
            }
            Status::ParseError(details) => {
                let _ = write!(out, " (errors");
                for d in details {
                    let _ = write!(out, " \"{}\"", sexp::escape_string(d));
                }
                let _ = write!(out, ")");
            }
            _ => {}
        }
        out.push_str(")\n");
    }
    out.push_str(" ))\n");
    out
}

/// Parse a results snapshot back into `PackageResult`s (for `report`).
pub fn from_sexp(source: &str, heap: &SharedHeap) -> Result<Vec<PackageResult>, String> {
    let forms = sexp::parse_all(source, heap)?;
    let top = forms
        .first()
        .and_then(|f| sexp::tagged_form(*f, "patina-compat-results", heap))
        .ok_or("not a patina-compat-results file")?;

    let mut results = Vec::new();
    for section in top {
        let Some(rows) = sexp::tagged_form(section, "results", heap) else {
            continue;
        };
        for row in rows {
            let fields = sexp::list_elements(row, heap).ok_or("malformed result row")?;
            let mut slug = None;
            let mut mode = "probe";
            let mut status_key = None;
            let mut names = Vec::new();
            for field in fields {
                if let Some(rest) = sexp::tagged_form(field, "slug", heap) {
                    slug = rest.first().and_then(|tv| sexp::string_value(*tv, heap));
                } else if let Some(rest) = sexp::tagged_form(field, "mode", heap) {
                    if rest.first().and_then(|tv| sexp::symbol_name(*tv, heap))
                        == Some("test".to_string())
                    {
                        mode = "test";
                    }
                } else if let Some(rest) = sexp::tagged_form(field, "status", heap) {
                    status_key = rest.first().and_then(|tv| sexp::symbol_name(*tv, heap));
                } else if let Some(rest) = sexp::tagged_form(field, "missing", heap)
                    .or_else(|| sexp::tagged_form(field, "unbound", heap))
                    .or_else(|| sexp::tagged_form(field, "errors", heap))
                {
                    names = rest
                        .iter()
                        .filter_map(|tv| sexp::string_value(*tv, heap))
                        .collect();
                }
            }
            let slug = slug.ok_or("result row without slug")?;
            let status = match status_key.as_deref() {
                Some("pass") => Status::Pass,
                Some("missing-library") => Status::MissingLibrary(names),
                Some("parse-error") => Status::ParseError(names),
                Some("unbound-identifier") => Status::UnboundIdentifier(names),
                Some("wrong-result") => Status::WrongResult,
                Some("runtime-error") => Status::RuntimeError,
                Some("timeout") => Status::Timeout,
                Some("out-of-scope") => Status::OutOfScope(names),
                other => return Err(format!("unknown status {:?}", other)),
            };
            results.push(PackageResult { slug, mode, status });
        }
    }
    Ok(results)
}

/// Render the markdown report.
pub fn render(results: &[PackageResult], backend: &str) -> String {
    let total = results.len();
    let count = |key: &str| results.iter().filter(|r| r.status.key() == key).count();
    let pass = count("pass");
    let out_of_scope = count("out-of-scope");
    let achievable = total - out_of_scope;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Patina third-party compatibility ({} backend)\n",
        backend
    );
    let _ = writeln!(
        out,
        "**{} of {} packages pass** — {} of {} achievable (excluding {} out-of-scope pending FFI).\n",
        pass, total, pass, achievable, out_of_scope
    );

    out.push_str("| Status | Packages |\n|---|---|\n");
    for key in [
        "pass",
        "missing-library",
        "parse-error",
        "unbound-identifier",
        "wrong-result",
        "runtime-error",
        "timeout",
        "out-of-scope",
    ] {
        let _ = writeln!(out, "| {} | {} |", key, count(key));
    }

    let missing = histogram(results, |s| match s {
        Status::MissingLibrary(libs) => Some(libs),
        _ => None,
    });
    if !missing.is_empty() {
        out.push_str("\n## Missing libraries — the bundling work queue\n\n");
        out.push_str("| Library | Packages blocked |\n|---|---|\n");
        for (lib, n) in &missing {
            let _ = writeln!(out, "| ({}) | {} |", lib, n);
        }
    }

    let parse_errors = histogram(results, |s| match s {
        Status::ParseError(details) => Some(details),
        _ => None,
    });
    if !parse_errors.is_empty() {
        out.push_str("\n## Parse errors\n\n");
        out.push_str("| Error | Packages |\n|---|---|\n");
        for (detail, n) in &parse_errors {
            let _ = writeln!(out, "| `{}` | {} |", detail, n);
        }
    }

    let unbound = histogram(results, |s| match s {
        Status::UnboundIdentifier(ids) => Some(ids),
        _ => None,
    });
    if !unbound.is_empty() {
        out.push_str("\n## Unbound identifiers\n\n");
        out.push_str("| Identifier | Packages |\n|---|---|\n");
        for (id, n) in &unbound {
            let _ = writeln!(out, "| {} | {} |", id, n);
        }
    }

    out.push_str("\n## Per-package matrix\n\n");
    out.push_str("| Package | Mode | Status |\n|---|---|---|\n");
    for r in results {
        let _ = writeln!(out, "| {} | {} | {} |", r.slug, r.mode, r.status.key());
    }
    out
}

/// Count name occurrences across packages, most-blocked first.
fn histogram<'a, F>(results: &'a [PackageResult], select: F) -> Vec<(String, usize)>
where
    F: Fn(&'a Status) -> Option<&'a Vec<String>>,
{
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for r in results {
        if let Some(names) = select(&r.status) {
            for name in names {
                *counts.entry(name).or_insert(0) += 1;
            }
        }
    }
    let mut list: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    list
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
        ];

        let text = to_sexp(&results, "vm");
        let heap = patina_core::new_shared_heap();
        let parsed = from_sexp(&text, &heap).unwrap();

        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].slug, "a");
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
    }

    #[test]
    fn report_headline_counts_achievable() {
        let results = vec![
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
        ];
        let report = render(&results, "vm");
        assert!(report.contains("**1 of 2 packages pass**"));
        assert!(report.contains("1 of 1 achievable"));
    }
}
