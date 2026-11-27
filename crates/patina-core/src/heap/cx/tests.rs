//! The context, the funnel and the reshaped reads (#703).

use std::rc::Rc;

use super::*;
use crate::heap::new_shared_heap;
use crate::{GcDriver, Instance, NoGcScope, SharedHeap};

/// Run `f` with the driver's context over a fresh heap.
fn with_cx<R>(f: impl for<'gc> FnOnce(&mut Cx<'gc>) -> R) -> R {
    let heap = new_shared_heap();
    let instance = Instance::default();
    let mut driver = GcDriver::acquire(&heap).expect("the carrier's driver is free");
    driver.mutate(&instance, f)
}

/// Brand a word the test's own raw heap made, for the slot kinds the safe
/// crates cannot allocate (cells and closures are the machines').
fn branded<'gc>(_cx: &Cx<'gc>, word: TaggedValue) -> Value<'gc> {
    // SAFETY: the word is live in the context's heap for the test's extent.
    unsafe { Value::from_tagged(word) }
}

#[test]
fn a_context_reads_allocates_and_stores() {
    with_cx(|cx| {
        let pair = cx.alloc_pair(Value::fixnum(1), Value::NULL);
        assert_eq!(cx.car(pair), Value::fixnum(1));
        cx.set_cdr(pair, Value::TRUE);
        assert_eq!(cx.get_pair(pair), (Value::fixnum(1), Value::TRUE));
        let list = cx.list_from_iter([Value::fixnum(1), Value::fixnum(2)]);
        assert_eq!(cx.list_len(list), Some(2));
        assert_eq!(cx.type_name(list), "pair");
    });
}

#[test]
fn every_slot_kind_stores_through_the_funnel() {
    let heap: SharedHeap = new_shared_heap();
    let instance = Instance::default();
    let cell = heap.borrow_mut().alloc_mutable_cell(TaggedValue::FALSE);
    let closure = heap
        .borrow_mut()
        .alloc_vm_closure(0, vec![TaggedValue::FALSE]);
    let mut driver = GcDriver::acquire(&heap).unwrap();
    driver.mutate(&instance, |cx| {
        let pair = cx.alloc_pair(Value::NULL, Value::NULL);
        cx.store(PairCar(pair), Value::fixnum(1));
        cx.store(PairCdr(pair), Value::fixnum(2));
        assert_eq!(cx.get_pair(pair), (Value::fixnum(1), Value::fixnum(2)));

        let vector = cx.alloc_vector_fill(3, Value::FALSE);
        cx.store(VectorElem(vector, 2), Value::TRUE);
        assert_eq!(cx.vector_ref(vector, 2), Value::TRUE);

        let cell = branded(cx, cell);
        cx.store(CellValue(cell), Value::fixnum(3));
        assert_eq!(
            cx.heap.borrow().read_mutable_cell(cell.tagged()),
            Some(TaggedValue::fixnum(3))
        );

        let rtd = Rc::new(crate::record_type::RecordTypeDescriptor::new(
            "point",
            vec!["x".into(), "y".into()],
        ));
        let record = cx.alloc_record(rtd, vec![Value::fixnum(0), Value::fixnum(0)]);
        cx.store(RecordField(record, 1), Value::fixnum(4));
        assert_eq!(cx.record_ref(record, 1), Some(Value::fixnum(4)));
        assert_eq!(cx.record_len(record), Some(2));

        let closure = branded(cx, closure);
        cx.store(ClosureFree(closure, 0), Value::fixnum(5));
        assert_eq!(
            cx.heap
                .borrow()
                .get_vm_closure_free_var(closure.tagged(), 0),
            Some(TaggedValue::fixnum(5))
        );

        let promise = cx.alloc_promise(PromiseView::Delayed(Value::FALSE));
        cx.store(PromiseBox(promise), Value::fixnum(6));
        assert_eq!(
            cx.promise_state(promise),
            Some(PromiseView::Forced(Value::fixnum(6)))
        );

        let param = cx.alloc_parameter(Value::fixnum(0), None);
        cx.store(ParamValue(param), Value::fixnum(7));
        assert_eq!(cx.parameter_value(param), Some(Value::fixnum(7)));
        cx.store(ConvertedParamValue(param), Value::fixnum(8));
        assert_eq!(cx.parameter_value(param), Some(Value::fixnum(8)));
        assert_eq!(cx.parameter_converter(param), Some(None));
    });
}

