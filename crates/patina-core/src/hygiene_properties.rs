//! Track H/H2, families 36, 38, 39. Bounded sampling, not a proof.
//! Kernel: ordered tables of 0..12 masks over six scopes (including empty
//! sets, duplicates and invisible candidates). Environments: 1..4 frames,
//! 0..6 insertions/frame, nonempty scope masks, optional plain `x`, both
//! name-visible and scope-only bindings, and values in 0..3 (deliberate ties).
//! Nonempty references exercise scoped resolution; plain access, aliases,
//! compiler/desugarer integration and GC are outside this domain.
//!
//! Frames are stored root first; insertions oldest first. Redefining an
//! identical scope set replaces its cell, retaining its insertion position.
//! The independent oracle uses integer set inclusion and binding identities.
//! Every write is checked against a snapshot of *all* generated bindings.
//! Replays use seed 284, 256 cases/property and at most 4096 shrink steps.
//! Named quarantines must fail (unexpected success fails the gate). The
//! unrestricted symmetry property partitions only the three named defect
//! predicates; every generated case runs and unexpected success is an error.

use super::*;
use crate::scope::ScopeId;
use crate::scope_resolve::resolve_index;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed, TestCaseError, TestError, TestRunner};

const SEED: u64 = 284;
const SENTINEL: i64 = 999;

fn runner() -> TestRunner {
    TestRunner::new(Config {
        cases: 256,
        max_shrink_iters: 4096,
        rng_seed: RngSeed::Fixed(SEED),
        // No machine-local seed files: failures print the minimized input,
        // and fixed seeds replay identically in the normal cargo test gate.
        failure_persistence: None,
        ..Config::default()
    })
}

fn check<S: Strategy>(strategy: S, test: impl Fn(S::Value) -> Result<(), TestCaseError>) {
    if let Err(error) = runner().run(&strategy, test) {
        panic!("H2 seed={SEED}, cases=256, shrink limit=4096: {error}");
    }
}

fn quarantine<S: Strategy>(
    name: &str,
    strategy: S,
    test: impl Fn(S::Value) -> Result<(), TestCaseError>,
) {
    // Exercise the entire bounded quarantine domain, not just the first
    // counterexample. A partial fix must fail on unexpected success too.
    if let Err(error) = runner().run(&strategy, |input| match test(input) {
        Err(TestCaseError::Fail(reason)) if reason.to_string().starts_with(name) => Ok(()),
        other => Err(TestCaseError::fail(format!(
            "{name}: unexpected outcome: {other:?}"
        ))),
    }) {
        panic!("{name}, seed={SEED}: {error}");
    }
    // Run the intended property unchanged to retain a minimized witness.
    match runner().run(&strategy, test) {
        Err(TestError::Fail(reason, minimal)) if reason.to_string().starts_with(name) => {
            eprintln!("QUARANTINE {name}, seed={SEED}, cases=256: {reason}; minimized={minimal:?}");
        }
        other => panic!("{name}: unexpected success or unrelated failure: {other:?}"),
    }
}

fn scopes(mask: u8) -> ScopeSet {
    let mut set = ScopeSet::new();
    for bit in 0..6 {
        if mask & (1 << bit) != 0 {
            set.add_scope(ScopeId(bit));
        }
    }
    set
}

// Independent specification: the first eligible set containing *all*
// eligible sets wins. No maximum-size heuristic or production scope helper.
fn oracle(reference: u8, masks: &[u8]) -> Result<Option<usize>, ()> {
    let eligible: Vec<_> = masks
        .iter()
        .enumerate()
        .filter(|(_, m)| **m & reference == **m)
        .collect();
    if eligible.is_empty() {
        return Ok(None);
    }
    eligible
        .iter()
        .find(|(_, m)| eligible.iter().all(|(_, n)| **n & **m == **n))
        .map(|(i, _)| Some(*i))
        .ok_or(())
}

