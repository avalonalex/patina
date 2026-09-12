//! Track H1: a bounded binding-aware metamorphic gate, not a proof of hygiene.
//! 7 binders × 2 definition sites × 2 actions × 4 deterministic samples.
//! Every positive case runs all five variants on both backends; no rejected
//! program, process crash or timeout is counted as equivalent successful output.
//! The fixed cross-product and seed stream are documented in Track H's PRD.
mod common;
#[path = "hygiene/generator.rs"]
mod generator;
#[path = "hygiene/runner.rs"]
mod runner;

use generator::{Action, BINDERS, Binder, Case, Site, VARIANTS};
use runner::{BACKENDS, Failure, Kind};
use std::time::{Duration, Instant};

const SAMPLES: usize = 4;
const SUITE_BUDGET: Duration = Duration::from_secs(120);
const SHRINK_BUDGET: usize = 32;

#[test]
fn hygiene_backend_worker() {
    runner::worker();
}

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

fn compare(values: &[String]) -> Result<(), Failure> {
    for index in 1..values.len() {
        if values[0] != values[index] {
            return Err(Failure {
                kind: Kind::Mismatch,
                variant: index,
                detail: format!("original={} transformed={}", values[0], values[index]),
            });
        }
    }
    Ok(())
}

fn probe(case: &Case, backend: &str, deadline: Instant) -> Result<Vec<String>, Failure> {
    let values = runner::run(backend, &case.sources(), remaining(deadline))?;
    compare(&values)?;
    Ok(values)
}

// Greedy reductions of the *binding specification*, followed by re-emission
// of every variant. Keep the same backend, failure class and transformation.
// Neither a new error nor a now-unobservable capture can replace the witness.
fn shrink(
    mut case: Case,
    backend: &str,
    mut failure: Failure,
    deadline: Instant,
) -> (Case, Failure, usize) {
    let mut attempts = 0;
    loop {
        let mut accepted = None;
        for candidate in case.reductions() {
            if attempts >= SHRINK_BUDGET || remaining(deadline).is_zero() {
                return (case, failure, attempts);
            }
            attempts += 1;
            if let Err(next) = probe(&candidate, backend, deadline)
                && (next.kind, next.variant) == (failure.kind, failure.variant)
            {
                accepted = Some((candidate, next));
                break;
            }
        }
        match accepted {
            Some((next_case, next_failure)) => {
                case = next_case;
                failure = next_failure;
            }
            None => return (case, failure, attempts),
        }
    }
}

fn failure_report(case: &Case, backend: &str, failure: Failure, deadline: Instant) -> String {
    let initial = format!("{case:?}");
    let (minimal, failure, attempts) = shrink(case.clone(), backend, failure, deadline);
    let sources = minimal.sources();
    format!(
        "H1 backend={backend} transformation={} class={:?}\nseed={} samples/shape={SAMPLES} shrink-attempts={attempts}/{SHRINK_BUDGET}\ninitial={initial}\nminimized={minimal:?}\noutcome={}\noriginal:\n{}\ntransformed:\n{}",
        VARIANTS[failure.variant],
        failure.kind,
        case.seed,
        failure.detail,
        sources[0],
        sources[failure.variant]
    )
}

