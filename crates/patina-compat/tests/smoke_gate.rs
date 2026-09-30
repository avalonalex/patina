#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture {
    dir: tempfile::TempDir,
    patina: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        for slug in ["alpha", "beta", "probe-only", "suite-only"] {
            let package = dir.path().join("vendor").join(slug);
            fs::create_dir_all(&package).unwrap();
            let metadata = if slug == "suite-only" {
                fs::write(package.join("test.scm"), "(import (scheme base))").unwrap();
                "(package (test \"test.scm\"))"
            } else {
                "(package)"
            };
            fs::write(package.join("package.scm"), metadata).unwrap();
        }
        fs::create_dir(dir.path().join("smoke")).unwrap();
        fs::write(
            dir.path().join("smoke/manifest.scm"),
            "(patina-compat-smokes (tests
              ((slug \"alpha\") (assertions 1))
              ((slug \"beta\") (assertions 1))))",
        )
        .unwrap();
        let patina = dir.path().join("patina");
        // The stand-in tests the CLI boundary: selection, backend forwarding,
        // classified results, artifacts and exit status. Real Scheme drivers
        // run in CI on both backends; comments here supply controlled output.
        fs::write(
            &patina,
            r#"#!/bin/sh
backend=vm
previous=
for arg do
    if [ "$previous" = --diagnostics-file ]; then
        printf '%s\n' '{"protocol":"patina-diagnostics","version":1}' > "$arg"
    fi
    [ "$arg" != --tree-walker ] || backend=tree-walker
    program=$arg
    previous=$arg
done
[ "$backend" = "$SMOKE_TEST_BACKEND" ] || exit 90
case "$program" in
    *-self-check/probe.scm) exit 0 ;;
    */smoke.scm)
        sed -n 's/^;; stdout: //p' "$program"
        code=$(sed -n 's/^;; exit: //p' "$program")
        exit "${code:-0}"
        ;;
    *) echo 'Error: a non-smoke package was executed' >&2; exit 91 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(&patina, fs::Permissions::from_mode(0o755)).unwrap();
        let fixture = Self { dir, patina };
        fixture.driver("alpha", ";; stdout: (patina-compat-smoke 1 0)\n");
        fixture.driver("beta", ";; stdout: (patina-compat-smoke 1 0)\n");
        fixture
    }

    fn driver(&self, slug: &str, source: &str) {
        fs::write(self.dir.path().join(format!("smoke/{slug}.scm")), source).unwrap();
    }

    fn command(&self, command: &str, tree_walker: bool) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_patina-compat"));
        cmd.arg(command)
            .arg("--patina")
            .arg(&self.patina)
            .arg("--vendor")
            .arg(self.dir.path().join("vendor"))
            .current_dir(self.dir.path())
            .env(
                "SMOKE_TEST_BACKEND",
                if tree_walker { "tree-walker" } else { "vm" },
            );
        if tree_walker {
            cmd.arg("--tree-walker");
        }
        cmd
    }
}

fn assert_exit(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn checks_all_smoke_drivers_on_the_requested_backend_without_overwriting_the_corpus() {
    let fixture = Fixture::new();
    let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/reports");
    let snapshot = fs::read(reports.join("results.scm")).unwrap();
    let report = fs::read(reports.join("report.md")).unwrap();
    for tree_walker in [false, true] {
        let output = fixture
            .command("check-smoke", tree_walker)
            .output()
            .unwrap();
        assert_exit(&output, 0);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("**2 of 2 packages pass.**"), "{stdout}");
        assert!(stdout.contains("**2 passed smoke checks**"), "{stdout}");
        assert!(stdout.contains("| alpha | smoke | pass |"), "{stdout}");
        assert!(stdout.contains("| beta | smoke | pass |"), "{stdout}");
        assert!(!stdout.contains("probe-only"));
        assert!(!stdout.contains("suite-only"));
        assert_eq!(fs::read(reports.join("results.scm")).unwrap(), snapshot);
        assert_eq!(fs::read(reports.join("report.md")).unwrap(), report);
    }
}

