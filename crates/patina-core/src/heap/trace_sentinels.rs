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

use super::gc::{Collector, GcVisitor, MarkSweepCollector, RootProvider, SlotVisitor};
use super::sentinels::Sentinels;
use super::trace::Trace;
use super::{Heap, HeapObjectData, Interior, PromiseState, SharedHeap, new_shared_heap};
use crate::compiled_macro::{CompiledMacro, CompiledRule, LiteralId, Pattern, Template};
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

impl<F: Fn(&mut dyn SlotVisitor)> RootProvider for Only<F> {
    fn trace_roots(&self, visitor: &mut dyn SlotVisitor) {
        (self.0)(visitor)
    }
}

/// Collect `heap` with `trace` as the only root.
pub(crate) fn collect_only(heap: &mut Heap, trace: impl Fn(&mut dyn SlotVisitor)) {
    MarkSweepCollector::new().collect(heap, &[&Only(trace)]);
}

/// Collect with these values as the only roots.
pub(crate) fn collect_from(heap: &mut Heap, roots: &[TaggedValue]) {
    collect_only(heap, |visitor| {
        for &value in roots {
            visitor.pinned(value);
        }
    });
}

/// An environment on `heap` whose one binding holds `value`.
pub(crate) fn env_holding(heap: &SharedHeap, value: TaggedValue) -> Rc<Environment> {
    let env = Rc::new(Environment::with_heap(heap));
    env.define(heap, "held", value);
    env
}

/// [`env_holding`] for a test that holds the heap, `held`, borrowed mutably:
/// the namespace takes its account from it rather than borrowing it again,
/// and its binding is defined through the namespace step's core.
fn env_holding_on(held: &mut Heap, value: TaggedValue) -> Rc<Environment> {
    let env = Rc::new(Environment::namespace(held));
    env.define_in(held, "held", value);
    env
}

/// A macro object, every edge: each literal of the vector its patterns and
/// templates name by index (#718), its body's `definition_env` (#38), each
/// of two `foreign_expansions` environments (#462), and a carried relink's
/// record (#837), in no slot and reachable only through the object: a
/// record over a free cell that holds the sentinel.
#[test]
fn compiled_macro_fields() {
    let shared = new_shared_heap();
    let mut heap = shared.borrow_mut();
    let mut s = Sentinels::new(&mut heap);
    let h = &mut *heap;
    let literals = vec![
        s.pair(h, "HeapObjectData::Macro.literals[0]: a pattern's"),
        s.vector(h, "HeapObjectData::Macro.literals[1]: a template's"),
        s.string(h, "HeapObjectData::Macro.literals[2]: rule 2's pattern's"),
        s.object(h, "HeapObjectData::Macro.literals[3]: rule 2's template's"),
    ];
    let definition_value = s.pair(h, "CompiledMacro.definition_env");
    let foreign_first = s.vector(h, "CompiledMacro.foreign_expansions[0]");
    let foreign_second = s.string(h, "CompiledMacro.foreign_expansions[1]");
    let carried = s.pair(
        h,
        "HeapObjectData::Macro.relinks[0].link: a carried relink's",
    );
    let relinks = vec![carried_relink(h, carried)];
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
                Pattern::List(vec![Pattern::Literal(LiteralId::new(0))]),
                Template::List(vec![Template::Literal(LiteralId::new(1))]),
            ),
            rule(
                Pattern::Literal(LiteralId::new(2)),
                Template::Literal(LiteralId::new(3)),
            ),
        ],
        max_pvars: 0,
        definition_scopes: ScopeSet::new(),
        template_symbols: HashSet::new(),
        inherited_identifiers: HashMap::new(),
        definition_env: Some(env_holding(&shared, definition_value)),
        foreign_expansions: vec![
            (ScopeId(1), env_holding(&shared, foreign_first)),
            (ScopeId(2), env_holding(&shared, foreign_second)),
        ],
    };

    let mut heap = shared.borrow_mut();
    let root = heap.alloc_macro_with_relinks(Rc::new(compiled), literals, relinks);
    collect_from(&mut heap, &[root]);
    s.assert_survived(&heap);
}

