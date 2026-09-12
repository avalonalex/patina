//! Positive programs run in a child test process. A shared frontend failure
//! cannot become a passing comparison of two errors, crashes or timeouts.
use std::fs::{self, File};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const WORKER_DIR: &str = "PATINA_H1_WORKER_DIR";
const WORKER_BACKEND: &str = "PATINA_H1_WORKER_BACKEND";
pub const BACKENDS: [&str; 2] = ["vm", "tree-walker"];
pub const CASE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rejected,
    Crash,
    Timeout,
    Protocol,
    Mismatch,
}

#[derive(Clone, Debug)]
pub struct Failure {
    pub kind: Kind,
    pub variant: usize,
    pub detail: String,
}

pub fn run(backend: &str, sources: &[String], budget: Duration) -> Result<Vec<String>, Failure> {
    if budget.is_zero() {
        return Err(Failure {
            kind: Kind::Timeout,
            variant: 0,
            detail: "H1 total time budget exhausted".into(),
        });
    }
    let dir = tempfile::tempdir().expect("H1 scratch directory");
    for (i, source) in sources.iter().enumerate() {
        fs::write(dir.path().join(format!("{i}.scm")), source).expect("write H1 program");
    }
    fs::write(dir.path().join("count"), sources.len().to_string()).unwrap();
    let log = File::create(dir.path().join("worker.log")).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "hygiene_backend_worker", "--nocapture"])
        .env(WORKER_DIR, dir.path())
        .env(WORKER_BACKEND, backend)
        .stdin(Stdio::null())
        // Files avoid pipe-buffer deadlock if a failing runtime emits a trace.
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .expect("start H1 worker");
    let started = Instant::now();
    let budget = budget.min(CASE_TIMEOUT);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if started.elapsed() >= budget {
            let _ = child.kill();
            child.wait().expect("reap timed-out H1 worker");
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let active = || {
        fs::read_to_string(dir.path().join("active"))
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    };
    let log = || fs::read_to_string(dir.path().join("worker.log")).unwrap();
    match status {
        None => {
            return Err(Failure {
                kind: Kind::Timeout,
                variant: active(),
                detail: format!("exceeded {budget:?}; {}", log()),
            });
        }
        Some(status) if !status.success() => {
            return Err(Failure {
                kind: Kind::Crash,
                variant: active(),
                detail: format!("exit={status}; {}", log()),
            });
        }
        Some(_) => {}
    }
    let mut results = Vec::new();
    for index in 0..sources.len() {
        let output = fs::read_to_string(dir.path().join(format!("{index}.out")));
        match output {
            Ok(text) if text.starts_with("value\n") => results.push(text[6..].to_string()),
            Ok(text) if text.starts_with("error\n") => {
                return Err(Failure {
                    kind: Kind::Rejected,
                    variant: index,
                    detail: text[6..].to_string(),
                });
            }
            other => {
                return Err(Failure {
                    kind: Kind::Protocol,
                    variant: index,
                    detail: format!("missing or malformed worker answer: {other:?}; {}", log()),
                });
            }
        }
    }
    Ok(results)
}

pub fn worker() {
    let Some(dir) = std::env::var_os(WORKER_DIR) else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let backend = std::env::var(WORKER_BACKEND).unwrap();
    let eval = match backend.as_str() {
        "vm" => crate::common::try_eval_program_vm,
        "tree-walker" => crate::common::try_eval_program_tree_walker,
        // Used solely to exercise the parent's crashed-process handling.
        "test-exit" => std::process::exit(23),
        _ => panic!("unknown H1 backend {backend}"),
    };
    let count: usize = fs::read_to_string(dir.join("count"))
        .unwrap()
        .parse()
        .unwrap();
    for index in 0..count {
        fs::write(dir.join("active"), index.to_string()).unwrap();
        let source = fs::read_to_string(dir.join(format!("{index}.scm"))).unwrap();
        let answer = match eval(&source) {
            Ok(value) => format!("value\n{value}"),
            Err(error) => format!("error\n{error}"),
        };
        fs::write(dir.join(format!("{index}.out")), answer).unwrap();
    }
}
