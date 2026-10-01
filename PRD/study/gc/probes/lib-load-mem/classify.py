import sys, os, collections
prof = sys.argv[1]
sys.argv=["sym.py", prof]
exec(open("sym.py").read().replace('if __name__ == "__main__":','if False:'))

def classify(frs):
    names = [f for f, _ in frs]
    own_frames = [f for f in names if own(f)]
    lf = own_frames[0] if own_frames else ""
    allnames = " | ".join(names)
    def has(s): return s in allnames
    inner = " | ".join(names[:30])
    if "Vec<(patina_core::tagged_value::TaggedValue, patina_core::tagged_value::TaggedValue)>>::grow" in inner or "RawVec<(patina_core::tagged_value::TaggedValue, patina_core::tagged_value::TaggedValue)>>::grow" in inner: return "A2 GC arena: pair slots (16 B)"
    if "RawVec<patina_core::heap::HeapObjectData>>::grow" in inner or "Vec<patina_core::heap::HeapObjectData>>::grow" in inner: return "A1 GC arena: objects slots (72 B)"
    if "Heap>::alloc_object" in lf: return "A1 GC arena: objects slots (72 B)"
    if "Heap>::alloc_pair" in lf: return "A2 GC arena: pair slots (16 B)"
    if "Heap>::alloc_vector" in lf or "Heap>::alloc_string" in lf: return "A3 GC arena: vector/string slots"
    if "sweep_arena" in lf or "gc::" in lf: return "A4 GC free lists / mark bits"
    if "Heap>::record_source" in lf or "Heap>::inherit_source" in lf:
        if "hashbrown" in allnames.split("Heap>::")[0]: return "P1 syntax_sources hash table"
        return "P2 syntax_sources Rc<SyntaxSource> entries + child spans"
    if "stamp_expansion_source" in lf: return "P3 expansion chains (Arc<[String]>) + SourceMap records"
    if "SourceMap" in lf or "source_document" in lf or "Parser>::new_program" in lf or "record_location" in lf: return "P4 SourceMap / SourceDocument text"
    if "pass5_codegen" in lf and "record_source" in lf: return "C2 CodeObject.source_map"
    if "register_root_maps" in lf: return "C3 CodeObject.register_roots"
    if "Codegen>::emit" in lf: return "C1 CodeObject.instructions"
    if "GlobalCacheEntry" in lf: return "C4 CodeObject.global_cache"
    if "load_unit" in lf or "next_code_id" in lf or "add_constant" in lf: return "C5 code store / constants"
    if "patina_vm::compiler" in lf or "patina_vm::compiler" in allnames and "patina_frontend" not in lf: return "C6 VM compiler passes (transient IR)"
    if "cps_transform" in lf or "cps_expr" in lf or "patina_ir" in lf: return "T1 CpsExpr trees (patina-ir)"
    if "patina_tree_walker" in lf:
        if "make_cps_closure" in lf: return "T2 TW closures (Procedure Rc + params)"
        return "T3 TW runtime (call envs, continuations, steps)"
    if "Environment>::insert_scoped" in lf: return "E2 Environment scoped bindings"
    if "patina_core::environment" in lf or "library::Library" in lf: return "E1 Environment / Library binding tables"
    if "ScopeSet" in lf or "scope::" in lf: return "S1 ScopeSet spills / scope tables"
    if "Parser>::identifier" in lf: return "I1 Identifier name Rc<str> (parser)"
    if "patina_frontend::parser" in lf or "library_support" in lf or "library_parser" in lf: return "F1 reader/library parser"
    if "patina_frontend::desugarer" in lf: return "F2 desugarer (CoreExpr, memos)"
    if "patina_frontend" in lf: return "F3 frontend other"
    if "patina_macros::macro_expander::compiler" in lf or "compiled_macro" in lf: return "M1 CompiledMacro"
    if "patina_macros" in lf or "pvref" in lf: return "M2 macro expander transient (MatchEnv etc)"
    if "patina_vm::runtime" in lf or "patina_vm" in lf: return "V1 VM runtime"
    if "patina_primitives" in lf: return "R1 primitives"
    if "patina_core" in lf: return "X1 core other: " + short(lf)[:60]
    return "Z other: " + short(lf)[:60]

for name in (sys.argv_rest if False else os.environ.get("SNAPS","peak,load,gc,churn").split(",")):
    live, items = snaps[name]
    agg = collections.Counter()
    for sid, w in items:
        agg[classify(frames_of(sid))] += w
    tot = sum(agg.values())
    print(f"\n=== {name}: sampled {tot/1048576:.1f} MB (live {live/1048576:.1f} MB)")
    for k, w in sorted(agg.items(), key=lambda kv: -kv[1]):
        if w/1048576 >= float(os.environ.get("MIN","0.3")):
            print(f"{w/1048576:9.1f} MB {100*w/tot:5.1f}%  {k}")
