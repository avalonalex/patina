//! A program that opens files and drops the ports does not run out of
//! descriptors while the dead ports hold them (#607).
//!
//! A dropped port's file closes when a collection finds the port dead and
//! drops it, so collecting at the right moment is enough. Two things make
//! sure one comes: descriptor pressure, which posts a collection once
//! `min(128, RLIMIT_NOFILE / 4)` of the file ports opened since the last one
//! are still open (`patina_core::heap` `account.rs`), and an open that runs
//! out of descriptors anyway (`EMFILE`), which collects at its call and
//! tries once more (`patina_primitives` `io/file.rs`), in every GC mode,
//! `PATINA_GC=0` included.
//!
//! These run the binary in a subprocess whose `RLIMIT_NOFILE` the shell sets
//! (`ulimit -n`), in the default GC mode and under `PATINA_GC=0`, on both
//! backends. Unix only: the limit is `/bin/sh`'s to set, and only there is
//! running out of descriptors told apart from another failed open
//! (`patina_core::port::out_of_descriptors`).
//!
//! # The oracle
//!
//! Measured 2026-10-04 with chibi 0.12.0 and Gauche 0.9.15, each under
//! `/bin/sh -c 'ulimit -n …; exec …'`, against Patina at b2a270d, before
//! #607:
//!
//! | Program | chibi | Gauche | Patina before |
//! |---|---|---|---|
//! | 100,000 `open-input-file`s dropped, at 1024 | `(ok 100000)` | `(ok 100000)` | `(failed-at 1021 #t)` |
//! | 5,000 `open-input-file`s dropped, at 128 | `(ok 5000)` | `(failed-at 124 #t)` | `(failed-at 125 #t)` |
//! | 5,000 `open-output-file`s dropped, at 128 | `(ok 5000)` | `(failed-at 124 #f)` | `(failed-at 125 #t)` |
//! | a dropped output port's file, read before and after a collection | `("" "hello")` | `("" "hello")` | `("" "hello")` |
//! | `load` once the ports that filled the table are dropped, at 64 | loads it | file error | file error |
//!
//! chibi collects and retries when an open fails with `EMFILE`, `load`'s
//! included; Gauche collects when its table of buffered ports fills, and
//! does not retry. Patina follows chibi, and both backends now answer `ok`
//! on every row, and load the file.
//! The last row was already right: the collection that finds a file port
//! dead writes out its buffer as it closes it, which every collection, the
//! one `(gc)` asks for and one the allocations trigger, does.
//!
//! The library loader, measured 2026-10-25 with chibi 0.12.0
//! (`chibi-scheme -A lib`) and Gauche 0.9.15 (`gosh -r7 -A lib`) at
//! `ulimit -n 64`, once a program has filled the table with ports and
//! dropped them, against Patina before #783:
//!
//! | Program | chibi | Gauche | Patina before |
//! |---|---|---|---|
//! | a bare `import` of a library that includes its body | `42` | system error | I/O error |
//! | `(import (foo outer))`, whose first import fills the table | `(#t 42)` | system error | I/O error |
//! | a top-level `define-library` importing it, or including a file | no top-level `define-library` | system error | I/O error |
//! | `(begin (import (foo bar)))` | `42` | system error | I/O error |
//! | `(environment '(foo bar))` under a `guard` | `42` | file error | an error, not a file error |
//! | a program's `(include …)` | `42` | system error | `include: cannot read` |
//!
//! Since #783 the first three load on both backends, as chibi's first two
//! do (chibi takes no top-level `define-library`). The last three are where
//! the load cannot collect, and stay a documented limit, as does an
//! `include` form in a library's body, read as the body expands: Gauche
//! loads that one, reading it before the library's imports run, and chibi
//! resolves a body's `include` against the working directory, so it is no
//! oracle there.
//!
//! GC-time flushing is observable, so these stay outside the byte-identical
//! differential lane, `scripts/run_gc_differential.sh`, which compares the
//! chibi suite's output alone.

#![cfg(unix)]

mod common;

use common::{BOTH_BACKENDS, patina_command};
use std::process::Command;
use tempfile::TempDir;