/// A carried relink over a fresh record over a fresh free cell that holds
/// `value`: three pairs' worth of edge, none in any slot.
fn carried_relink(h: &mut Heap, value: TaggedValue) -> crate::binding_record::CarriedRelink {
    use crate::binding_record::{BindingCell, BindingLink, CarriedRelink, Record};
    let cell = BindingCell::free(h, value);
    let record = Record::new(h, cell);
    CarriedRelink::new(
        Rc::from("secret.1"),
        ScopeId(7),
        Rc::from("secret"),
        BindingLink::new(record),
    )
}

/// A macro's carried relinks live exactly as long as the macro (#837):
/// while it is rooted, the record, its cell and the value the cell holds
/// are kept, and the object reads them back; once it is unreachable, the
/// same sweep frees all three with it. So the edge cannot keep a replaced
/// library's binding past the last macro that carries it. A macro made by
/// `alloc_macro` carries none.
#[test]
fn a_macros_relinks_die_with_the_macro() {
    let shared = new_shared_heap();
    let mut heap = shared.borrow_mut();
    let h = &mut *heap;
    let compiled = || {
        Rc::new(CompiledMacro {
            name: Rc::from("m"),
            rules: vec![],
            max_pvars: 0,
            definition_scopes: ScopeSet::new(),
            template_symbols: HashSet::new(),
            inherited_identifiers: HashMap::new(),
            definition_env: None,
            foreign_expansions: vec![],
        })
    };
    let held = h.alloc_pair(TaggedValue::fixnum(7), TaggedValue::NULL);
    let relink = carried_relink(h, held);
    let link = relink.link();
    let root = h.alloc_macro_with_relinks(compiled(), vec![], vec![relink]);
    let plain = h.alloc_macro(compiled(), vec![]);
    assert!(h.macro_relinks(plain).is_empty());
    assert!(h.macro_relinks(held).is_empty(), "no macro, no relinks");

    let collect = |h: &mut Heap, roots: &[TaggedValue]| {
        MarkSweepCollector::new()
            .collect(
                h,
                &[&Only(|visitor: &mut dyn SlotVisitor| {
                    for &value in roots {
                        visitor.pinned(value);
                    }
                })],
            )
            .last_swept
    };
    let swept = collect(h, &[root]);
    assert_eq!(
        (swept.pairs, swept.objects),
        (0, 1),
        "the macro is rooted: only the plain one is swept"
    );
    let carried = h.macro_relinks(root);
    assert_eq!(carried.len(), 1);
    assert_eq!(carried[0].link(), link);
    assert_eq!(&**carried[0].spelling(), "secret.1");
    assert_eq!(link.value(h), Some(held));

    let swept = collect(h, &[]);
    assert_eq!(
        (swept.pairs, swept.objects),
        (3, 1),
        "the macro was swept, and its relink's record, cell and value with it"
    );
}

