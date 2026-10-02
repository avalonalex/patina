//! Sentinel tests for patina-core's trace code (#623): one per traced
//! structure, each built by a struct literal, with a fresh value in every
//! field that can hold one and the structure as the only root. See
//! `heap/sentinels.rs` for what a test asserts and why.
//!
//! `Environment`'s and `Library`'s tests live beside their private fields, in
//! `environment.rs` and `library.rs`; the backends' records are tested on
//! their own backend.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::gc::{Collector, GcRoots, GcVisitor, MarkSweepCollector};
use super::sentinels::Sentinels;
use super::{Heap, SharedHeap, new_shared_heap};
use crate::compiled_macro::{CompiledMacro, CompiledRule, Pattern, Template};
use crate::environment::Environment;
use crate::scope::{ScopeId, ScopeSet};
use crate::tagged_value::TaggedValue;

/// A root provider that runs one closure: the structure under test.
struct Only<F>(F);

impl<F: Fn(&mut GcVisitor<'_>)> GcRoots for Only<F> {
    fn trace_roots(&self, visitor: &mut GcVisitor<'_>) {
        (self.0)(visitor)
    }
}

/// Collect `heap` with `trace` as the only root.
pub(crate) fn collect_only(heap: &mut Heap, trace: impl Fn(&mut GcVisitor<'_>)) {
    MarkSweepCollector::new().collect(heap, &[&Only(trace)]);
}

/// Collect with these values as the only roots.
fn collect_from(heap: &mut Heap, roots: &[TaggedValue]) {
    collect_only(heap, |visitor| visitor.visit_slice(roots));
}

/// An environment on `heap` whose one binding holds `value`.
fn env_holding(heap: &SharedHeap, value: TaggedValue) -> Rc<Environment> {
    let env = Rc::new(Environment::with_heap(heap.clone()));
    env.define("held", value);
    env
}

/// `CompiledMacro`, every edge: the literals of its patterns and templates
/// (nested, so the walks below the rule are covered too), `definition_env`
/// (#38) and `foreign_expansions` (#462).
#[test]
fn compiled_macro_fields() {
    let shared = new_shared_heap();
    let mut heap = shared.borrow_mut();
    let mut sentinels = Sentinels::new(&mut heap);
    let pattern_literal = sentinels.pair(&mut heap, "CompiledRule.pattern");
    let template_literal = sentinels.vector(&mut heap, "CompiledRule.template");
    let definition_value = sentinels.string(&mut heap, "CompiledMacro.definition_env");
    let foreign_value = sentinels.pair(&mut heap, "CompiledMacro.foreign_expansions");
    drop(heap);

    let compiled = CompiledMacro {
        name: Rc::from("m"),
        rules: vec![CompiledRule {
            pattern: Pattern::DottedList {
                patterns: vec![Pattern::Ellipsis {
                    subpattern: Box::new(Pattern::List(vec![Pattern::Literal(pattern_literal)])),
                    level: 1,
                    num_following: 0,
                    vars: vec![],
                }],
                tail: Box::new(Pattern::Wildcard),
            },
            template: Template::Vector(vec![Template::Ellipsis {
                subtemplate: Box::new(Template::DottedList {
                    templates: vec![],
                    tail: Box::new(Template::Literal(template_literal)),
                }),
                level: 1,
                nesting: 1,
                vars: vec![],
            }]),
            num_pvars: 0,
            max_level: 0,
            pvar_names: HashMap::new(),
        }],
        max_pvars: 0,
        definition_scopes: ScopeSet::new(),
        heap: shared.clone(),
        template_symbols: HashSet::new(),
        inherited_identifiers: HashMap::new(),
        definition_env: Some(env_holding(&shared, definition_value)),
        foreign_expansions: vec![(ScopeId(1), env_holding(&shared, foreign_value))],
    };

    let mut heap = shared.borrow_mut();
    let root = heap.alloc_macro(Rc::new(compiled));
    collect_from(&mut heap, &[root]);
    sentinels.assert_survived(&heap);
}
