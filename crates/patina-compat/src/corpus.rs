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
    /// Library names only the package's *test* program imports: the
    /// package-level `(test-depends ...)` clause, plus whatever the test
    /// script's own `(import ...)` forms name — upstream metadata routinely
    /// omits the clause (srfi-235 imports `(srfi 64)` and declares nothing),
    /// and a missed test dependency misfiles the package as
    /// `missing-library` naming a library the corpus provides. Held apart
    /// from `depends` because they are not part of what an importer of this
    /// package needs: they widen `-A` for this package's own test run and
    /// are never followed transitively.
    pub test_depends: Vec<String>,
    /// The package's own test program, when it ships one.
    pub test_script: Option<PathBuf>,
    /// Libraries whose `.sld` does not sit where its name says it should.
    pub off_path_libraries: Vec<OffPathLibrary>,
}

/// A library whose declared `(path ...)` is not the one its `(name ...)`
/// implies — chibi-irregex ships `(chibi irregex)` as a bare `irregex.sld`.
///
/// Patina resolves libraries by name, so such a package cannot find its own
/// library from its own root and is misfiled as `missing-library` naming the
/// library it is itself providing. Snow's installer has the same split — it
/// *reads* the declared path and *writes* the name-derived one
/// (`default-installer`, chibi's `lib/chibi/snow/commands.scm`) — so the
/// runner stages the same layout before running the package.
#[derive(Debug)]
pub struct OffPathLibrary {
    /// Library name, e.g. `"chibi irregex"`.
    pub name: String,
    /// The `.sld` where the package actually keeps it.
    pub source: PathBuf,
}

/// Where a search root must hold library `(a b c)`: `a/b/c.sld`.
///
/// Must agree with the resolver under test — `LibraryRegistry::find_library_file`
/// in `patina-runtime` — or staging writes to a path nothing reads. The rule
/// is not shared as code because the harness deliberately depends on the
/// patina *binary* rather than on runtime internals.
pub fn name_relative_path(name: &str) -> PathBuf {
    let mut path: PathBuf = name.split(' ').collect();
    path.as_mut_os_string().push(".sld");
    path
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
    let mut test_depends = Vec::new();
    let mut test_script = None;
    let mut off_path_libraries = Vec::new();

    for form in forms {
        let Some(decls) = sexp::tagged_form(form, "package", heap) else {
            continue;
        };
        for decl in decls {
            if let Some(body) = sexp::tagged_form(decl, "library", heap) {
                collect_component(&body, &mut provides, &mut depends, heap);
                // Only libraries: a program is run by path, never imported by
                // name, so where its file sits cannot mislead a lookup.
                if let Some(lib) = off_path_library(&body, root, heap) {
                    off_path_libraries.push(lib);
                }
            } else if let Some(body) = sexp::tagged_form(decl, "program", heap) {
                collect_component(&body, &mut provides, &mut depends, heap);
            } else if let Some(rest) = sexp::tagged_form(decl, "test-depends", heap) {
                for dep in rest {
                    if let Some(name) = sexp::library_name(dep, heap) {
                        test_depends.push(name);
                    }
                }
            } else if let Some(name) = sexp::clause_argument(decl, "test", heap)
                && let Some(name) = sexp::string_value(name, heap)
            {
                let path = root.join(&name);
                if path.is_file() {
                    test_script = Some(path);
                } else {
                    // A package that declares a test and does not ship it
                    // silently becomes a probe-mode row — which still passes,
                    // and which nothing else would notice. A typo here would
                    // drop a suite and improve the score (audit E2).
                    eprintln!(
                        "warning: {slug}: package.scm names test \"{name}\",                          which is not there — running it as a probe instead"
                    );
                }
            }
        }
    }

    if let Some(script) = &test_script {
        test_depends.extend(test_script_imports(script, heap));
    }

    depends.sort();
    depends.dedup();
    test_depends.sort();
    test_depends.dedup();
    Ok(Package {
        slug: slug.to_string(),
        root: root.to_path_buf(),
        provides,
        depends,
        test_depends,
        test_script,
        off_path_libraries,
    })
}

