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
    for key in Status::KEYS {
        let _ = writeln!(out, "| {} | {} |", key, count(key));
    }

    histogram_section(
        &mut out,
        results,
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
        results,
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
        results,
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
        results,
        "Unbound identifiers",
        "Identifier",
        |s| match s {
            Status::UnboundIdentifier(l) => Some(l),
            _ => None,
        },
        |name| name.to_string(),
    );

    out.push_str("\n## Per-package matrix\n\n");
    out.push_str("| Package | Mode | Status |\n|---|---|---|\n");
    for r in results {
        let _ = writeln!(out, "| {} | {} | {} |", r.slug, r.mode, r.status.key());
    }
    out
}

/// Append one histogram section (skipped when empty): name occurrences
/// across packages, most-blocked first.
fn histogram_section<'a>(
    out: &mut String,
    results: &'a [PackageResult],
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
