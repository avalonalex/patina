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
    let env = Rc::new(crate::environment::Environment::with_heap(heap.clone()));
    let closure = heap
        .borrow_mut()
        .alloc_vm_closure(0, vec![TaggedValue::FALSE], env);
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
        assert_eq!(cx.parameter_converter(param), Some(None));
    });
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
    let mut instance = Instance::default();
    instance.set_command_line("prog".into(), vec!["arg".into()]);
    let mut driver = GcDriver::acquire(&heap).unwrap();
    driver.mutate(&instance, |cx| {
        assert_eq!(cx.instance().command_line(), ["prog", "arg"]);
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