#[test]
fn failed_or_incomplete_smoke_runs_fail_the_gate_and_still_write_diagnostics() {
    let fixture = Fixture::new();
    let snapshot = fixture.dir.path().join("results.scm");
    let report = fixture.dir.path().join("report.txt");
    for (source, status, evidence) in [
        (
            ";; stdout: FAIL: beta assertion\n;; stdout: (patina-compat-smoke 0 1)\n",
            "wrong-result",
            "FAIL: beta assertion",
        ),
        (
            ";; no completion tally\n",
            "runtime-error",
            "Missing smoke completion tally; expected 1 assertions",
        ),
        (
            ";; stdout: (patina-compat-smoke 1 0)\n;; exit: 1\n",
            "runtime-error",
            "exit status: 1",
        ),
    ] {
        fixture.driver("beta", source);
        for tree_walker in [false, true] {
            let output = fixture
                .command("check-smoke", tree_walker)
                .arg("--results")
                .arg(&snapshot)
                .arg("--report")
                .arg(&report)
                .output()
                .unwrap();
            assert_exit(&output, 1);
            let results = fs::read_to_string(&snapshot).unwrap();
            assert!(results.contains("((slug \"alpha\") (mode smoke) (status pass))"));
            assert!(results.contains(&format!("((slug \"beta\") (mode smoke) (status {status}) ")));
            assert!(results.contains(evidence), "{results}");
            let report = fs::read_to_string(&report).unwrap();
            assert!(report.contains("**1 of 2 packages pass.**"));
            assert!(report.contains(evidence), "{report}");
        }
    }
}

#[test]
fn filters_exclusions_and_empty_or_invalid_registrations_cannot_pass() {
    let fixture = Fixture::new();
    for args in [
        vec!["--filter", "alpha"],
        vec!["--exclusions", "none"],
        vec!["--no-exclusions"],
    ] {
        let output = fixture
            .command("check-smoke", false)
            .args(args)
            .output()
            .unwrap();
        assert_exit(&output, 2);
    }
    fs::remove_file(fixture.dir.path().join("smoke/beta.scm")).unwrap();
    assert_exit(&fixture.command("check-smoke", false).output().unwrap(), 2);
    fs::write(
        fixture.dir.path().join("smoke/manifest.scm"),
        "(patina-compat-smokes (tests))",
    )
    .unwrap();
    assert_exit(&fixture.command("check-smoke", false).output().unwrap(), 2);
}

#[test]
fn an_unwritable_requested_artifact_fails_the_gate() {
    let fixture = Fixture::new();
    for option in ["--results", "--report"] {
        let output = fixture
            .command("check-smoke", false)
            .arg(option)
            .arg(fixture.dir.path()) // A directory cannot be overwritten as a file.
            .output()
            .unwrap();
        assert_exit(&output, 2);
        assert!(String::from_utf8_lossy(&output.stderr).contains("could not write"));
    }
}

#[test]
fn ordinary_measurement_keeps_its_non_gating_exit_status() {
    let fixture = Fixture::new();
    fixture.driver("beta", ";; stdout: (patina-compat-smoke 0 1)\n");
    let output = fixture
        .command("run", false)
        .args(["--filter", "beta"])
        .output()
        .unwrap();
    assert_exit(&output, 0);
    assert!(String::from_utf8_lossy(&output.stdout).contains("| beta | smoke | wrong-result |"));
}

#[test]
fn a_binary_without_the_diagnostic_protocol_cannot_score_the_corpus() {
    let fixture = Fixture::new();
    fs::write(&fixture.patina, "#!/bin/sh\nexit 0\n").unwrap();
    for command in ["run", "check-smoke"] {
        let output = fixture.command(command, false).output().unwrap();
        assert_exit(&output, 2);
        assert!(String::from_utf8_lossy(&output.stderr).contains("diagnostic stream"));
        assert!(!fixture.dir.path().join("results.scm").exists());
    }
}
