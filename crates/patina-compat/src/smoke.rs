//! Maintained assertion drivers for packages that ship no upstream suite.
//! The manifest and drivers live beside the corpus, never inside vendor/.

use crate::corpus::Package;
use crate::evidence;
use crate::run::Status;
use crate::sexp;
use patina_core::SharedHeap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Smoke {
    pub script: PathBuf,
    pub assertions: usize,
    pub source: String,
}

/// A custom corpus may have no smoke directory. Once it has one, malformed
/// metadata, stale registrations and missing drivers are configuration errors,
/// not reasons to silently fall back to an import probe.
pub fn attach(packages: &mut [Package], vendor: &Path, heap: &SharedHeap) -> Result<(), String> {
    let root = vendor.join("../smoke");
    if !root.exists() {
        return Ok(());
    }
    let manifest = root.join("manifest.scm");
    let source =
        std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let (sections, rows) = sexp::document_rows(&source, "patina-compat-smokes", "tests", heap)?;
    if sections
        .iter()
        .filter(|s| sexp::tagged_form(**s, "tests", heap).is_some())
        .count()
        != 1
    {
        return Err(format!(
            "{}: expected one tests section",
            manifest.display()
        ));
    }
    let mut seen = BTreeSet::new();
    for row in rows {
        let fields = sexp::list_elements(row, heap).ok_or("malformed smoke row")?;
        let slug = sexp::row_string(&fields, "slug", heap).ok_or("smoke row without slug")?;
        if !seen.insert(slug.clone()) {
            return Err(format!("duplicate smoke registration: {slug}"));
        }
        // Resolve the slug before using it as a file name. Package slugs are
        // directory names from discovery, not arbitrary paths from metadata.
        let package = packages
            .iter_mut()
            .find(|p| p.slug == slug)
            .ok_or_else(|| format!("smoke registration names absent package: {slug}"))?;
        let assertions = sexp::row_field(&fields, "assertions", heap)
            .and_then(|v| v.as_fixnum())
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| format!("{slug}: smoke assertions must be a positive integer"))?;
        let script = root
            .join(format!("{slug}.scm"))
            .canonicalize()
            .map_err(|e| format!("{slug}: missing smoke driver: {e}"))?;
        let source =
            std::fs::read_to_string(&script).map_err(|e| format!("{}: {e}", script.display()))?;
        // Authored drivers must parse; a missing dependency scan must not
        // quietly measure a different program from the registered one.
        let forms =
            sexp::parse_all(&source, heap).map_err(|e| format!("{}: {e}", script.display()))?;
        if package.test_script.is_none() {
            for form in forms {
                crate::corpus::collect_imports(form, &mut package.test_depends, heap);
            }
            package.test_depends.sort();
            package.test_depends.dedup();
            package.smoke = Some(Smoke {
                script,
                assertions,
                source,
            });
        }
    }
    Ok(())
}

/// Import every provided library, preserving the probe's loading coverage.
/// Prefixes keep these checks from changing a driver's own imported bindings.
/// Keep the driver's forms at top level: imports must be installed before
/// subsequent forms are expanded. Its helper library has an explicit root;
/// running from scratch must not rely on the script's directory or cwd.
pub fn source(package: &Package, smoke: &Smoke) -> String {
    let mut source = String::from("(import (scheme base)");
    for (i, library) in package.provides.iter().enumerate() {
        source.push_str(&format!(" (prefix ({library}) smoke-import-{i}:)"));
    }
    source.push_str(")\n");
    source.push_str(&smoke.source);
    source
}

