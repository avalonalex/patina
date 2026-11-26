//! `--stack-max` and `PATINA_STACK_MAX` through the command line (#907, GC
//! stage 4d.6b): the VM's stack cap, what an uncaught `&stack-exhausted`
//! prints and exits with, and what a malformed size is told. The rows of
//! the condition itself are in `crates/patina-tests/tests/
//! stack_cap_matrix.rs`; these are the runner's own.

mod common;

use common::patina_command;
use tempfile::TempDir;

/// A recursion 20,000 frames deep, about 2.7 MiB of stack on the VM: past a
/// 2 MiB cap, and well inside the default.
const DEEP: &str = "(import (scheme base) (scheme write))
(define (deep n) (if (= n 0) 0 (+ 1 (deep (- n 1)))))
";

/// Run `program` with `args` before the file's name and `envs` set: what it
/// wrote to each stream, and its exit status.
fn run(args: &[&str], envs: &[(&str, &str)], program: &str) -> (String, String, Option<i32>) {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("program.scm"), program).unwrap();
    let mut all = args.to_vec();
    all.push("program.scm");
    let output = patina_command(dir.path(), &all, envs)
        .output()
        .expect("spawn patina");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

/// An uncaught `&stack-exhausted` is an uncaught error (the owner's ruling
/// on Q4, 2026-11-24): the message on stderr, and exit status 1, the status
/// of `(car 5)`. With the flag and with the variable alike.
#[test]
fn an_uncaught_exhaustion_exits_as_any_uncaught_error() {
    let program = format!("{DEEP}(display \"before\")\n(deep 20000)\n(display \"after\")\n");
    let ordinary = run(&[], &[], "(import (scheme base))\n(car 5)\n");
    assert_eq!(ordinary.2, Some(1), "{ordinary:?}");
    for (args, envs) in [
        (&["--stack-max", "2M"][..], &[][..]),
        (&[][..], &[("PATINA_STACK_MAX", "2M")][..]),
    ] {
        let (stdout, stderr, status) = run(args, envs, &program);
        assert_eq!(stdout, "before", "{args:?} {envs:?}");
        assert!(
            stderr.contains("stack exhausted") && stderr.contains("cap of 2097152 bytes"),
            "{args:?} {envs:?}: {stderr}"
        );
        assert!(
            stderr.contains("--stack-max"),
            "it says how to raise it: {stderr}"
        );
        assert_eq!(status, Some(1), "{args:?} {envs:?}: {stderr}");
    }
}