/// The issue's program: open `count` files with `opener` and drop each port,
/// reporting how far it got.
fn dropping(opener: &str, file: &str, count: usize) -> String {
    format!(
        "(import (scheme base) (scheme file) (scheme write))
(define count 0)
(write
 (guard (e (#t (list 'failed-at count (file-error? e))))
   (let loop ()
     (if (< count {count})
         (begin ({opener} \"{file}\") (set! count (+ count 1)) (loop))
         (list 'ok count)))))
(newline)
"
    )
}

/// The GC modes each run is made in: the default, and the reference run of
/// the differential lanes, which collects only where `(gc)`, or an open or a
/// library read that ran out of descriptors (#783), asks.
const MODES: [(&str, &[(&str, &str)]); 2] =
    [("default", &[]), ("PATINA_GC=0", &[("PATINA_GC", "0")])];

/// `command`, run by a shell that first lowers the descriptor limit to
/// `limit`: the same program, arguments, environment and directory, which
/// [`patina_command`] scrubs.
fn limited(command: Command, limit: u32) -> Command {
    let mut shell = Command::new("/bin/sh");
    shell
        .arg("-c")
        .arg(format!("ulimit -n {limit} && exec \"$0\" \"$@\""))
        .arg(command.get_program())
        .args(command.get_args());
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => shell.env(key, value),
            None => shell.env_remove(key),
        };
    }
    if let Some(dir) = command.get_current_dir() {
        shell.current_dir(dir);
    }
    shell
}

/// Run `program` (and the libraries in `libraries`, under `-A lib`) in a
/// fresh directory holding `data.txt`, on `backend`, in a process whose
/// descriptor limit is `limit`, with `envs` set and the GC variables
/// otherwise cleared. Returns stdout, asserting the run succeeded.
fn run(
    backend: &[&str],
    limit: u32,
    envs: &[(&str, &str)],
    program: &str,
    libraries: &[(&str, &str)],
) -> String {
    let (status, stdout, stderr) = output(backend, limit, envs, program, libraries, &[]);
    assert!(
        status.success(),
        "patina {backend:?} at ulimit -n {limit} {envs:?} failed: {status}\nstdout: {stdout}\nstderr: {stderr}"
    );
    stdout
}

/// As [`run`], with `flags` before the program: the run's exit status, its
/// stdout and its stderr.
fn output(
    backend: &[&str],
    limit: u32,
    envs: &[(&str, &str)],
    program: &str,
    libraries: &[(&str, &str)],
    flags: &[&str],
) -> (std::process::ExitStatus, String, String) {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("data.txt"), "hello\n").unwrap();
    std::fs::write(dir.path().join("program.scm"), program).unwrap();
    for (path, text) in libraries {
        let path = dir.path().join("lib").join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let mut args = backend.to_vec();
    args.extend_from_slice(flags);
    args.extend(["-A", "lib", "program.scm"]);
    let mut command = patina_command(dir.path(), &args, &[]);
    command
        .env_remove("PATINA_GC")
        .env_remove("PATINA_GC_STRESS")
        .env_remove("PATINA_GC_ZEAL")
        .envs(envs.iter().copied());
    let output = limited(command, limit)
        .output()
        .expect("spawn patina under a shell");
    (
        output.status,
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The issue's rows, and the binary opens beside them: every one finishes,
/// on both backends, in the default mode and under `PATINA_GC=0`.
#[test]
fn dropped_ports_do_not_run_out_of_descriptors() {
    for (limit, count, opener, file) in [
        (1024, 100_000, "open-input-file", "data.txt"),
        (128, 5_000, "open-input-file", "data.txt"),
        (128, 5_000, "open-output-file", "out.txt"),
        (128, 5_000, "open-binary-input-file", "data.txt"),
        (128, 5_000, "open-binary-output-file", "out.bin"),
    ] {
        let program = dropping(opener, file, count);
        for backend in BOTH_BACKENDS {
            for (mode, envs) in MODES {
                assert_eq!(
                    run(backend, limit, envs, &program, &[]),
                    format!("(ok {count})"),
                    "{backend:?} {mode} at ulimit -n {limit}: {count} x {opener}"
                );
            }
        }
    }
}

/// Descriptor pressure counts the ports opened since the last collection
/// and not closed: a loop that closes each port it opens posts no
/// collection, however long it runs, and one that drops them posts one
/// every 128 opens, the threshold at `ulimit -n 1024`. Under `PATINA_GC=0`
/// it posts none at all.
#[test]
fn closed_ports_post_no_collection_and_dropped_ones_do() {
    const PROGRAM: &str = "(import (scheme base) (scheme file) (scheme write) (patina debug))
(define (stat key) (cdr (assq key (gc-stats))))
(define (closing n) (when (> n 0) (close-port (open-input-file \"data.txt\")) (closing (- n 1))))
(define (dropping n) (when (> n 0) (open-input-file \"data.txt\") (dropping (- n 1))))
(define (posts thunk)
  (let ((before (stat 'descriptor-collections)))
    (thunk)
    (- (stat 'descriptor-collections) before)))
(define closed (posts (lambda () (closing 10000))))
(define left (stat 'descriptors-since-gc))
(define dropped (posts (lambda () (dropping 10000))))
(write (list closed left dropped))
(newline)
";
    for backend in BOTH_BACKENDS {
        let out = run(backend, 1024, &[], PROGRAM, &[]);
        let counts: Vec<u64> = out
            .trim_matches(['(', ')'])
            .split_whitespace()
            .map(|n| n.parse().unwrap_or_else(|_| panic!("{backend:?}: {out}")))
            .collect();
        let [closed, left, dropped] = counts[..] else {
            panic!("{backend:?}: {out}")
        };
        assert_eq!((closed, left), (0, 0), "{backend:?}: {out}");
        // One post every 128 dropped opens, less any interval the byte
        // trigger collected first, which resets the count.
        assert!(
            (10_000 / 128 / 2..=10_000 / 128).contains(&dropped),
            "{backend:?}: {out}"
        );

        assert_eq!(
            run(backend, 1024, &[("PATINA_GC", "0")], PROGRAM, &[]),
            "(0 0 0)",
            "{backend:?} under PATINA_GC=0"
        );
    }
}

/// What an open file port is charged, and when it is given back: 8 KiB of
/// external bytes and one place in descriptor pressure's count while it is
/// open, both given back when it is closed. A collection starts the count
/// again, and closing a port it found live gives back its bytes but takes
/// nothing off the count of the ports opened since.
#[test]
fn a_close_gives_back_what_the_open_charged() {
    // Measured inside one procedure, so that no definition between the
    // reads grows the global environment's tables, which are external bytes
    // too (#615).
    const PROGRAM: &str = "(import (scheme base) (scheme file) (scheme write) (patina debug))
(define (stat key) (cdr (assq key (gc-stats))))
(define (measure)
  (gc)
  (let* ((e0 (stat 'external-bytes))
         (now (lambda () (list (stat 'descriptors-since-gc) (- (stat 'external-bytes) e0))))
         (p (open-input-file \"data.txt\"))
         (q (open-output-file \"out.txt\"))
         (opened (now)))
    (close-port q)
    (let ((closed (now)))
      (gc)
      (let* ((collected (now))
             (r (open-input-file \"data.txt\")))
        (close-port p)
        (let ((old-closed (now)))
          (close-port r)
          (list opened closed collected old-closed (now)))))))
(write (measure))
(newline)
";
    for backend in BOTH_BACKENDS {
        assert_eq!(
            run(backend, 1024, &[], PROGRAM, &[]),
            "((2 16384) (1 8192) (0 8192) (1 8192) (0 0))",
            "{backend:?}"
        );
    }
}

/// After a collection the heap's `PortTable` holds entries only for the port
/// objects still live, the ones dropped since staying until the next
/// collection finds them (#785): `ports` and `open-file-ports`, read after a
/// `(gc)`, then
/// 1,000 string ports and 100 file ports opened and dropped, then a `(gc)`
/// in a later form, read back at their first values. A table that kept a
/// dead string port's entry would show only here: a string port holds no
/// descriptor and charges no external bytes.
#[test]
fn a_collection_leaves_the_port_table_as_it_found_it() {
    const PROGRAM: &str = "(import (scheme base) (scheme file) (scheme write) (patina debug))
(define (stat key) (cdr (assq key (gc-stats))))
(define (churn n open) (if (> n 0) (begin (open) (churn (- n 1) open))))
(gc)
(define before (list (stat 'ports) (stat 'open-file-ports)))
(churn 1000 (lambda () (open-input-string \"text\")))
(churn 100 (lambda () (open-input-file \"data.txt\")))
(define opened (list (- (stat 'ports) (car before)) (- (stat 'open-file-ports) (cadr before))))
(gc)
(write (list opened (equal? before (list (stat 'ports) (stat 'open-file-ports)))))
(newline)
";
    for backend in BOTH_BACKENDS {
        for (mode, envs) in MODES {
            assert_eq!(
                run(backend, 1024, envs, PROGRAM, &[]),
                "((1100 100) #t)",
                "{backend:?} {mode}"
            );
        }
    }
}

/// Each descriptor limit beside what counts against it (#663). Under
/// `ulimit -n 200`, descriptor pressure's threshold is min(128, 200 / 4) =
/// 50, and `#f` under `PATINA_GC=0`, which counts nothing. `open-file-ports`
/// counts the ports open now, whenever they opened: a collection starts
/// `descriptors-since-gc` again and leaves it alone, and a close takes one
/// off.
#[test]
fn each_descriptor_limit_shows_beside_its_use() {
    const PROGRAM: &str = "(import (scheme base) (scheme file) (scheme write) (patina debug))
(define (stat key) (cdr (assq key (gc-stats))))
(define (measure)
  (let* ((p (open-input-file \"data.txt\"))
         (q (open-input-file \"data.txt\"))
         (opened (list (stat 'open-file-ports) (stat 'descriptors-since-gc))))
    (gc)
    (let ((collected (list (stat 'open-file-ports) (stat 'descriptors-since-gc))))
      (close-port p)
      (let ((closed (stat 'open-file-ports)))
        (close-port q)
        (list opened collected closed (stat 'open-file-ports)
              (stat 'descriptors-trigger) (stat 'descriptor-limit))))))
(write (measure))
(newline)
";
    for backend in BOTH_BACKENDS {
        for (mode, envs) in MODES {
            let trigger = if mode == "default" { "50" } else { "#f" };
            assert_eq!(
                run(backend, 200, envs, PROGRAM, &[]),
                format!("((2 2) (2 0) 1 0 {trigger} 200)"),
                "{backend:?} {mode}"
            );
        }
    }
}

/// F2 of GC_PRD §9.6: a collection that finds a file port dead writes out
/// what was left in its buffer and closes it. Before the collection the
/// file is empty, after it it holds the output, both for the collection
/// `(gc)` asks for and for one the allocations trigger. chibi and Gauche
/// answer the same.
#[test]
fn a_dropped_output_port_is_written_out_by_the_collection_that_finds_it() {
    for (how, collect) in [
        ("(gc)", "(gc)"),
        (
            "an allocation-triggered collection",
            "(let ((c (stat 'collections)))
  (let churn () (when (= c (stat 'collections)) (make-vector 1000 0) (churn))))",
        ),
    ] {
        let program = format!(
            "(import (scheme base) (scheme file) (scheme write) (patina debug))
(define (stat key) (cdr (assq key (gc-stats))))
(define (contents)
  (call-with-input-file \"out.txt\"
    (lambda (p) (let ((s (read-string 100 p))) (if (eof-object? s) \"\" s)))))
(define (write-and-drop)
  (let ((p (open-output-file \"out.txt\"))) (write-string \"hello\" p))
  #f)
(write-and-drop)
(define before (contents))
{collect}
(define after (contents))
(write (list before after))
(newline)
"
        );
        for backend in BOTH_BACKENDS {
            assert_eq!(
                run(backend, 1024, &[], &program, &[]),
                "(\"\" \"hello\")",
                "{backend:?} {how}"
            );
        }
    }
}

/// `load` reads its file as an open does: a read that runs out of
/// descriptors collects at the call and reads once more. The table is
/// filled with ports held live, so that the open that finds it full raises,
/// and then dropped: `load` finds it full of ports nothing reaches, which
/// the collection closes. The file it loads is in `lib/`, where [`run`]
/// writes the files it is given. chibi 0.12, whose `load` opens its file
/// through `open-input-file`'s retry, loads it, and so do both backends
/// since #607; Gauche 0.9.15 raises the file error, as both did before
/// (measured 2026-10-04).
#[test]
fn load_collects_and_reads_again_when_descriptors_ran_out() {
    const PROGRAM: &str = "(import (scheme base) (scheme file) (scheme load) (scheme write))
(define held '())
(define (exhaust!)
  (let loop ()
    (let ((p (guard (e ((file-error? e) #f)) (open-input-file \"data.txt\"))))
      (when p (set! held (cons p held)) (loop))))
  (set! held '()))
(exhaust!)
(write (guard (e ((file-error? e) 'file-error)) (load \"lib/loaded.scm\") loaded-value))
(newline)
";
    for backend in BOTH_BACKENDS {
        for (mode, envs) in MODES {
            assert_eq!(
                run(
                    backend,
                    64,
                    envs,
                    PROGRAM,
                    &[("loaded.scm", "(define loaded-value 42)\n")]
                ),
                "42",
                "{backend:?} {mode}"
            );
        }
    }
}

/// A library that exhausts the descriptors in its body, drops the ports,
/// then opens once more and reports what that open did, with how many
/// collections were posted rather than run meanwhile.
const EXHAUSTING_LIBRARY: &str = "(define-library (deferred open)
  (import (scheme base) (scheme file) (patina debug))
  (export result)
  (begin
    (define (stat key) (cdr (assq key (gc-stats))))
    (define held '())
    (define (exhaust!)
      (let loop ()
        (let ((p (guard (e ((file-error? e) #f)) (open-input-file \"data.txt\"))))
          (when p (set! held (cons p held)) (loop))))
      (set! held '()))
    (exhaust!)
    (define before (stat 'deferred-collections))
    (define result
      (list (guard (e ((file-error? e) 'file-error)) (open-input-file \"data.txt\") 'opened)
            (- (stat 'deferred-collections) before)))))
";

/// `program`'s output on both backends, in each GC mode, all of which must
/// be `expected`, with `EXHAUSTING_LIBRARY` beside it.
fn assert_exhausting_library(program: &str, expected: &str) {
    for backend in BOTH_BACKENDS {
        for (mode, envs) in MODES {
            assert_eq!(
                run(
                    backend,
                    64,
                    envs,
                    program,
                    &[("deferred/open.sld", EXHAUSTING_LIBRARY)]
                ),
                expected,
                "{backend:?} {mode}"
            );
        }
    }
}

/// The documented limit: where collection is deferred, the first `EMFILE`
/// raises. A library body that an `import` inside a `begin` loads is such a
/// place, under the expansion's guard (point C of `PRD/GC_PRD.md` §11.3,
/// docs/GC_DESIGN.md §7): the open's collection is posted for the next safe
/// point that may collect and counted in `deferred-collections`, the second
/// attempt fails as the first did, and the file error is raised. Once the
/// load is done the program's own open succeeds. chibi collects anywhere,
/// and would open the file.
#[test]
fn where_collection_is_deferred_the_first_emfile_raises() {
    assert_exhausting_library(
        "(import (scheme base) (scheme file) (scheme write))
(begin (import (deferred open)))
(write (list result (read-char (open-input-file \"data.txt\"))))
(newline)
",
        "((file-error 1) #\\h)",
    );
}

/// A bare `import` loads the same library outside the expansion's guard, so
/// its body collects (#677): the open collects at its call and opens the
/// file, in every GC mode, as chibi's does. Until #677 this answered as the
/// deferred row above.
#[test]
fn a_library_a_bare_import_loads_collects_at_its_open() {
    assert_exhausting_library(
        "(import (scheme base) (scheme file) (scheme write) (deferred open))
(write (list result (read-char (open-input-file \"data.txt\"))))
(newline)
",
        "((opened 0) #\\h)",
    );
}

/// `(foo bar)`, which includes its body, `(exhaust now)`, whose body fills
/// the table with ports and drops them, `(foo outer)`, which imports both in
/// that order, and `(foo body-include)`, whose body's `include` form comes
/// after its imports filled the table (#783).
const LOADER_LIBRARIES: [(&str, &str); 5] = [
    (
        "foo/body-include.sld",
        "(define-library (foo body-include) (import (scheme base) (exhaust now)) (export v)
  (begin (include \"bar-body.scm\") (define v (list exhausted bar-value))))",
    ),
    (
        "foo/bar.sld",
        "(define-library (foo bar) (import (scheme base)) (export bar-value) (include \"bar-body.scm\"))",
    ),
    ("foo/bar-body.scm", "(define bar-value 42)\n"),
    (
        "exhaust/now.sld",
        "(define-library (exhaust now)
  (import (scheme base) (scheme file))
  (export exhausted)
  (begin
    (define held '())
    (define (exhaust!)
      (let loop ()
        (let ((p (guard (e ((file-error? e) #f)) (open-input-file \"data.txt\"))))
          (when p (set! held (cons p held)) (loop))))
      (set! held '()))
    (exhaust!)
    (define exhausted #t)))",
    ),
    (
        "foo/outer.sld",
        "(define-library (foo outer)
  (import (scheme base) (exhaust now) (foo bar))
  (export outer-value)
  (begin (define outer-value (list exhausted bar-value))))",
    ),
];

/// A program's opening: it fills the table with ports and drops them, so a
/// collection would close them all, and records `deferred-collections`.
const EXHAUSTED: &str =
    "(import (scheme base) (scheme file) (scheme write) (scheme eval) (patina debug))
(define (stat key) (cdr (assq key (gc-stats))))
(define held '())
(define (exhaust!)
  (let loop ()
    (let ((p (guard (e ((file-error? e) #f)) (open-input-file \"data.txt\"))))
      (when p (set! held (cons p held)) (loop))))
  (set! held '()))
(exhaust!)
(define before (stat 'deferred-collections))
";

/// The library loader reads as `load` does: a read that runs out of
/// descriptors collects where the load may and parses the library again,
/// its `include` files' reads covered too (#783). A bare `import`, a library
/// another loads after its own imports filled the table, and a top-level
/// `define-library` that imports one or includes a file each load, in every
/// GC mode, on both backends. Until #783 each raised the read's I/O error.
#[test]
fn the_loader_collects_and_reads_again_when_descriptors_ran_out() {
    for (program, expected) in [
        (
            format!("{EXHAUSTED}(import (foo bar))\n(write bar-value)\n"),
            "42",
        ),
        (
            "(import (scheme base) (scheme write) (foo outer))\n(write outer-value)\n".to_owned(),
            "(#t 42)",
        ),
        (
            format!(
                "{EXHAUSTED}(define-library (local) (import (scheme base) (foo bar)) (export v) \
                 (begin (define v bar-value)))\n(import (local))\n(write v)\n"
            ),
            "42",
        ),
        (
            format!(
                "{EXHAUSTED}(define-library (local) (import (scheme base)) (export bar-value) \
                 (include \"lib/foo/bar-body.scm\"))\n(import (local))\n(write bar-value)\n"
            ),
            "42",
        ),
    ] {
        for backend in BOTH_BACKENDS {
            for (mode, envs) in MODES {
                assert_eq!(
                    run(backend, 64, envs, &program, &LOADER_LIBRARIES),
                    expected,
                    "{backend:?} {mode}: {program}"
                );
            }
        }
    }
}

/// The documented limit, narrowed: where the load cannot collect, the
/// loader's first `EMFILE` raises. An `import` met inside a `begin` loads
/// beneath the expansion's guard (point C), and `environment` on a nested
/// loop; each posts its collection and counts it in `deferred-collections`,
/// once, and the read's error stands. An `include` form reads during its
/// expansion, under the same guard, in a program and in a library's body,
/// and raises as before, a limit decision 7 on #782 recommends keeping.
/// chibi loads the first three; Gauche raises a system error, or for
/// `environment` a file error, and loads the library-body row.
#[test]
fn where_a_load_cannot_collect_the_loader_s_first_emfile_raises() {
    let begin = format!(
        "{EXHAUSTED}(begin (import (foo bar)))\n(write (- (stat 'deferred-collections) before))\n"
    );
    let environment = format!(
        "{EXHAUSTED}(write (list (guard (e (#t (if (file-error? e) 'file-error 'error))) \
         (environment '(foo bar)) 'loaded) (- (stat 'deferred-collections) before)))\n"
    );
    let include = format!("{EXHAUSTED}(include \"lib/foo/bar-body.scm\")\n(write bar-value)\n");
    let body_include = "(import (scheme base) (scheme write) (foo body-include))\n(write v)\n";
    for backend in BOTH_BACKENDS {
        for (mode, envs) in MODES {
            let case = format!("{backend:?} {mode}");
            let (status, stdout, stderr) =
                output(backend, 64, envs, &begin, &LOADER_LIBRARIES, &["-k"]);
            assert!(!status.success(), "{case}: {stdout}");
            assert!(
                stderr.contains("I/O error: Failed to read") && stderr.contains("bar.sld"),
                "{case}: {stderr}"
            );
            assert_eq!(stdout, "1", "{case}: {stderr}");
            assert_eq!(
                run(backend, 64, envs, &environment, &LOADER_LIBRARIES),
                "(error 1)",
                "{case}"
            );
            let (status, _, stderr) = output(backend, 64, envs, &include, &LOADER_LIBRARIES, &[]);
            assert!(!status.success(), "{case}");
            assert!(
                stderr.contains("include: cannot read 'lib/foo/bar-body.scm'"),
                "{case}: {stderr}"
            );
            let (status, _, stderr) =
                output(backend, 64, envs, body_include, &LOADER_LIBRARIES, &[]);
            assert!(!status.success(), "{case}");
            assert!(
                stderr.contains("include: cannot read") && stderr.contains("bar-body.scm"),
                "{case}: {stderr}"
            );
        }
    }
}