/// Require one complete tally, the registered positive assertion count, and
/// no failed assertions. Exit status and interpreter errors are checked by
/// the ordinary classifier first. This is our protocol, not parsed prose
/// from a third-party test framework.
pub fn completion(stdout: &str, expected: usize) -> Status {
    let mut tallies = stdout
        .lines()
        .filter(|line| line.starts_with("(patina-compat-smoke"));
    let Some(line) = tallies.next() else {
        return Status::RuntimeError(evidence::bounded(&format!(
            "Missing smoke completion tally; expected {expected} assertions"
        )));
    };
    if let Some(duplicate) = tallies.next() {
        return Status::RuntimeError(evidence::bounded(&format!(
            "Duplicate smoke completion tallies; expected exactly one\n{line}\n{duplicate}"
        )));
    }
    if expected == 0 {
        return Status::RuntimeError(evidence::bounded(
            "Expected smoke assertion count must be positive",
        ));
    }
    let Some(body) = line
        .strip_prefix("(patina-compat-smoke ")
        .and_then(|s| s.strip_suffix(')'))
    else {
        return Status::RuntimeError(evidence::bounded(&format!(
            "Malformed smoke completion tally: {line}"
        )));
    };
    let counts: Vec<_> = body.split_whitespace().map(str::parse::<usize>).collect();
    let [Ok(passed), Ok(failed)] = counts.as_slice() else {
        return Status::RuntimeError(evidence::bounded(&format!(
            "Invalid smoke completion counts: {line}"
        )));
    };
    if passed.checked_add(*failed) != Some(expected) {
        Status::RuntimeError(evidence::bounded(&format!(
            "Smoke assertion count mismatch: expected {expected}, got {passed} passed and {failed} failed"
        )))
    } else if *failed > 0 {
        let mut details = evidence::Evidence::default();
        details.push(&format!(
            "Smoke tally: {passed} passed, {failed} failed, {expected} expected"
        ));
        for line in crate::run::smoke_failure_evidence(stdout) {
            details.push(&line);
        }
        Status::WrongResult(details.finish())
    } else {
        Status::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_rejects_empty_truncated_duplicate_and_failed_runs() {
        assert_eq!(completion("(patina-compat-smoke 3 0)\n", 3), Status::Pass);
        let Status::WrongResult(lines) =
            completion("FAIL: example\n(patina-compat-smoke 2 1)\n", 3)
        else {
            panic!("failed smoke was not a wrong result");
        };
        assert!(lines.iter().any(|line| line == "FAIL: example"));
        assert!(
            lines
                .iter()
                .any(|line| line == "Smoke tally: 2 passed, 1 failed, 3 expected")
        );
        for (stdout, reason) in [
            ("", "Missing smoke completion tally"),
            (
                "(patina-compat-smoke 0 0)\n",
                "Smoke assertion count mismatch",
            ),
            (
                "(patina-compat-smoke 2 0)\n",
                "Smoke assertion count mismatch",
            ),
            (
                "(patina-compat-smoke 3 0)\n(patina-compat-smoke 3 0)\n",
                "Duplicate smoke completion tallies",
            ),
            ("(patina-compat-smoke 3", "Malformed smoke completion tally"),
            (
                "(patina-compat-smoke nope 0)",
                "Invalid smoke completion counts",
            ),
            (
                "(patina-compat-smoke 3 0 extra)",
                "Invalid smoke completion counts",
            ),
        ] {
            let Status::RuntimeError(lines) = completion(stdout, 3) else {
                panic!("{stdout}");
            };
            assert!(lines[0].starts_with(reason), "{lines:?}");
        }
    }

    #[test]
    fn manifest_validates_drivers_and_prefers_an_upstream_suite() {
        let temp = tempfile::tempdir().unwrap();
        let vendor = temp.path().join("vendor");
        let package = vendor.join("example");
        let smoke = temp.path().join("smoke");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::create_dir(&smoke).unwrap();
        std::fs::write(package.join("package.scm"), "(package)").unwrap();
        let heap = patina_core::new_shared_heap();
        let manifest = smoke.join("manifest.scm");
        std::fs::write(
            &manifest,
            "(patina-compat-smokes (tests ((slug \"example\") (assertions 1))))",
        )
        .unwrap();
        assert!(
            crate::corpus::discover(&vendor, &heap)
                .unwrap_err()
                .contains("missing smoke driver")
        );
        std::fs::write(
            smoke.join("example.scm"),
            "(import (only (sample dependency) f))",
        )
        .unwrap();
        let packages = crate::corpus::discover(&vendor, &heap).unwrap();
        assert_eq!(packages[0].test_depends, ["sample dependency"]);
        assert_eq!(packages[0].smoke.as_ref().unwrap().assertions, 1);
        for row in [
            "((slug \"example\") (assertions 0))",
            "((slug \"absent\") (assertions 1))",
            "((slug \"example\") (assertions 1)) ((slug \"example\") (assertions 1))",
        ] {
            std::fs::write(&manifest, format!("(patina-compat-smokes (tests {row}))")).unwrap();
            assert!(crate::corpus::discover(&vendor, &heap).is_err());
        }
        std::fs::write(
            &manifest,
            "(patina-compat-smokes (tests ((slug \"example\") (assertions 1))))",
        )
        .unwrap();
        std::fs::write(package.join("package.scm"), "(package (test \"test.scm\"))").unwrap();
        std::fs::write(package.join("test.scm"), "(import (scheme base))").unwrap();
        let packages = crate::corpus::discover(&vendor, &heap).unwrap();
        assert!(packages[0].test_script.is_some());
        assert!(packages[0].smoke.is_none());
        assert_eq!(packages[0].test_depends, ["scheme base"]);
    }

    /// #429 is complete only while every in-scope package executes assertions.
    /// This guards additions as well as deletion of a driver AND its manifest row,
    /// which the registration-consistency check alone cannot detect.
    #[test]
    fn every_in_scope_package_has_execution_coverage() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat");
        let heap = patina_core::new_shared_heap();
        let packages = crate::corpus::discover(&root.join("vendor"), &heap).unwrap();
        let exclusions = crate::exclusions::load(&root.join("EXCLUSIONS.scm"), &heap).unwrap();
        let probes: Vec<_> = packages
            .iter()
            .filter(|p| p.test_script.is_none() && p.smoke.is_none())
            .filter(|p| !exclusions.iter().any(|e| e.slug == p.slug))
            .map(|p| &p.slug)
            .collect();
        assert!(
            probes.is_empty(),
            "in-scope packages without assertions: {probes:?}"
        );
    }

    #[test]
    fn committed_smoke_manifest_keeps_all_drivers_registered() {
        let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/vendor");
        let heap = patina_core::new_shared_heap();
        let packages = crate::corpus::discover(&vendor, &heap).unwrap();
        let registered: Vec<_> = packages.iter().filter(|p| p.smoke.is_some()).collect();
        let mut drivers: Vec<_> = std::fs::read_dir(vendor.join("../smoke"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "scm")
                    && path.file_stem().unwrap() != "manifest"
            })
            .map(|path| path.file_stem().unwrap().to_str().unwrap().to_owned())
            .collect();
        drivers.sort();
        assert!(!drivers.is_empty());
        assert_eq!(
            registered.iter().map(|p| &p.slug).collect::<Vec<_>>(),
            drivers.iter().collect::<Vec<_>>()
        );
        for package in registered {
            let smoke = package.smoke.as_ref().unwrap();
            let generated = source(package, smoke);
            for library in &package.provides {
                assert!(generated.contains(&format!("(prefix ({library}) smoke-import-")));
            }
            assert!(generated.ends_with(&smoke.source));
        }
    }
}