/// The same edge through a context (#837), which is how a safe crate
/// stores and reads a macro's relinks: `Cx::relink_link` finds two records,
/// a snapshot of a frame's keyword and a library's slot; they cross the
/// brand into the object as `Carried` (`Cx::alloc_macro_with_relinks`),
/// and, once the frame, the program and the library are dropped and two
/// collections have run with the macro as the only root, come back branded
/// by a second context (`Cx::macro_relinks`) with the spellings and the
/// guard they were stored with, in order, of kind `Relinked`, each reading
/// what its record held, and the object's literal beside them. One pooled
/// from there is a relinked link that
/// `Cx::linkable` reads back at the same location. A macro that carries
/// none, and a value that is no macro, answer none. The trace arm deleted,
/// or `CarriedRelink::link` skipped, and the first read is of a freed
/// record, which a check build refuses.
#[test]
fn a_macros_relinks_cross_a_context_and_a_collection() {
    use crate::binding_record::Carried;
    use crate::environment::RelinkTarget;
    use crate::literal_pool::{LinkKind, LiteralPool};
    use crate::namespace_view::NamespaceView;
    use crate::{NoGcScope, Value};

    let compiled = |name: &str| {
        Rc::new(CompiledMacro {
            name: Rc::from(name),
            rules: vec![],
            max_pvars: 0,
            definition_scopes: ScopeSet::new(),
            template_symbols: HashSet::new(),
            inherited_identifiers: HashMap::new(),
            definition_env: None,
            foreign_expansions: vec![],
        })
    };
    let shared = new_shared_heap();
    let program = Rc::new(Environment::with_heap(&shared));
    let frame = Rc::new(Environment::child_frame(Rc::clone(&program)));
    let keyword = shared.borrow_mut().alloc_macro(compiled("kw"), vec![]);
    frame.define(&shared, "kw", keyword);
    let library = Rc::new(Environment::with_heap(&shared));
    library.define(&shared, "secret", TaggedValue::fixnum(42));

    let (carrying, plain) = NoGcScope::with_cx(&shared, |cx| {
        let snapshot = cx
            .relink_link(NamespaceView::of(&frame), "kw", RelinkTarget::ByName)
            .expect("a frame's keyword is linked by a snapshot");
        let slot = cx
            .relink_link(NamespaceView::of(&library), "secret", RelinkTarget::ByName)
            .expect("the library's slot");
        let relinks = vec![
            Carried {
                spelling: Rc::from("kw.1"),
                guard: ScopeId(7),
                written: Rc::from("kw"),
                link: snapshot,
            },
            Carried {
                spelling: Rc::from("secret.2"),
                guard: ScopeId(8),
                written: Rc::from("secret"),
                link: slot,
            },
        ];
        let carrying = cx.alloc_macro_with_relinks(compiled("g"), vec![Value::fixnum(5)], relinks);
        let plain = cx.alloc_macro_with_relinks(compiled("p"), vec![], vec![]);
        (carrying.tagged(), plain.tagged())
    });
    // The macro is now the only holder of the snapshot and of the
    // library's record.
    drop(frame);
    drop(program);
    drop(library);
    collect_from(&mut shared.borrow_mut(), &[carrying, plain]);
    collect_from(&mut shared.borrow_mut(), &[carrying, plain]);

    let pool = LiteralPool::new(&shared.borrow());
    NoGcScope::cross(&shared, [carrying, plain], |cx, [carrying, plain]| {
        let back = cx.macro_relinks(carrying);
        let keys: Vec<(&str, ScopeId, &str)> = back
            .iter()
            .map(|carried| (&*carried.spelling, carried.guard, &*carried.written))
            .collect();
        assert_eq!(
            keys,
            [
                ("kw.1", ScopeId(7), "kw"),
                ("secret.2", ScopeId(8), "secret")
            ],
            "each key as it was stored, in order"
        );
        assert!(
            (back.iter()).all(|carried| carried.link.kind() == LinkKind::Relinked),
            "a carried relink comes back relinked"
        );
        let held: Value<'_> = cx.linked_value(back[0].link).expect("the snapshot's cell");
        assert_eq!(held.tagged(), keyword);
        assert!(cx.get_macro(held).is_some(), "the keyword is live");
        assert_eq!(
            cx.linked_value(back[1].link).map(|v| v.tagged()),
            Some(TaggedValue::fixnum(42))
        );

        let link = pool.add_link(back[1].link);
        assert_eq!(link.kind(), LinkKind::Relinked);
        let again = cx.linkable(&pool, link);
        assert_eq!(again.link(), back[1].link.link());
        assert_eq!(cx.linked_location(again), cx.linked_location(back[1].link));

        assert_eq!(
            cx.macro_literal(carrying, crate::LiteralId::new(0))
                .map(|v| v.tagged()),
            Some(TaggedValue::fixnum(5)),
            "its literals are stored beside its relinks"
        );
        assert!(cx.macro_relinks(plain).is_empty(), "a macro with none");
        assert!(cx.macro_relinks(held).is_empty(), "nor the keyword's");
        let fixnum = cx.linked_value(back[1].link).unwrap();
        assert!(cx.macro_relinks(fixnum).is_empty(), "no macro, no relinks");
    });
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
        template_symbols: HashSet::new(),
        inherited_identifiers: HashMap::new(),
        definition_env: None,
        foreign_expansions: vec![(ScopeId(1), foreign)],
    };

    let mut heap = shared.borrow_mut();
    let root = heap.alloc_macro(Rc::new(compiled), vec![]);
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
    let linked_set_value = s.pair(h, "CpsExprKind::LinkedSet.value");
    let linked_set_cont = s.object(h, "CpsExprKind::LinkedSet.cont");
    let define_value = s.object(h, "CpsExprKind::Define.value");
    let define_cont = s.pair(h, "CpsExprKind::Define.cont");
    let primop_arg = s.vector(h, "CpsExprKind::PrimOp.args");
    // A linked node's record, which `expression_links_under_var_and_set`
    // holds to its trace; here it is only what the variants carry.
    let link = {
        let cell = crate::binding_record::BindingCell::free(h, TaggedValue::fixnum(0));
        crate::binding_record::BindingLink::new(crate::binding_record::Record::new(h, cell))
    };

    let node = CpsExpr::new;
    let literal = |value| CpsExpr::new(CpsExprKind::Literal(value));
    let shared = |value| Rc::new(literal(value));
    let name = || -> Rc<str> { Rc::from("x") };
    // A leaf first in each argument list, so a walk that stopped after the
    // first argument would miss the literal.
    let var = || node(CpsExprKind::var(name(), ScopeSet::new()));
    let every_node = vec![
        var(),
        node(CpsExprKind::LinkedVar(Rc::new(
            crate::cps_expr::LinkedRef {
                name: name(),
                scopes: ScopeSet::from_iter([ScopeId(1)]),
                link,
            },
        ))),
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
        node(CpsExprKind::LinkedSet(Rc::new(
            crate::cps_expr::LinkedAssign {
                var: name(),
                scopes: ScopeSet::from_iter([ScopeId(1)]),
                value: shared(linked_set_value),
                cont: shared(linked_set_cont),
                link,
            },
        ))),
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

    collect_only(&mut heap, |visitor| visitor.expr_literals(&expr));
    s.assert_survived(&heap);
}