#[test]
fn family39_ordered_tables_determinism_and_ties() {
    check(
        (0u8..64, prop::collection::vec(0u8..64, 0..13)),
        |(reference, masks)| {
            // Equal payloads cannot stand in for binding identities.
            let table: Vec<_> = masks.iter().map(|m| (scopes(*m), 0)).collect();
            let expected = oracle(reference, &masks);
            for _ in 0..2 {
                prop_assert_eq!(
                    resolve_index("x", &scopes(reference), &table).map_err(|_| ()),
                    expected
                );
            }
            Ok(())
        },
    );
}

#[test]
fn family39_chains_and_identical_scope_recency() {
    check(
        (0u8..64, prop::collection::vec(0u8..7, 0..13)),
        |(reference, levels)| {
            let masks: Vec<_> = levels.iter().map(|n| ((1u16 << n) - 1) as u8).collect();
            let table: Vec<_> = masks
                .iter()
                .enumerate()
                .map(|(i, m)| (scopes(*m), i))
                .collect();
            let expected = oracle(reference, &masks).expect("prefix sets form a chain");
            prop_assert_eq!(
                resolve_index("x", &scopes(reference), &table).unwrap(),
                expected
            );
            // A guaranteed tie on every case, even when the generated table is empty.
            let ties = [(scopes(reference), 10), (scopes(reference), 20)];
            prop_assert_eq!(
                resolve_index("x", &scopes(reference), &ties).unwrap(),
                Some(0)
            );
            Ok(())
        },
    );
}

#[derive(Clone, Debug)]
struct Frame {
    plain: Option<i64>,
    // (scope mask, value, name-visible). Nonempty scopes only.
    bindings: Vec<(u8, i64, bool)>,
}

fn frame() -> impl Strategy<Value = Frame> {
    (
        prop::option::of(0i64..3),
        prop::collection::vec((1u8..64, 0i64..3, any::<bool>()), 0..7),
    )
        .prop_map(|(plain, insertions)| {
            let mut bindings: Vec<(u8, i64, bool)> = Vec::new();
            for entry in insertions {
                if let Some(old) = bindings.iter_mut().find(|old| old.0 == entry.0) {
                    *old = entry;
                } else {
                    bindings.push(entry);
                }
            }
            Frame { plain, bindings }
        })
}

fn environments(frames: &[Frame]) -> Vec<Rc<Environment>> {
    let mut envs = Vec::new();
    for frame in frames {
        let env = Rc::new(match envs.last() {
            None => Environment::new(),
            Some(parent) => Environment::with_parent(Rc::clone(parent)),
        });
        if let Some(value) = frame.plain {
            env.define("x", TaggedValue::fixnum(value));
        }
        for &(mask, value, visible) in &frame.bindings {
            if visible {
                env.define_scoped_definition("x", scopes(mask), TaggedValue::fixnum(value));
            } else {
                env.define_with_scopes("x", scopes(mask), TaggedValue::fixnum(value));
            }
        }
        // Unrelated names must never change either.
        env.define("untouched", TaggedValue::fixnum(17));
        envs.push(env);
    }
    envs
}

// A cell's identity is (frame index, scope mask); zero denotes plain `x`.
fn target(frames: &[Frame], reference: u8) -> Result<Option<(usize, u8)>, ()> {
    let ordered: Vec<_> = frames
        .iter()
        .enumerate()
        .rev()
        .flat_map(|(i, f)| f.bindings.iter().rev().map(move |b| (i, b.0)))
        .collect();
    let masks: Vec<_> = ordered.iter().map(|(_, mask)| *mask).collect();
    if let Some(index) = oracle(reference, &masks)? {
        return Ok(Some(ordered[index]));
    }
    Ok(frames
        .iter()
        .enumerate()
        .rev()
        .find(|(_, f)| f.plain.is_some())
        .map(|(i, _)| (i, 0)))
}

fn snapshot(envs: &[Rc<Environment>]) -> Vec<(usize, u8, i64)> {
    let mut result = Vec::new();
    for (i, env) in envs.iter().enumerate() {
        if let Some(value) = env.bindings.borrow().get("x") {
            result.push((i, 0, value.as_fixnum().unwrap()));
        }
        if let Some(bindings) = env.scoped_bindings.borrow().get("x") {
            for binding in bindings {
                let mask = binding.scopes.iter().fold(0, |m, s| m | (1 << s.0));
                result.push((i, mask, binding.tagged_value.as_fixnum().unwrap()));
            }
        }
        assert_eq!(
            env.bindings
                .borrow()
                .get("untouched")
                .and_then(|value| value.as_fixnum()),
            Some(17)
        );
    }
    result
}

