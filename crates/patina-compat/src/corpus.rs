//! Corpus discovery: walk `compat/vendor/` and parse each package's
//! `package.scm` (Snow metadata, an s-expression) into what the runner
//! needs — provided library names, dependency names, and the test entry.

use crate::sexp;
use patina_core::SharedHeap;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One vendored package.
#[derive(Debug)]
pub struct Package {
    pub slug: String,
    pub root: PathBuf,
    /// Library names this package defines, e.g. `"chibi match"`.
    pub provides: Vec<String>,
    /// Library names its libraries/programs import (all cond-expand branches
    /// pooled — an over-approximation is harmless, it only widens `-A`).
    pub depends: Vec<String>,
    /// The package's own test program, when it ships one.
    pub test_script: Option<PathBuf>,
}

/// Every package under `vendor`, sorted by slug.
pub fn discover(vendor: &Path, heap: &SharedHeap) -> Result<Vec<Package>, String> {
    let mut packages = Vec::new();
    let entries = std::fs::read_dir(vendor)
        .map_err(|e| format!("cannot read {}: {}", vendor.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let root = entry.path();
        if !root.is_dir() {
            continue;
        }
        let package_scm = root.join("package.scm");
        if !package_scm.is_file() {
            continue;
        }
        let slug = entry.file_name().to_string_lossy().into_owned();
        packages.push(parse_package(&slug, &root, &package_scm, heap)?);
    }
    packages.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(packages)
}

/// Map every provided library name to the index of its providing package.
/// First provider (by slug order) wins on the rare duplicate.
pub fn providers(packages: &[Package]) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    for (i, p) in packages.iter().enumerate() {
        for lib in &p.provides {
            map.entry(lib.clone()).or_insert(i);
        }
    }
    map
}

fn parse_package(
    slug: &str,
    root: &Path,
    package_scm: &Path,
    heap: &SharedHeap,
) -> Result<Package, String> {
    let source = std::fs::read_to_string(package_scm)
        .map_err(|e| format!("{}: {}", package_scm.display(), e))?;
    let forms = sexp::parse_all(&source, heap)
        .map_err(|e| format!("{}: parse error: {}", package_scm.display(), e))?;

    let mut provides = Vec::new();
    let mut depends = Vec::new();
    let mut test_script = None;

    for form in forms {
        let Some(decls) = sexp::tagged_form(form, "package", heap) else {
            continue;
        };
        for decl in decls {
            if let Some(body) = sexp::tagged_form(decl, "library", heap)
                .or_else(|| sexp::tagged_form(decl, "program", heap))
            {
                collect_component(&body, &mut provides, &mut depends, heap);
            } else if let Some(rest) = sexp::tagged_form(decl, "test", heap)
                && let Some(name) = rest.first().and_then(|tv| sexp::string_value(*tv, heap))
            {
                let path = root.join(&name);
                if path.is_file() {
                    test_script = Some(path);
                }
            }
        }
    }

    depends.sort();
    depends.dedup();
    Ok(Package {
        slug: slug.to_string(),
        root: root.to_path_buf(),
        provides,
        depends,
        test_script,
    })
}

/// Pull `(name ...)` and every `(depends ...)` — including those inside
/// `cond-expand` branches — out of a `library`/`program` component body.
fn collect_component(
    body: &[patina_core::TaggedValue],
    provides: &mut Vec<String>,
    depends: &mut Vec<String>,
    heap: &SharedHeap,
) {
    for decl in body {
        if let Some(rest) = sexp::tagged_form(*decl, "name", heap) {
            if let Some(name) = rest.first().and_then(|tv| sexp::library_name(*tv, heap)) {
                provides.push(name);
            }
        } else if let Some(rest) = sexp::tagged_form(*decl, "depends", heap) {
            for dep in rest {
                if let Some(name) = sexp::library_name(dep, heap) {
                    depends.push(name);
                }
            }
        } else if let Some(clauses) = sexp::tagged_form(*decl, "cond-expand", heap) {
            // Each clause is (condition decl ...); pool depends from every
            // branch rather than evaluating conditions. Snow cond-expand
            // branches only carry (depends ...), so passing `provides`
            // through is harmless and keeps one signature.
            for clause in clauses {
                if let Some(elems) = sexp::list_elements(clause, heap)
                    && elems.len() > 1
                {
                    collect_component(&elems[1..], provides, depends, heap);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_snow_package_scm() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            temp.path().join("package.scm"),
            r#"
            (package
              (version "0.9.1")
              (license public-domain)
              (library
                (name (chibi match))
                (path "chibi/match.sld")
                (cond-expand
                  (chibi (depends (chibi)))
                  (else (depends (scheme base))))
                (depends))
              (library
                (name (chibi match-test))
                (path "chibi/match-test.sld")
                (depends (scheme base) (chibi match) (chibi test))
                (use-for test))
              (test "run-tests.scm"))
        "#,
        )
        .unwrap();
        std::fs::write(temp.path().join("run-tests.scm"), "(display 1)").unwrap();

        let heap = patina_core::new_shared_heap();
        let pkg = parse_package(
            "chibi-match",
            temp.path(),
            &temp.path().join("package.scm"),
            &heap,
        )
        .unwrap();

        assert_eq!(pkg.provides, vec!["chibi match", "chibi match-test"]);
        assert!(pkg.depends.contains(&"scheme base".to_string()));
        assert!(pkg.depends.contains(&"chibi test".to_string()));
        assert!(pkg.depends.contains(&"chibi".to_string()));
        assert!(pkg.test_script.is_some());
    }

    #[test]
    fn integer_library_names_normalize() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            temp.path().join("package.scm"),
            r#"(package (library (name (srfi 1)) (depends (srfi 8))))"#,
        )
        .unwrap();

        let heap = patina_core::new_shared_heap();
        let pkg = parse_package(
            "srfi-1",
            temp.path(),
            &temp.path().join("package.scm"),
            &heap,
        )
        .unwrap();
        assert_eq!(pkg.provides, vec!["srfi 1"]);
        assert_eq!(pkg.depends, vec!["srfi 8"]);
        assert!(pkg.test_script.is_none());
    }
}
