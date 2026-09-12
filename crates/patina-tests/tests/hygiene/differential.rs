//! Manual four-implementation sweep. Findings remain failures until investigated;
//! neither agreement between errors nor an oracle majority establishes an answer.
use super::generator::VARIANTS;
use super::generator::extended::{self, AXES, ExtendedCase, Source};
use super::oracles::{IMPLEMENTATIONS, Kind, Oracles, Outcome};
use super::shrinker;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SUITE_SECONDS: u64 = 1800;
const SHRINK_ATTEMPTS: usize = 32;

// Equality classes preserve which backend/variant pairs disagree without
// requiring incidental numeric spellings to survive shrinking.
fn signature(outcomes: &[Outcome]) -> Vec<(Kind, usize)> {
    outcomes
        .iter()
        .enumerate()
        .map(|(index, outcome)| {
            (
                outcome.kind,
                if outcome.kind == Kind::Value {
                    outcomes[..=index]
                        .iter()
                        .position(|other| other.kind == Kind::Value && other.value == outcome.value)
                        .unwrap()
                } else {
                    0
                },
            )
        })
        .collect()
}

fn agrees(outcomes: &[Outcome]) -> bool {
    !outcomes.is_empty()
        && outcomes
            .iter()
            .all(|o| o.kind == Kind::Value && o.value == outcomes[0].value)
}

fn probe(oracles: &Oracles, case: &ExtendedCase, dir: &Path, deadline: Instant) -> Vec<Outcome> {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("case.txt"), format!("{case:#?}\n")).unwrap();
    case.sources()
        .iter()
        .zip(VARIANTS)
        .flat_map(|(source, variant)| oracles.run(source, &dir.join(variant), deadline))
        .collect()
}

fn describe(outcomes: &[Outcome]) -> String {
    outcomes
        .iter()
        .enumerate()
        .map(|(index, outcome)| {
            format!(
                "{} / {}: {:?}: {}\n",
                VARIANTS[index / 4],
                IMPLEMENTATIONS[index % 4],
                outcome.kind,
                outcome.value
            )
        })
        .collect()
}