/// The trusted crates' entry to the funnel, `Heap::store`, writes each slot
/// as `Cx::store` does, the machines' twins included (#708).
#[test]
fn the_heap_stores_through_the_funnel() {
    let heap = new_shared_heap();
    let mut h = heap.borrow_mut();
    let vector = h.alloc_vector(vec![TaggedValue::FALSE; 2]);
    let promise = h.alloc_promise(PromiseState::Delayed(TaggedValue::FALSE));
    let param = h.alloc_parameter(TaggedValue::fixnum(0), None);
    // SAFETY: each word was allocated above, on this heap, which nothing
    // collects here.
    let (vector_v, promise_v, param_v) = unsafe {
        (
            Value::from_tagged(vector),
            Value::from_tagged(promise),
            Value::from_tagged(param),
        )
    };
    h.store(VectorElem(vector_v, 0), Value::fixnum(1));
    h.store(MachineVectorElem(vector_v, 1), Value::fixnum(2));
    assert_eq!(
        h.vector_slice(vector),
        [TaggedValue::fixnum(1), TaggedValue::fixnum(2)]
    );
    h.store(MachinePromiseBox(promise_v), Value::fixnum(3));
    assert!(matches!(
        h.promise_state(promise),
        Some(PromiseState::Forced(v)) if v == TaggedValue::fixnum(3)
    ));
    h.store(ParamValue(param_v), Value::fixnum(4));
    assert_eq!(
        h.parameter_parts(param),
        Some((Some(TaggedValue::fixnum(4)), None))
    );
}

/// `Heap::try_store` stores into a slot that exists and answers `true`; for
/// one that does not, it writes nothing and answers `false`, where
/// `Heap::store` panics (#708).
#[test]
fn the_heap_tries_a_store_into_a_slot_that_may_not_exist() {
    let heap = new_shared_heap();
    let mut h = heap.borrow_mut();
    let vector = h.alloc_vector(vec![TaggedValue::FALSE; 2]);
    let cell = h.alloc_mutable_cell(TaggedValue::FALSE);
    // SAFETY: each word was allocated above, on this heap, which nothing
    // collects here.
    let (vector_v, cell_v) = unsafe { (Value::from_tagged(vector), Value::from_tagged(cell)) };
    assert!(h.try_store(MachineVectorElem(vector_v, 1), Value::fixnum(1)));
    assert!(!h.try_store(MachineVectorElem(vector_v, 2), Value::fixnum(2)));
    assert!(!h.try_store(MachineVectorElem(cell_v, 0), Value::fixnum(3)));
    assert_eq!(
        h.vector_slice(vector),
        [TaggedValue::FALSE, TaggedValue::fixnum(1)]
    );
    assert!(h.try_store(CellValue(cell_v), Value::fixnum(4)));
    assert!(!h.try_store(CellValue(vector_v), Value::fixnum(5)));
    assert_eq!(h.read_mutable_cell(cell), Some(TaggedValue::fixnum(4)));
}

#[test]
#[should_panic(expected = "Cx::store: no such slot")]
fn a_store_into_the_wrong_object_panics() {
    with_cx(|cx| {
        let vector = cx.alloc_vector_fill(1, Value::FALSE);
        cx.store(PairCar(vector), Value::TRUE);
    });
}

#[test]
#[should_panic(expected = "Cx::store: no such slot")]
fn a_store_past_a_vectors_end_panics() {
    with_cx(|cx| {
        let vector = cx.alloc_vector_fill(1, Value::FALSE);
        cx.store(VectorElem(vector, 1), Value::TRUE);
    });
}

