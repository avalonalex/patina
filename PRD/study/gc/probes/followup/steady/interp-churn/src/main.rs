//! Create and drop interpreters in a loop; print RSS after each batch.
use patina_interpreter::VmInterpreter;
use std::process::Command;

fn rss_mib() -> f64 {
    let pid = std::process::id().to_string();
    let out = Command::new("ps").args(["-o", "rss=", "-p", &pid]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().unwrap_or(0.0) / 1024.0
}

fn main() {
    let n: usize = std::env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(200);
    let batch = n / 8;
    println!("start rss={:.1}", rss_mib());
    for i in 1..=n {
        let interp = VmInterpreter::new_vm();
        let (r, _) = interp.eval_program_with_source_name(
            "(import (scheme base)) (define (f x) (* x 2)) (f 21)", "churn.scm");
        assert!(r.is_ok());
        drop(interp);
        if i % batch == 0 { println!("after {i} interpreters rss={:.1} MiB", rss_mib()); }
    }
}