fn read_matches(
    frames: &[Frame],
    reference: u8,
    envs: &[Rc<Environment>],
) -> Result<(), TestCaseError> {
    let before = snapshot(envs);
    let expected = target(frames, reference).map(|id| {
        id.map(|(i, mask)| {
            before
                .iter()
                .find(|cell| cell.0 == i && cell.1 == mask)
                .unwrap()
                .2
        })
    });
    let read = envs
        .last()
        .unwrap()
        .get_with_scopes("x", &scopes(reference))
        .map(|value| value.map(|v| v.as_fixnum().unwrap()))
        .map_err(|_| ());
    prop_assert_eq!(read, expected, "read disagrees with independent oracle");
    Ok(())
}

fn symmetry(frames: &[Frame], reference: u8) -> Result<(), TestCaseError> {
    let envs = environments(frames);
    read_matches(frames, reference, &envs)?;
    let mut expected = snapshot(&envs);
    let target = target(frames, reference);
    if let Ok(Some((i, mask))) = target {
        expected
            .iter_mut()
            .find(|c| c.0 == i && c.1 == mask)
            .unwrap()
            .2 = SENTINEL;
    }
    let written = envs.last().unwrap().set_with_scopes(
        "x",
        &scopes(reference),
        TaggedValue::fixnum(SENTINEL),
    );
    let after = snapshot(&envs);
    let should_write = matches!(target, Ok(Some(_)));
    if written.is_ok() != should_write || after != expected {
        let kind = match target {
            Err(()) => "H2-A ambiguous write",
            Ok(None) => "H2-U unbound mutation",
            Ok(Some((_, 0))) => "H2-C plain fallback",
            Ok(Some(_)) => "H2-B binding identity",
        };
        return Err(TestCaseError::fail(format!(
            "{kind}: target={target:?}, result={written:?}, expected={expected:?}, actual={after:?}"
        )));
    }
    if should_write {
        // The other cells may have started with equal values. Reading back
        // the unique sentinel proves that the read reaches this same cell.
        prop_assert_eq!(
            envs.last()
                .unwrap()
                .get_with_scopes("x", &scopes(reference))
                .unwrap(),
            Some(TaggedValue::fixnum(SENTINEL))
        );
    }
    Ok(())
}

#[test]
fn family36_39_arbitrary_environment_reads() {
    check(
        (prop::collection::vec(frame(), 1..5), 1u8..64),
        |(frames, reference)| read_matches(&frames, reference, &environments(&frames)),
    );
}

#[test]
fn family38_nested_chain_binding_identity_with_equal_values() {
    // Runtime lexical nesting: scopes accumulate towards the leaf. Includes
    // empty frames, repeated scopes across frames, and unbound references.
    check(
        (
            prop::collection::vec(prop::collection::vec((1u8..7, 0i64..3), 0..7), 1..5),
            1u8..64,
        ),
        |(input, reference)| {
            let mut floor = 1;
            let frames: Vec<_> = input
                .into_iter()
                .map(|entries| {
                    let mut bindings = Vec::new();
                    for (level, value) in entries {
                        floor = floor.max(level);
                        let entry = (((1u16 << floor) - 1) as u8, value, false);
                        if let Some(old) = bindings
                            .last_mut()
                            .filter(|b: &&mut (u8, i64, bool)| b.0 == entry.0)
                        {
                            *old = entry;
                        } else {
                            bindings.push(entry);
                        }
                    }
                    Frame {
                        plain: None,
                        bindings,
                    }
                })
                .collect();
            symmetry(&frames, reference)
        },
    );
}