/// A `LinkedVar`'s and a `LinkedSet`'s record (#837): each a record over
/// a free cell holding a fresh value, reachable only through the node's
/// `link`, and the nodes rooted through `visit_expr_literals`, as a closure's
/// body, a continuation's and the trampoline's step are. A collection keeps
/// each record, its cell and the value, so the tree-walker's read by record
/// never meets a swept pair.
#[test]
fn expression_links_under_var_and_set() {
    use crate::binding_record::{BindingCell, BindingLink, Record};

    let mut heap = Heap::new();
    let mut s = Sentinels::new(&mut heap);
    let h = &mut heap;
    let var_value = s.pair(h, "CpsExprKind::LinkedVar.link: its record's cell's value");
    let set_value = s.vector(h, "CpsExprKind::LinkedSet.link: its record's cell's value");
    let mut link_to = |value| {
        let cell = BindingCell::free(h, value);
        Some(BindingLink::new(Record::new(h, cell)))
    };
    let (var_link, set_link) = (link_to(var_value), link_to(set_value));
    let scopes = ScopeSet::from_iter([ScopeId(1)]);
    let unspecified = || Rc::new(CpsExpr::new(CpsExprKind::Literal(TaggedValue::UNSPECIFIED)));
    let set = CpsExpr::new(CpsExprKind::linked_set(
        Rc::from("x"),
        scopes.clone(),
        unspecified(),
        unspecified(),
        set_link,
    ));
    let var = CpsExpr::new(CpsExprKind::linked_var(Rc::from("x"), scopes, var_link));
    let expr = CpsExpr::new(CpsExprKind::App {
        func: Rc::new(set),
        args: vec![var],
        cont: Rc::from("k"),
    });

    collect_only(&mut heap, |visitor| visitor.expr_literals(&expr));
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
        .map(|data| match data {
            // Through its own path, which charges a tree-walker closure's
            // payload (`heap/account.rs`); sweep checks that it was.
            HeapObjectData::Procedure(procedure) => heap.alloc_procedure(procedure),
            data => heap.alloc_object(data),
        })
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
            cont.trace(visitor);
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
    objects_keep_their_sentinels(|_shared, heap, s| {
        let body = s.pair(heap, "Procedure::CpsLambda.body");
        let env = s.vector(heap, "Procedure::CpsLambda.env");
        vec![HeapObjectData::Procedure(Rc::new(Procedure::CpsLambda {
            params: vec![],
            variadic: None,
            cont_param: Rc::from("k"),
            body: literal(body),
            env: env_holding_on(heap, env),
            binding_scopes: Rc::new(ScopeSet::new()),
        }))]
    });
}