#[test]
fn ranges_store_and_fill_a_run_of_elements() {
    with_cx(|cx| {
        let vector = cx.alloc_vector_fill(5, Value::FALSE);
        cx.store_range(vector, 1, [Value::fixnum(1), Value::fixnum(2)].into_iter());
        cx.fill_range(vector, 3, 2, Value::TRUE);
        let elements: Vec<_> = cx.vector_slice(vector).to_vec();
        assert_eq!(
            elements,
            [
                Value::FALSE,
                Value::fixnum(1),
                Value::fixnum(2),
                Value::TRUE,
                Value::TRUE
            ]
        );
    });
}

#[test]
#[should_panic(expected = "Cx::store: no such slot")]
fn a_range_past_a_vectors_end_panics() {
    with_cx(|cx| {
        let vector = cx.alloc_vector_fill(2, Value::FALSE);
        cx.fill_range(vector, 1, 2, Value::TRUE);
    });
}

#[test]
fn a_fresh_vector_takes_its_initializing_stores() {
    with_cx(|cx| {
        let mut fresh = cx.fresh_vector(3, Value::UNSPECIFIED);
        for i in 0..3 {
            fresh.init(i, Value::fixnum(i as i64));
        }
        let vector = fresh.finish();
        assert_eq!(cx.vector_ref(vector, 2), Value::fixnum(2));
    });
}

#[test]
fn strings_are_written_through_the_context() {
    with_cx(|cx| {
        let s = cx.alloc_string("abcde".into());
        cx.string_fill(s, 'x', 1..3);
        cx.string_copy_chars(s, 3, &['y', 'z']);
        assert_eq!(&*cx.get_string_chars(s), ['a', 'x', 'x', 'y', 'z']);
        assert_eq!(cx.get_string_contents(s).as_deref(), Some("axxyz"));
    });
}

#[test]
fn reads_that_handed_out_heap_references_answer_owned_or_borrowed() {
    with_cx(|cx| {
        let big = cx.alloc_bigint(num_bigint::BigInt::from(1u64 << 62) * 4);
        assert_eq!(
            cx.get_bigint(big),
            Some(num_bigint::BigInt::from(1u64 << 62) * 4)
        );
        let sym = cx.intern_symbol("hello");
        assert_eq!(cx.get_symbol_name(sym).as_deref(), Some("hello"));
        let values = cx.alloc_values(vec![Value::fixnum(1), Value::TRUE]);
        assert_eq!(
            cx.get_values(values).map(|vs| vs.to_vec()),
            Some(vec![Value::fixnum(1), Value::TRUE])
        );
    });
}

#[test]
fn a_context_is_counted_while_it_lives() {
    let heap = new_shared_heap();
    let instance = Instance::default();
    let live = || heap.borrow().mutator().live_cx();
    let mut driver = GcDriver::acquire(&heap).unwrap();
    driver.mutate(&instance, |_| {
        assert_eq!(live(), u32::from(crate::GC_CHECK));
    });
    assert_eq!(live(), 0);
    let scope = NoGcScope::enter(&heap);
    scope.mutate(&instance, |_| {
        assert_eq!(live(), u32::from(crate::GC_CHECK));
    });
    drop(scope);
    assert_eq!(live(), 0);
}

#[test]
fn a_context_carries_its_instance() {
    let heap = new_shared_heap();
    let instance = Instance::default();
    instance.set_command_line("prog".into(), vec!["arg".into()]);
    let mut driver = GcDriver::acquire(&heap).unwrap();
    driver.mutate(&instance, |cx| {
        assert_eq!(&*cx.instance().command_line(), ["prog", "arg"]);
    });
}

