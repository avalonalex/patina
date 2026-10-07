//! The GC census (#651) on both backends, through the binary.
//!
//! The census is compiled in only with `patina-core`'s `gc-census` feature,
//! so most of this file measures only when the binary has it:
//!
//! ```text
//! cargo test -p patina-repl --test gc_census --features patina-core/gc-census
//! ```
//!
//! Without the feature, the one test that runs pins what the shipped build
//! does with the census's variables: nothing.

mod common;

use common::{BOTH_BACKENDS, patina_command};
use patina_core::census::GC_CENSUS;
use std::collections::HashMap;
use tempfile::TempDir;

/// 100,000 iterations, each allocating a pair and a flonum, storing a fixnum
/// with `set-car!` and the pair with `vector-set!` into a vector the program
/// keeps: so at least 100,000 of each, and nothing the loop allocated
/// survives longer than eight iterations.
const CHURN: &str = "(import (scheme base) (scheme write))
(define keep (make-vector 8 #f))
(define (churn n)
  (when (> n 0)
    (let ((p (cons n (* 1.5 n))))
      (set-car! p (+ n 1))
      (vector-set! keep (modulo n 8) p))
    (churn (- n 1))))
(churn 100000)
(display \"done\")
";

/// What one census run wrote: its summary's numbers and lists, its sites,
/// and its log's lines.
struct Census {
    numbers: HashMap<String, u64>,
    lists: HashMap<String, Vec<u64>>,
    sites: HashMap<String, HashMap<String, u64>>,
    log: Vec<String>,
}

impl Census {
    fn list(&self, key: &str) -> &[u64] {
        self.lists
            .get(key)
            .unwrap_or_else(|| panic!("no {key} in the summary"))
    }

    fn site(&self, name: &str, key: &str) -> u64 {
        self.sites
            .get(name)
            .and_then(|site| site.get(key))
            .copied()
            .unwrap_or(0)
    }
}

fn run(backend: &[&str], program: &str, env: &[(&str, &str)]) -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("program.scm"), program).unwrap();
    let mut args = backend.to_vec();
    args.push("program.scm");
    let mut command = patina_command(dir.path(), &args, env);
    for var in ["PATINA_GC", "PATINA_GC_STRESS", "PATINA_GC_ZEAL"] {
        command.env_remove(var);
    }
    for (name, value) in env {
        command.env(name, value);
    }
    let output = command.output().expect("spawn patina");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "patina {args:?} failed\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (dir, stdout)
}

/// Run `program` with the census on, its summary and log in the run's
/// directory.
fn census(backend: &[&str], program: &str, env: &[(&str, &str)]) -> Census {
    let probe = TempDir::new().unwrap();
    let out = probe.path().join("census.txt");
    let log = probe.path().join("census.csv");
    let mut vars = vec![
        ("PATINA_GC_CENSUS", "1"),
        ("PATINA_GC_CENSUS_OUT", out.to_str().unwrap()),
        ("PATINA_GC_CENSUS_LOG", log.to_str().unwrap()),
    ];
    vars.extend_from_slice(env);
    let (_dir, stdout) = run(backend, program, &vars);
    assert!(stdout.contains("done"), "{backend:?}: {stdout}");
    let text = std::fs::read_to_string(&out).expect("the census wrote its summary");
    let mut census = Census {
        numbers: HashMap::new(),
        lists: HashMap::new(),
        sites: HashMap::new(),
        log: std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect(),
    };
    for line in text.lines() {
        let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
        if key == "site" {
            let (name, fields) = rest.split_once(' ').unwrap();
            let words: Vec<&str> = fields.split(' ').collect();
            let counts = words
                .chunks(2)
                .filter_map(|kv| Some((kv[0].to_owned(), kv.get(1)?.parse().ok()?)))
                .collect();
            census.sites.insert(name.to_owned(), counts);
        } else if let Some(list) = rest.strip_prefix('[').and_then(|r| r.split(']').next()) {
            let values = list
                .split(", ")
                .filter(|v| !v.is_empty())
                .map(|v| v.parse().unwrap())
                .collect();
            census.lists.insert(key.to_owned(), values);
        } else {
            let words: Vec<&str> = line.split(' ').collect();
            for kv in words.chunks(2) {
                if let [k, v] = kv
                    && let Ok(v) = v.parse()
                {
                    census.numbers.insert((*k).to_owned(), v);
                }
            }
        }
    }
    census
}

#[test]
fn without_the_feature_the_variables_change_nothing() {
    if GC_CENSUS {
        return;
    }
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("census.txt");
    let heaps = dir.path().join("heaps.log");
    for backend in BOTH_BACKENDS {
        let (_run, stdout) = run(
            backend,
            CHURN,
            &[
                ("PATINA_GC_CENSUS", "1"),
                ("PATINA_GC_CENSUS_OUT", out.to_str().unwrap()),
                ("PATINA_GC_CENSUS_HEAPS", heaps.to_str().unwrap()),
            ],
        );
        assert_eq!(stdout, "done", "{backend:?}");
        assert!(!out.exists() && !heaps.exists(), "{backend:?}");
    }
}

#[test]
fn the_census_counts_allocations_by_kind_and_stores_by_site() {
    if !GC_CENSUS {
        return;
    }
    for backend in BOTH_BACKENDS {
        let c = census(backend, CHURN, &[]);
        let total = c.numbers["alloc_total"];
        assert_eq!(
            c.list("alloc_by_arena").iter().sum::<u64>(),
            total,
            "{backend:?}"
        );
        assert_eq!(
            c.list("size_hist").iter().sum::<u64>(),
            total,
            "{backend:?}"
        );
        assert!(c.list("alloc_by_arena")[0] >= 100_000, "{backend:?}: pairs");
        assert!(c.list("obj_variant")[2] >= 100_000, "{backend:?}: flonums");
        // Each pair is two words in both layouts; each flonum a header and
        // a word in the headered one.
        assert!(c.list("bytes_by_arena")[0] == 16 * c.list("alloc_by_arena")[0]);
        assert_eq!(
            c.list("obj_variant_bytes")[2],
            16 * c.list("obj_variant")[2]
        );
        // set-car! stores a fixnum: an immediate, which a barrier's filter
        // drops. vector-set! stores the pair: the VM writes it inline.
        assert!(c.site("set_car", "total") >= 100_000, "{backend:?}");
        assert!(c.site("set_car", "imm") >= 100_000, "{backend:?}");
        let vector_stores = c.site("vector_set", "heapval") + c.site("vm_vector_set", "heapval");
        assert!(vector_stores >= 100_000, "{backend:?}: {vector_stores}");
        // A collection per log line, after the header.
        assert_eq!(
            c.log.len() as u64,
            c.numbers["collections"] + 1,
            "{backend:?}"
        );
    }
}

/// Under a 16 K-allocation nursery the loop's garbage dies young: its pairs
/// and flonums survive their first collection at a few per cent at most,
/// while a list the program keeps survives whole.
#[test]
fn survival_is_measured_per_collection_interval() {
    if !GC_CENSUS {
        return;
    }
    let kept = "(import (scheme base) (scheme write))
(define (build n acc) (if (= n 0) acc (build (- n 1) (cons n acc))))
(define kept (build 200000 '()))
(display (if (= (length kept) 200000) \"done\" \"wrong\"))
";
    for backend in BOTH_BACKENDS {
        let stress = [("PATINA_GC_STRESS", "16384")];
        let churn = census(backend, CHURN, &stress);
        assert!(churn.numbers["collections"] >= 10, "{backend:?}");
        let (allocated, survived) = (
            churn.list("young_alloc_class1"),
            churn.list("young_surv_class1"),
        );
        // pair, flonum
        for class in [0, 3] {
            assert!(
                survived[class] * 20 <= allocated[class],
                "{backend:?}: class {class}: {survived:?} of {allocated:?}"
            );
        }
        let keep = census(backend, kept, &stress);
        let (allocated, survived) = (
            keep.list("young_alloc_class1"),
            keep.list("young_surv_class1"),
        );
        assert!(
            survived[0] * 10 >= allocated[0] * 9,
            "{backend:?}: pairs {survived:?} of {allocated:?}"
        );
    }
}

#[test]
fn heaps_are_counted_per_process() {
    if !GC_CENSUS {
        return;
    }
    let dir = TempDir::new().unwrap();
    let heaps = dir.path().join("heaps.log");
    for backend in BOTH_BACKENDS {
        run(
            backend,
            CHURN,
            &[("PATINA_GC_CENSUS_HEAPS", heaps.to_str().unwrap())],
        );
    }
    let log = std::fs::read_to_string(&heaps).unwrap();
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        ["patina total=1 max_live=1 live_at_exit=1"; 2],
        "{log}"
    );
}
