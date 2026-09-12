//! The H3 transport executes the same main.scm and generated.sld on four
//! implementations. Racket's collection wrapper only includes generated.sld;
//! it does not translate Scheme or change the program's bindings.
use super::generator::extended::Source;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const IMPLEMENTATIONS: [&str; 4] = ["vm", "tree-walker", "chibi", "racket"];
pub const RUN_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Value,
    Rejected,
    Unsupported,
    Timeout,
    Crash,
    Protocol,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub kind: Kind,
    pub value: String,
}

pub struct Oracles {
    pub patina: PathBuf,
    pub chibi: PathBuf,
    pub racket: PathBuf,
}

impl Oracles {
    pub fn configured() -> Self {
        Self {
            patina: std::env::var_os("H3_PATINA")
                .expect("use scripts/run_hygiene_differential.sh")
                .into(),
            chibi: std::env::var_os("CHIBI")
                .unwrap_or_else(|| "chibi-scheme".into())
                .into(),
            racket: std::env::var_os("RACKET")
                .unwrap_or_else(|| "racket".into())
                .into(),
        }
    }

    pub fn versions(&self, dir: &Path, deadline: Instant) -> Result<String, String> {
        let mut result = String::new();
        for (label, exe, args) in [
            ("patina", &self.patina, vec!["--version"]),
            ("chibi", &self.chibi, vec!["-V"]),
            ("racket", &self.racket, vec!["--version"]),
        ] {
            let mut cmd = Command::new(exe);
            cmd.args(args);
            let run = execute(cmd, dir, label, deadline);
            result.push_str(&format!(
                "{label} executable={exe:?} status={:?}\n{}\n",
                run.kind, run.value
            ));
            if run.kind != Kind::Value {
                return Err(result);
            }
        }
        // Capture the language package revision too, not just Racket's VM.
        let mut cmd = Command::new(std::env::var_os("RACO").unwrap_or_else(|| "raco".into()));
        cmd.args(["pkg", "show", "--full-checksum", "r7rs-lib"]);
        let run = execute(cmd, dir, "r7rs-package", deadline);
        result.push_str(&format!(
            "r7rs package status={:?}\n{}\n",
            run.kind, run.value
        ));
        if run.kind != Kind::Value {
            return Err(result);
        }
        Ok(result)
    }

    pub fn run(&self, source: &Source, dir: &Path, deadline: Instant) -> Vec<Outcome> {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("main.scm"), &source.main).unwrap();
        if let Some(library) = &source.library {
            fs::create_dir_all(dir.join("h3")).unwrap();
            fs::write(dir.join("h3/generated.sld"), library).unwrap();
            fs::write(
                dir.join("h3/generated.rkt"),
                "#lang r7rs\n(include \"generated.sld\")\n",
            )
            .unwrap();
        }
        IMPLEMENTATIONS
            .iter()
            .map(|name| {
                let mut cmd = match *name {
                    "vm" | "tree-walker" => {
                        let mut cmd = Command::new(&self.patina);
                        if *name == "tree-walker" {
                            cmd.arg("--tree-walker");
                        }
                        cmd.arg("-I").arg(dir).arg(dir.join("main.scm"));
                        cmd
                    }
                    "chibi" => {
                        let mut cmd = Command::new(&self.chibi);
                        cmd.args(["-h", "2M/256M"])
                            .arg("-I")
                            .arg(dir)
                            .arg(dir.join("main.scm"));
                        cmd
                    }
                    "racket" => {
                        let mut cmd = Command::new(&self.racket);
                        cmd.arg("-S")
                            .arg(dir)
                            .args(["-I", "r7rs", "-f"])
                            .arg(dir.join("main.scm"));
                        cmd
                    }
                    _ => unreachable!(),
                };
                cmd.current_dir(dir);
                let mut result = execute(cmd, dir, name, deadline);
                if result.kind == Kind::Value {
                    // Only one explicit datum is allowed. No error text, missing
                    // result, partial output or repeated marker can pass as a value.
                    let output = fs::read_to_string(dir.join(format!("{name}.stdout"))).unwrap();
                    result = match output.trim().strip_prefix("H3-VALUE ") {
                        Some(value) if !value.is_empty() && !value.contains('\n') => Outcome {
                            kind: Kind::Value,
                            value: value.into(),
                        },
                        _ => Outcome {
                            kind: Kind::Protocol,
                            value: result.value,
                        },
                    };
                }
                fs::write(
                    dir.join(format!("{name}.outcome")),
                    format!("{:?}\n{}\n", result.kind, result.value),
                )
                .unwrap();
                result
            })
            .collect()
    }
}

fn execute(mut cmd: Command, dir: &Path, name: &str, deadline: Instant) -> Outcome {
    fs::write(dir.join(format!("{name}.command")), format!("{cmd:?}\n")).unwrap();
    let out_path = dir.join(format!("{name}.stdout"));
    let err_path = dir.join(format!("{name}.stderr"));
    let start = Instant::now();
    let stop = deadline.min(start + RUN_TIMEOUT);
    if start >= stop {
        return Outcome {
            kind: Kind::Timeout,
            value: "total budget exhausted before launch".into(),
        };
    }
    let spawn = cmd
        .stdin(Stdio::null())
        .stdout(File::create(&out_path).unwrap())
        .stderr(File::create(&err_path).unwrap())
        .spawn();
    let mut child = match spawn {
        Ok(child) => child,
        Err(error) => {
            return Outcome {
                kind: Kind::Unsupported,
                value: format!("could not launch {cmd:?}: {error}"),
            };
        }
    };
    let kind = loop {
        if let Some(status) = child.try_wait().unwrap() {
            fs::write(dir.join(format!("{name}.exit")), status.to_string()).unwrap();
            break if status.success() {
                Kind::Value
            } else if status.code().is_none() || status.code() == Some(101) {
                Kind::Crash
            } else {
                Kind::Rejected
            };
        }
        if Instant::now() >= stop {
            let _ = child.kill();
            child.wait().expect("reap timed-out oracle");
            break Kind::Timeout;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    Outcome {
        kind,
        value: format!(
            "{}{}",
            fs::read_to_string(out_path).unwrap(),
            fs::read_to_string(err_path).unwrap()
        ),
    }
}

#[cfg(unix)]
#[test]
fn process_failures_keep_their_distinct_outcomes() {
    let dir = tempfile::tempdir().unwrap();
    let deadline = Instant::now() + RUN_TIMEOUT;
    let absent = execute(
        Command::new(dir.path().join("absent-oracle")),
        dir.path(),
        "absent",
        deadline,
    );
    assert_eq!(absent.kind, Kind::Unsupported);
    let mut rejected = Command::new("sh");
    rejected.args(["-c", "exit 2"]);
    assert_eq!(
        execute(rejected, dir.path(), "rejected", deadline).kind,
        Kind::Rejected
    );
    let mut crash = Command::new("sh");
    crash.args(["-c", "kill -TERM $$"]);
    assert_eq!(
        execute(crash, dir.path(), "crash", deadline).kind,
        Kind::Crash
    );
    let mut hanging = Command::new("sh");
    hanging.args(["-c", "exec sleep 10"]);
    assert_eq!(
        execute(
            hanging,
            dir.path(),
            "timeout",
            Instant::now() + Duration::from_millis(100)
        )
        .kind,
        Kind::Timeout
    );
}
