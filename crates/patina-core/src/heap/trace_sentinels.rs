//! Sentinel tests for patina-core's trace code (#623): one per traced
//! structure, each built by a struct literal, with a fresh value in every
//! field that can hold one and the structure as the only root. See
//! `heap/sentinels.rs` for what a test asserts and why.
//!
//! `Environment`'s and `Library`'s tests live beside their private fields, in
//! `environment.rs` and `library.rs`; the backends' records are tested on
//! their own backend.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::gc::{Collector, GcRoots, GcVisitor, MarkSweepCollector, trace_cont_value};
use super::sentinels::Sentinels;
use super::{Heap, HeapObjectData, PromiseState, SharedHeap, new_shared_heap};
use crate::compiled_macro::{CompiledMacro, CompiledRule, Pattern, Template};
use crate::cont_value::{ContEnv, ContValue, ExceptionHandler, PromptFrame};
use crate::continuation::{CpsContinuation, WindRecord};
use crate::cps_expr::{CpsExpr, CpsExprKind, CpsParam, CpsPrimitive};
use crate::environment::Environment;
use crate::error::ExceptionKind;
use crate::procedure::Procedure;
use crate::record_type::RecordTypeDescriptor;
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
pub(crate) fn collect_from(heap: &mut Heap, roots: &[TaggedValue]) {
    collect_only(heap, |visitor| visitor.visit_slice(roots));
}

/// An environment on `heap` whose one binding holds `value`.
pub(crate) fn env_holding(heap: &SharedHeap, value: TaggedValue) -> Rc<Environment> {
    let env = Rc::new(Environment::with_heap(heap.clone()));
    env.define("held", value);
    env
}

/// `CompiledMacro`, every edge: a literal through each walk of
/// `Pattern::for_each_literal` and `Template::for_each_literal` (a list's and
/// a vector's elements, a dotted list's elements and its tail, an ellipsis's
/// body), the pattern and template of a second rule, `definition_env` (#38)
/// and each of two `foreign_expansions` environments (#462).
#[test]
fn compiled_macro_fields() {
    let shared = new_shared_heap();
    let mut heap = shared.borrow_mut();
    let mut s = Sentinels::new(&mut heap);
    let h = &mut *heap;
    let p_list = s.pair(h, "CompiledRule.pattern: Pattern::List");
    let p_vector = s.vector(h, "CompiledRule.pattern: Pattern::Vector");
    let p_dotted = s.string(h, "CompiledRule.pattern: Pattern::DottedList.patterns");
    let p_tail = s.object(h, "CompiledRule.pattern: Pattern::DottedList.tail");
    let p_ellipsis = s.pair(h, "CompiledRule.pattern: Pattern::Ellipsis.subpattern");
    let t_list = s.vector(h, "CompiledRule.template: Template::List");
    let t_vector = s.string(h, "CompiledRule.template: Template::Vector");
    let t_dotted = s.object(h, "CompiledRule.template: Template::DottedList.templates");
    let t_tail = s.pair(h, "CompiledRule.template: Template::DottedList.tail");
    let t_ellipsis = s.vector(h, "CompiledRule.template: Template::Ellipsis.subtemplate");
    let second_pattern = s.string(h, "CompiledMacro.rules[1]: CompiledRule.pattern");
    let second_template = s.object(h, "CompiledMacro.rules[1]: CompiledRule.template");
    let definition_value = s.pair(h, "CompiledMacro.definition_env");
    let foreign_first = s.vector(h, "CompiledMacro.foreign_expansions[0]");
    let foreign_second = s.string(h, "CompiledMacro.foreign_expansions[1]");
    drop(heap);

    let rule = |pattern, template| CompiledRule {
        pattern,
        template,
        num_pvars: 0,
        max_level: 0,
        pvar_names: HashMap::new(),
    };
    let compiled = CompiledMacro {
        name: Rc::from("m"),
        rules: vec![
            rule(
                Pattern::List(vec![
                    Pattern::Literal(p_list),
                    Pattern::Vector(vec![Pattern::Literal(p_vector)]),
                    Pattern::DottedList {
                        patterns: vec![Pattern::Literal(p_dotted)],
                        tail: Box::new(Pattern::Literal(p_tail)),
                    },
                    Pattern::Ellipsis {
                        subpattern: Box::new(Pattern::Literal(p_ellipsis)),
                        level: 1,
                        num_following: 0,
                        vars: vec![],
                    },
                ]),
                Template::List(vec![
                    Template::Literal(t_list),
                    Template::Vector(vec![Template::Literal(t_vector)]),
                    Template::DottedList {
                        templates: vec![Template::Literal(t_dotted)],
                        tail: Box::new(Template::Literal(t_tail)),
                    },
                    Template::Ellipsis {
                        subtemplate: Box::new(Template::Literal(t_ellipsis)),
                        level: 1,
                        nesting: 1,
                        vars: vec![],
                    },
                ]),
            ),
            rule(
                Pattern::Literal(second_pattern),
                Template::Literal(second_template),
            ),
        ],
        max_pvars: 0,
        definition_scopes: ScopeSet::new(),
        heap: shared.clone(),
        template_symbols: HashSet::new(),
        inherited_identifiers: HashMap::new(),
        definition_env: Some(env_holding(&shared, definition_value)),
        foreign_expansions: vec![
            (ScopeId(1), env_holding(&shared, foreign_first)),
            (ScopeId(2), env_holding(&shared, foreign_second)),
        ],
    };

    let mut heap = shared.borrow_mut();
    let root = heap.alloc_macro(Rc::new(compiled));
    collect_from(&mut heap, &[root]);
    s.assert_survived(&heap);
}