#[test]
fn renaming_only_the_local_binding_preserves_a_templates_global_reference() {
    let deadline = Instant::now() + SUITE_BUDGET;
    let case = Case::baseline(Binder::Let, Site::Outside, Action::Read);
    let sources = case.sources();
    let mut failures = Vec::new();
    for backend in BACKENDS {
        let values = match runner::run(backend, &sources, remaining(deadline)) {
            Ok(values) => values,
            Err(failure) => {
                failures.push(failure_report(&case, backend, failure, deadline));
                continue;
            }
        };
        eprintln!(
            "H1 seed=285 backend={backend}: original={}, local-rename={}, uniform={}",
            values[0], values[1], values[4]
        );
        // Deliberately keep the negative control: a uniform spelling permutation
        // also agrees on the *wrong* old answer. It cannot detect this capture.
        assert_eq!(values[0], values[4], "uniform spelling permutation");
        if let Err(failure) = compare(&values) {
            failures.push(failure_report(&case, backend, failure, deadline));
        } else {
            assert_eq!(
                values[0], "(1 1 x)",
                "historical seed, including unchanged quoted data"
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn generated_capture_case_retains_its_failure_while_shrinking() {
    let deadline = Instant::now() + SUITE_BUDGET;
    let mut case = Case::generated(Binder::Let, Site::Outside, Action::Read, 285);
    case.padding = 2;
    let mut failures = Vec::new();
    for backend in BACKENDS {
        if let Err(failure) = probe(&case, backend, deadline) {
            failures.push(failure_report(&case, backend, failure, deadline));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn generated_matrix_programs_preserve_bindings_and_ordered_effects() {
    let deadline = Instant::now() + SUITE_BUDGET;
    let mut total = 0;
    let mut evaluations = [0; 2];
    let mut effect_cases = [0; 2];
    let mut padding_cases = [0; 3];
    for (binder_index, binder) in BINDERS.into_iter().enumerate() {
        for (site_index, site) in [Site::Outside, Site::Inside].into_iter().enumerate() {
            for (action_index, action) in [Action::Read, Action::Write].into_iter().enumerate() {
                let mut completed = 0;
                for sample in 0..SAMPLES {
                    let shape = binder_index * 4 + site_index * 2 + action_index;
                    let case = if sample == 0 {
                        Case::baseline(binder, site, action)
                    } else {
                        Case::generated(
                            binder,
                            site,
                            action,
                            285 + (shape * SAMPLES + sample) as u64,
                        )
                    };
                    let mut answers = Vec::new();
                    for (backend_index, backend) in BACKENDS.into_iter().enumerate() {
                        match probe(&case, backend, deadline) {
                            Ok(values) => {
                                evaluations[backend_index] += values.len();
                                answers.push(values);
                            }
                            Err(failure) => panic!(
                                "completed cases={total} evaluations={evaluations:?}\n{}",
                                failure_report(&case, backend, failure, deadline)
                            ),
                        }
                    }
                    // Additional evidence only: each backend has already passed
                    // the binding-specific checks independently above.
                    assert_eq!(
                        answers[0],
                        answers[1],
                        "H1 backend disagreement: {case:?}\n{}",
                        case.sources().join("\n\n")
                    );
                    effect_cases[usize::from(case.effects)] += 1;
                    padding_cases[case.padding] += 1;
                    total += 1;
                    completed += 1;
                }
                assert_eq!(completed, SAMPLES);
                eprintln!(
                    "H1 {binder:?}/{site:?}/{action:?}: eligible={SAMPLES} executed={completed}, both backends, all 5 variants"
                );
            }
        }
    }
    let expected = BINDERS.len() * 2 * 2 * SAMPLES;
    assert_eq!(total, expected);
    assert_eq!(evaluations, [expected * VARIANTS.len(); 2]);
    assert_eq!(effect_cases, [28, 84]);
    assert!(padding_cases.iter().all(|count| *count > 0));
    eprintln!("H1 effect logging off/on={effect_cases:?}; padding depths 0/1/2={padding_cases:?}");
    eprintln!(
        "H1 seed-base=285: cases={total}, rename-local={total}/{total}, rename-global={total}/{total}, poison-shadow={total}/{total}, uniform={total}/{total}; evaluations vm={} tree-walker={}; rejected=0 timed-out=0 crashed=0; elapsed={:?}",
        evaluations[0],
        evaluations[1],
        Instant::now() + SUITE_BUDGET - deadline
    );
    eprintln!(
        "H1 excluded grammar: macro arguments, syntax-rules literals/ellipsis, generated macros, libraries, reflective symbols/eval, other binders, unspecified-order effects, intentional negative programs."
    );
}

#[test]
fn rejected_programs_crashes_and_timeouts_are_failures() {
    // Exercise actual process handling, not equivalent error-string comparison.
    let rejected = runner::run(
        "vm",
        &["h1-unbound-a".into(), "h1-unbound-b".into()],
        runner::CASE_TIMEOUT,
    )
    .unwrap_err();
    assert_eq!(rejected.kind, Kind::Rejected);
    let crash = runner::run("test-exit", &["1".into()], runner::CASE_TIMEOUT).unwrap_err();
    assert_eq!(crash.kind, Kind::Crash);
    let timeout = runner::run(
        "vm",
        &["(let loop () (loop))".into()],
        Duration::from_millis(200),
    )
    .unwrap_err();
    assert_eq!(timeout.kind, Kind::Timeout);
}
