use std::collections::HashSet;
use std::mem::size_of;
use std::rc::Rc;
use std::time::Instant;

use patina_core::environment::Environment;
use patina_core::heap::HeapObjectData;
use patina_core::{GcRoots, GcVisitor, TaggedValue};

fn sizes() {
    println!("== sizes (bytes) ==");
    println!("TaggedValue                {}", size_of::<TaggedValue>());
    println!("HeapObjectData             {}", size_of::<HeapObjectData>());
    println!("(TV,TV) pair slot          {}", size_of::<(TaggedValue, TaggedValue)>());
    println!("Vec<TV> vector slot        {}", size_of::<Vec<TaggedValue>>());
    println!("Vec<char> string slot      {}", size_of::<Vec<char>>());
    println!("Environment                {}", size_of::<Environment>());
    println!("ScopeSet                   {}", size_of::<patina_core::ScopeSet>());
    println!("Procedure                  {}", size_of::<patina_core::Procedure>());
    println!("CompiledMacro              {}", size_of::<patina_core::CompiledMacro>());
    println!("CpsContinuation            {}", size_of::<patina_core::continuation::CpsContinuation>());
    println!("ContValue                  {}", size_of::<patina_core::cont_value::ContValue>());
    println!("Library                    {}", size_of::<patina_core::Library>());
    println!("RecordTypeDescriptor       {}", size_of::<patina_core::RecordTypeDescriptor>());
    println!("Port                       {}", size_of::<patina_core::Port>());
    println!("PromiseState               {}", size_of::<patina_core::PromiseState>());
    println!("CpsExpr                    {}", size_of::<patina_core::CpsExpr>());
    println!("vm CodeObject              {}", size_of::<patina_vm::types::CodeObject>());
    println!("vm CallFrame               {}", size_of::<patina_vm::types::CallFrame>());
    println!("vm VmContinuation          {}", size_of::<patina_vm::types::VmContinuation>());
    println!("vm Instruction             {}", size_of::<patina_vm::types::Instruction>());
}

/// Walk every environment reachable from `root` the way GcVisitor::visit_env
/// does, counting environments, bindings and cross edges.
fn env_census(root: &Rc<Environment>) -> (usize, usize, usize, usize) {
    let mut seen: HashSet<usize> = HashSet::new();
    let mut stack: Vec<Rc<Environment>> = vec![root.clone()];
    let (mut envs, mut values, mut alias_edges, mut owner_edges) = (0, 0, 0, 0);
    while let Some(e) = stack.pop() {
        let mut cur = Some(e);
        while let Some(env) = cur {
            if !seen.insert(env.gc_identity()) {
                break;
            }
            envs += 1;
            env.for_each_local_value(&mut |_| values += 1);
            env.for_each_alias_target(&mut |t| {
                alias_edges += 1;
                stack.push(t.clone())
            });
            env.for_each_shared_owner(&mut |t| {
                owner_edges += 1;
                stack.push(t.clone())
            });
            cur = env.parent().cloned();
        }
    }
    (envs, values, alias_edges, owner_edges)
}

struct EnvRoot(Rc<Environment>);
impl GcRoots for EnvRoot {
    fn trace_roots(&self, v: &mut GcVisitor<'_>) {
        v.visit_env(&self.0);
    }
}

fn object_census(heap: &patina_core::heap::SharedHeap) {
    let h = heap.borrow();
    let st = h.stats();
    println!(
        "arenas: pairs={} vectors={} strings={} objects={} symbols={} free(p/v/s/o)={}/{}/{}/{} collections={}",
        st.pairs, st.vectors, st.strings, st.objects, st.symbols, st.free_pairs, st.free_vectors,
        st.free_strings, st.free_objects, st.gc_collections
    );
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    let mut closure_globals: HashSet<usize> = HashSet::new();
    let mut lambda_envs: HashSet<usize> = HashSet::new();
    let mut macro_def_envs: HashSet<usize> = HashSet::new();
    let mut macros_with_def_env = 0;
    let mut foreign = 0;
    let mut macro_literals = 0;
    for i in 0..st.objects {
        let tv = TaggedValue::object(i as u32);
        let data = h.get_object_type(tv);
        *counts.entry(format!("{:?}", data)).or_default() += 1;
        if format!("{:?}", data) == "Free" {
            continue;
        }
        match h.get_object(tv) {
            HeapObjectData::VmClosure { globals, .. } => {
                closure_globals.insert(Rc::as_ptr(globals) as usize);
            }
            HeapObjectData::Procedure(p) => {
                if let patina_core::Procedure::CpsLambda { env, .. } = p.as_ref() {
                    lambda_envs.insert(Rc::as_ptr(env) as usize);
                }
            }
            HeapObjectData::Macro(m) => {
                if let Some(e) = &m.definition_env {
                    macros_with_def_env += 1;
                    macro_def_envs.insert(Rc::as_ptr(e) as usize);
                }
                foreign += m.foreign_expansions.len();
                m.for_each_literal(&mut |_| macro_literals += 1);
            }
            _ => {}
        }
    }
    println!("object kinds: {:?}", counts);
    println!(
        "distinct VmClosure.globals envs={} distinct CpsLambda envs={} macros_with_def_env={} distinct macro def envs={} foreign_expansions entries={} macro literal TVs={}",
        closure_globals.len(), lambda_envs.len(), macros_with_def_env, macro_def_envs.len(), foreign, macro_literals
    );
}