/// Tracing `foreign_expansions` keeps an environment no longer than its
/// macro: once the macro is unreachable, the sweep frees its slot, and that
/// drops the environment's last `Rc`. So the edge cannot keep a replaced
/// library's environment alive past the last macro it generated (#614).
#[test]
fn a_foreign_expansion_environment_dies_with_its_macro() {
    let shared = new_shared_heap();
    let held = shared
        .borrow_mut()
        .alloc_pair(TaggedValue::fixnum(0), TaggedValue::NULL);
    let foreign = env_holding(&shared, held);
    let weak = Rc::downgrade(&foreign);
    let compiled = CompiledMacro {
        name: Rc::from("m"),
        rules: vec![],
        max_pvars: 0,
        definition_scopes: ScopeSet::new(),
        heap: shared.clone(),
        template_symbols: HashSet::new(),
        inherited_identifiers: HashMap::new(),
        definition_env: None,
        foreign_expansions: vec![(ScopeId(1), foreign)],
    };

    let mut heap = shared.borrow_mut();
    let root = heap.alloc_macro(Rc::new(compiled));
    collect_from(&mut heap, &[root]);
    assert!(
        weak.upgrade().is_some(),
        "the macro is rooted, so its foreign expansion environment lives"
    );
    collect_from(&mut heap, &[]);
    assert!(
        weak.upgrade().is_none(),
        "the macro was swept, and its foreign expansion environment is still alive"
    );
}

