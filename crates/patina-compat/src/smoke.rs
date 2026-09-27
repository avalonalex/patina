//! Maintained assertion drivers for packages that ship no upstream suite.
//! The manifest and drivers live beside the corpus, never inside vendor/.

use crate::corpus::Package;
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
        return Status::RuntimeError;
    };
    if tallies.next().is_some() || expected == 0 {
        return Status::RuntimeError;
    }
    let Some(body) = line
        .strip_prefix("(patina-compat-smoke ")
        .and_then(|s| s.strip_suffix(')'))
    else {
        return Status::RuntimeError;
    };
    let counts: Vec<_> = body.split_whitespace().map(str::parse::<usize>).collect();
    let [Ok(passed), Ok(failed)] = counts.as_slice() else {
        return Status::RuntimeError;
    };
    if passed.checked_add(*failed) != Some(expected) {
        Status::RuntimeError
    } else if *failed > 0 {
        Status::WrongResult
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
        assert_eq!(
            completion("FAIL: example\n(patina-compat-smoke 2 1)\n", 3),
            Status::WrongResult
        );
        for stdout in [
            "",
            "(patina-compat-smoke 0 0)\n",
            "(patina-compat-smoke 2 0)\n",
            "(patina-compat-smoke 3 0)\n(patina-compat-smoke 3 0)\n",
            "(patina-compat-smoke 3",
            "(patina-compat-smoke nope 0)",
            "(patina-compat-smoke 3 0 extra)",
        ] {
            assert_eq!(completion(stdout, 3), Status::RuntimeError, "{stdout}");
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

    #[test]
    fn committed_smoke_manifest_keeps_all_six_drivers_registered() {
        let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/vendor");
        let heap = patina_core::new_shared_heap();
        let packages = crate::corpus::discover(&vendor, &heap).unwrap();
        let registered: Vec<_> = packages.iter().filter(|p| p.smoke.is_some()).collect();
        assert_eq!(registered.len(), 6);
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
