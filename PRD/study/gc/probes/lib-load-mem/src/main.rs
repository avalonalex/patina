mod prof;

use patina_core::heap::{Heap, HeapObjectType};
use patina_core::{ScopeSet, TaggedValue};
use patina_interpreter::Interpreter;
use patina_runtime::Backend;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;

#[global_allocator]
static A: prof::Prof = prof::Prof;

fn mb(b: i64) -> String {
    format!("{:.1} MB", b as f64 / 1048576.0)
}

fn class_table(label: &str) {
    println!("-- live size classes ({label}) --");
    let mut tot = 0i64;
    for c in 0..48 {
        let b = prof::CLASS_BYTES[c].load(Relaxed);
        let n = prof::CLASS_COUNT[c].load(Relaxed);
        let t = prof::CLASS_TOTAL[c].load(Relaxed);
        if n != 0 || t != 0 {
            let lo = if c == 0 { 0 } else { 1usize << (c - 1) };
            let hi = (1usize << c) - 1;
            println!("  [{lo:>9}..{hi:>9}] live_blocks={n:>9} live={:>10}  cumulative={:>10}", mb(b), mb(t as i64));
            tot += b;
        }
    }
    println!("  total live {}", mb(tot));
}

fn census(heap: &Heap, label: &str) {
    let st = heap.stats();
    println!("== census {label}: pairs={} vectors={} strings={} objects={} symbols={} | free p={} v={} s={} o={} | collections={}",
        st.pairs, st.vectors, st.strings, st.objects, st.symbols, st.free_pairs, st.free_vectors, st.free_strings, st.free_objects, st.gc_collections);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut scope_len: BTreeMap<usize, usize> = BTreeMap::new();
    let mut distinct_sets: HashMap<ScopeSet, usize> = HashMap::new();
    let mut distinct_ids: HashSet<(usize, ScopeSet)> = HashSet::new();
    let mut distinct_names: HashSet<String> = HashSet::new();
    let mut distinct_str_ids: HashSet<(String, ScopeSet)> = HashSet::new();
    let mut written_n = 0usize;
    let mut name_ptrs: HashSet<usize> = HashSet::new();
    let mut name_bytes_unique_ptr = 0usize;
    let mut idents = 0usize;
    let mut ident_with_src = 0usize;
    let mut obj_with_src = 0usize;
    let mut max_scope_id = 0usize;
    for i in 0..st.objects {
        let tv = TaggedValue::object(i as u32);
        let t = heap.get_object_type(tv);
        *counts.entry(format!("{:?}", t)).or_default() += 1;
        if t == HeapObjectType::Free {
            continue;
        }
        if heap.source(tv).is_some() || heap.child_source(tv, 0).is_some() {
            obj_with_src += 1;
        }
        if let Some((name, scopes)) = heap.get_identifier_data(tv) {
            idents += 1;
            if heap.source(tv).is_some() {
                ident_with_src += 1;
            }
            *scope_len.entry(scopes.len()).or_default() += 1;
            for s in scopes.iter() {
                max_scope_id = max_scope_id.max(s.0);
            }
            *distinct_sets.entry(scopes.clone()).or_default() += 1;
            let p = Rc::as_ptr(name) as *const u8 as usize;
            if name_ptrs.insert(p) {
                name_bytes_unique_ptr += name.len();
            }
            distinct_names.insert(name.to_string());
            distinct_ids.insert((Rc::as_ptr(name) as *const u8 as usize, scopes.clone()));
            distinct_str_ids.insert((name.to_string(), scopes.clone()));
            if let patina_core::heap::HeapObjectData::Identifier { written: true, .. } = heap.get_object(tv) { written_n += 1; }
        }
    }
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, n) in &v {
        println!("  {k:<28} {n:>9}  ({})", mb((*n * 72) as i64));
    }
    println!("  identifiers={idents} with_source={ident_with_src}; objects with any provenance={obj_with_src}");
    println!("  identifier scope-set length histogram: {:?}", scope_len);
    let spilled: usize = scope_len.iter().filter(|(k, _)| **k > 3).map(|(_, v)| *v).sum();
    println!("  spilled (len>3) scope sets: {spilled}; distinct scope sets: {}; distinct (name-ptr, scopes): {}; distinct name ptrs: {}; distinct name strings: {}; unique-ptr name bytes: {}; max scope id {}",
        distinct_sets.len(), distinct_ids.len(), name_ptrs.len(), distinct_names.len(), name_bytes_unique_ptr, max_scope_id);
    println!("  distinct (name-string, scopes) = {}; written identifiers = {}", distinct_str_ids.len(), written_n);
    // Top scope sets by frequency
    let mut ds: Vec<_> = distinct_sets.iter().map(|(k, v)| (*v, k.len())).collect();
    ds.sort_by(|a, b| b.cmp(a));
    println!("  top-10 scope-set frequencies (count, len): {:?}", &ds[..ds.len().min(10)]);
    let mut len_hist_distinct: BTreeMap<usize, usize> = BTreeMap::new();
    for k in distinct_sets.keys() {
        *len_hist_distinct.entry(k.len()).or_default() += 1;
    }
    println!("  distinct scope sets by length: {:?}", len_hist_distinct);

    // Pairs / vectors / strings with provenance (only meaningful before any sweep).
    let mut pairs_src = 0usize;
    let mut pairs_child = 0usize;
    for i in 0..st.pairs {
        let tv = TaggedValue::pair(i as u32);
        if heap.source(tv).is_some() {
            pairs_src += 1;
        }
        if heap.child_source(tv, 0).is_some() || heap.child_source(tv, 1).is_some() {
            pairs_child += 1;
        }
    }
    let mut vec_elems = 0usize;
    let mut vec_src = 0usize;
    for i in 0..st.vectors {
        let tv = TaggedValue::vector(i as u32);
        vec_elems += heap.vector_len(tv);
        if heap.source(tv).is_some() {
            vec_src += 1;
        }
    }
    let mut str_chars = 0usize;
    for i in 0..st.strings {
        let tv = TaggedValue::string(i as u32);
        str_chars += heap.get_string_chars(tv).len();
    }
    println!("  pairs with location={pairs_src}, with child spans={pairs_child}; vectors elems={vec_elems} (with location {vec_src}); string chars={str_chars}");
}