#[test]
fn syntax_identifiers_map_through_the_context() {
    with_cx(|cx| {
        let x = cx.alloc_identifier("x".into(), ScopeSet::new());
        let list = cx.list_from_iter([x, Value::fixnum(1)]);
        let mapped =
            cx.map_syntax_identifiers(list, |cx, _id, name, _scopes| Some(cx.intern_symbol(&name)));
        assert_ne!(mapped, list, "a list holding an identifier is copied");
        let first = cx.car(mapped);
        assert_eq!(cx.get_symbol_name(first).as_deref(), Some("x"));
        assert_eq!(cx.car(list), x, "the original is unchanged");
    });
}

/// A frame write through a shared context takes no borrow and allocates
/// nothing (#834), which is why it needs no `&mut`; a namespace's write
/// borrows the heap mutably, and is refused there rather than taken.
#[test]
#[should_panic(expected = "a frame's own binding")]
fn a_frame_write_through_a_shared_context_refuses_a_namespace() {
    with_cx(|cx| {
        let namespace = cx.new_namespace();
        cx.define_with_scopes_in_frame(
            crate::NamespaceView::of(&namespace),
            "x",
            ScopeSet::new(),
            Value::UNSPECIFIED,
        );
    });
}

/// The id of the identifier `v`'s set in its heap's table (#788).
fn set_id_of(cx: &Cx<'_>, v: Value<'_>) -> crate::heap::scope_sets::ScopeSetId {
    match cx.heap.borrow().get_object(v.tagged()) {
        HeapObjectData::Identifier(identifier) => identifier.scopes,
        other => panic!("not an identifier: {other:?}"),
    }
}

/// A scope walk makes, and interns, each distinct set once (#788): two
/// identifiers with one set, flipped in one call, run the flip once and
/// share the set their copies get; a written identifier is left alone.
#[test]
fn a_rescoping_walk_edits_each_distinct_set_once() {
    with_cx(|cx| {
        let set = ScopeSet::singleton(ScopeId(41)).with_scope(ScopeId(42));
        let first = cx.alloc_identifier("a".into(), set.clone());
        let second = cx.alloc_identifier("b".into(), set.clone());
        let other = cx.alloc_identifier("c".into(), ScopeSet::singleton(ScopeId(44)));
        let written = cx.alloc_source_identifier("w".into());
        let list = cx.list_from_iter([first, second, other, written]);
        let held = cx.heap.borrow().scope_sets.len();
        let edits = std::cell::Cell::new(0);
        let flipped = cx.rescope_identifiers(
            list,
            |_, _| true,
            |scopes| {
                edits.set(edits.get() + 1);
                scopes.flip_scope(ScopeId(43))
            },
        );
        assert_eq!(edits.get(), 2, "one edit for each distinct set");
        let copies = cx.list_to_vec(flipped).expect("a list");
        assert_eq!(set_id_of(cx, copies[0]), set_id_of(cx, copies[1]));
        assert_ne!(set_id_of(cx, copies[0]), set_id_of(cx, copies[2]));
        assert_eq!(copies[3], written, "a written identifier is no copy");
        assert_eq!(
            cx.get_identifier_data(copies[1]).unwrap().1,
            set.flip_scope(ScopeId(43))
        );
        assert_eq!(cx.get_identifier_data(first).unwrap().1, set);
        assert_eq!(cx.heap.borrow().scope_sets.len(), held + 2);
    });
}

