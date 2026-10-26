//! Each interpreter has its own current ports (#618).
//!
//! The current input, output and error ports were thread-local statics, so
//! every interpreter on a thread shared them: one that redirected its output
//! redirected the others', and the output of every interpreter made on the
//! thread afterwards. chibi's contexts, two made on one thread through its C
//! API, keep theirs apart (measured 2026-10-01). The ports are now the
//! interpreter's (`patina_core::Instance::current_port`).
//!
//! The host is this test binary run again in a child process, with
//! `(from stdin)` on its standard input, and the parent reads the child's
//! standard output. A standard output port writes through `print!`, which
//! libtest captures unless the child runs with `--nocapture`. Each case runs
//! on four pairs of interpreters, A then B: both on the VM, both on the
//! tree-walker, and the two mixed, since a slot that stayed per backend
//! would keep a pair of different backends apart while it failed the others.
//! Each redirection is checked to take effect in A too, so that a case does
//! not pass because A's redirection did nothing.

mod common;
use common::{tree_walker_interpreter, vm_interpreter};
use patina_interpreter::{Backend, Interpreter};
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

/// Set when this test binary runs itself as the host, naming the directory
/// it writes files into.
const HOST: &str = "PATINA_CURRENT_PORTS_HOST";

/// The four pairs, each named for its backends.
const PAIRS: [&str; 4] = ["vm+vm", "tw+tw", "vm+tw", "tw+vm"];

/// What a case answers, reported by the host on a line of its own.
fn report(case: &str, pair: &str, answer: &str) {
    println!("\nREPORT {case} {pair} {answer}");
}

/// `program`'s value on `interp`, displayed, or its error.
fn run<B: Backend>(interp: &Interpreter<B>, program: &str) -> String {
    match interp.eval_program_owned(program) {
        Ok(value) => interp.display_tagged(&value),
        Err(error) => format!("error: {error}"),
    }
}

/// A case's setup, which must not raise.
fn must<B: Backend>(interp: &Interpreter<B>, program: &str) {
    if let Err(error) = interp.eval_program_owned(program) {
        panic!("setup failed: {error}\n{program}");
    }
}

