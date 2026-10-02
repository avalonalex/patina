#!/usr/bin/env python3
"""Apply the par-measure perturbations to the scratch copy of Patina.

Every perturbation is behind a `--cfg` so one source tree builds every variant:
  perturb_lock        heap RefCell borrow additionally pays an uncontended
                      RwLock's atomics (read: ldadda/ldaddl; write: casa/stlr)
  perturb_arc         Rc<CodeObject> becomes Arc<CodeObject> (real change);
                      the Rc<Environment> clone+drop in Load/StoreGlobal pays an
                      Arc's atomics (emulated)
  perturb_order       heap reference loads (car, cdr, vector-ref, cell read,
                      closure free variable, closure code id) are Acquire;
                      stores of heap values into heap objects are Release
  perturb_allocfence  one `dmb ishst` after each allocation's initialising stores
"""
import re, sys, pathlib

ROOT = pathlib.Path(sys.argv[1])
heap = ROOT / "crates/patina-core/src/heap/mod.rs"
s = heap.read_text()

def sub1(old, new, text, count=1):
    if old not in text:
        raise SystemExit(f"pattern not found: {old[:80]!r}")
    return text.replace(old, new, count)

# --- perturb_order: loads and stores through pt_ld / pt_st -------------------------
s = sub1("""    pub fn car(&self, ptr: TaggedValue) -> TaggedValue {
        self.get_pair(ptr).0
    }""", """    pub fn car(&self, ptr: TaggedValue) -> TaggedValue {
        pt_ld(&self.pairs[ptr.heap_index() as usize].0)
    }""", s)
s = sub1("""    pub fn cdr(&self, ptr: TaggedValue) -> TaggedValue {
        self.get_pair(ptr).1
    }""", """    pub fn cdr(&self, ptr: TaggedValue) -> TaggedValue {
        pt_ld(&self.pairs[ptr.heap_index() as usize].1)
    }""", s)
s = sub1("""        debug_assert!(ptr.is_vector());
        self.vectors[ptr.heap_index() as usize][index]
    }""", """        debug_assert!(ptr.is_vector());
        pt_ld(&self.vectors[ptr.heap_index() as usize][index])
    }""", s)
s = sub1("""            HeapObjectData::MutableCell(cell) => Some(*cell.borrow()),""",
         """            HeapObjectData::MutableCell(cell) => Some(pt_ld(&cell.borrow())),""", s)
s = sub1("""            HeapObjectData::VmClosure { free_vars, .. } => free_vars.get(slot).copied(),""",
         """            HeapObjectData::VmClosure { free_vars, .. } => free_vars.get(slot).map(pt_ld),""", s)
s = sub1("""            HeapObjectData::VmClosure { code_id, .. } => Some(*code_id),""",
         """            HeapObjectData::VmClosure { code_id, .. } => Some(pt_ld_u64(code_id)),""", s)
s = sub1("""        self.pairs[ptr.heap_index() as usize].0 = value;
    }""", """        pt_st(&mut self.pairs[ptr.heap_index() as usize].0, value);
    }""", s)
s = sub1("""        self.pairs[ptr.heap_index() as usize].1 = value;
    }""", """        pt_st(&mut self.pairs[ptr.heap_index() as usize].1, value);
    }""", s)
s = sub1("""        debug_assert!(ptr.is_vector());
        self.vectors[ptr.heap_index() as usize][index] = value;
    }""", """        debug_assert!(ptr.is_vector());
        pt_st(&mut self.vectors[ptr.heap_index() as usize][index], value);
    }""", s)
s = sub1("""            HeapObjectData::MutableCell(cell) => {
                *cell.borrow_mut() = val;
                true
            }""", """            HeapObjectData::MutableCell(cell) => {
                pt_st(&mut cell.borrow_mut(), val);
                true
            }""", s)
s += r"""

// ── par-measure: heap-word access under perturb_order ─────────────────────────
#[cfg(not(perturb_order))]
#[inline(always)]
pub fn pt_ld(p: &TaggedValue) -> TaggedValue {
    *p
}
#[cfg(perturb_order)]
#[inline(always)]
pub fn pt_ld(p: &TaggedValue) -> TaggedValue {
    let a = p as *const TaggedValue as *mut u64;
    TaggedValue::from_raw(
        unsafe { std::sync::atomic::AtomicU64::from_ptr(a) }.load(std::sync::atomic::Ordering::Acquire),
    )
}
#[cfg(not(perturb_order))]
#[inline(always)]
pub fn pt_ld_u64(p: &u64) -> u64 {
    *p
}
#[cfg(perturb_order)]
#[inline(always)]
pub fn pt_ld_u64(p: &u64) -> u64 {
    unsafe { std::sync::atomic::AtomicU64::from_ptr(p as *const u64 as *mut u64) }
        .load(std::sync::atomic::Ordering::Acquire)
}
#[cfg(not(perturb_order))]
#[inline(always)]
pub fn pt_st(p: &mut TaggedValue, v: TaggedValue) {
    *p = v;
}
/// The design's funnel under `threaded`: immediates plain, heap values Release.
#[cfg(perturb_order)]
#[inline(always)]
pub fn pt_st(p: &mut TaggedValue, v: TaggedValue) {
    if v.is_heap_pointer() {
        let a = p as *mut TaggedValue as *mut u64;
        unsafe { std::sync::atomic::AtomicU64::from_ptr(a) }
            .store(v.raw(), std::sync::atomic::Ordering::Release);
    } else {
        *p = v;
    }
}
"""