fn run<B: Backend>(make: impl FnOnce() -> Interpreter<B>, prog: &str, out: &str, do_census: bool)
where
    B::Error: std::error::Error,
{
    let t = Instant::now();
    let interp = make();
    let boot_ms = t.elapsed().as_secs_f64() * 1e3;
    let boot_live = prof::LIVE.load(Relaxed);
    let boot_peak = prof::PEAK.load(Relaxed);
    let boot_snap = prof::snapshot();
    let heap = interp.backend().global_env().heap().clone();
    println!("BOOT  {boot_ms:.0} ms live={} peak={} rss={} KB maxrss={} KB total_alloc={} count={}",
        mb(boot_live), mb(boot_peak), prof::rss_kb(), prof::max_rss_kb(), mb(prof::TOTAL.load(Relaxed) as i64), prof::COUNT.load(Relaxed));
    {
        let st = heap.borrow().stats();
        println!("      heap after boot: pairs={} objects={} vectors={} strings={} symbols={} collections={}", st.pairs, st.objects, st.vectors, st.strings, st.symbols, st.gc_collections);
    }
    class_table("after boot");
    prof::reset_peak();
    let total0 = prof::TOTAL.load(Relaxed);
    let count0 = prof::COUNT.load(Relaxed);
    let t = Instant::now();
    let r = interp.eval_program(prog);
    let load_ms = t.elapsed().as_secs_f64() * 1e3;
    if let Err(e) = &r {
        println!("ERROR {e}");
    }
    let load_live = prof::LIVE.load(Relaxed);
    let load_peak = prof::PEAK.load(Relaxed);
    let load_snap = prof::snapshot();
    let (peak_live, peak_snap) = prof::take_peak_snapshot();
    println!("LOAD  {load_ms:.0} ms live={} peak={} (peak-snapshot at {}) rss={} KB maxrss={} KB alloc_during_load={} count={}",
        mb(load_live), mb(load_peak), mb(peak_live), prof::rss_kb(), prof::max_rss_kb(),
        mb((prof::TOTAL.load(Relaxed) - total0) as i64), prof::COUNT.load(Relaxed) - count0);
    class_table("after load");
    if do_census {
        let h = heap.borrow();
        census(&h, "after load (before gc)");
    }
    let before_gc_live = prof::LIVE.load(Relaxed);
    let t = Instant::now();
    let _ = interp.eval_program("(gc)");
    let gc_ms = t.elapsed().as_secs_f64() * 1e3;
    let _ = interp.eval_program("(gc)");
    let gc_live = prof::LIVE.load(Relaxed);
    let gc_snap = prof::snapshot();
    println!("GC    {gc_ms:.1} ms live={} (freed {}) rss={} KB", mb(gc_live), mb(before_gc_live - gc_live), prof::rss_kb());
    class_table("after gc");
    if do_census {
        let h = heap.borrow();
        census(&h, "after gc");
    }
    prof::dump(out, &[("boot", boot_live, &boot_snap), ("peak", peak_live, &peak_snap), ("load", load_live, &load_snap), ("gc", gc_live, &gc_snap)]);
    std::mem::forget(interp);
}