/// Every walk of `CpsExpr::for_each_literal`, each with a literal of its
/// own: a lambda's body, both parts of a `LetVal` and of a `LetCont`, the
/// operator and an argument of an `App` and of an `Apply`, a `Continue`'s
/// value, the three parts of an `If`, both parts of a `Set` and of a
/// `Define`, and a `PrimOp`'s arguments, all under a `Halt`. The other tests
/// put a bare literal in each expression field; this one roots the tree
/// through `visit_expr_literals`, the collector's one entry to the walk, for
/// procedures, continuations, continuation values and the tree-walker's
/// steps alike.
#[test]
fn expression_literals_under_every_node() {
    let mut heap = Heap::new();
    let mut s = Sentinels::new(&mut heap);
    let h = &mut heap;
    let lambda_body = s.pair(h, "CpsExprKind::Lambda.body");
    let let_value = s.vector(h, "CpsExprKind::LetVal.value");
    let let_body = s.string(h, "CpsExprKind::LetVal.body");
    let cont_body = s.object(h, "CpsExprKind::LetCont.cont_body");
    let let_cont_body = s.pair(h, "CpsExprKind::LetCont.body");
    let app_func = s.vector(h, "CpsExprKind::App.func");
    let app_arg = s.string(h, "CpsExprKind::App.args");
    let apply_func = s.object(h, "CpsExprKind::Apply.func");
    let apply_arg = s.pair(h, "CpsExprKind::Apply.args");
    let continue_value = s.vector(h, "CpsExprKind::Continue.value");
    let test = s.string(h, "CpsExprKind::If.test");
    let consequent = s.object(h, "CpsExprKind::If.consequent");
    let alternate = s.pair(h, "CpsExprKind::If.alternate");
    let set_value = s.vector(h, "CpsExprKind::Set.value");
    let set_cont = s.string(h, "CpsExprKind::Set.cont");
    let define_value = s.object(h, "CpsExprKind::Define.value");
    let define_cont = s.pair(h, "CpsExprKind::Define.cont");
    let primop_arg = s.vector(h, "CpsExprKind::PrimOp.args");

    let node = CpsExpr::new;
    let literal = |value| CpsExpr::new(CpsExprKind::Literal(value));
    let shared = |value| Rc::new(literal(value));
    let name = || -> Rc<str> { Rc::from("x") };
    // A leaf first in each argument list, so a walk that stopped after the
    // first argument would miss the literal.
    let var = || {
        node(CpsExprKind::Var {
            name: name(),
            scopes: ScopeSet::new(),
        })
    };
    let every_node = vec![
        var(),
        literal(primop_arg),
        node(CpsExprKind::ContRef(name())),
        node(CpsExprKind::Lambda {
            params: vec![CpsParam::simple("x")],
            variadic: None,
            cont_param: name(),
            body: shared(lambda_body),
            binding_scopes: Rc::new(ScopeSet::new()),
        }),
        node(CpsExprKind::LetVal {
            name: name(),
            value: shared(let_value),
            body: shared(let_body),
        }),
        node(CpsExprKind::LetCont {
            name: name(),
            param: name(),
            cont_body: shared(cont_body),
            body: shared(let_cont_body),
        }),
        node(CpsExprKind::App {
            func: shared(app_func),
            args: vec![var(), literal(app_arg)],
            cont: name(),
        }),
        node(CpsExprKind::Apply {
            func: shared(apply_func),
            args: vec![var(), literal(apply_arg)],
            cont: name(),
        }),
        node(CpsExprKind::Continue {
            cont: name(),
            value: shared(continue_value),
        }),
        node(CpsExprKind::If {
            test: shared(test),
            consequent: shared(consequent),
            alternate: shared(alternate),
        }),
        node(CpsExprKind::Set {
            var: name(),
            scopes: ScopeSet::new(),
            value: shared(set_value),
            cont: shared(set_cont),
        }),
        node(CpsExprKind::Define {
            name: name(),
            scopes: ScopeSet::new(),
            visible_by_name: true,
            value: shared(define_value),
            cont: shared(define_cont),
        }),
    ];
    let expr = node(CpsExprKind::Halt(Rc::new(node(CpsExprKind::PrimOp {
        op: CpsPrimitive::Cons,
        args: every_node,
        cont: name(),
    }))));

    collect_only(&mut heap, |visitor| visitor.visit_expr_literals(&expr));
    s.assert_survived(&heap);
}

/// An expression whose one literal is `value`.
fn literal(value: TaggedValue) -> Rc<CpsExpr> {
    Rc::new(CpsExpr::new(CpsExprKind::Literal(value)))
}

/// A continuation whose only value is the binding in its environment, for the
/// variants that hold one. `cps_continuation_fields` covers the rest.
fn continuation_holding(heap: &SharedHeap, value: TaggedValue) -> Rc<CpsContinuation> {
    Rc::new(CpsContinuation {
        body: Rc::new(CpsExpr::new(CpsExprKind::ContRef(Rc::from("k")))),
        param: Rc::from("v"),
        env: env_holding(heap, value),
        boundary: None,
        trampoline: 0,
        crosses_callback: false,
        dynamic_winds: vec![],
        prompt_stack: vec![],
        exception_handlers: vec![],
        captured_cont_env: ContEnv::new(),
        resume: None,
    })
}

/// Allocate each object, collect with them as the only roots, and check
/// every sentinel survived.
fn objects_keep_their_sentinels(
    build: impl FnOnce(&SharedHeap, &mut Heap, &mut Sentinels) -> Vec<HeapObjectData>,
) {
    let shared = new_shared_heap();
    let mut heap = shared.borrow_mut();
    let mut sentinels = Sentinels::new(&mut heap);
    let objects = build(&shared, &mut heap, &mut sentinels);
    let roots: Vec<TaggedValue> = objects
        .into_iter()
        .map(|data| heap.alloc_object(data))
        .collect();
    collect_from(&mut heap, &roots);
    sentinels.assert_survived(&heap);
}

