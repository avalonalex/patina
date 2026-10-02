//! Census of top-level / library bindings, to size a heap "binding cell" model.
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

use patina_core::environment::{BindingLocation, Environment};
use patina_core::TaggedValue;

const FORWARDED_BITS: u64 = 0xF0 | 0b001;

#[derive(Default, Debug)]
struct Census {
    root_envs: usize,
    child_envs: usize,
    plain_slots: usize,      // own + forwarded (incl. hidden import aliases)
    own: usize,              // slots this env owns (would each be a cell)
    forwarded: usize,        // FORWARDED markers (imports + import aliases)
    visible_imports: usize,  // local_names that are imports
    hidden_import_aliases: usize,
    scoped: usize,           // scoped_bindings entries
    alias_edges: usize,      // macro-expansion aliases into another env
    owner_edges: usize,
    name_bytes_own: usize,
    kinds: BTreeMap<String, usize>,
    biggest: Vec<(usize, usize)>, // (own, forwarded) per root env
}

fn kind(heap: &patina_core::heap::Heap, tv: TaggedValue) -> String {
    if tv.is_object() || tv.is_heap_pointer() {
        if tv.is_object() {
            let t = format!("{:?}", heap.get_object_type(tv));
            if t == "Procedure" {
                if let Some(p) = heap.get_procedure(tv) {
                    return match p.as_ref() {
                        patina_core::Procedure::Primitive { .. } => "Procedure::Primitive".into(),
                        patina_core::Procedure::CpsLambda { .. } => "Procedure::CpsLambda".into(),
                        _ => "Procedure::other".into(),
                    };
                }
            }
            return t;
        }
        if tv.is_pair() { return "pair".into(); }
        return "vector/string/closure-tag".into();
    }
    "immediate".into()
}

fn census(root: &Rc<Environment>) -> Census {
    let mut c = Census::default();
    let mut seen: HashSet<usize> = HashSet::new();
    let mut stack: Vec<Rc<Environment>> = vec![root.clone()];
    let heap_rc = root.heap().clone();
    while let Some(env) = stack.pop() {
        if !seen.insert(env.gc_identity()) { continue; }
        if let Some(p) = env.parent() { stack.push(p.clone()); c.child_envs += 1; } else { c.root_envs += 1; }
        let mut total_values = 0usize;
        let mut fwd = 0usize;
        env.for_each_local_value(&mut |tv| { total_values += 1; if tv.raw_bits() == FORWARDED_BITS { fwd += 1; } });
        let names = env.local_names();
        let mut own = 0usize; let mut vis_imp = 0usize;
        let heap = heap_rc.borrow();
        for n in &names {
            match env.binding_location(n) {
                Some(BindingLocation::Slot(id, _)) if id == env.env_id() => {
                    own += 1; c.name_bytes_own += n.len();
                    if let Some(slot) = env.local_slot(n) {
                        let v = env.slot_value(slot);
                        *c.kinds.entry(kind(&heap, v)).or_default() += 1;
                    }
                }
                _ => vis_imp += 1,
            }
        }
        drop(heap);
        // own slots that were defined over a hidden alias are counted as own visible; ignore.
        c.own += own; c.forwarded += fwd; c.visible_imports += vis_imp;
        c.hidden_import_aliases += fwd.saturating_sub(vis_imp);
        let plain = own + fwd;
        c.plain_slots += plain;
        c.scoped += total_values.saturating_sub(plain);
        env.for_each_alias_target(&mut |t| { c.alias_edges += 1; stack.push(t.clone()); });
        env.for_each_shared_owner(&mut |t| { c.owner_edges += 1; stack.push(t.clone()); });
        if env.parent().is_none() { c.biggest.push((own, fwd)); }
    }
    c.biggest.sort_by(|a, b| (b.0 + b.1).cmp(&(a.0 + a.1)));
    c.biggest.truncate(6);
    c
}

fn report(label: &str, c: &Census, objects: usize) {
    println!("\n==== {label} ====");
    println!("root envs={} child envs={} plain slots={} own(=cells)={} forwarded={} (visible imports={}, hidden import aliases={}) scoped={} alias edges={} owner edges={}",
        c.root_envs, c.child_envs, c.plain_slots, c.own, c.forwarded, c.visible_imports, c.hidden_import_aliases, c.scoped, c.alias_edges, c.owner_edges);
    println!("own-binding value kinds: {:?}", c.kinds);
    println!("largest root envs (own, forwarded): {:?}", c.biggest);
    println!("heap objects (incl free) = {objects}");
    // Byte model.
    // Today: per plain slot a SmallVec entry (Rc<str>,TV)=24B; indexed envs add a hash entry
    // (Rc<str>,u32) ~ 24B/0.875 ≈ 28B; links: 16B per Option<(Rc<Env>,u32)> per slot index up to the max forwarded.
    let today_slot = c.plain_slots * 24;
    let today_index = c.plain_slots * 28;
    let today_links_lower = c.forwarded * 16;
    println!("bytes today (approx): slot table {} + hash index {} + link table >= {} = {}",
        today_slot, today_index, today_links_lower, today_slot + today_index + today_links_lower);
    for (name, cell) in [("16B cell (header+value)", 16usize), ("24B cell (+symbol/name)", 24), ("32B cell (+pvalue/flags)", 32)] {
        let cells = c.own * cell;
        // name table stays in Rust: name -> cell pointer for every plain slot (own+import), ~ (Rc<str>,u64)+ctrl ≈ 28B/entry
        let table = c.plain_slots * 28;
        println!("cell model with {name}: cells {} B ({} cells) + Rust name tables {} B = {} B", cells, c.own, table, cells + table);
    }
}

fn run<B: patina_runtime::Backend>(backend: &str, make: impl Fn() -> patina_interpreter::Interpreter<B>) {
    let programs: [(&str, &str); 3] = [
        ("base only", "(import (scheme base))"),
        ("14 R7RS-small libs + srfi 1", "(import (scheme base) (scheme write) (scheme char) (scheme cxr) (scheme lazy) (scheme case-lambda) (scheme inexact) (scheme complex) (scheme process-context) (scheme time) (scheme file) (scheme read) (scheme eval) (srfi 1))"),
        ("R7RS-large set (biglibs.scm)", "(import (scheme base) (scheme write) (scheme time) (scheme list) (scheme hash-table) (scheme char) (scheme comparator) (scheme generator) (scheme set) (scheme sort) (scheme vector) (scheme text) (scheme ideque) (scheme ilist) (scheme rlist) (scheme mapping) (scheme stream) (scheme show) (scheme regex) (scheme charset) (scheme lseq) (scheme bytevector) (scheme flonum) (scheme fixnum) (scheme bitwise))"),
    ];
    for (label, prog) in programs {
        let interp = make();
        let t = Instant::now();
        let r = interp.eval_program(prog);
        let el = t.elapsed();
        if let Err(e) = &r { println!("{backend} {label}: error {e}"); continue; }
        let g = interp.global_env();
        let objects = g.heap().borrow().stats().objects;
        let t2 = Instant::now();
        let c = census(&g);
        let el2 = t2.elapsed();
        report(&format!("{backend}: {label} (load {:?}, census {:?})", el, el2), &c, objects);
    }
}

fn main() {
    println!("size_of Environment = {}", std::mem::size_of::<Environment>());
    run("VM", patina_interpreter::Interpreter::new_vm);
    run("TW", patina_interpreter::Interpreter::new_tree_walker);
}