fn probe<B: patina_runtime::Backend>(label: &str, interp: &patina_interpreter::Interpreter<B>, program: &str) {
    println!("\n==== {label} ====");
    let t = Instant::now();
    let r = interp.eval_program(program);
    println!("eval ok={} in {:?}", r.is_ok(), t.elapsed());
    if let Err(e) = &r {
        println!("error: {e}");
    }
    let g = interp.global_env();
    let heap = g.heap().clone();
    let probe_env = Environment::with_parent(g.clone());
    println!("environments minted so far (env_id counter) = {}", probe_env.env_id());
    let (envs, values, alias, owners) = env_census(&g);
    println!(
        "reachable from global env: envs={envs} bound values={values} alias edges={alias} owner edges={owners}; global local names={}",
        g.local_names().len()
    );
    object_census(&heap);
    // Time a mark phase rooted only at the global env (env tracing cost).
    let h = heap.borrow();
    let root = EnvRoot(g.clone());
    let t = Instant::now();
    let marks = patina_core::run_mark_phase(&h, &[&root]);
    let el = t.elapsed();
    println!("mark phase from global env only: {:?}, marked {:?}", el, marks.marked());
}

fn env_counter() -> u64 { Environment::new().env_id() }

fn per_call<B: patina_runtime::Backend>(label: &str, interp: &patina_interpreter::Interpreter<B>) {
    interp.eval_program("(import (scheme base)) (define (f n) (if (= n 0) '() (cons n (f (- n 1))))) (define (g n acc) (if (= n 0) acc (g (- n 1) (+ acc 1))))").unwrap();
    let a = env_counter();
    interp.eval_program("(f 10000)").unwrap();
    let b = env_counter();
    interp.eval_program("(g 10000 0)").unwrap();
    let c = env_counter();
    interp.eval_program("(let loop ((i 0)) (if (< i 10000) (loop (+ i 1)) i))").unwrap();
    let d = env_counter();
    println!("{label}: envs minted: (f 10000) non-tail={} ; (g 10000 0) tail={} ; named-let loop 10000={}", b-a-1, c-b-1, d-c-1);
}

fn leak_check() {
    for (label, weak) in [
        ("VM", { let i = patina_interpreter::Interpreter::new_vm(); i.eval_program("(import (scheme base)) (define (f) 1)").unwrap(); Rc::downgrade(i.global_env().heap()) }),
        ("TW", { let i = patina_interpreter::Interpreter::new_tree_walker(); i.eval_program("(import (scheme base)) (define (f) 1)").unwrap(); Rc::downgrade(i.global_env().heap()) }),
        ("bare Heap+env", { let e = Rc::new(Environment::new()); Rc::downgrade(e.heap()) }),
    ] {
        println!("{label}: heap alive after interpreter dropped = {} (strong={})", weak.upgrade().is_some(), weak.strong_count());
    }
}

fn main() {
    leak_check();
    per_call("VM", &patina_interpreter::Interpreter::new_vm());
    per_call("TW", &patina_interpreter::Interpreter::new_tree_walker());
    sizes();
    let prog_basic = "(import (scheme base) (scheme write)) (define (f n) (if (= n 0) '() (cons n (f (- n 1))))) (length (f 1000))";
    let prog_big = "(import (scheme base) (scheme write) (scheme char) (scheme cxr) (scheme lazy) (scheme case-lambda) (scheme inexact) (scheme complex) (scheme process-context) (scheme time) (scheme file) (scheme read) (scheme eval) (srfi 1)) (define v (make-vector 10 0)) (vector-length v)";
    let vm = patina_interpreter::Interpreter::new_vm();
    probe("VM basic", &vm, prog_basic);
    let vm2 = patina_interpreter::Interpreter::new_vm();
    probe("VM many libs", &vm2, prog_big);
    let tw = patina_interpreter::Interpreter::new_tree_walker();
    probe("TW basic", &tw, prog_basic);
}