# --- perturb_allocfence ---------------------------------------------------------
FENCE = """        #[cfg(perturb_allocfence)]
        unsafe {
            core::arch::asm!("dmb ishst", options(nostack, preserves_flags))
        };
"""
s = sub1("""            let index = self.pairs.len() as HeapIndex;
            self.pairs.push((car, cdr));
            index
        };
        TaggedValue::pair(index)""", """            let index = self.pairs.len() as HeapIndex;
            self.pairs.push((car, cdr));
            index
        };
""" + FENCE + """        TaggedValue::pair(index)""", s)
s = sub1("""            let index = self.objects.len() as HeapIndex;
            self.objects.push(data);
            index
        };
        TaggedValue::object(index)""", """            let index = self.objects.len() as HeapIndex;
            self.objects.push(data);
            index
        };
""" + FENCE + """        TaggedValue::object(index)""", s)

# --- perturb_lock / perturb_arc helpers, appended -------------------------------
s += r'''

// ── par-measure perturbations (scratch only; never in the repository) ──────────
pub static SIM_LOCK: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
pub static SIM_RC: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

/// A heap shared borrow that, under `perturb_lock`, also pays an uncontended
/// RwLock read's two atomic RMWs (Acquire to enter, Release to leave).
pub struct SimRef<'a>(std::cell::Ref<'a, Heap>);
pub struct SimRefMut<'a>(std::cell::RefMut<'a, Heap>, bool);
impl std::ops::Deref for SimRef<'_> {
    type Target = Heap;
    #[inline(always)]
    fn deref(&self) -> &Heap {
        &self.0
    }
}
impl std::ops::Deref for SimRefMut<'_> {
    type Target = Heap;
    #[inline(always)]
    fn deref(&self) -> &Heap {
        &self.0
    }
}
impl std::ops::DerefMut for SimRefMut<'_> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Heap {
        &mut self.0
    }
}
impl Drop for SimRef<'_> {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(perturb_lock)]
        SIM_LOCK.fetch_sub(1, std::sync::atomic::Ordering::Release);
    }
}
impl Drop for SimRefMut<'_> {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(perturb_lock)]
        if self.1 {
            SIM_LOCK.store(0, std::sync::atomic::Ordering::Release);
        }
    }
}
pub trait SimBorrow {
    fn sim_borrow(&self) -> SimRef<'_>;
    fn sim_borrow_mut(&self) -> SimRefMut<'_>;
}
impl SimBorrow for RefCell<Heap> {
    #[inline(always)]
    fn sim_borrow(&self) -> SimRef<'_> {
        #[cfg(perturb_lock)]
        SIM_LOCK.fetch_add(1, std::sync::atomic::Ordering::Acquire);
        SimRef(self.borrow())
    }
    #[inline(always)]
    fn sim_borrow_mut(&self) -> SimRefMut<'_> {
        #[cfg(perturb_lock)]
        let ok = SIM_LOCK
            .compare_exchange(
                0,
                -1,
                std::sync::atomic::Ordering::Acquire,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_ok();
        #[cfg(not(perturb_lock))]
        let ok = false;
        SimRefMut(self.borrow_mut(), ok)
    }
}
/// Under `perturb_arc`, an Arc clone (relaxed ldadd) at creation and an Arc
/// drop (release ldaddl, acquire fence on the last reference) at drop.
pub struct SimArcGuard;
impl SimArcGuard {
    #[inline(always)]
    pub fn new() -> Self {
        #[cfg(perturb_arc)]
        SIM_RC.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        SimArcGuard
    }
}
impl Drop for SimArcGuard {
    #[inline(always)]
    fn drop(&mut self) {
        #[cfg(perturb_arc)]
        if SIM_RC.fetch_sub(1, std::sync::atomic::Ordering::Release) == 1 {
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        }
    }
}
'''
heap.write_text(s)