/// `CpsContinuation`, every edge, with `resume` holding a value that
/// `captured_cont_env` does **not** also hold: every reify site stores the
/// two aliased, which is what hid `resume` untraced for 1.6 days (#47).
/// Rooted through a `Continuation` heap object.
#[test]
fn cps_continuation_fields() {
    let shared = new_shared_heap();
    let mut heap = shared.borrow_mut();
    let mut sentinels = Sentinels::new(&mut heap);
    let body = sentinels.pair(&mut heap, "CpsContinuation.body");
    let env = sentinels.vector(&mut heap, "CpsContinuation.env");
    let before = sentinels.pair(
        &mut heap,
        "CpsContinuation.dynamic_winds: WindRecord.before",
    );
    let after = sentinels.string(&mut heap, "CpsContinuation.dynamic_winds: WindRecord.after");
    let wind_handler = sentinels.object(
        &mut heap,
        "CpsContinuation.dynamic_winds: WindRecord.handlers",
    );
    let prompt_handler = sentinels.pair(
        &mut heap,
        "CpsContinuation.prompt_stack: PromptFrame.handler",
    );
    let prompt_cont = sentinels.vector(&mut heap, "CpsContinuation.prompt_stack: PromptFrame.cont");
    let handler = sentinels.string(&mut heap, "CpsContinuation.exception_handlers");
    let cont_env = sentinels.object(&mut heap, "CpsContinuation.captured_cont_env");
    let resume = sentinels.pair(&mut heap, "CpsContinuation.resume");
    drop(heap);

    let k = CpsContinuation {
        body: literal(body),
        param: Rc::from("v"),
        env: env_holding(&shared, env),
        boundary: None,
        trampoline: 0,
        crosses_callback: false,
        dynamic_winds: vec![WindRecord {
            id: 0,
            before,
            after,
            handlers: Rc::from(vec![ExceptionHandler {
                handler: wind_handler,
            }]),
        }],
        prompt_stack: vec![PromptFrame {
            id: 0,
            tag: None,
            handler: prompt_handler,
            cont: ContValue::ForceCache {
                promise: prompt_cont,
                original_cont: Box::new(ContValue::Halt),
            },
            wind_depth: 0,
            trampoline: 0,
            handler_depth: 0,
        }],
        exception_handlers: vec![ExceptionHandler { handler }],
        captured_cont_env: ContEnv::new().insert(
            Rc::from("k"),
            ContValue::CallWithValuesConsumer {
                consumer: cont_env,
                original_cont: Box::new(ContValue::Halt),
            },
        ),
        resume: Some(ContValue::DynamicWindAfterDone {
            result_value: resume,
            original_cont: Box::new(ContValue::Halt),
        }),
    };

    let mut heap = shared.borrow_mut();
    let root = heap.alloc_object(HeapObjectData::Continuation(Rc::new(k)));
    collect_from(&mut heap, &[root]);
    sentinels.assert_survived(&heap);
}