/// A scope addition's memo lasts across calls (#788): identifiers with one
/// set, copied in separate calls, share the set their copies get, and a
/// symbol made an identifier with the scope alone takes the set an
/// identifier with no scopes gets.
#[test]
fn a_scope_addition_interns_each_set_once_across_calls() {
    with_cx(|cx| {
        let scope = ScopeId(51);
        let set = ScopeSet::singleton(ScopeId(50));
        let first = cx.alloc_identifier("a".into(), set.clone());
        let second = cx.alloc_identifier("b".into(), set.clone());
        let bare = cx.alloc_identifier("c".into(), ScopeSet::new());
        let written = cx.alloc_source_identifier("w".into());
        let held = cx.heap.borrow().scope_sets.len();
        let mut addition = cx.scope_addition(scope);
        let copies: Vec<_> = [first, second, bare, written]
            .into_iter()
            .map(|v| {
                cx.add_scope_copying(&mut addition, v)
                    .expect("an identifier")
            })
            .collect();
        let d = cx.intern_symbol("d");
        let made = cx.alloc_identifier_adding_scope(&mut addition, d);
        assert_eq!(cx.add_scope_copying(&mut addition, Value::fixnum(1)), None);
        assert_eq!(set_id_of(cx, copies[0]), set_id_of(cx, copies[1]));
        assert_eq!(set_id_of(cx, copies[2]), set_id_of(cx, copies[3]));
        assert_eq!(set_id_of(cx, copies[2]), set_id_of(cx, made));
        assert_eq!(
            cx.get_identifier_data(copies[0]).unwrap().1,
            set.with_scope(scope)
        );
        assert_eq!(
            cx.get_identifier_data(made).unwrap(),
            ("d".into(), ScopeSet::singleton(scope))
        );
        assert!(
            !cx.is_source_identifier(copies[3]),
            "a copy is no written name"
        );
        assert_eq!(cx.get_identifier_data(first).unwrap().1, set);
        assert_eq!(cx.heap.borrow().scope_sets.len(), held + 2);
    });
}

/// Copies write no location (#791): a scope edit's copy, a substitution
/// mark's copy, a relinked alias's and a map's each carry their original's
/// source id and location, and the document tables hold as many entries
/// after the copies as before them.
#[test]
fn copies_of_an_identifier_keep_its_location_and_write_no_entry() {
    with_cx(|cx| {
        let mut map = crate::SourceMap::new();
        map.set_source_text("(f x)".into());
        let at = map.location("copies.scm", 1, 4, 1, 5);
        let original = cx.alloc_identifier("x".into(), ScopeSet::singleton(ScopeId(81)));
        cx.record_source(original, at.clone());
        let id = cx
            .heap
            .borrow()
            .identifier(original.tagged())
            .unwrap()
            .source;
        let entries = cx.heap.borrow().source_entries();
        let list = cx.list_from_iter([original]);
        let flipped = cx.rescope_identifiers(list, |_, _| true, |s| s.flip_scope(ScopeId(82)));
        let flipped = cx.car(flipped);
        let mut addition = cx.scope_addition(ScopeId(83));
        let marked = cx.add_scope_copying(&mut addition, original).unwrap();
        let name = cx.relinked_name("x.8-1".into());
        let alias = cx.alloc_relinked_copy(original, &name, ScopeSet::new());
        for copy in [flipped, marked, alias] {
            assert_ne!(copy, original);
            assert_eq!(
                cx.heap.borrow().identifier(copy.tagged()).unwrap().source,
                id
            );
            assert_eq!(cx.source(copy), Some(at.clone()));
        }
        assert_eq!(
            cx.heap.borrow().source_entries(),
            entries,
            "a copy wrote an entry"
        );
    });
}

/// The word the identifier `v` holds as its name (#790).
/// The word the identifier `v` holds as its name (#790).
fn name_of(cx: &Cx<'_>, v: Value<'_>) -> TaggedValue {
    match cx.heap.borrow().get_object(v.tagged()) {
        HeapObjectData::Identifier(identifier) => identifier.name,
        other => panic!("not an identifier: {other:?}"),
    }
}

