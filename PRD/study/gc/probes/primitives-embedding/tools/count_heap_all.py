import os, re, sys, collections
ROOT=os.path.expanduser('~/Project/patina/crates')
methods=[l.strip() for l in open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'heap_methods.txt'))]
generic={"new","with_capacity","default","stats","features","source","command_line","fmt"}
methods=[m for m in methods if m not in generic]
cats={
 'alloc': lambda m: m.startswith('alloc_') or m in ('intern_symbol','list_from_iter','list_from_iter_with_tail','values_from','core_syntax','list_reverse','list_append') ,
 'mutate': lambda m: m.startswith('set_') or m in ('vector_set','string_set_char','bytevector_u8_set','bytevector_copy_into','get_string_chars_mut','vector_slice_mut','get_bytevector_mut','promise_update','write_mutable_cell','break_ephemeron','retire_vm_closure','record_source','record_source_children','inherit_source'),
 'borrowref': lambda m: m in ('vector_slice','get_string_chars','get_object','get_bytevector','get_symbol_name','get_symbol_or_identifier_name','get_bigint','get_rational','get_procedure','get_port','get_macro','get_record_type','get_record','get_continuation','get_library','get_prompt_tag','get_values','get_identifier_data','get_identifier_data_any','get_environment_specifier'),
}
def cat(m):
    for k,f in cats.items():
        if f(m): return k
    return 'read'
pat=re.compile(r'\.(' + '|'.join(sorted(methods,key=len,reverse=True)) + r')\s*(::<[^>]*>)?\(')
crates=sorted(os.listdir(ROOT))
tot=collections.Counter()
for c in crates:
    per=collections.Counter(); files=set(); mc=collections.Counter(); testfiles=0
    for d,_,fs in os.walk(os.path.join(ROOT,c)):
        for f in fs:
            if not f.endswith('.rs'): continue
            p=os.path.join(d,f)
            src=open(p).read()
            # split off #[cfg(test)] mod tests heuristically
            idx=src.find('#[cfg(test)]')
            body=src
            n=0
            for mm in pat.finditer(body):
                m=mm.group(1); per[cat(m)]+=1; mc[m]+=1; n+=1
            if n: files.add(p)
    if sum(per.values()):
        print(f"{c:22s} files={len(files):3d} total={sum(per.values()):5d} " + ' '.join(f"{k}={per[k]}" for k in ('alloc','mutate','borrowref','read')))
        if '-v' in sys.argv: print('   top:', mc.most_common(25))