#[test]
fn family36_rejected_scoped_binding_is_not_plain_fallback() {
    check(
        (1u8..32, 0i64..3, any::<bool>(), 1usize..5),
        |(reference, value, plain, depth)| {
            // Bit 5 guarantees rejection even for name-visible scoped bindings.
            let mut frames = vec![Frame {
                plain: plain.then_some(value),
                bindings: vec![(32, value, true)],
            }];
            frames.extend((1..depth).map(|_| Frame {
                plain: None,
                bindings: vec![(reference | 32, value, true)],
            }));
            symmetry(&frames, reference)
        },
    );
}

#[test]
fn quarantine_h2_a_family38_ambiguous_write_must_reject_without_mutation() {
    quarantine(
        "H2-A ambiguous write",
        (0i64..3, any::<bool>()),
        |(value, split)| {
            let frames = if split {
                vec![
                    Frame {
                        plain: None,
                        bindings: vec![(1, value, false)],
                    },
                    Frame {
                        plain: None,
                        bindings: vec![(2, value, false)],
                    },
                ]
            } else {
                vec![Frame {
                    plain: None,
                    bindings: vec![(1, value, false), (2, value, false)],
                }]
            };
            symmetry(&frames, 3)
        },
    );
}

#[test]
fn quarantine_h2_b_family38_outer_more_specific_binding() {
    quarantine("H2-B binding identity", 0i64..3, |value| {
        symmetry(
            &[
                Frame {
                    plain: None,
                    bindings: vec![(3, value, false)],
                },
                Frame {
                    plain: None,
                    bindings: vec![(1, value, false)],
                },
            ],
            3,
        )
    });
}

#[test]
fn quarantine_h2_c_family38_nonroot_plain_fallback() {
    quarantine(
        "H2-C plain fallback",
        (0i64..3, any::<bool>()),
        |(value, root_plain)| {
            symmetry(
                &[
                    Frame {
                        plain: root_plain.then_some(value),
                        bindings: vec![],
                    },
                    Frame {
                        plain: Some(value),
                        bindings: vec![],
                    },
                ],
                1,
            )
        },
    );
}

// Classify only a specifically known defect shape, independently of the
// observed result. This does not turn arbitrary property failures into passes.
fn known_defect(frames: &[Frame], reference: u8) -> Option<&'static str> {
    match target(frames, reference) {
        Err(()) => Some("H2-A ambiguous write"),
        Ok(Some((frame, 0))) if frame != 0 => Some("H2-C plain fallback"),
        Ok(Some((frame, _)))
            if frames
                .iter()
                .enumerate()
                .any(|(i, f)| i > frame && f.bindings.iter().any(|b| b.0 & reference == b.0)) =>
        {
            Some("H2-B binding identity")
        }
        _ => None,
    }
}

#[test]
fn family38_unrestricted_environment_symmetry_with_named_quarantines() {
    check(
        (prop::collection::vec(frame(), 1..5), 1u8..64),
        |(frames, reference)| {
            let defect = known_defect(&frames, reference);
            match (defect, symmetry(&frames, reference)) {
                (None, result) => result,
                (Some(name), Err(TestCaseError::Fail(reason)))
                    if reason.to_string().starts_with(name) =>
                {
                    Ok(())
                }
                (Some(name), other) => Err(TestCaseError::fail(format!(
                    "{name}: unexpected success or unrelated failure: {other:?}"
                ))),
            }
        },
    );
}

#[test]
fn family39_ties_remain_reportable() {
    const CHILD: &str = "PATINA_H2_TIE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let table = [(scopes(1), 10), (scopes(1), 20)];
        assert_eq!(
            resolve_index("h2-tie", &scopes(3), &table).unwrap(),
            Some(0)
        );
        return;
    }
    // The log sink is process-global and initialized once. Isolate it without
    // changing environment variables underneath other Rust test threads.
    let path = std::env::temp_dir().join(format!("patina-h2-ties-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "environment::hygiene_properties::family39_ties_remain_reportable",
        ])
        .env(CHILD, "1")
        .env("PATINA_AMBIGUITY_LOG", &path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let log = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(log.contains("TIE name=\"h2-tie\""), "{log}");
    assert!(!log.contains("AMBIG"), "{log}");
}