#[test]
#[ignore = "manual lane; requires Chibi and Racket with r7rs-lib; use scripts/run_hygiene_differential.sh"]
fn generated_programs_match_external_oracles() {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(SUITE_SECONDS);
    let seed: u64 = std::env::var("H3_SEED")
        .unwrap_or_else(|_| "286".into())
        .parse()
        .expect("numeric H3_SEED");
    let root = PathBuf::from(std::env::var_os("H3_OUTPUT").expect("H3_OUTPUT from script"));
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let oracles = Oracles::configured();
    let versions = oracles.versions(&root, deadline).unwrap_or_else(|failure| {
        fs::write(root.join("preflight-failure.txt"), &failure).unwrap();
        panic!("required implementation unavailable: {failure}");
    });
    fs::write(root.join("versions.txt"), &versions).unwrap();
    eprintln!("H3 versions:\n{versions}");
    // A present command is insufficient: all four must run this language and
    // its explicit output protocol before any generated case counts.
    let smoke = Source { main: "(import (scheme base) (scheme write))\n(display \"H3-VALUE \") (write '(1 x)) (newline)\n".into(), library: None };
    let smoke = oracles.run(&smoke, &root.join("preflight"), deadline);
    assert!(
        smoke
            .iter()
            .all(|o| o.kind == Kind::Value && o.value == "(1 x)"),
        "required R7RS oracle preflight failed: {smoke:?}"
    );
    let library_smoke = Source {
        main: "(import (scheme base) (scheme write) (h3 generated))\n(display \"H3-VALUE \") (write (h3-smoke)) (newline)\n".into(),
        library: Some("(define-library (h3 generated) (export h3-smoke) (import (scheme base)) (begin (define (h3-smoke) '(1 x))))\n".into()),
    };
    let library_smoke = oracles.run(&library_smoke, &root.join("preflight-library"), deadline);
    assert!(
        library_smoke
            .iter()
            .all(|o| o.kind == Kind::Value && o.value == "(1 x)"),
        "required library adapter preflight failed: {library_smoke:?}"
    );
    let historical = std::env::var_os("H3_HISTORICAL").is_some();
    let all_cases = if historical {
        vec![ExtendedCase::historical()]
    } else {
        extended::sweep(seed)
    };
    let selected = std::env::var("H3_CASE")
        .ok()
        .map(|v| v.parse::<usize>().expect("numeric H3_CASE"));
    if let Some(index) = selected {
        assert!(
            index < all_cases.len(),
            "case index outside this seed's fixed sweep"
        );
    }
    let run_seconds = super::oracles::RUN_TIMEOUT.as_secs();
    fs::write(root.join("budget.txt"), format!("seed-base={seed}\ncases={}\nselected={selected:?}\nhistorical={historical}\nsuite-seconds={SUITE_SECONDS}\nrun-seconds={run_seconds}\nshrink-attempts={SHRINK_ATTEMPTS}\n", all_cases.len())).unwrap();
    let mut counts = [[0usize; 5]; 8]; // generated, accepted, compared, excluded, disagreements
    let mut run_counts = [0usize; 6];
    let mut failures = 0;
    for (index, case) in all_cases.into_iter().enumerate() {
        if selected.is_some_and(|s| s != index) {
            continue;
        }
        let axis_index = AXES.iter().position(|a| *a == case.axis).unwrap();
        counts[axis_index][0] += 1;
        let dir = root.join(format!(
            "{index:03}-{}-{}",
            case.axis.name(),
            case.base.seed
        ));
        fs::create_dir_all(&dir).unwrap();
        if let Some(reason) = case.exclusion() {
            counts[axis_index][3] += 1;
            fs::write(dir.join("excluded.txt"), format!("{case:#?}\n{reason}\n")).unwrap();
            continue;
        }
        eprintln!(
            "H3 case={index} axis={} seed={} {:?}/{:?}/{:?} depth={} width={}",
            case.axis.name(),
            case.base.seed,
            case.base.binder,
            case.base.site,
            case.base.action,
            case.depth,
            case.width
        );
        let initial = probe(&oracles, &case, &dir.join("initial"), deadline);
        for o in &initial {
            run_counts[o.kind as usize] += 1;
        }
        if initial.iter().all(|o| o.kind == Kind::Value) {
            counts[axis_index][1] += 1;
            counts[axis_index][2] += 1;
        }
        if !agrees(&initial) {
            failures += 1;
            counts[axis_index][4] += 1;
            let original_signature = signature(&initial);
            let mut attempt = 0;
            let (minimal, minimized, attempts) = shrinker::minimize(
                case,
                initial.clone(),
                SHRINK_ATTEMPTS,
                deadline,
                ExtendedCase::reductions,
                |candidate| {
                    attempt += 1;
                    let outcomes = probe(
                        &oracles,
                        candidate,
                        &dir.join(format!("attempt-{attempt:02}")),
                        deadline,
                    );
                    (signature(&outcomes) == original_signature).then_some(outcomes)
                },
            );
            let retained = dir.join("minimized");
            fs::create_dir_all(&retained).unwrap();
            fs::write(retained.join("case.txt"), format!("{minimal:#?}\n")).unwrap();
            for (source, variant) in minimal.sources().iter().zip(VARIANTS) {
                let dest = retained.join(variant);
                fs::create_dir_all(&dest).unwrap();
                fs::write(dest.join("main.scm"), &source.main).unwrap();
                if let Some(library) = &source.library {
                    fs::create_dir_all(dest.join("h3")).unwrap();
                    fs::write(dest.join("h3/generated.sld"), library).unwrap();
                    fs::write(
                        dest.join("h3/generated.rkt"),
                        "#lang r7rs\n(include \"generated.sld\")\n",
                    )
                    .unwrap();
                }
            }
            let report = format!(
                "classification=needs-investigation\nshrink-attempts={attempts}/{SHRINK_ATTEMPTS}\nsignature={original_signature:?}\ninitial:\n{}\nminimized:\n{}\n{versions}",
                describe(&initial),
                describe(&minimized)
            );
            fs::write(dir.join("finding.txt"), &report).unwrap();
            eprintln!(
                "H3 retained finding {} (shrunk in {attempts} attempts)",
                dir.display()
            );
        }
    }
    let mut summary = "axis\tgenerated\taccepted\tcompared\texcluded\tdisagreements\n".to_string();
    for (axis, row) in AXES.into_iter().zip(counts) {
        summary.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            axis.name(),
            row[0],
            row[1],
            row[2],
            row[3],
            row[4]
        ));
    }
    fs::write(root.join("summary.tsv"), &summary).unwrap();
    fs::write(root.join("outcomes.txt"), format!("Initial runs only; shrinking and preflight excluded:\nValue/Rejected/Unsupported/Timeout/Crash/Protocol={run_counts:?}\nelapsed={:?}\n", started.elapsed())).unwrap();
    eprintln!(
        "{summary}H3 outcome counts={run_counts:?}, elapsed={:?}, retained={}",
        started.elapsed(),
        root.display()
    );
    if !historical && selected.is_none() {
        assert!(
            counts.iter().all(|row| row[2] > 0),
            "an advertised axis has no fully compared case; see retained outcomes"
        );
    }
    assert_eq!(
        failures,
        0,
        "H3 disagreements require investigation; retained in {}",
        root.display()
    );
}

#[test]
fn rejected_or_disagreeing_results_cannot_be_a_clean_comparison() {
    let value = |v: &str| Outcome {
        kind: Kind::Value,
        value: v.into(),
    };
    assert!(agrees(&[value("1"), value("1")]));
    assert!(!agrees(&[value("1"), value("2")]));
    for kind in [
        Kind::Rejected,
        Kind::Unsupported,
        Kind::Timeout,
        Kind::Crash,
        Kind::Protocol,
    ] {
        assert!(!agrees(
            &[Outcome {
                kind,
                value: "same error".into()
            }; 1]
        ));
    }
    assert_eq!(
        signature(&[value("1"), value("2"), value("1")]),
        signature(&[value("5"), value("6"), value("5")])
    );
    assert_ne!(
        signature(&[value("1"), value("2")]),
        signature(&[value("1"), value("1")])
    );
}
