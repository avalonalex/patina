//! Family 38 historical non-vacuity probe. Nonempty proper-subset binders,
//! 1..4 frames, equal-valued cells, fixed seed and bounded shrinking.
//! Kept on public APIs so this exact harness can be ported before #137.
use patina_core::tagged_value::TaggedValue;
use patina_core::{Environment, ScopeId, ScopeSet};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed, TestRunner};
use std::rc::Rc;

fn scopes(mask: u8) -> ScopeSet {
    let mut scopes = ScopeSet::new();
    for bit in 0..6 {
        if mask & (1 << bit) != 0 {
            scopes.add_scope(ScopeId(bit));
        }
    }
    scopes
}

#[test]
fn family38_proper_subset_write_reaches_read_binding() {
    let config = Config {
        cases: 256,
        max_shrink_iters: 4096,
        rng_seed: RngSeed::Fixed(284),
        failure_persistence: None,
        ..Config::default()
    };
    let result =
        TestRunner::new(config).run(&(1u8..32, 1usize..5, 0i64..3), |(mask, depth, value)| {
            let binding = scopes(mask);
            let reference = scopes(mask | 32);
            let mut envs = vec![Rc::new(Environment::new())];
            envs[0].define_with_scopes("x", binding.clone(), TaggedValue::fixnum(value));
            for _ in 1..depth {
                let env = Rc::new(Environment::with_parent(envs.last().unwrap().clone()));
                env.define_with_scopes("x", scopes(32), TaggedValue::fixnum(value));
                envs.push(env);
            }
            // Probe from the root so its proper-subset target is unambiguous;
            // inspect each descendant separately to detect writes to equal values.
            prop_assert_eq!(
                envs[0].get_with_scopes("x", &reference).unwrap(),
                Some(TaggedValue::fixnum(value))
            );
            let write = envs[0].set_with_scopes("x", &reference, TaggedValue::fixnum(999));
            prop_assert!(write.is_ok(), "proper-subset write rejected: {:?}", write);
            prop_assert_eq!(
                envs[0].get_with_scopes("x", &binding).unwrap(),
                Some(TaggedValue::fixnum(999))
            );
            for env in &envs[1..] {
                prop_assert_eq!(
                    env.get_with_scopes("x", &scopes(32)).unwrap(),
                    Some(TaggedValue::fixnum(value))
                );
            }
            Ok(())
        });
    if let Err(error) = result {
        panic!("H2 seed=284 cases=256 shrink_limit=4096: {error}");
    }
}

#[test]
fn family38_scoped_write_updates_a_proper_subset_binding() {
    // The generated probe shrank to these scopes on 5b93bf7, the parent of
    // #137 (6a86e21): read succeeds, but the exact-match write returns Err("x").
    // Track H's PRD records the command, seed and compatibility adaptations.
    let binding = scopes(0b000001);
    let reference = scopes(0b100001);
    assert!(binding.is_proper_subset_of(&reference));

    let env = Environment::new();
    let original = TaggedValue::fixnum(7);
    let sentinel = TaggedValue::fixnum(999);
    env.define_with_scopes("x", binding.clone(), original);
    assert_eq!(
        env.get_with_scopes("x", &reference).unwrap(),
        Some(original)
    );

    env.set_with_scopes("x", &reference, sentinel).unwrap();
    assert_eq!(env.get_with_scopes("x", &binding).unwrap(), Some(sentinel));
}