/// An identifier's name is its spelling's interned symbol (#790): each
/// allocator interns it once, every identifier of one spelling holds the
/// one word, a copy holds its original's, and quote stripping answers that
/// word with no lookup. `eqv?` answers as it did, by the words.
#[test]
fn an_identifiers_name_is_its_spellings_interned_symbol() {
    with_cx(|cx| {
        let symbols = cx.heap.borrow().stats().symbols;
        let x = cx.intern_symbol("x");
        let scoped = cx.alloc_identifier("x".into(), ScopeSet::singleton(ScopeId(61)));
        let written = cx.alloc_source_identifier("x".into());
        let read = cx.alloc_source_identifier_for(x);
        let made = cx.alloc_identifier_for(x, ScopeSet::new());
        let y = cx.alloc_identifier("y".into(), ScopeSet::new());
        let mut addition = cx.scope_addition(ScopeId(62));
        let copy = cx.add_scope_copying(&mut addition, written).unwrap();
        let marked = cx.alloc_identifier_adding_scope(&mut addition, x);
        for v in [scoped, written, read, made, copy, marked] {
            assert_eq!(name_of(cx, v), x.tagged());
            assert_eq!(cx.identifier_datum(v), Some(x));
            assert!(cx.values_eqv(v, x) && cx.values_eqv(x, v) && cx.values_eqv(v, scoped));
            assert!(!cx.values_eqv(v, y));
        }
        assert_eq!(cx.identifier_datum(x), None, "a symbol is no identifier");
        assert_eq!(cx.get_identifier_data(copy).unwrap().0.as_ref(), "x");
        assert_eq!(cx.format_brief(marked), "x");
        // `x` and `y`, once each.
        assert_eq!(cx.heap.borrow().stats().symbols, symbols + 2);
    });
}

/// A relinked identifier's name is a symbol no table holds (#837, #790):
/// relinking adds no symbol, two relinked identifiers of one spelling are
/// `eqv?` to each other and to the interned symbol of it whether or not
/// they share a name, as every identifier of one spelling was, a copy stays
/// relinked, and in quoted data one stands for its spelling interned, as
/// quote stripping always gave it.
#[test]
fn a_relinked_identifiers_spelling_stays_uninterned() {
    with_cx(|cx| {
        let symbols = cx.heap.borrow().stats().symbols;
        let name = cx.relinked_name("secret.7-3".into());
        let other_name = cx.relinked_name("secret.7-3".into());
        let first = cx.alloc_relinked_identifier(&name, ScopeSet::singleton(ScopeId(71)));
        let second = cx.alloc_relinked_identifier(&name, ScopeSet::new());
        let elsewhere = cx.alloc_relinked_identifier(&other_name, ScopeSet::new());
        let mut addition = cx.scope_addition(ScopeId(72));
        let copy = cx.add_scope_copying(&mut addition, first).unwrap();
        assert_eq!(name_of(cx, first), name_of(cx, second));
        assert_ne!(name_of(cx, first), name_of(cx, elsewhere));
        assert_eq!(name_of(cx, copy), name_of(cx, first));
        assert!(
            matches!(
                cx.heap.borrow().get_object(copy.tagged()),
                HeapObjectData::Identifier(crate::heap::IdentifierData { relinked: true, .. })
            ),
            "a copy of a relinked identifier is relinked"
        );
        assert_eq!(
            cx.heap.borrow().stats().symbols,
            symbols,
            "relinking interns nothing"
        );
        assert_eq!(
            cx.get_identifier_data(elsewhere).unwrap().0.as_ref(),
            "secret.7-3"
        );
        assert_eq!(
            &*cx.get_symbol_or_identifier_name(copy).unwrap(),
            "secret.7-3"
        );
        assert_eq!(cx.format_brief(first), "secret.7-3");
        assert!(cx.values_eqv(first, second) && cx.values_eqv(first, elsewhere));
        let plain = cx.alloc_identifier("secret".into(), ScopeSet::new());
        assert!(!cx.values_eqv(first, plain) && !cx.values_eqv(plain, first));
        // Stripped, it is its spelling's interned symbol, the one a reader
        // of the spelling gets, and so `eqv?` to every relinked identifier
        // of it.
        let datum = cx.identifier_datum(elsewhere).unwrap();
        assert_eq!(cx.intern_symbol("secret.7-3"), datum);
        assert_eq!(cx.identifier_datum(copy), Some(datum));
        assert!(cx.values_eqv(datum, first) && cx.values_eqv(second, datum));
        assert_eq!(cx.heap.borrow().stats().symbols, symbols + 2);
    });
}
