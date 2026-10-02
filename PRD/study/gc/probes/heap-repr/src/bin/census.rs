use patina_core::heap::HeapObjectType;
use patina_core::TaggedValue;
use patina_interpreter::VmInterpreter;
use patina_runtime::Backend;
use std::collections::BTreeMap;

fn main() {
    let prog = std::env::args().nth(1).expect("program text");
    let interp = VmInterpreter::new_vm();
    let (r, _) = interp.eval_program_with_source_name(&prog, "census.scm");
    if let Err(e) = r { eprintln!("error: {e:?}"); }
    let heap = interp.backend().global_env().heap().clone();
    let h = heap.borrow();
    let st = h.stats();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for i in 0..st.objects {
        let t = h.get_object_type(TaggedValue::object(i as u32));
        *counts.entry(format!("{:?}", t)).or_default() += 1;
    }
    println!("arena slots: pairs={} vectors={} strings={} objects={} | free: p={} v={} s={} o={} | collections={}",
        st.pairs, st.vectors, st.strings, st.objects, st.free_pairs, st.free_vectors, st.free_strings, st.free_objects, st.gc_collections);
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, n) in v { println!("  {k:<28} {n}"); }
    let _ = HeapObjectType::Free;
}