/// Library names the package's test script imports, pooling every
/// `cond-expand` branch — the same over-approximation `collect_component`
/// applies to `(depends ...)`, and harmless for the same reason: an
/// unresolvable name only widens `-A` by nothing.
///
/// A script the parser cannot read is scanned as empty, with a warning
/// rather than silence: the metadata-only closure this falls back to is
/// exactly the state that misfiled srfi-235, so the degradation should be
/// visible when it happens.
fn test_script_imports(script: &Path, heap: &SharedHeap) -> Vec<String> {
    let source = match std::fs::read_to_string(script) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("warning: {}: not read: {}", script.display(), e);
            return Vec::new();
        }
    };
    let forms = match sexp::parse_all(&source, heap) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "warning: {}: not scanned for test imports: {}",
                script.display(),
                e
            );
            return Vec::new();
        }
    };
    let mut found = Vec::new();
    for form in forms {
        collect_imports(form, &mut found, heap);
    }
    found
}

/// Collect the library names of every `(import ...)` in `form`, descending
/// into `cond-expand` clause bodies (conditions ignored) and `begin`.
fn collect_imports(form: patina_core::TaggedValue, found: &mut Vec<String>, heap: &SharedHeap) {
    if let Some(specs) = sexp::tagged_form(form, "import", heap) {
        for spec in specs {
            if let Some(name) = import_spec_library(spec, heap) {
                found.push(name);
            }
        }
    } else if let Some(clauses) = sexp::tagged_form(form, "cond-expand", heap) {
        for clause in clauses {
            if let Some(elems) = sexp::list_elements(clause, heap)
                && elems.len() > 1
            {
                for inner in &elems[1..] {
                    collect_imports(*inner, found, heap);
                }
            }
        }
    } else if let Some(body) = sexp::tagged_form(form, "begin", heap) {
        for inner in body {
            collect_imports(inner, found, heap);
        }
    }
}

/// The library a possibly-wrapped import spec names: R7RS's four modifiers
/// unwrap to the spec in their first position — nesting included, as in
/// `(rename (except (chibi test) test-equal) (test test-equal))` — and
/// anything else is read as a library name.
fn import_spec_library(spec: patina_core::TaggedValue, heap: &SharedHeap) -> Option<String> {
    for wrapper in ["only", "except", "prefix", "rename"] {
        if let Some(inner) = sexp::clause_argument(spec, wrapper, heap) {
            return import_spec_library(inner, heap);
        }
    }
    sexp::library_name(spec, heap)
}