#[test]
fn record_fields() {
    objects_keep_their_sentinels(|_, heap, s| {
        vec![HeapObjectData::Record {
            record_type: Rc::new(RecordTypeDescriptor::new("r", vec!["a".into(), "b".into()])),
            fields: Rc::new(Interior::new(vec![
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
            values: Rc::new(Interior::new(vec![
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
            HeapObjectData::Promise(Rc::new(Interior::new(PromiseState::Delayed(delayed)))),
            HeapObjectData::Promise(Rc::new(Interior::new(PromiseState::Forced(forced)))),
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
    objects_keep_their_sentinels(|_shared, heap, s| {
        let held = s.pair(heap, "HeapObjectData::EnvironmentSpecifier.env");
        vec![HeapObjectData::EnvironmentSpecifier {
            env: env_holding_on(heap, held),
            mutable: false,
        }]
    });
}

#[test]
fn mutable_cell_contents() {
    objects_keep_their_sentinels(|_, heap, s| {
        vec![HeapObjectData::MutableCell(Interior::new(
            s.object(heap, "HeapObjectData::MutableCell"),
        ))]
    });
}

/// A VM closure, every field: its free variables are visited, and its code
/// id is queued for `RootProvider::trace_code_ids` (#836), which
/// `Recording` records. It holds no environment: the namespace its code
/// reads is its unit's link table's, which the VM traces by that id.
#[test]
fn vm_closure_free_variables() {
    let shared = new_shared_heap();
    let mut heap = shared.borrow_mut();
    let mut s = Sentinels::new(&mut heap);
    let free_var = s.pair(&mut heap, "HeapObjectData::VmClosure.free_vars");
    let closure = heap.alloc_object(HeapObjectData::VmClosure {
        code_id: 7,
        free_vars: vec![free_var],
    });
    let recording = Recording {
        roots: vec![closure],
        ..Recording::default()
    };
    MarkSweepCollector::new().collect(&mut heap, &[&recording]);
    s.assert_survived(&heap);
    assert_eq!(
        *recording.code_ids.borrow(),
        [7],
        "HeapObjectData::VmClosure.code_id: queued for trace_code_ids"
    );
}

// ----------------------------------------------------------------------------
// Code ids (#836)
// ----------------------------------------------------------------------------

/// A provider standing in for the VM: it roots `roots`; records each batch
/// of code ids and of continuation ids the fixpoint broadcasts to it; for
/// a code id it is given, keeps the value `keyed` pairs with it, as the VM
/// traces the unit's link table; and records, for each id in `probe`,
/// whether marking reached it as a code id and as a continuation id.
#[derive(Default)]
struct Recording {
    roots: Vec<TaggedValue>,
    keyed: Vec<(u64, TaggedValue)>,
    probe: Vec<u64>,
    code_ids: RefCell<Vec<u64>>,
    weak_ids: RefCell<Vec<u64>>,
    /// `(id, code_id_is_live, weak_continuation_id_is_live)`.
    live: RefCell<Vec<(u64, bool, bool)>>,
}

impl RootProvider for Recording {
    fn trace_roots(&self, visitor: &mut dyn SlotVisitor) {
        for &value in &self.roots {
            visitor.pinned(value);
        }
    }

    fn trace_weak_ids(&self, ids: &[u64], _visitor: &mut dyn SlotVisitor) {
        self.weak_ids.borrow_mut().extend_from_slice(ids);
    }

    fn trace_code_ids(&self, ids: &[u64], visitor: &mut dyn SlotVisitor) {
        self.code_ids.borrow_mut().extend_from_slice(ids);
        for &(id, value) in &self.keyed {
            if ids.contains(&id) {
                visitor.pinned(value);
            }
        }
    }

    fn sweep_weak(&self, visitor: &GcVisitor<'_>) {
        let mut live = self.live.borrow_mut();
        for &id in &self.probe {
            live.push((
                id,
                visitor.code_id_is_live(id),
                visitor.weak_continuation_id_is_live(id),
            ));
        }
    }
}

/// A VM closure with code id `code_id` over `free_vars`.
fn vm_closure(heap: &mut Heap, code_id: u64, free_vars: Vec<TaggedValue>) -> TaggedValue {
    heap.alloc_vm_closure(code_id, free_vars)
}

/// A closure reachable only from a root broadcasts its code id to
/// `trace_code_ids`, which keeps what the provider keys by it: here a
/// sentinel nothing else reaches, as a unit's table is in the VM. Its id is
/// live by `code_id_is_live` and not by the continuation ids' test.
#[test]
fn a_marked_closure_broadcasts_its_code_id() {
    let mut heap = Heap::new();
    let mut s = Sentinels::new(&mut heap);
    let keyed = s.pair(&mut heap, "trace_code_ids(7): what the provider keys by it");
    let closure = vm_closure(&mut heap, 7, Vec::new());
    let recording = Recording {
        roots: vec![closure],
        keyed: vec![(7, keyed)],
        probe: vec![7],
        ..Recording::default()
    };
    MarkSweepCollector::new().collect(&mut heap, &[&recording]);
    s.assert_survived(&heap);
    assert_eq!(*recording.code_ids.borrow(), [7]);
    assert_eq!(*recording.live.borrow(), [(7, true, false)]);
}

/// A retired closure names no code and broadcasts nothing. It is reached
/// through closure 7's free variable, so it is traced after closure 7 has
/// queued its id and moved `note_code_id`'s last id off
/// `RETIRED_VM_CLOSURE_CODE`, where it starts: a retired closure traced
/// first would be passed over by that fast path alone, and could not show
/// the retired id's own test.
#[test]
fn a_retired_closure_broadcasts_no_code_id() {
    let mut heap = Heap::new();
    let retired = vm_closure(&mut heap, Heap::RETIRED_VM_CLOSURE_CODE, Vec::new());
    let closure = vm_closure(&mut heap, 7, vec![retired]);
    let recording = Recording {
        roots: vec![closure],
        probe: vec![Heap::RETIRED_VM_CLOSURE_CODE],
        ..Recording::default()
    };
    MarkSweepCollector::new().collect(&mut heap, &[&recording]);
    assert_eq!(
        *recording.code_ids.borrow(),
        [7],
        "the retired id is not queued"
    );
    assert_eq!(
        *recording.live.borrow(),
        [(Heap::RETIRED_VM_CLOSURE_CODE, false, false)]
    );
}

/// Code ids and continuation ids are apart (S6): both count from 0, so on a
/// fresh heap the first continuation ref and a closure of the first unit a
/// VM loads have the same id, and each must still reach its own provider
/// method and its own liveness test, whichever is marked first.
#[test]
fn a_code_id_equal_to_a_continuation_id_reaches_both_rounds() {
    let mut heap = Heap::new();
    let mut s = Sentinels::new(&mut heap);
    let keyed = s.pair(&mut heap, "trace_code_ids(0): what the provider keys by it");
    let (continuation, id) = heap.alloc_vm_continuation_ref(0);
    assert_eq!(id, 0, "the first continuation id a heap mints");
    let closure = vm_closure(&mut heap, 0, Vec::new());
    let recording = Recording {
        roots: vec![continuation, closure],
        keyed: vec![(0, keyed)],
        probe: vec![0],
        ..Recording::default()
    };
    MarkSweepCollector::new().collect(&mut heap, &[&recording]);
    s.assert_survived(&heap);
    assert_eq!(*recording.code_ids.borrow(), [0], "the code id");
    assert_eq!(*recording.weak_ids.borrow(), [0], "the continuation id");
    assert_eq!(*recording.live.borrow(), [(0, true, true)]);
}

/// A closure reachable only through an ephemeron's datum (S5) is marked in
/// the fixpoint's ephemeron pass, after the round's code ids were taken, so
/// its id is broadcast in the next round: the code-id round is in the
/// loop, not once after the roots.
#[test]
fn a_closure_under_an_ephemeron_broadcasts_in_a_later_round() {
    let mut heap = Heap::new();
    let mut s = Sentinels::new(&mut heap);
    let keyed = s.pair(&mut heap, "trace_code_ids(9): what the provider keys by it");
    let key = heap.alloc_pair(TaggedValue::fixnum(0), TaggedValue::NULL);
    let datum = vm_closure(&mut heap, 9, Vec::new());
    let ephemeron = heap.alloc_object(HeapObjectData::Ephemeron(Interior::new(Some((key, datum)))));
    let recording = Recording {
        roots: vec![ephemeron, key],
        keyed: vec![(9, keyed)],
        ..Recording::default()
    };
    MarkSweepCollector::new().collect(&mut heap, &[&recording]);
    s.assert_survived(&heap);
    assert_eq!(*recording.code_ids.borrow(), [9]);
}

/// A code-id round that marks a continuation ref is followed by a weak-id
/// round that broadcasts it: the code-id round asks for another because it
/// queued a weak id, so the loop does not stop with the ref marked and its
/// payload untraced.
#[test]
fn a_code_id_round_feeds_the_weak_id_round() {
    let mut heap = Heap::new();
    let (continuation, id) = heap.alloc_vm_continuation_ref(0);
    let closure = vm_closure(&mut heap, 7, Vec::new());
    let recording = Recording {
        roots: vec![closure],
        keyed: vec![(7, continuation)],
        ..Recording::default()
    };
    MarkSweepCollector::new().collect(&mut heap, &[&recording]);
    assert_eq!(*recording.code_ids.borrow(), [7]);
    assert_eq!(
        *recording.weak_ids.borrow(),
        [id],
        "broadcast in a later round"
    );
}

/// A code-id round that marks a closure of another unit is followed by a
/// round that broadcasts that closure's id, as a unit's table reaches a
/// closure of another unit: the round asks for another because it queued
/// an id.
#[test]
fn a_code_id_round_feeds_the_next_code_id_round() {
    let mut heap = Heap::new();
    let mut s = Sentinels::new(&mut heap);
    let keyed = s.pair(&mut heap, "trace_code_ids(8): what the provider keys by it");
    let first = vm_closure(&mut heap, 7, Vec::new());
    let second = vm_closure(&mut heap, 8, Vec::new());
    let recording = Recording {
        roots: vec![first],
        keyed: vec![(7, second), (8, keyed)],
        ..Recording::default()
    };
    MarkSweepCollector::new().collect(&mut heap, &[&recording]);
    s.assert_survived(&heap);
    assert_eq!(
        *recording.code_ids.borrow(),
        [7, 8],
        "8 broadcast in a later round"
    );
}

/// An ephemeron a code-id round reaches, under a key rooted elsewhere, is
/// retained by the same round's ephemeron pass, which comes after the
/// code-id block: the round queued no id, so no later round would see it.
#[test]
fn an_ephemeron_a_code_id_round_reaches_is_retained_in_that_round() {
    let mut heap = Heap::new();
    let mut s = Sentinels::new(&mut heap);
    let key = heap.alloc_pair(TaggedValue::fixnum(0), TaggedValue::NULL);
    let datum = s.vector(
        &mut heap,
        "HeapObjectData::Ephemeron datum, the ephemeron reached by trace_code_ids(7)",
    );
    let ephemeron = heap.alloc_object(HeapObjectData::Ephemeron(Interior::new(Some((key, datum)))));
    let closure = vm_closure(&mut heap, 7, Vec::new());
    let recording = Recording {
        roots: vec![closure, key],
        keyed: vec![(7, ephemeron)],
        ..Recording::default()
    };
    MarkSweepCollector::new().collect(&mut heap, &[&recording]);
    s.assert_survived(&heap);
    assert_eq!(
        heap.ephemeron_state(ephemeron),
        Some(Some((key, datum))),
        "retained, not broken"
    );
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
    let ephemeron = heap.alloc_object(HeapObjectData::Ephemeron(Interior::new(Some((key, datum)))));
    collect_from(&mut heap, &[ephemeron, key]);
    s.assert_survived(&heap);
}

// ----------------------------------------------------------------------------
// The scope-set table (#788): an identifier's set lives while the identifier
// does, and an entry no marked identifier holds is recycled. A set's id
// carries no generation stamp, so a test of reuse asserts that the new set
// took the freed id, which holds in a check build as in any other.
// ----------------------------------------------------------------------------

/// The set made of these scope ids. Not minted (`ScopeId::fresh`), so the
/// tests leave the process's scope counter as it was.
fn scope_set_of(ids: &[usize]) -> ScopeSet {
    let mut set = ScopeSet::new();
    for &id in ids {
        set.add_scope(ScopeId(id));
    }
    set
}

/// The id of `identifier`'s set in its heap's table.
fn scope_set_id(heap: &Heap, identifier: TaggedValue) -> super::scope_sets::ScopeSetId {
    match heap.get_object(identifier) {
        HeapObjectData::Identifier { scopes, .. } => *scopes,
        other => panic!("not an identifier: {other:?}"),
    }
}

/// A set only one identifier carries, the identifier reachable only through
/// a pair, keeps its entry across a collection: its id, and its contents. A
/// new set interned after the collection takes another entry, which it would
/// not if the sweep had freed this one. Fails with the identifier's arm in
/// `trace_object_children` recording nothing (#164's rule).
#[test]
fn an_identifiers_scope_set_lives_with_it() {
    let mut heap = Heap::new();
    let decoy = heap.alloc_pair(TaggedValue::fixnum(-1), TaggedValue::NULL);
    // Four scopes, so the set spills past the inline three.
    let set = scope_set_of(&[901, 902, 903, 904]);
    let identifier = heap.alloc_identifier(Rc::from("x"), set.clone());
    let id = scope_set_id(&heap, identifier);
    let root = heap.alloc_pair(identifier, TaggedValue::NULL);
    collect_from(&mut heap, &[root]);
    super::sentinels::assert_freed(&heap, decoy, "the decoy");
    assert_eq!(scope_set_id(&heap, identifier), id);
    let newcomer = heap.scope_sets.intern(scope_set_of(&[905]));
    assert_ne!(newcomer, id, "the live identifier's entry was reused");
    assert_eq!(heap.scope_set(id), &set);
    assert_eq!(heap.get_identifier_data(identifier).unwrap().1, &set);
    assert_eq!(heap.scope_sets.intern(set), id);
}

/// A set no marked identifier carries is freed, and the next set interned
/// takes its entry: the same id, asserted equal (the section's note), with
/// every live identifier's set as it was. The freed entry lies between two
/// live ones, so it is the free list that hands it back.
#[test]
fn a_scope_set_no_identifier_carries_is_recycled() {
    let mut heap = Heap::new();
    let low = heap.alloc_identifier(Rc::from("low"), scope_set_of(&[911]));
    let dead = heap.alloc_identifier(Rc::from("dead"), scope_set_of(&[912, 913, 914, 915]));
    let high = heap.alloc_identifier(Rc::from("high"), scope_set_of(&[916, 917]));
    let freed = scope_set_id(&heap, dead);
    assert_eq!(heap.scope_sets.len(), 3);
    collect_from(&mut heap, &[low, high]);
    super::sentinels::assert_freed(&heap, dead, "the identifier no root reaches");
    assert_eq!(heap.scope_sets.len(), 2);
    let set = scope_set_of(&[918, 919, 920]);
    let newcomer = heap.alloc_identifier(Rc::from("new"), set.clone());
    assert_eq!(scope_set_id(&heap, newcomer), freed);
    assert_eq!(heap.get_identifier_data(newcomer).unwrap().1, &set);
    assert_eq!(
        heap.get_identifier_data(low).unwrap().1,
        &scope_set_of(&[911])
    );
    assert_eq!(
        heap.get_identifier_data(high).unwrap().1,
        &scope_set_of(&[916, 917])
    );
}

/// The empty set's entry, id 0, survives a collection that marks no
/// identifier at all, written or not: a set interned after it takes another
/// entry, and an identifier made after it still reads the empty set.
#[test]
fn the_empty_scope_set_is_never_recycled() {
    let mut heap = Heap::new();
    let written = heap.alloc_source_identifier(Rc::from("written"));
    let plain = heap.alloc_identifier(Rc::from("plain"), ScopeSet::new());
    let scoped = heap.alloc_identifier(Rc::from("scoped"), scope_set_of(&[921]));
    collect_from(&mut heap, &[]);
    for (identifier, what) in [(written, "written"), (plain, "plain"), (scoped, "scoped")] {
        super::sentinels::assert_freed(&heap, identifier, what);
    }
    let set = scope_set_of(&[922]);
    let newcomer = heap.alloc_identifier(Rc::from("new"), set.clone());
    let fresh = heap.alloc_source_identifier(Rc::from("fresh"));
    assert_ne!(
        scope_set_id(&heap, newcomer),
        super::scope_sets::ScopeSetId::EMPTY
    );
    assert_eq!(heap.get_identifier_data(newcomer).unwrap().1, &set);
    assert_eq!(
        scope_set_id(&heap, fresh),
        super::scope_sets::ScopeSetId::EMPTY
    );
    assert!(heap.get_identifier_data(fresh).unwrap().1.is_empty());
}

/// In a check build, reading a free entry panics: here an id kept past the
/// collection that freed it. The entry lies between two live ones, so the
/// sweep keeps it as a free entry rather than dropping it from the end.
#[test]
#[cfg(any(debug_assertions, feature = "gc-check"))]
#[should_panic(expected = "is free")]
fn a_check_build_refuses_a_free_scope_set() {
    let mut heap = Heap::new();
    let low = heap.alloc_identifier(Rc::from("low"), scope_set_of(&[931]));
    let dead = heap.alloc_identifier(Rc::from("dead"), scope_set_of(&[932]));
    let high = heap.alloc_identifier(Rc::from("high"), scope_set_of(&[933]));
    let freed = scope_set_id(&heap, dead);
    collect_from(&mut heap, &[low, high]);
    heap.scope_set(freed);
}