/// The flag wins over the variable, and with neither the default cap holds
/// the recursion.
#[test]
fn the_flag_wins_over_the_variable() {
    let program = format!(
        "{DEEP}(import (patina debug))
(write (cdr (assq 'stack-limit (gc-stats))))
(write (deep 20000))
"
    );
    let (stdout, stderr, status) = run(
        &["--stack-max", "64M"],
        &[("PATINA_STACK_MAX", "2M")],
        &program,
    );
    assert_eq!(
        (stdout.as_str(), status),
        ("6710886420000", Some(0)),
        "{stderr}"
    );
    let (stdout, stderr, status) = run(
        &["--stack-max", "2M"],
        &[("PATINA_STACK_MAX", "64M")],
        &program,
    );
    assert_eq!(stdout, "2097152", "{stderr}");
    assert_eq!(status, Some(1), "{stderr}");
    // Neither: the machine's default, which is never under the floor and
    // holds 20,000 frames wherever it is.
    let (stdout, stderr, status) = run(&[], &[], &program);
    assert!(stdout.ends_with("20000"), "{stdout} {stderr}");
    assert_eq!(status, Some(0), "{stderr}");
    // An empty variable is no setting.
    let (stdout, _, status) = run(&[], &[("PATINA_STACK_MAX", "")], &program);
    assert!(stdout.ends_with("20000") && status == Some(0), "{stdout}");
}

/// A size is bytes, or a number with K, M or G; in either case a value
/// under the 2 MiB floor is raised to it.
#[test]
fn a_size_takes_a_binary_suffix_and_has_a_floor() {
    let program = "(import (scheme base) (scheme write) (patina debug))
(write (cdr (assq 'stack-limit (gc-stats))))
";
    for (size, bytes) in [
        ("4194304", "4194304"),
        ("4096K", "4194304"),
        ("4m", "4194304"),
        ("1G", "1073741824"),
        ("1", "2097152"),
        ("0", "2097152"),
    ] {
        let (stdout, stderr, status) = run(&["--stack-max", size], &[], program);
        assert_eq!(
            (stdout.as_str(), status),
            (bytes, Some(0)),
            "{size}: {stderr}"
        );
    }
}

/// What is not a size is refused before anything runs, by the flag and by
/// the variable, with the text, the reason and the grammar; and the flag
/// needs its value.
#[test]
fn a_malformed_size_is_refused_before_anything_runs() {
    let program = "(import (scheme base) (scheme write))\n(display \"ran\")\n";
    for size in ["2MB", "lots", "-1", "1.5G", "2 M"] {
        let (stdout, stderr, status) = run(&["--stack-max", size], &[], program);
        assert_eq!((stdout.as_str(), status), ("", Some(1)), "{size}: {stderr}");
        assert!(
            stderr.starts_with("Error: --stack-max: ") && stderr.contains(&format!("`{size}`")),
            "{size}: {stderr}"
        );
        assert!(stderr.contains("K, M or G"), "{size}: {stderr}");

        let (stdout, stderr, status) = run(&[], &[("PATINA_STACK_MAX", size)], program);
        assert_eq!((stdout.as_str(), status), ("", Some(1)), "{size}: {stderr}");
        assert!(
            stderr.starts_with("Error: PATINA_STACK_MAX: ")
                && stderr.contains(&format!("`{size}`")),
            "{size}: {stderr}"
        );
        // The tree-walker keeps no cap, and a malformed size is still
        // refused: the setting is the runner's, not the backend's.
        let (stdout, _, status) = run(&["--tree-walker"], &[("PATINA_STACK_MAX", size)], program);
        assert_eq!((stdout.as_str(), status), ("", Some(1)), "{size}");
    }
    let dir = TempDir::new().unwrap();
    let output = patina_command(dir.path(), &["--stack-max"], &[])
        .output()
        .expect("spawn patina");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--stack-max requires a size"),
        "{output:?}"
    );
}

/// The hard cap abandons one form (the owner's ruling on Q6): under `-k`
/// the next form runs, with the soft limit armed again, and the exit status
/// still reports the failure.
#[test]
fn keep_going_runs_the_form_after_the_hard_cap() {
    let program = format!(
        "{DEEP}(import (patina limits))
(display \"one \")
(with-exception-handler (lambda (e) (deep 20000)) (lambda () (deep 20000)))
(display \"two \")
(display (guard (e ((stack-exhausted? e) 'caught)) (deep 20000)))
"
    );
    let (stdout, stderr, status) = run(&["-k", "--stack-max", "2M"], &[], &program);
    assert_eq!(stdout, "one two caught", "{stderr}");
    assert!(stderr.contains("ran out of room"), "{stderr}");
    assert_eq!(status, Some(1), "{stderr}");
    // Without -k the script ends at the abandoned form.
    let (stdout, _, status) = run(&["--stack-max", "2M"], &[], &program);
    assert_eq!((stdout.as_str(), status), ("one ", Some(1)));
}

/// The tree-walker keeps no cap (decision A9 on #782): the flag is accepted
/// and the recursion runs to its end, and `stack-limit` reads 0.
#[test]
fn the_tree_walker_keeps_no_cap() {
    let program = format!(
        "{DEEP}(import (patina debug))
(write (cdr (assq 'stack-limit (gc-stats))))
(write (deep 20000))
"
    );
    let (stdout, stderr, status) = run(&["--tree-walker", "--stack-max", "2M"], &[], &program);
    assert_eq!((stdout.as_str(), status), ("020000", Some(0)), "{stderr}");
}

/// Under a real address-space limit the default cap is at most a quarter of
/// it, and startup succeeds (#907's acceptance): the binary under
/// `ulimit -v` of 4 GiB reads a `stack-limit` of at most 1 GiB. macOS
/// refuses to set `RLIMIT_AS`, so this runs on the Linux leg only.
#[cfg(target_os = "linux")]
#[test]
fn the_default_cap_fits_under_a_real_address_space_limit() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("program.scm"),
        "(import (scheme base) (scheme write) (patina debug))
(write (cdr (assq 'stack-limit (gc-stats))))
",
    )
    .unwrap();
    let binary = env!("CARGO_BIN_EXE_patina");
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg("ulimit -S -v 4194304 && exec \"$0\" \"$@\"")
        .arg(binary)
        .arg("program.scm")
        .env_remove("PATINA_STACK_MAX")
        .env("HOME", dir.path())
        .current_dir(dir.path())
        .output()
        .expect("spawn sh");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    let cap: u64 = stdout.trim().parse().unwrap_or_else(|_| panic!("{stdout}"));
    assert!((2 << 20..=1 << 30).contains(&cap), "{cap}");
}