# --- route VM and primitive heap borrows through SimBorrow -----------------------
IMPORT = "\n#[allow(unused_imports)]\nuse patina_core::heap::SimBorrow as _;\n"
changed = []
# These primitive files pass `&Ref<Heap>` to helpers, so they keep plain borrows
# (their primitives are not on the measured hot paths).
SKIP = {"bytevectors.rs", "characters.rs", "conversion.rs", "strings.rs", "binary.rs", "file.rs",
        "ports.rs", "read.rs", "text_input.rs", "text_output.rs", "directory.rs"}
for crate in ["patina-vm/src", "patina-primitives/src"]:
    for f in sorted((ROOT / "crates" / crate).rglob("*.rs")):
        if crate == "patina-primitives/src" and f.name in SKIP:
            continue
        t = f.read_text()
        t2 = re.sub(r"\bheap\.borrow\(\)", "heap.sim_borrow()", t)
        t2 = re.sub(r"\bheap\.borrow_mut\(\)", "heap.sim_borrow_mut()", t2)
        t2 = re.sub(r"\bheap\(\)\.borrow\(\)", "heap().sim_borrow()", t2)
        t2 = re.sub(r"\bheap\(\)\.borrow_mut\(\)", "heap().sim_borrow_mut()", t2)
        if t2 != t:
            f.write_text(t2 + IMPORT)
            changed.append(str(f.relative_to(ROOT)))
print("sim_borrow routed in", len(changed), "files")

# --- perturb_arc: Rc<CodeObject> -> CodeRc ------------------------------------------
types = ROOT / "crates/patina-vm/src/types/mod.rs"
t = types.read_text()
t += """

// par-measure: the code-object pointer type (Arc under perturb_arc).
#[cfg(not(perturb_arc))]
pub type CodeRc = std::rc::Rc<CodeObject>;
#[cfg(perturb_arc)]
pub type CodeRc = std::sync::Arc<CodeObject>;
#[cfg(not(perturb_arc))]
pub use std::rc::Rc as CodeRcT;
#[cfg(perturb_arc)]
pub use std::sync::Arc as CodeRcT;
"""
types.write_text(t)
for f in sorted((ROOT / "crates/patina-vm/src").rglob("*.rs")):
    t = f.read_text()
    t2 = t.replace("Rc<CodeObject>", "crate::types::CodeRc")
    t2 = t2.replace("Rc::ptr_eq(cur_code", "crate::types::CodeRcT::ptr_eq(cur_code")
    t2 = t2.replace("Rc::ptr_eq(code, &self.empty_code)", "crate::types::CodeRcT::ptr_eq(code, &self.empty_code)")
    t2 = t2.replace("empty_code: Rc::new(CodeObject {", "empty_code: crate::types::CodeRcT::new(CodeObject {")
    t2 = t2.replace("= Rc::new(code);", "= crate::types::CodeRcT::new(code);")
    t2 = t2.replace("Rc::clone(&self.empty_code)", "crate::types::CodeRcT::clone(&self.empty_code)")
    t2 = t2.replace("*slot = Rc::clone(empty);", "*slot = crate::types::CodeRcT::clone(empty);")
    t2 = t2.replace("|| Rc::strong_count(code) > 1)", "|| crate::types::CodeRcT::strong_count(code) > 1)")
    # undo the replacement inside the alias itself
    t2 = t2.replace("pub type CodeRc = std::rc::crate::types::CodeRc;", "pub type CodeRc = std::rc::Rc<CodeObject>;")
    if t2 != t:
        f.write_text(t2)

# Rc<Environment> clone in Load/StoreGlobal (emulated Arc traffic)
vm = ROOT / "crates/patina-vm/src/runtime/vm_state.rs"
t = vm.read_text()
t = sub1("""        Instruction::LoadGlobal { dst, ref name } => {
            // Per-site inline cache (Track P P4): see `GlobalCacheEntry`.
            let globals = frame_globals(state);""", """        Instruction::LoadGlobal { dst, ref name } => {
            // Per-site inline cache (Track P P4): see `GlobalCacheEntry`.
            let _arc_sim = patina_core::heap::SimArcGuard::new();
            let globals = frame_globals(state);""", t)
t = sub1("""        Instruction::StoreGlobal { ref name, src } => {
            let val = state.reg_at(base, src);
            let globals = frame_globals(state);""", """        Instruction::StoreGlobal { ref name, src } => {
            let val = state.reg_at(base, src);
            let _arc_sim = patina_core::heap::SimArcGuard::new();
            let globals = frame_globals(state);""", t)
# VectorSet writes through vector_slice_mut: route its heap-value stores through Release
t = sub1("""                            let slot = heap.vector_slice_mut(vec).get_mut(i)?;
                            *slot = x;""", """                            let slot = heap.vector_slice_mut(vec).get_mut(i)?;
                            patina_core::heap::pt_st(slot, x);""", t)
vm.write_text(t)
print("ok")