fn forms<B: Backend>(make: impl FnOnce() -> Interpreter<B>, deps: &str, forms: &str)
where
    B::Error: std::error::Error,
{
    let interp = make();
    let heap = interp.backend().global_env().heap().clone();
    if let Err(e) = interp.eval_program(deps) { println!("deps error {e}"); }
    let _ = interp.eval_program("(gc)");
    let mut rows = vec![];
    for (i, f) in forms.split("\n\x1e\n").enumerate() {
        let _ = interp.eval_program("(gc)");
        let a0 = heap.borrow().stats().allocs_since_gc;
        let live0 = prof::LIVE.load(Relaxed);
        prof::PEAK.store(live0, Relaxed);
        let tot0 = prof::TOTAL.load(Relaxed);
        let t = Instant::now();
        let r = interp.eval_program(f);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let a1 = heap.borrow().stats().allocs_since_gc;
        let peak = prof::PEAK.load(Relaxed) - live0;
        let tot = prof::TOTAL.load(Relaxed) - tot0;
        let head: String = f.chars().take(40).collect::<String>().replace('\n', " ");
        rows.push((i, a1 - a0, peak, tot, ms, r.is_ok(), head));
    }
    let tot_allocs: usize = rows.iter().map(|r| r.1).sum();
    let mut sorted = rows.clone();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    println!("forms={} total heap allocs={} ", rows.len(), tot_allocs);
    for r in sorted.iter().take(12) {
        println!("form {:>3}: heap_allocs={:>8} ({:>4.1}%) malloc_peak_delta={:>9} malloc_total={:>9} {:>6.1} ms ok={} {}", r.0, r.1, 100.0 * r.1 as f64 / tot_allocs as f64, mb(r.2), mb(r.3 as i64), r.4, r.5, r.6);
    }
    let mut cum = 0usize;
    for (k, r) in sorted.iter().enumerate() { cum += r.1; if k == 4 { println!("top-5 forms = {:.1}% of heap allocs", 100.0 * cum as f64 / tot_allocs as f64); } }
    std::mem::forget(interp);
}

fn main() {
    if std::env::var("SIZES").is_ok() {
        use std::mem::size_of;
        println!("SourceLocation={} Option<SourceLocation>={} SourceSpan={} ScopeSet={} HeapObjectData={} CoreExpr={} CpsExpr={} CompiledMacro={} Environment={} (Rc<str>,ScopeSet)={} Rc<[Option<SourceLocation>]>={}",
            size_of::<patina_core::SourceLocation>(), size_of::<Option<patina_core::SourceLocation>>(),
            size_of::<patina_core::source_document::SourceSpan>(), size_of::<ScopeSet>(),
            size_of::<patina_core::heap::HeapObjectData>(), size_of::<patina_core::CoreExpr>(), size_of::<patina_core::cps_expr::CpsExpr>(),
            size_of::<patina_core::CompiledMacro>(), size_of::<patina_core::environment::Environment>(), size_of::<(Rc<str>, ScopeSet)>(), size_of::<Rc<[Option<patina_core::SourceLocation>]>>()); println!("Instruction={} CallFrame-ish skip", size_of::<patina_vm::types::instruction::Instruction>());
        return;
    }
    if std::env::var("FORMS").is_ok() {
        let args: Vec<String> = std::env::args().collect();
        let deps = std::fs::read_to_string(&args[2]).unwrap();
        let fs = std::fs::read_to_string(&args[3]).unwrap();
        match args[1].as_str() {
            "vm" => forms(patina_interpreter::VmInterpreter::new_vm, &deps, &fs),
            _ => forms(Interpreter::new_tree_walker, &deps, &fs),
        }
        return;
    }
    let args: Vec<String> = std::env::args().collect();
    let backend = args[1].clone();
    let prog = std::fs::read_to_string(&args[2]).unwrap();
    let out = args.get(3).cloned().unwrap_or_else(|| "/dev/null".into());
    let do_census = std::env::var("CENSUS").is_ok();
    if let Ok(r) = std::env::var("PROF_RATE") {
        prof::RATE.store(r.parse().unwrap(), Relaxed);
    }
    if std::env::var("NOPROF").is_err() {
        prof::init();
    }
    match backend.as_str() {
        "vm" => run(patina_interpreter::VmInterpreter::new_vm, &prog, &out, do_census),
        "tw" => run(Interpreter::new_tree_walker, &prog, &out, do_census),
        _ => panic!("backend vm|tw"),
    }
}