/// The cases on one pair: A is made by `new_a`, and B and C by `new_b`.
fn pair<A: Backend, B: Backend>(
    new_a: fn() -> Interpreter<A>,
    new_b: fn() -> Interpreter<B>,
    pair: &str,
    dir: &Path,
) {
    // 1. A installs a string port as its current output port, by the
    // one-argument call; B's display reaches standard output.
    let a = new_a();
    let b = new_b();
    must(
        &a,
        "(import (scheme base) (scheme write) (scheme read))
         (define s (open-output-string)) (current-output-port s)",
    );
    must(
        &b,
        &format!(
            "(import (scheme base) (scheme write)) (display \"written by B {pair}\") (newline)"
        ),
    );
    report("output", pair, &run(&a, "(get-output-string s)"));

    // 2. A installs a string port as its current input port; B's read reads
    // standard input, and A's reads the string port.
    must(&a, "(current-input-port (open-input-string \"(from A)\"))");
    report("input", pair, &run(&b, "(import (scheme read)) (read)"));
    report("own-input", pair, &run(&a, "(read)"));

    // 3. C, made after A's redirection, starts with standard ports of its
    // own; A's own display still lands in its string port.
    let c = new_b();
    must(
        &c,
        &format!("(import (scheme base) (scheme write)) (display \"and by C {pair}\") (newline)"),
    );
    report("later", pair, &run(&a, "(get-output-string s)"));
    report(
        "own-output",
        pair,
        &run(&a, "(display \"by A\") (get-output-string s)"),
    );

    // 4. A's redirection is a parameterize its body leaves by an uncaught
    // error, after writing into the port. What A writes afterwards is
    // #776's; B's display reaches standard output.
    let a = new_a();
    let b = new_b();
    must(
        &a,
        "(import (scheme base) (scheme write)) (define s (open-output-string))",
    );
    let raised = run(
        &a,
        "(parameterize ((current-output-port s)) (display \"inside\") (car 5))",
    );
    report("raised", pair, &raised.starts_with("error").to_string());
    must(
        &b,
        &format!(
            "(import (scheme base) (scheme write)) (display \"after the raise {pair}\") (newline)"
        ),
    );
    report("after-raise", pair, &run(&a, "(get-output-string s)"));

    // 5. A closes its current output port; B's display still works. And a
    // current-port read still makes a new port object each call, so `eq?`
    // answers #f, until #608 gives each port one object.
    let a = new_a();
    let b = new_b();
    report(
        "closed",
        pair,
        &run(
            &a,
            "(import (scheme base)) (close-port (current-output-port)) 'closed",
        ),
    );
    report(
        "after-close",
        pair,
        &run(
            &b,
            &format!(
                "(import (scheme base) (scheme write)) (display \"after the close {pair}\") (newline) 'written"
            ),
        ),
    );
    report(
        "identity",
        pair,
        &run(&b, "(eq? (current-output-port) (current-output-port))"),
    );

    // 6. A installs a file port as its current output port, writes into it
    // with no port argument, and is dropped. The host keeps A's heap past
    // the drop, as a host may, so the port is let go of only because the
    // teardown the drop runs puts A's standard ports back: the file is read
    // here, on the same thread, while the heap lives.
    let path = dir.join(pair);
    let a = new_a();
    must(
        &a,
        &format!(
            "(import (scheme base) (scheme file) (scheme write))
             (current-output-port (open-output-file {path:?}))
             (display \"left current {pair}\")"
        ),
    );
    let heap = Rc::clone(a.heap());
    drop(a);
    let written = std::fs::read_to_string(&path).unwrap_or_default();
    report("dropped", pair, &format!("{written:?}"));
    drop(heap);

    // 7. A port-less flush-output-port flushes A's current output port: a
    // file port's buffer reaches the file while the port stays open.
    let path = dir.join(format!("{pair}.flushed"));
    let a = new_a();
    must(
        &a,
        &format!(
            "(import (scheme base) (scheme file) (scheme write))
             (current-output-port (open-output-file {path:?}))
             (display \"flushed {pair}\")
             (flush-output-port)"
        ),
    );
    let written = std::fs::read_to_string(&path).unwrap_or_default();
    report("flushed", pair, &format!("{written:?}"));
}

#[test]
fn two_interpreters_on_one_thread_keep_their_own_current_ports() {
    if let Some(dir) = std::env::var_os(HOST) {
        let dir = Path::new(&dir);
        // Off the line libtest opens with the test's name.
        println!();
        pair(vm_interpreter, vm_interpreter, "vm+vm", dir);
        pair(
            tree_walker_interpreter,
            tree_walker_interpreter,
            "tw+tw",
            dir,
        );
        pair(vm_interpreter, tree_walker_interpreter, "vm+tw", dir);
        pair(tree_walker_interpreter, vm_interpreter, "tw+vm", dir);
        return;
    }
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut host = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .env(HOST, dir.path())
        .args([
            "--exact",
            "two_interpreters_on_one_thread_keep_their_own_current_ports",
            "--test-threads=1",
            "--nocapture",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run the host");
    {
        use std::io::Write;
        let mut stdin = host.stdin.take().expect("the host's standard input");
        stdin
            .write_all("(from stdin)\n".repeat(PAIRS.len()).as_bytes())
            .expect("write the host's standard input");
    }
    let host = host.wait_with_output().expect("wait for the host");
    let stdout = String::from_utf8_lossy(&host.stdout);
    assert!(
        host.status.success(),
        "the host failed: {}\n{stdout}{}",
        host.status,
        String::from_utf8_lossy(&host.stderr)
    );
    let reports: BTreeMap<(String, String), String> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("REPORT "))
        .map(|line| {
            let mut words = line.splitn(3, ' ');
            let case = words.next().unwrap_or_default().to_string();
            let pair = words.next().unwrap_or_default().to_string();
            ((case, pair), words.next().unwrap_or_default().to_string())
        })
        .collect();
    let lines: Vec<&str> = stdout.lines().collect();
    for pair in PAIRS {
        let answer = |case: &str| {
            reports
                .get(&(case.to_string(), pair.to_string()))
                .unwrap_or_else(|| panic!("{pair}: no report of {case}\n{stdout}"))
                .as_str()
        };
        let on_stdout = |text: String| {
            assert!(
                lines.contains(&text.as_str()),
                "{pair}: {text:?} is not on standard output\n{stdout}"
            );
        };
        on_stdout(format!("written by B {pair}"));
        assert_eq!(answer("output"), "\"\"", "{pair}: A's string port");
        assert_eq!(answer("input"), "(from stdin)", "{pair}: B's read");
        assert_eq!(answer("own-input"), "(from A)", "{pair}: A's read");
        on_stdout(format!("and by C {pair}"));
        assert_eq!(answer("later"), "\"\"", "{pair}: A's string port after C");
        assert_eq!(answer("own-output"), "\"by A\"", "{pair}: A's own display");
        assert_eq!(answer("raised"), "true", "{pair}: A's parameterize raises");
        on_stdout(format!("after the raise {pair}"));
        assert_eq!(
            answer("after-raise"),
            "\"inside\"",
            "{pair}: A's port after B's display"
        );
        assert_eq!(answer("closed"), "closed", "{pair}: A closes its port");
        assert_eq!(answer("after-close"), "written", "{pair}: B's display");
        on_stdout(format!("after the close {pair}"));
        assert_eq!(answer("identity"), "#f", "{pair}: two current-port reads");
        assert_eq!(
            answer("dropped"),
            format!("{:?}", format!("left current {pair}")),
            "{pair}: the file port A left current"
        );
        assert_eq!(
            answer("flushed"),
            format!("{:?}", format!("flushed {pair}")),
            "{pair}: a port-less flush"
        );
    }
}