/// Every `ContValue` variant that holds a value, each field with a sentinel,
/// chained through each variant's inner continuation so the walk down
/// `original_cont`, `cleanup_cont` and `cont` is covered too.
#[test]
fn cont_value_variants() {
    let shared = new_shared_heap();
    let mut heap = shared.borrow_mut();
    let mut s = Sentinels::new(&mut heap);
    let h = &mut *heap;
    let consumer = s.pair(h, "ContValue::CallWithValuesConsumer.consumer");
    let promise = s.vector(h, "ContValue::ForceCache.promise");
    let state = s.string(h, "ContValue::ResumePrimitive.state");
    let cleanup_after = s.object(h, "ContValue::DynamicWindCleanup.after");
    let setup_before = s.pair(
        h,
        "ContValue::DynamicWindSetup.wind_record: WindRecord.before",
    );
    let setup_after = s.vector(
        h,
        "ContValue::DynamicWindSetup.wind_record: WindRecord.after",
    );
    let setup_handler = s.string(
        h,
        "ContValue::DynamicWindSetup.wind_record: WindRecord.handlers",
    );
    let setup_body = s.object(h, "ContValue::DynamicWindSetup.body");
    let result_value = s.pair(h, "ContValue::DynamicWindAfterDone.result_value");
    let original_exception = s.vector(h, "ContValue::RaiseHandlerReturn.original_exception");
    let popped_handler = s.string(h, "ContValue::RaiseHandlerReturn.popped_handler");
    let abort_handler = s.object(h, "ContValue::AbortLanding.handler");
    let delimited = s.pair(h, "ContValue::AbortLanding.delimited");
    let invoke_target = s.vector(h, "ContValue::ComposableInvokeStep.target");
    let invoke_value = s.string(h, "ContValue::ComposableInvokeStep.value");
    let local_body = s.object(h, "ContValue::Local.body");
    let local_env = s.pair(h, "ContValue::Local.env");
    let local_cont_env = s.vector(h, "ContValue::Local.cont_env");
    let jump_before = s.string(h, "ContValue::Jump.entered: WindRecord.before");
    let jump_after = s.object(h, "ContValue::Jump.entered: WindRecord.after");
    let jump_handler = s.pair(h, "ContValue::Jump.entered: WindRecord.handlers");
    let jump_value = s.vector(h, "ContValue::Jump.value");
    let jump_target = s.string(h, "ContValue::Jump.target");
    let captured = s.object(h, "ContValue::Captured");
    drop(heap);

    let halt = || Box::new(ContValue::Halt);
    let local = ContValue::Local {
        param: Rc::from("v"),
        body: literal(local_body),
        env: env_holding(&shared, local_env),
        cont_env: ContEnv::new().insert(
            Rc::from("k"),
            ContValue::ForceCache {
                promise: local_cont_env,
                original_cont: halt(),
            },
        ),
    };
    let invoke = ContValue::ComposableInvokeStep {
        target: continuation_holding(&shared, invoke_target),
        value: invoke_value,
        index: 0,
        cont: Box::new(local),
    };
    let abort = ContValue::AbortLanding {
        handler: abort_handler,
        delimited,
        cont: Box::new(invoke),
    };
    let raise = ContValue::RaiseHandlerReturn {
        continuable: false,
        original_exception: Some(original_exception),
        original_cont: Box::new(abort),
        popped_handler: Some(ExceptionHandler {
            handler: popped_handler,
        }),
    };
    let handler_cleanup = ContValue::ExceptionHandlerCleanup {
        original_cont: Box::new(raise),
    };
    let after_done = ContValue::DynamicWindAfterDone {
        result_value,
        original_cont: Box::new(handler_cleanup),
    };
    let setup = ContValue::DynamicWindSetup {
        wind_record: WindRecord {
            id: 0,
            before: setup_before,
            after: setup_after,
            handlers: Rc::from(vec![ExceptionHandler {
                handler: setup_handler,
            }]),
        },
        body: setup_body,
        cleanup_cont: Box::new(after_done),
    };
    let cleanup = ContValue::DynamicWindCleanup {
        after: cleanup_after,
        wind_id: 0,
        original_cont: Box::new(setup),
    };
    let resume = ContValue::ResumePrimitive {
        index: 0,
        state,
        original_cont: Box::new(cleanup),
    };
    let force = ContValue::ForceCache {
        promise,
        original_cont: Box::new(resume),
    };
    let chain = ContValue::CallWithValuesConsumer {
        consumer,
        original_cont: Box::new(force),
    };
    let jump = ContValue::Jump {
        entered: Some(WindRecord {
            id: 0,
            before: jump_before,
            after: jump_after,
            handlers: Rc::from(vec![ExceptionHandler {
                handler: jump_handler,
            }]),
        }),
        value: jump_value,
        target: continuation_holding(&shared, jump_target),
    };
    let captured = ContValue::Captured(continuation_holding(&shared, captured));
    // The variants that hold no value, so that every variant is rooted here.
    let leaves = [
        ContValue::Halt,
        ContValue::PromptBoundary { id: 0 },
        ContValue::ExitLanding { status: 0 },
    ];
    let roots: Vec<ContValue> = [chain, jump, captured].into_iter().chain(leaves).collect();

    let mut heap = shared.borrow_mut();
    collect_only(&mut heap, |visitor| {
        for cont in &roots {
            trace_cont_value(cont, visitor);
        }
    });
    s.assert_survived(&heap);
}

#[test]
fn complex_parts() {
    objects_keep_their_sentinels(|_, heap, s| {
        vec![HeapObjectData::Complex {
            real: s.pair(heap, "HeapObjectData::Complex.real"),
            imag: s.vector(heap, "HeapObjectData::Complex.imag"),
        }]
    });
}