/// The `.sld` of a `library` component whose declared path differs from the
/// one its name implies — `None` when they agree, which is the norm.
fn off_path_library(
    body: &[patina_core::TaggedValue],
    root: &Path,
    heap: &SharedHeap,
) -> Option<OffPathLibrary> {
    let name = body
        .iter()
        .find_map(|decl| sexp::clause_argument(*decl, "name", heap))
        .and_then(|tv| sexp::library_name(tv, heap))?;
    let declared = body
        .iter()
        .find_map(|decl| sexp::clause_argument(*decl, "path", heap))
        .and_then(|tv| sexp::string_value(tv, heap))?;
    if Path::new(&declared) == name_relative_path(&name) {
        return None;
    }
    let source = root.join(&declared);
    source.is_file().then_some(OffPathLibrary { name, source })
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
        if let Some(name) = sexp::clause_argument(*decl, "name", heap) {
            if let Some(name) = sexp::library_name(name, heap) {
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

    /// Write `package.scm` into `temp` and parse it. Auxiliary files a case
    /// needs (a test script, an `.sld` to find) are written by the case.
    fn parsed(slug: &str, temp: &Path, package_scm: &str) -> Package {
        let path = temp.join("package.scm");
        std::fs::write(&path, package_scm).unwrap();
        let heap = patina_core::new_shared_heap();
        parse_package(slug, temp, &path, &heap).unwrap()
    }

    #[test]
    fn parses_a_snow_package_scm() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(temp.path().join("run-tests.scm"), "(display 1)").unwrap();

        let pkg = parsed(
            "chibi-match",
            temp.path(),
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
        );

        assert_eq!(pkg.provides, vec!["chibi match", "chibi match-test"]);
        assert!(pkg.depends.contains(&"scheme base".to_string()));
        assert!(pkg.depends.contains(&"chibi test".to_string()));
        assert!(pkg.depends.contains(&"chibi".to_string()));
        assert!(pkg.test_script.is_some());
        // Paths match the names, so nothing needs staging.
        assert!(pkg.off_path_libraries.is_empty());
    }

    /// A dependency the *test* program alone needs is declared in a clause of
    /// its own. Ignoring it filed packages as `missing-library` naming a
    /// library sitting in the corpus, which put bogus entries at the head of
    /// the bundling work queue.
    #[test]
    fn collects_package_level_test_depends() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(temp.path().join("t.scm"), "(display 1)").unwrap();

        let pkg = parsed(
            "lassik-string-inflection",
            temp.path(),
            r#"
            (package
              (library (name (lassik string-inflection))
                       (path "lassik/string-inflection.sld")
                       (depends (scheme base) (scheme char)))
              (test "t.scm")
              (test-depends (scheme base) (srfi 64)))
        "#,
        );

        assert_eq!(pkg.test_depends, vec!["scheme base", "srfi 64"]);
        // and they stay out of what an importer of this package would need
        assert!(!pkg.depends.contains(&"srfi 64".to_string()));
    }

    /// The srfi-235 shape: the test script imports its framework, the
    /// metadata declares nothing. The closure must come from the script —
    /// through `cond-expand` branches and R7RS import modifiers alike —
    /// or the package is misfiled as `missing-library` naming a library
    /// the corpus provides.
    #[test]
    fn scans_the_test_script_for_undeclared_test_imports() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            temp.path().join("t.scm"),
            r#"
            (cond-expand
              (chibi
               (import (scheme base)
                       (rename (except (chibi test) test-equal)
                               (test test-equal))))
              (else
               (import (scheme base) (srfi 64))))
            (import (only (srfi 1) fold))
            "#,
        )
        .unwrap();

        let pkg = parsed(
            "srfi-235",
            temp.path(),
            r#"
            (package
              (library (name (srfi 235)) (depends (scheme base)))
              (test "t.scm"))
        "#,
        );

        for lib in ["srfi 64", "chibi test", "srfi 1"] {
            assert!(
                pkg.test_depends.contains(&lib.to_string()),
                "test_depends missing {:?}: {:?}",
                lib,
                pkg.test_depends
            );
        }
        // and none of it leaks into what an importer of the package needs
        assert!(!pkg.depends.contains(&"srfi 64".to_string()));
    }

    /// A script the parser cannot read falls back to declared metadata
    /// rather than failing discovery for the whole corpus.
    #[test]
    fn an_unparseable_test_script_degrades_to_declared_metadata() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(temp.path().join("t.scm"), "(((").unwrap();

        let pkg = parsed(
            "broken-script",
            temp.path(),
            r#"
            (package
              (library (name (broken lib)))
              (test "t.scm")
              (test-depends (srfi 64)))
        "#,
        );

        assert!(pkg.test_script.is_some());
        assert_eq!(pkg.test_depends, vec!["srfi 64"]);
    }

    #[test]
    fn records_a_library_whose_path_defies_its_name() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            temp.path().join("irregex.sld"),
            "(define-library (chibi irregex))",
        )
        .unwrap();
        std::fs::write(temp.path().join("foo.scm"), "(display 1)").unwrap();

        let pkg = parsed(
            "chibi-irregex",
            temp.path(),
            r#"
            (package
              (library (name (chibi irregex)) (path "irregex.sld")
                       (depends (scheme base)))
              (program (name (bin foo)) (path "foo.scm")))
        "#,
        );

        assert_eq!(pkg.off_path_libraries.len(), 1);
        assert_eq!(pkg.off_path_libraries[0].name, "chibi irregex");
        assert_eq!(
            pkg.off_path_libraries[0].source,
            temp.path().join("irregex.sld")
        );
    }

    #[test]
    fn name_relative_path_is_the_sld_convention() {
        assert_eq!(
            name_relative_path("chibi irregex"),
            PathBuf::from("chibi/irregex.sld")
        );
        assert_eq!(
            name_relative_path("srfi 197"),
            PathBuf::from("srfi/197.sld")
        );
        assert_eq!(name_relative_path("foo"), PathBuf::from("foo.sld"));
    }

    #[test]
    fn integer_library_names_normalize() {
        let temp = tempfile::TempDir::new().unwrap();
        let pkg = parsed(
            "srfi-1",
            temp.path(),
            r#"(package (library (name (srfi 1)) (depends (srfi 8))))"#,
        );
        assert_eq!(pkg.provides, vec!["srfi 1"]);
        assert_eq!(pkg.depends, vec!["srfi 8"]);
        assert!(pkg.test_script.is_none());
        assert!(pkg.test_depends.is_empty());
    }
}