#[test]
fn exception_irritants() {
    objects_keep_their_sentinels(|_, heap, s| {
        vec![HeapObjectData::Exception {
            kind: ExceptionKind::Error,
            message: String::from("sentinel"),
            irritants: vec![
                s.pair(heap, "HeapObjectData::Exception.irritants"),
                s.string(heap, "HeapObjectData::Exception.irritants"),
            ],
        }]
    });
}

#[test]
fn cps_lambda_body_and_environment() {
    objects_keep_their_sentinels(|shared, heap, s| {
        let body = s.pair(heap, "Procedure::CpsLambda.body");
        let env = s.vector(heap, "Procedure::CpsLambda.env");
        vec![HeapObjectData::Procedure(Rc::new(Procedure::CpsLambda {
            params: vec![],
            variadic: None,
            cont_param: Rc::from("k"),
            body: literal(body),
            env: env_holding(shared, env),
            binding_scopes: Rc::new(ScopeSet::new()),
        }))]
    });
}

#[test]
fn record_fields() {
    objects_keep_their_sentinels(|_, heap, s| {
        vec![HeapObjectData::Record {
            record_type: Rc::new(RecordTypeDescriptor::new("r", vec!["a".into(), "b".into()])),
            fields: Rc::new(RefCell::new(vec![
                s.pair(heap, "HeapObjectData::Record.fields"),
                s.object(heap, "HeapObjectData::Record.fields"),
            ])),
        }]
    });
}

#[test]
fn parameter_values_and_converter() {
    objects_keep_their_sentinels(|_, heap, s| {
        vec![HeapObjectData::Parameter {
            values: Rc::new(RefCell::new(vec![
                s.pair(heap, "HeapObjectData::Parameter.values"),
            ])),
            converter: Some(s.vector(heap, "HeapObjectData::Parameter.converter")),
        }]
    });
}

/// Both states: the thunk of a delayed promise and the value of a forced one.
#[test]
fn promise_states() {
    objects_keep_their_sentinels(|_, heap, s| {
        let delayed = s.pair(heap, "HeapObjectData::Promise: PromiseState::Delayed");
        let forced = s.string(heap, "HeapObjectData::Promise: PromiseState::Forced");
        vec![
            HeapObjectData::Promise(Rc::new(RefCell::new(PromiseState::Delayed(delayed)))),
            HeapObjectData::Promise(Rc::new(RefCell::new(PromiseState::Forced(forced)))),
        ]
    });
}

#[test]
fn values_elements() {
    objects_keep_their_sentinels(|_, heap, s| {
        vec![HeapObjectData::Values(vec![
            s.pair(heap, "HeapObjectData::Values"),
            s.vector(heap, "HeapObjectData::Values"),
        ])]
    });
}

#[test]
fn environment_specifier_environment() {
    objects_keep_their_sentinels(|shared, heap, s| {
        let held = s.pair(heap, "HeapObjectData::EnvironmentSpecifier.env");
        vec![HeapObjectData::EnvironmentSpecifier {
            env: env_holding(shared, held),
            mutable: false,
        }]
    });
}

#[test]
fn mutable_cell_contents() {
    objects_keep_their_sentinels(|_, heap, s| {
        vec![HeapObjectData::MutableCell(RefCell::new(
            s.object(heap, "HeapObjectData::MutableCell"),
        ))]
    });
}

#[test]
fn vm_closure_free_variables_and_globals() {
    objects_keep_their_sentinels(|shared, heap, s| {
        let free_var = s.pair(heap, "HeapObjectData::VmClosure.free_vars");
        let global = s.vector(heap, "HeapObjectData::VmClosure.globals");
        vec![HeapObjectData::VmClosure {
            code_id: 0,
            free_vars: vec![free_var],
            globals: env_holding(shared, global),
        }]
    });
}

/// An ephemeron keeps its datum while its key is reachable some other way:
/// here the key is a root of its own, and the datum is reachable only through
/// the pair, by the fixpoint in `run_mark_phase`.
#[test]
fn ephemeron_datum_under_a_live_key() {
    let mut heap = Heap::new();
    let mut s = Sentinels::new(&mut heap);
    let key = s.pair(&mut heap, "HeapObjectData::Ephemeron key, rooted elsewhere");
    let datum = s.vector(&mut heap, "HeapObjectData::Ephemeron datum");
    let ephemeron = heap.alloc_object(HeapObjectData::Ephemeron(RefCell::new(Some((key, datum)))));
    collect_from(&mut heap, &[ephemeron, key]);
    s.assert_survived(&heap);
}
