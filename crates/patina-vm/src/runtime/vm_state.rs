//! `VmState` — the complete mutable state of the VM during execution.
//!
//! Procedure calls and nonlocal transfers live in `runtime::control`.
//! This module owns state, the dispatch loop, GC safe points, library loading,
//! and global-binding invalidation. See `docs/VM_RUNTIME.md` for the design.

use super::control::{
    Reentry, abort_to_prompt, across_reentry, call_any, call_closure_from_regs, call_value,
    call_value_with_probe, capture_delimited, captured_handlers, classify_error,
    exec_call_primitive, exec_call_primitive_direct, find_prompt, finish_delimited_invoke,
    invoke_step, is_catchable, park_escape, park_transfer, pop_resolved_extents, push_invoke_step,
    raise_step, self_tail_call, spread_apply_args, step_wind_jump, tail_call_closure_resolved,
    tail_call_value, tail_call_value_with_probe, tail_invoke_delimited, unpack_values,
    vm_raise_value, wind_step,
};
use crate::error::VmError;
use crate::types::code_object::{Arity, CodeObject, GlobalCacheEntry};
use crate::types::continuation::{
    DynamicWindRecord, ExceptionHandler, PromptFrame, VmContinuation, VmDelimitedContinuation,
};
use crate::types::instruction::{ControlForm, Instruction, TestOp};
use crate::types::{CallFrame, CodeObjectId};
use patina_core::core_expr::{CoreExpr, CoreExprKind};
use patina_core::environment::Environment;
use patina_core::heap::SharedHeap;
use patina_core::procedure::Procedure;
use patina_core::tagged_value::TaggedValue;
use patina_core::{GcController, GcDeferGuard};
use patina_primitives::PrimitiveRegistry;
use patina_runtime::{LibraryLoaderRegistry, LibraryRegistry};
use rustc_hash::FxHashMap;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────────────────
// VmState
// ─────────────────────────────────────────────────────────────────────────────

/// The complete mutable state of the VM during execution.
pub struct VmState {
    /// Flat register array. Each `CallFrame` owns a slice via `register_base + num_regs`.
    pub registers: Vec<TaggedValue>,
    /// The call stack. Currently-executing frame is `frames.last()`.
    pub frames: Vec<CallFrame>,
    /// Value carried by a continuation that escaped past a re-entry boundary,
    /// parked between `across_reentry` and the dispatch loop that resumes
    /// with it. A field rather than a return value because the
    /// `ApplyContext` methods return `Result<T, EvalError>` and cannot say
    /// "escaped". Mirrors the tree-walker's `set_pending_escape` /
    /// `take_pending_escape` pair (`cps_eval/types.rs`) — same split, same
    /// reason, and rooted for the same reason (`gc_roots.rs`).
    pub(crate) pending_escape: Option<TaggedValue>,
    /// Set while an `abort-current-continuation` is travelling to the landing
    /// it built, and only then: by `abort_to_prompt`, and again by each later
    /// step of a travel that has after thunks to run (`ResumeWindJump`), since
    /// the loop that runs a thunk clears it on the way in (#342).
    ///
    /// An abort is not a return. It cuts every stack back to its prompt and
    /// pushes one stub frame that has yet to run, so a Rust primitive whose
    /// callback it left must be abandoned, not handed a value. The frame
    /// depths cannot say that: the landing sits at exactly the depth a
    /// callback returning normally would leave, and a continuation captured
    /// *and* invoked inside a callback — `member` with a comparator that does
    /// it, pinned in `escape_from_primitive.rs` — leaves the identical stack
    /// while genuinely being a return. Two opposite meanings, one shape; the
    /// only thing that can distinguish them is the transfer itself (#177).
    ///
    /// Read by `across_reentry`, which is every re-entry boundary at once —
    /// a primitive's callback, `eval`, a parameter converter, library
    /// loading. Read per-boundary first, and that covered one of four: an
    /// abort out of `eval` kept the old failure verbatim, and one out of a
    /// tail-position parameter *set* stored its value into the parameter and
    /// never ran the handler. `parameterize` was the wrong thing to measure —
    /// it reaches the callback through `apply_proc`, the boundary that was
    /// already fixed.
    ///
    /// Cleared in two places: whichever dispatch loop resumes into the
    /// landing, which is the point past which no boundary is owed the news;
    /// and `execute`, with the rest of the machine, so a form cannot leave
    /// one latched for the next.
    pub(crate) pending_transfer: bool,
    /// The re-entry boundaries the machine is inside, outermost first: one id
    /// per [`across_reentry`](super::control) still on the Rust stack — a
    /// primitive running a callback, `eval`, a parameter converter. Ids are
    /// never reused, so one names one boundary for the life of the machine.
    ///
    /// A full continuation records the stack at capture
    /// (`VmContinuation::reentry`), and that is what tells an escape out of a
    /// boundary from a return into it. Frame depths could not: a callback's
    /// own continuation, captured after a tail call popped its frame, and one
    /// captured before the primitive was called restore byte-identical
    /// machines (#469, #472, #474), and one captured outside at a deeper
    /// stack restores more frames than the callback's loop started with
    /// (#473).
    pub(crate) reentry: Vec<u64>,
    /// The id the next boundary takes.
    pub(crate) next_reentry: u64,
    /// Set by a full continuation's arrival (`step_wind_jump`) when the
    /// continuation was captured outside some of the boundaries now on
    /// [`Self::reentry`]: how many of them it keeps. Every boundary past that
    /// count is left, and each dispatch loop and `across_reentry` inside one
    /// unwinds rather than resuming. Cleared by the loop that resumes into
    /// the arrival, which is inside the boundaries kept, and by `execute`.
    pub(crate) reentry_kept: Option<usize>,
    /// Stack of active continuation prompts (SRFI-226).
    pub prompt_stack: Vec<PromptFrame>,
    /// Stack of active `dynamic-wind` records.
    pub dynamic_winds: Vec<DynamicWindRecord>,
    /// Stack of installed exception handlers (`with-exception-handler`).
    pub exception_handlers: Vec<ExceptionHandler>,
    /// The loaded `CodeObject`s, each in the slot its `CodeObjectId` names.
    /// A slot whose code has been let go holds `empty_code` until
    /// [`VmState::load_unit`] gives it to other code.
    pub(crate) code_store: Vec<Rc<CodeObject>>,
    /// What an empty slot of `code_store` holds: code with no instructions,
    /// named by a label, which no loaded code's id is. A lookup then checks
    /// only that the slot's code has the id asked for, which turns away an
    /// empty slot as well as one holding later code — a branch fewer on every
    /// closure call than an `Option` slot, and about 1% of a call-heavy loop.
    pub(crate) empty_code: Rc<CodeObject>,
    /// The ids the next code loaded is given, one for each empty slot: the
    /// slot with the generation after the code it last held. A slot that has
    /// used every generation is left out, and stays empty (#352).
    pub(crate) free_code_ids: Vec<CodeObjectId>,
    /// The code each compilation loaded together, keyed by the id it runs
    /// from: a top-level form's code, with the code of the lambdas in it. A
    /// unit is kept or let go whole, since code that is not running can still
    /// make a closure of a lambda nested in it.
    pub(crate) code_units: FxHashMap<CodeObjectId, Vec<CodeObjectId>>,
    /// Id of the one-instruction stub each step of a continuation jump runs
    /// in (`wind_jump_stub`). Built on the first jump that has a wind thunk
    /// to run; most states never build one. The id, not the `Rc` — the code
    /// object has exactly one owner, `code_store`, and this is a note of
    /// where to find it.
    pub(crate) wind_jump_code: Option<CodeObjectId>,
    /// Id of the six-instruction stub the *value* form of `dynamic-wind` runs
    /// in (`value_wind_stub`). Built on the first such call; a program that
    /// only ever calls `dynamic-wind` in head position never builds one.
    pub(crate) value_wind_code: Option<CodeObjectId>,
    /// Id of the two-instruction stub the *value* form of `call-with-values`
    /// runs in (`value_cwv_stub`). Built on the first such call.
    pub(crate) value_cwv_code: Option<CodeObjectId>,
    /// Id of the two-instruction stub an abort's prompt handler is called in
    /// (`abort_handler_stub`). Built on the first abort; a program with no
    /// prompts never builds one.
    pub(crate) abort_handler_code: Option<CodeObjectId>,
    /// Id of the one-instruction stub each step of a composable invoke's
    /// extent re-entry runs in (`invoke_step_stub`). Built on the first
    /// invoke that has a `before` thunk to run.
    pub(crate) invoke_step_code: Option<CodeObjectId>,
    /// Id of the three-instruction stub a raise's handler is called in
    /// (`raise_step_stub`). Built on the first raise that reaches a
    /// handler; a program that never raises never builds one.
    pub(crate) raise_step_code: Option<CodeObjectId>,
    /// Id of the three-instruction stub a delayed promise's thunk is run in
    /// (`force_stub`). Built on the first `force` of a promise not yet done.
    pub(crate) force_code: Option<CodeObjectId>,
    /// Ids of the three-instruction stubs a resumable primitive's call runs
    /// in (`resume_stub`), one per argument count up to
    /// `resume_step::INLINE_ARGS` and one that spreads a list for more. Each
    /// built on the first such call.
    pub(crate) resume_codes: [Option<CodeObjectId>; super::control::resume_step::VARIANTS],
    /// `%parameter-set!`'s registry index, which a call `(p v)` of a
    /// parameter object runs (#478); looked up once, here, not per call.
    pub(crate) parameter_set: Option<usize>,
    /// Global variable environment, shared with the library loader.
    /// `Environment` has interior mutability, so no outer `RefCell` is needed.
    pub globals: Rc<Environment>,
    /// The heap, shared with `patina-runtime` primitives.
    pub heap: SharedHeap,
    /// Registry of all primitive procedures.
    pub primitive_registry: Rc<PrimitiveRegistry>,
    /// Bitset over registry indices: primitives whose global binding was
    /// overwritten by a top-level `define`/`set!` after code was compiled.
    /// `CallPrimitive` sites check their bit and deoptimize to the
    /// name-lookup `Call` path when it is set, so rebinding a primitive name
    /// behaves exactly as it did before `CallPrimitive` emission.
    pub shadowed_primitives: Vec<u64>,
    /// The same for the two control forms pass 5 compiles to a sequence of
    /// their own, one bit each (`ControlForm::bit`): set when a global
    /// binding holding the form's procedure is given another value, after
    /// which `JumpUnlessShadowed` sends every such site to an ordinary call.
    /// Separate from `shadowed_primitives` because `dynamic-wind` has no
    /// registry entry to index it by.
    pub(crate) shadowed_controls: u8,
    /// Reusable argument buffer for `CallPrimitive` dispatch, taken out of
    /// the state (`mem::take`) for the duration of each call so re-entrant
    /// primitives see an empty buffer and simply allocate — only nested
    /// primitive calls pay an allocation; the common depth-1 case is
    /// allocation-free. An allocation pool, never read for meaning after a
    /// call returns — never a channel for values, which travel in registers.
    pub(crate) scratch_args: Vec<TaggedValue>,
    /// Side table for full (call/cc) continuations — keyed by the heap-minted
    /// id inside the `VmContinuationRef(id)` handle. **Weak** (design §9.5):
    /// entries whose ref object dies are pruned at collection via
    /// `GcRoots::sweep_weak`, which runs with `&VmState` — hence the
    /// `RefCell`. Ids come from the heap's counter (unique across both
    /// continuation kinds and every `VmState` on the heap, never reused), so
    /// a pruned id cannot alias another entry.
    pub continuation_store: RefCell<FxHashMap<u64, Rc<VmContinuation>>>,
    /// Side table for delimited continuations — keyed by opaque u64 id.
    /// Weak, like `continuation_store`.
    pub delimited_continuation_store: RefCell<FxHashMap<u64, Rc<VmDelimitedContinuation>>>,
    /// Structured tracer for instruction-level debugging.
    pub tracer: Option<crate::tracer::TracerHandle>,
    /// Shared library registry for `load_scheme_library` in eval primitives.
    /// `None` for temporary VmStates created during library loading.
    pub library_registry: Option<Rc<RefCell<LibraryRegistry>>>,
    /// Shared library loader registry for `load_scheme_library` in eval primitives.
    /// `None` for temporary VmStates created during library loading.
    pub loader_registry: Option<Rc<RefCell<LibraryLoaderRegistry>>>,
    /// Virtual filesystem for all file I/O operations.
    pub fs: Arc<dyn patina_core::FileSystem>,
    /// Garbage collector policy and state (see `docs/GC_DESIGN.md`).
    /// Serviced at the dispatch-loop safe point; always adaptive outside the
    /// differential test lanes. Behind a `RefCell` so `collect` can take
    /// `&VmState` as a root while mutating the collector.
    pub(crate) gc: RefCell<GcController>,
    /// The heap's collection-pending flag, cached at construction so
    /// dispatch-loop entry costs no heap borrow and the per-instruction safe
    /// point is a single load.
    pub(crate) gc_pending: Rc<Cell<bool>>,
}

impl VmState {
    pub fn new(globals: Rc<Environment>) -> Self {
        let mut registry = PrimitiveRegistry::new();
        patina_primitives::register_all(&mut registry);
        // Share the heap with the environment so TaggedValue indices produced by
        // the parser (which also uses global_env().heap()) remain valid.
        let heap = globals.heap().clone();
        // Pairing heap with controller: install the policy's trigger
        // threshold (a bare heap defaults to inert) and cache the pending
        // flag the safe point reads.
        let gc = GcController::from_env();
        heap.borrow_mut().set_gc_threshold(gc.current_threshold());
        let gc_pending = heap.borrow().gc_pending_handle();
        heap.borrow_mut().enable_gc_freed_closure_tracking();
        let parameter_set = registry.resolve_index("scheme.base/%parameter-set!");
        Self {
            registers: Vec::new(),
            frames: Vec::new(),
            pending_escape: None,
            pending_transfer: false,
            reentry: Vec::new(),
            next_reentry: 1,
            reentry_kept: None,
            prompt_stack: Vec::new(),
            dynamic_winds: Vec::new(),
            exception_handlers: Vec::new(),
            code_store: Vec::new(),
            empty_code: Rc::new(CodeObject {
                id: CodeObjectId::label(),
                name: None,
                instructions: Vec::new(),
                constants: Vec::new(),
                num_regs: 0,
                arity: Arity::Fixed(0),
                source_map: Vec::new(),
                global_cache: Vec::new(),
                live_closures: Cell::new(0),
            }),
            free_code_ids: Vec::new(),
            code_units: FxHashMap::default(),
            wind_jump_code: None,
            value_wind_code: None,
            value_cwv_code: None,
            abort_handler_code: None,
            invoke_step_code: None,
            raise_step_code: None,
            force_code: None,
            resume_codes: [None; super::control::resume_step::VARIANTS],
            parameter_set,
            globals,
            heap,
            primitive_registry: Rc::new(registry),
            shadowed_primitives: Vec::new(),
            shadowed_controls: 0,
            scratch_args: Vec::new(),
            continuation_store: RefCell::new(FxHashMap::default()),
            delimited_continuation_store: RefCell::new(FxHashMap::default()),
            tracer: None,
            library_registry: None,
            loader_registry: None,
            fs: Arc::new(patina_core::NativeFs),
            gc: RefCell::new(gc),
            gc_pending,
        }
    }

    /// Record that the primitive at `index` had its global binding
    /// overwritten; `CallPrimitive` sites for it deoptimize from now on.
    pub fn mark_shadowed_primitive(&mut self, index: usize) {
        let word = index / 64;
        if word >= self.shadowed_primitives.len() {
            self.shadowed_primitives.resize(word + 1, 0);
        }
        self.shadowed_primitives[word] |= 1 << (index % 64);
    }

    /// Record that a binding of `form`'s procedure was given another value;
    /// its sites call their operator from now on.
    pub(crate) fn mark_shadowed_control(&mut self, form: ControlForm) {
        self.shadowed_controls |= form.bit();
    }

    /// Has a binding of `form`'s procedure been given another value?
    #[inline]
    pub(crate) fn is_control_shadowed(&self, form: ControlForm) -> bool {
        self.shadowed_controls & form.bit() != 0
    }

    /// Has the primitive at `index` been rebound since compilation?
    #[inline]
    pub fn is_primitive_shadowed(&self, index: usize) -> bool {
        self.shadowed_primitives
            .get(index / 64)
            .is_some_and(|w| w & (1 << (index % 64)) != 0)
    }

    /// Install all registered primitives into the global environment,
    /// ignoring import scoping — test scaffolding only, for VM unit tests
    /// that build a bare `VmState` with no library machinery to bootstrap
    /// from. Production setup (`VmBackend::with_fs`) deliberately skips this
    /// and lets `load_bootstrap()` define exactly the bootstrap libraries'
    /// exports; calling it there would reopen the hole where unimported names
    /// resolve (`import_set_is_enforced.rs`).
    ///
    /// Each primitive is stored as a `Procedure::Primitive` heap object so that
    /// `LoadGlobal` + `Call` can dispatch them via `call_primitive_proc`.
    pub fn install_primitives(&mut self) {
        let prims: Vec<_> = self
            .primitive_registry
            .primitives_indexed()
            .map(|(i, p)| (i, p.name, p.qualified_name(), p.arity.clone()))
            .collect();
        for (index, name, qualified_name, arity) in prims {
            let proc =
                Procedure::primitive(name, arity, Rc::from(qualified_name.as_str()), Some(index));
            let tv = self.heap.borrow_mut().alloc_procedure(proc);
            self.globals.define(name, tv);
        }
    }

    /// Load a code object that makes no closure, outside any unit, and return
    /// the id it runs from. For the runtime's own stubs, which are never let
    /// go.
    pub(super) fn load(&mut self, mut code: CodeObject) -> CodeObjectId {
        debug_assert!(
            !code
                .instructions
                .iter()
                .any(|instr| matches!(instr, Instruction::MakeClosure { .. })),
            "a code object loaded alone makes a closure, of code not loaded with it"
        );
        let id = self.next_code_id();
        code.id = id;
        self.code_store[id.index()] = Rc::new(code);
        id
    }

    /// Load what one compilation produced — `top`, and the code nested in it —
    /// as a unit, and return the id to run it from.
    ///
    /// Each code object is given a slot here, and the labels the compiler
    /// named it by, in its own id and in the `MakeClosure`s that make a
    /// closure of it, are replaced by that slot's id (#352).
    ///
    /// Once it has run, [`VmState::release_unit_if_unused`] lets it go if
    /// nothing it left behind can run it again (#338).
    pub fn load_unit(&mut self, top: CodeObject, nested: Vec<CodeObject>) -> CodeObjectId {
        let ids: FxHashMap<CodeObjectId, CodeObjectId> = std::iter::once(&top)
            .chain(&nested)
            .map(|code| (code.id, self.next_code_id()))
            .collect();
        debug_assert_eq!(
            ids.len(),
            nested.len() + 1,
            "a unit names two of its code objects by one label"
        );
        let top_id = ids[&top.id];
        let members = nested.iter().map(|code| ids[&code.id]).collect();
        for mut code in std::iter::once(top).chain(nested) {
            code.id = ids[&code.id];
            for instr in &mut code.instructions {
                if let Instruction::MakeClosure { code_id, .. } = instr {
                    // The compiler returns the code of every lambda it
                    // compiled alongside the code that makes it.
                    debug_assert!(
                        ids.contains_key(code_id),
                        "{:?} makes a closure of {code_id:?}, which was not compiled with it",
                        code.id
                    );
                    *code_id = ids.get(code_id).copied().unwrap_or(*code_id);
                }
            }
            let slot = code.id.index();
            self.code_store[slot] = Rc::new(code);
        }
        self.code_units.insert(top_id, members);
        top_id
    }

    /// The id to give the next code loaded: an empty slot's, or a new slot's.
    fn next_code_id(&mut self) -> CodeObjectId {
        self.free_code_ids.pop().unwrap_or_else(|| {
            let slot = u32::try_from(self.code_store.len())
                .expect("more than u32::MAX code objects loaded at once");
            self.code_store.push(Rc::clone(&self.empty_code));
            CodeObjectId::new(slot, 0)
        })
    }

    /// Let go of the unit loaded to run from `top`, if nothing can run any of
    /// its code again: no frame and no captured continuation holds it — they
    /// hold the code itself, so the store's is then its only reference — and
    /// no live closure names it.
    ///
    /// Without this the VM kept the code and constants of every form it had
    /// run, about 600 bytes each, for as long as it lived (#338).
    pub fn release_unit_if_unused(&mut self, top: CodeObjectId) {
        let Some(nested) = self.code_units.get(&top) else {
            return;
        };
        if unit_in_use(&self.code_store, top, nested) {
            return;
        }
        let nested = self.code_units.remove(&top).unwrap_or_default();
        release_unit(
            &mut self.code_store,
            &self.empty_code,
            &mut self.free_code_ids,
            top,
            &nested,
        );
    }

    #[inline(always)]
    fn loaded_code(&self, id: CodeObjectId) -> Option<&Rc<CodeObject>> {
        loaded(&self.code_store, id)
    }

    /// A closure was made of the code `id`.
    pub(super) fn note_closure_made(&self, id: CodeObjectId) {
        let code = self.loaded_code(id);
        // The code making the closure is running, and a unit is let go only
        // whole, so the closure's own code is loaded; a count missed here
        // would free code this closure can still run.
        debug_assert!(
            code.is_some(),
            "a closure made of {id:?}, which is not loaded"
        );
        if let Some(code) = code {
            code.live_closures.set(code.live_closures.get() + 1);
        }
    }

    /// Retire `closure`, the closure a `Step::Eval` compiled, once its call
    /// has returned (`Heap::retire_vm_closure`): count it down, and let go of
    /// its unit if nothing else needs it — a frame, a continuation, or a
    /// closure its code made. Nothing if it was retired already.
    pub(super) fn retire_eval_closure(&mut self, closure: TaggedValue) {
        let Some(id) = self.heap.borrow_mut().retire_vm_closure(closure) else {
            return;
        };
        let id = CodeObjectId(id);
        if let Some(code) = self.loaded_code(id) {
            code.live_closures
                .set(code.live_closures.get().saturating_sub(1));
        }
        self.release_unit_if_unused(id);
    }

    /// After a collection: count down the closures it freed, and let go of
    /// every unit nothing can run any longer.
    ///
    /// Every unit, not only those whose closures died: a collection also
    /// prunes continuations nothing reaches, and their frames may have been
    /// the last thing holding a unit's code. A count kept too high only keeps
    /// code a collection longer; one too low would free code a closure can
    /// still run, which is why it only goes down for a closure the collector
    /// has freed.
    fn after_collection(&mut self) {
        let freed = self.heap.borrow_mut().take_gc_freed_closure_code_ids();
        for id in freed {
            if id == patina_core::heap::Heap::RETIRED_VM_CLOSURE_CODE {
                // Counted down when it was retired.
                continue;
            }
            let id = CodeObjectId(id);
            let code = self.loaded_code(id);
            // Counted when it was made, and its code kept while it lived.
            debug_assert!(
                code.is_some_and(|code| code.live_closures.get() > 0),
                "a freed closure of {id:?}, whose code counts no live closure"
            );
            if let Some(code) = code {
                code.live_closures
                    .set(code.live_closures.get().saturating_sub(1));
            }
        }
        let store = &mut self.code_store;
        let free = &mut self.free_code_ids;
        let empty = &self.empty_code;
        self.code_units.retain(|&top, nested| {
            let in_use = unit_in_use(store, top, nested);
            if !in_use {
                release_unit(store, empty, free, top, nested);
            }
            in_use
        });
    }

    /// Fetch a loaded `CodeObject` by id.
    #[inline(always)]
    pub(super) fn code_object(&self, id: CodeObjectId) -> Result<Rc<CodeObject>, VmError> {
        self.loaded_code(id)
            .cloned()
            .ok_or_else(|| missing_code_object(id))
    }

    // ── Register helpers ───────────────────────────────────────────────────

    // `inline(always)` throughout: `dispatch_one_instruction` is so large
    // that LLVM declines plain `#[inline]` for these, leaving a real call
    // on every register access (~6-8% of runtime; PRD §1.6 finding 1).

    #[inline(always)]
    fn frame_base(&self) -> usize {
        self.frames.last().expect("no active frame").register_base
    }

    #[inline(always)]
    pub fn reg(&self, reg: u16) -> TaggedValue {
        self.reg_at(self.frame_base(), reg)
    }

    #[inline(always)]
    pub fn set_reg(&mut self, reg: u16, val: TaggedValue) {
        let base = self.frame_base();
        self.set_reg_at(base, reg, val);
    }

    // Base-relative variants for the dispatch loop: `base` is the top
    // frame's `register_base`, hoisted once per dispatch. Only instruction
    // arms that cannot push or pop a frame before the access may use these
    // — after a frame change the hoisted base addresses the wrong window
    // (a `set_reg` at a dispatch site means "the frame changed here"). The
    // debug assert makes that rule machine-checked across the test suite.
    //
    // The primitive-call arms are the exception that proves it: they hand the
    // hoisted base to a callee that *can* pop frames, because a higher-order
    // primitive re-enters the VM. They are safe only because
    // `exec_call_primitive` re-reads the depth before writing.

    #[inline(always)]
    pub(super) fn reg_at(&self, base: usize, reg: u16) -> TaggedValue {
        debug_assert_eq!(base, self.frame_base());
        self.registers[base + reg as usize]
    }

    #[inline(always)]
    pub(super) fn set_reg_at(&mut self, base: usize, reg: u16, val: TaggedValue) {
        debug_assert_eq!(base, self.frame_base());
        self.registers[base + reg as usize] = val;
    }

    pub fn alloc_registers(&mut self, num_regs: u16) -> usize {
        let base = self.registers.len();
        self.registers
            .resize(base + num_regs as usize, TaggedValue::NULL);
        base
    }

    pub fn free_top_registers(&mut self, base: usize) {
        self.registers.truncate(base);
    }

    /// Allocate a full VM continuation, returning its heap `TaggedValue` handle.
    ///
    /// The ref object (which mints the id) and the store entry are created
    /// back-to-back within one instruction dispatch, so no safe point can
    /// observe one without the other — required for the weak-table protocol.
    pub fn alloc_vm_continuation(&mut self, cont: VmContinuation) -> TaggedValue {
        let (tv, id) = self.heap.borrow_mut().alloc_vm_continuation_ref();
        self.continuation_store
            .borrow_mut()
            .insert(id, Rc::new(cont));
        tv
    }

    /// Allocate a delimited VM continuation, returning its heap `TaggedValue` handle.
    pub fn alloc_vm_delimited_continuation(
        &mut self,
        cont: VmDelimitedContinuation,
    ) -> TaggedValue {
        let (tv, id) = self.heap.borrow_mut().alloc_vm_delimited_continuation_ref();
        self.delimited_continuation_store
            .borrow_mut()
            .insert(id, Rc::new(cont));
        tv
    }

    /// Look up a full continuation by its `TaggedValue` handle.
    pub fn get_vm_continuation(&self, tv: TaggedValue) -> Option<Rc<VmContinuation>> {
        let id = self.heap.borrow().get_vm_continuation_ref(tv)?;
        self.continuation_store.borrow().get(&id).cloned()
    }

    /// Look up a delimited continuation by its `TaggedValue` handle.
    pub fn get_vm_delimited_continuation(
        &self,
        tv: TaggedValue,
    ) -> Option<Rc<VmDelimitedContinuation>> {
        let id = self.heap.borrow().get_vm_delimited_continuation_ref(tv)?;
        self.delimited_continuation_store.borrow().get(&id).cloned()
    }

    pub fn current_code(&self) -> Result<Rc<CodeObject>, VmError> {
        Ok(self.frames.last().expect("no active frame").code.clone())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Library loading for eval primitives
// ─────────────────────────────────────────────────────────────────────────────

use crate::compiler::compile_with_qq_resolving;
use patina_frontend::Desugarer;
use patina_runtime::Library;
use patina_runtime::library_loader::{ImportSet, build_library};
use patina_runtime::library_registry::LibraryError;

/// Load a Scheme library by name using the shared registries on `VmState`.
///
/// This mirrors `VmBackend::load_library()` but operates through the
/// `Rc<RefCell<...>>` registries stored on `VmState`, so it can be called
/// from `VmApplyContext` during eval primitive execution.
pub(super) fn vm_load_library(
    state: &mut VmState,
    name: &[String],
) -> Result<Library, LibraryError> {
    let library_registry = state
        .library_registry
        .as_ref()
        .ok_or_else(|| LibraryError::not_found(name))?
        .clone();
    let loader_registry = state
        .loader_registry
        .as_ref()
        .ok_or_else(|| LibraryError::not_found(name))?
        .clone();

    // Check if already loaded
    {
        let registry = library_registry.borrow();
        if let Some(lib) = registry.get(name) {
            return Ok(lib.clone());
        }
    }

    // Circular dependency detection, ended on every way out (#436): a
    // failure left on the stack reads as a circular dependency to the next
    // import of the same library, which a program can make after catching the
    // first failure — `eval` of an `import` inside a `guard`, or `environment`.
    let loading = LibraryRegistry::begin_loading_scoped(&library_registry, name)?;

    let search_paths: Vec<std::path::PathBuf> = library_registry.borrow().search_paths().to_vec();
    let heap = state.globals.heap().clone();

    // Try simple (Rust) loaders first
    let rust_result = {
        let loaders = loader_registry.borrow();
        loaders.try_simple_load_with_heap(name, &search_paths, heap.clone())?
    };

    let lib = if let Some(lib) = rust_result {
        lib
    } else {
        // Try evaluating (Scheme .sld) loaders
        let can_load_library =
            |lib_name: &[String]| patina_frontend::cond_expand::library_available(&heap, lib_name);

        let parsed = {
            let loaders = loader_registry.borrow();
            loaders.try_parse_with_heap_and_library_checker(
                name,
                &search_paths,
                heap.clone(),
                &can_load_library,
            )?
        };

        match parsed {
            Some(parsed) => vm_evaluate_parsed_library(state, parsed)?,
            None => return Err(LibraryError::not_found_in(name, &search_paths)),
        }
    };

    // End loading tracking
    drop(loading);

    // Register the library
    let _ = library_registry.borrow_mut().register(lib);

    // Return from registry
    library_registry
        .borrow()
        .get(name)
        .cloned()
        .ok_or_else(|| LibraryError::not_found(name))
}

/// Evaluate a parsed library (.sld file) using the VM.
///
/// Mirrors `VmBackend::evaluate_parsed_library()`.
///
/// Instead of creating a temporary VmState, we swap `state.globals` to the
/// library's environment, execute body expressions directly in the main
/// state, then swap back. This ensures continuations, code objects, and
/// closures all live in the single real execution context.
fn vm_evaluate_parsed_library(
    state: &mut VmState,
    parsed: patina_runtime::library_loader::ParsedLibrary,
) -> Result<Library, LibraryError> {
    let lib_env = Rc::new(Environment::with_heap(state.globals.heap().clone()));

    // Step 1: Resolve imports into lib_env
    for import_set in &parsed.imports {
        vm_process_import_set(state, import_set, &lib_env)?;
    }

    // Step 2: Swap globals to lib_env, compile + execute each body expression
    // in the main VmState, then swap back.
    // Closures created during execution capture lib_env as their globals
    // (per-closure environment pointer), so no seeding or merge is needed.

    // Collection is already deferred for this whole function: `parsed`
    // carries a `GcDeferGuard` while it holds unevaluated body forms (see
    // `ParsedLibrary`), which also covers `saved_globals` and `lib_env`.

    let saved_globals = state.globals.clone();
    state.globals = lib_env.clone();

    // A relative `include` in the body resolves beside the `.sld` — the same
    // rule as the backend's loader; a library must not load or fail
    // depending on which door it came through.
    let desugarer = Desugarer::with_env(lib_env.clone())
        .with_fs(state.fs.clone())
        .with_include_base_of(parsed.source.as_deref());
    let shared_heap = lib_env.heap().clone();

    let body_result = (|| -> Result<(), LibraryError> {
        for tv in &parsed.body {
            let core_expr = desugarer.desugar_tagged(*tv, &shared_heap).map_err(|e| {
                LibraryError::ParseError {
                    file: parsed
                        .source
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    message: format!("desugar error: {}", e),
                }
            })?;

            let (top, nested) = compile_with_qq_resolving(
                &core_expr,
                &shared_heap,
                &lib_env,
                &state.primitive_registry,
            )
            .map_err(|e| LibraryError::ParseError {
                file: parsed
                    .source
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                message: format!("compile error: {}", e),
            })?;

            // Each body form is a re-entry boundary: a raise in it can reach
            // a handler of the program's, and one that escapes (a `guard`
            // around the `eval` or `environment` that loads the library)
            // leaves this load. The rest of the body must not run then, and
            // the library must not be registered or its exports bound.
            let top_id = state.load_unit(top, nested);
            let depth_before = state.frames.len();
            let result = across_reentry(state, depth_before, |s| execute_nested(s, top_id), |v| *v)
                .map_err(Reentry::into_vm_error);
            state.release_unit_if_unused(top_id);

            result.map_err(|e| LibraryError::ParseError {
                file: parsed
                    .source
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                message: format!("runtime error: {}", e),
            })?;
        }
        Ok(())
    })();

    // Always restore globals, even on error
    state.globals = saved_globals;
    body_result?;

    // Step 3: Assemble the library and resolve its exports
    build_library(parsed, lib_env)
}

/// Resolve an import set into the given environment.
fn vm_process_import_set(
    state: &mut VmState,
    import_set: &ImportSet,
    lib_env: &Rc<Environment>,
) -> Result<(), LibraryError> {
    match import_set {
        ImportSet::Library(lib_name) => {
            let imported_lib = vm_load_library(state, lib_name)?;
            for name in imported_lib.export_names() {
                import_export(state, lib_env, name.to_string(), &imported_lib, name);
            }
            Ok(())
        }
        ImportSet::Only {
            import_set,
            identifiers,
        } => {
            let temp_env = Rc::new(Environment::with_heap(state.globals.heap().clone()));
            vm_process_import_set(state, import_set, &temp_env)?;
            for id in identifiers {
                if temp_env.local_slot(id).is_none() {
                    return Err(LibraryError::parse(
                        None,
                        format!("Identifier '{}' not found in import set", id),
                    ));
                }
                import_staged(state, lib_env, id.clone(), &temp_env, id);
            }
            Ok(())
        }
        ImportSet::Except {
            import_set,
            identifiers,
        } => {
            let temp_env = Rc::new(Environment::with_heap(state.globals.heap().clone()));
            vm_process_import_set(state, import_set, &temp_env)?;
            let exclude: std::collections::HashSet<_> = identifiers.iter().collect();
            for name in temp_env.local_names() {
                if !exclude.contains(&name) {
                    import_staged(state, lib_env, name.clone(), &temp_env, &name);
                }
            }
            Ok(())
        }
        ImportSet::Prefix { import_set, prefix } => {
            let temp_env = Rc::new(Environment::with_heap(state.globals.heap().clone()));
            vm_process_import_set(state, import_set, &temp_env)?;
            for name in temp_env.local_names() {
                import_staged(
                    state,
                    lib_env,
                    format!("{}{}", prefix, name),
                    &temp_env,
                    &name,
                );
            }
            Ok(())
        }
        ImportSet::Rename {
            import_set,
            renames,
        } => {
            let temp_env = Rc::new(Environment::with_heap(state.globals.heap().clone()));
            vm_process_import_set(state, import_set, &temp_env)?;
            // Refused, as by the backend's resolver: see there (#489).
            for (old_name, _) in renames {
                if temp_env.local_slot(old_name).is_none() {
                    return Err(LibraryError::parse(
                        None,
                        format!("Identifier '{}' not found for rename", old_name),
                    ));
                }
            }
            let rename_map: std::collections::HashMap<_, _> = renames
                .iter()
                .map(|(o, n)| (o.clone(), n.clone()))
                .collect();
            for name in temp_env.local_names() {
                let exported_name = rename_map.get(&name).unwrap_or(&name).clone();
                import_staged(state, lib_env, exported_name, &temp_env, &name);
            }
            Ok(())
        }
    }
}

/// Evaluate a datum expression in the given environment using the VM.
///
/// `VmApplyContext::eval_expr`: for a caller with no frame to run the code
/// in, which runs a resumable primitive's `Step::Eval` synchronously. The
/// `eval` primitive itself has its datum run as a frame ([`eval_closure`],
/// #477), since a continuation cannot carry this Rust frame. Swaps
/// `state.globals` to the given environment, executes in the main VmState
/// using `execute_nested` (which respects the current frame depth), then
/// restores globals.
pub(super) fn vm_eval_expr(
    state: &mut VmState,
    expr: TaggedValue,
    env: &Rc<Environment>,
) -> Result<TaggedValue, VmError> {
    let (top, nested) = compile_for_eval(state, expr, env)?;

    // Swap globals to the eval environment, execute in the main state,
    // then restore. This keeps continuations and code objects valid.
    //
    // `saved_globals` is reachable only from this Rust frame while the swap
    // is in effect, so defer for its extent rather than relying on this
    // always being reached from inside a dispatch loop.
    let _gc_defer = GcDeferGuard::new(&state.heap);

    let saved_globals = state.globals.clone();
    state.globals = env.clone();

    let top_id = state.load_unit(top, nested);

    let result = execute_nested(state, top_id);
    state.release_unit_if_unused(top_id);

    // Always restore globals, even on error
    state.globals = saved_globals;

    result
}

/// Expand and compile the datum `expr` in `env`, as `eval` does.
///
/// An `import` is done here, into `env`, as the tree-walker's `eval` and the
/// backend's top level do it, and what is compiled is its value, unspecified:
/// the compiler has nothing to make of an import, and compiled one to
/// nothing, so an `import` that `eval` or `load` evaluated imported nothing
/// (#482).
///
/// # State contract
///
/// Reads the environment and registry; runs the expander, which may load
/// libraries, and for an `import` loads its libraries and binds their
/// exports in `env`. Loading a library runs its body, so this is a re-entry
/// into the machine: its callers cross it as a boundary (`across_reentry`).
/// Leaves no code of its own loaded and no frame pushed.
fn compile_for_eval(
    state: &mut VmState,
    expr: TaggedValue,
    env: &Rc<Environment>,
) -> Result<(CodeObject, Vec<CodeObject>), VmError> {
    let desugarer = Desugarer::with_env(env.clone()).with_fs(state.fs.clone());
    let heap = state.globals.heap().clone();

    let mut core_expr = desugarer
        .desugar_tagged(expr, &heap)
        .map_err(|e| VmError::Runtime {
            message: format!("eval: desugar error: {}", e),
        })?;

    if let CoreExprKind::Import { import_sets } = &core_expr.kind {
        for &import_set in import_sets {
            let import_set =
                patina_frontend::LibraryDefinition::parse_import_set_tagged(import_set, &heap)
                    .map_err(|e| VmError::Runtime {
                        message: format!("Invalid import set: {}", e),
                    })?;
            vm_process_import_set(state, &import_set, env).map_err(|e| VmError::Runtime {
                message: e.to_string(),
            })?;
        }
        core_expr = CoreExpr::new(CoreExprKind::Literal(TaggedValue::UNSPECIFIED));
    }

    compile_with_qq_resolving(&core_expr, &heap, env, &state.primitive_registry).map_err(|e| {
        VmError::Runtime {
            message: format!("eval: compile error: {}", e),
        }
    })
}

/// The datum `expr`, compiled in `env`, as a closure of no arguments whose
/// globals are `env`: what a resumable primitive's `Step::Eval` has its stub
/// frame call (#477). Its frames read and define `env`'s globals as any
/// closure's do, so nothing is swapped, and its code lives as long as the
/// closure or a frame running it, as any unit's does.
///
/// # State contract
///
/// Loads the compiled unit and allocates the closure; pushes no frame and
/// runs no Scheme code.
pub(super) fn eval_closure(
    state: &mut VmState,
    expr: TaggedValue,
    env: &Rc<Environment>,
) -> Result<TaggedValue, VmError> {
    let (top, nested) = compile_for_eval(state, expr, env)?;
    let top_id = state.load_unit(top, nested);
    let closure = state
        .heap
        .borrow_mut()
        .alloc_vm_closure(top_id.0, Vec::new(), env.clone());
    state.note_closure_made(top_id);
    Ok(closure)
}

// ─────────────────────────────────────────────────────────────────────────────
// Execution loop
// ─────────────────────────────────────────────────────────────────────────────

/// The code loaded for `id`, if any — none once it has been let go, even when
/// its slot holds other code by now.
#[inline(always)]
fn loaded(store: &[Rc<CodeObject>], id: CodeObjectId) -> Option<&Rc<CodeObject>> {
    store.get(id.index()).filter(|code| code.id == id)
}

/// Whether a frame, a captured continuation or a live closure still needs any
/// of the unit that runs from `top`. A frame and a continuation hold the code
/// itself, so the store's `Rc` is then not the only one; a closure names it by
/// id, and is counted.
fn unit_in_use(store: &[Rc<CodeObject>], top: CodeObjectId, nested: &[CodeObjectId]) -> bool {
    std::iter::once(&top).chain(nested).any(|&id| {
        loaded(store, id)
            .is_some_and(|code| code.live_closures.get() > 0 || Rc::strong_count(code) > 1)
    })
}

/// Drop a unit's code from the store, and give its slots to later code.
fn release_unit(
    store: &mut [Rc<CodeObject>],
    empty: &Rc<CodeObject>,
    free: &mut Vec<CodeObjectId>,
    top: CodeObjectId,
    nested: &[CodeObjectId],
) {
    for &id in std::iter::once(&top).chain(nested) {
        let Some(slot) = store.get_mut(id.index()) else {
            continue;
        };
        if slot.id == id {
            *slot = Rc::clone(empty);
            free.extend(id.next_generation());
        }
    }
}

/// Outlined error constructor for a `code_object` miss — `#[cold]` keeps the
/// formatting machinery out of the callers that inline the lookup.
#[cold]
#[inline(never)]
fn missing_code_object(id: CodeObjectId) -> VmError {
    VmError::Runtime {
        message: format!("missing CodeObject {id:?}"),
    }
}

/// Execute the code object identified by `code_id` in `state`, with no
/// arguments. Returns the value in register 0 of the top frame on completion.
///
/// This is the primary entry point for running a compiled top-level expression.
pub fn execute(state: &mut VmState, code_id: CodeObjectId) -> Result<TaggedValue, VmError> {
    // Set up the initial frame.
    let code = state.code_object(code_id)?;

    let base = state.alloc_registers(code.num_regs);
    state.frames.push(CallFrame {
        pc: 0,
        register_base: base,
        num_regs: code.num_regs,
        closure: None,
        return_reg: 0,
        code,
    });

    let result = run_loop_until(state, 0);
    // Nothing is left to resume: an escape that reached depth 0 *is* this
    // form's value, and a loop re-parks one on the way out for the boundary
    // it may have crossed. Dropped with the transfer flag so neither is
    // mistaken for in-flight when the next form runs.
    state.pending_escape = None;
    state.pending_transfer = false;
    state.reentry_kept = None;
    if result.is_err() {
        // An error that reaches the top level abandons whatever the machine
        // was doing: the frames it was running, the handlers and wind
        // records their extents installed, an escape parked mid-flight. None
        // of it can be resumed, and the next `execute` runs "until the frame
        // stack is empty" — left in place, the abandoned frames would be
        // where that next form *returns to*, and a REPL or a script run with
        // `-k` would run the dead frames after it, re-reporting the old
        // error. Winds are dropped, not unwound: their after-thunks were
        // never owed a run by an abort, and one that raised would abort the
        // recovery.
        //
        // Unless the error interrupted an `exit`: the runner that reports it
        // must still end the process, so it is noted before the frames that
        // say so are gone.
        if let Some(status) = super::control::exit_in_progress(state) {
            patina_runtime::exit_status::note_interrupted_exit(status);
        }
        state.frames.clear();
        state.registers.clear();
        state.pending_escape = None;
        state.prompt_stack.clear();
        state.dynamic_winds.clear();
        state.exception_handlers.clear();
    }
    result
}

/// Execute a code object in the **current** `VmState`, returning to the
/// caller's frame depth. Unlike `execute` (which always runs until the
/// frame stack is empty), this variant is safe to call when the state
/// already has in-flight frames (e.g. during library loading from `eval`).
pub fn execute_nested(state: &mut VmState, code_id: CodeObjectId) -> Result<TaggedValue, VmError> {
    let depth_before = state.frames.len();
    let code = state.code_object(code_id)?;

    let base = state.alloc_registers(code.num_regs);
    state.frames.push(CallFrame {
        pc: 0,
        register_base: base,
        num_regs: code.num_regs,
        closure: None,
        return_reg: 0,
        code,
    });

    run_loop_until(state, depth_before)
}

/// How a dispatch loop ended.
#[derive(Clone, Copy)]
pub(super) enum LoopExit {
    /// The frame this loop was driving returned normally. The value is its
    /// result, and the loop's caller still owns the frame below it.
    Returned(TaggedValue),
    /// A continuation unwound the stack to or past this loop's exit depth.
    /// The value is the one that continuation carries — it belongs to the
    /// resumed computation, and the loop's caller owns no live frame to put
    /// it in.
    Escaped(TaggedValue),
}

impl LoopExit {
    fn value(self) -> TaggedValue {
        match self {
            LoopExit::Returned(v) | LoopExit::Escaped(v) => v,
        }
    }
}

/// [`run_loop_until_outcome`] for callers that are not synchronous boundaries
/// — the top-level `execute`, which has no frame left to corrupt, and the
/// nested entry points that route the distinction through `across_reentry`
/// instead.
pub(super) fn run_loop_until(
    state: &mut VmState,
    exit_depth: usize,
) -> Result<TaggedValue, VmError> {
    run_loop_until_outcome(state, exit_depth).map(LoopExit::value)
}

/// The main dispatch loop. Runs until `state.frames.len() == exit_depth`.
///
/// Use `exit_depth = 0` to run until the frame stack is fully empty (top-level).
/// Use `exit_depth = N` to run a nested thunk until it returns to depth N.
///
/// This is the only place that decides whether a continuation invocation is
/// this loop's business: the frames it restored are either ones this loop
/// still owns (resume) or ones further out (exit, reporting `Escaped`). A
/// continuation that arrives from outside the re-entry boundary this loop
/// runs in is further out whatever frames it restored
/// (`VmState::reentry_kept`). Every
/// synchronous boundary below reports the invocation as
/// [`VmError::ContinuationEscape`] and lets the decision happen here once.
pub(super) fn run_loop_until_outcome(
    state: &mut VmState,
    exit_depth: usize,
) -> Result<LoopExit, VmError> {
    // Every dispatch loop defers collection for its own extent; only the
    // outermost one reaches its safe point un-deferred. A nested loop's
    // caller has live values in Rust locals — capture-time register clones,
    // `mem::take`n buffers, primitive argument vectors — that no root
    // provider can see. See `docs/GC_DESIGN.md` §7.
    let gc_defer = GcDeferGuard::new(&state.heap);
    // Loop invariant, hoisted out of the safe point. The cached pending-flag
    // handle makes the per-instruction check a single load — no borrow.
    let is_outermost = gc_defer.is_outermost();

    // Loop-resident copy of the top frame's code object: dispatch borrows it
    // instead of cloning the `Rc` out of the frame on every instruction (two
    // refcount writes per dispatch), refreshing it in its prologue when the
    // frame's code changes — call, return, tail call, continuation invoke.
    let mut cur_code = state.current_code()?;

    // A handler installed during this loop is dead once the loop's frame has
    // returned, and this count is the only thing that identifies it: a
    // tail-called `with-exception-handler` installs at `exit_depth`, which is
    // also where a handler this loop was *started under* sits when whatever
    // started it tail-replaced its thunk's frame. So the exit-depth `Return`
    // pops nothing (see `pop_resolved_extents`) and the loop closes its own on
    // the way out.
    //
    // The examples were control primitives that ran a thunk of the program's
    // on a nested loop: the value form of `dynamic-wind` until 2026-09-02
    // (issue #157), then `call-with-values`' producer reached as a value —
    // `(with-exception-handler h (lambda () (cwv p c)))` ran `p` with `h` at
    // that depth, still owed a raise from `c` — until 2026-09-23 (#442). Both
    // run their thunks as frames of a stub now. The count stays: whether a
    // loop still started some other way can begin from a tail-replaced frame
    // has not been re-checked, and it costs a load.
    let handlers_at_entry = state.exception_handlers.len();
    // The same for prompts. A nested loop that returns at its own exit depth
    // pops nothing there by design (`pop_resolved_extents`), so a prompt
    // opened *inside* it — by a parameter converter, by a primitive's
    // comparator — outlives the loop and is found by the next abort. The
    // handler half of this has been here since those extents were keyed on
    // frame depth; `finish_delimited_invoke`'s comment has named the prompt
    // half as missing for as long.
    //
    // Not the fix for a prompt a *continuation's snapshot* carries past its
    // own body: that one is closed on arrival (issue #176), because no loop
    // need return between the re-entry and the abort that finds it.
    let prompts_at_entry = state.prompt_stack.len();
    // The re-entry boundaries this loop runs inside. A continuation arriving
    // from outside the innermost of them leaves it, and this loop with it,
    // however many frames it restored (`VmState::reentry_kept`).
    let reentry_level = state.reentry.len();

    loop {
        // GC safe point: all live state is on `VmState`, capture temporaries
        // are dead, buffers are restored, and no heap borrow is outstanding.
        // A collection happened if one was pending and sweep has cleared the
        // flag since: a nested loop never collects, and leaves it set. Two
        // `Cell` reads, where following every pending safe point would borrow
        // the heap on each instruction of a callback that crossed the
        // threshold.
        let pending = state.gc_pending.get();
        maybe_collect(state, is_outermost);
        if pending && !state.gc_pending.get() {
            state.after_collection();
        }

        match dispatch_one_instruction(state, &mut cur_code, exit_depth) {
            Ok(Some(val)) => {
                state.exception_handlers.truncate(handlers_at_entry);
                state.prompt_stack.truncate(prompts_at_entry);
                return Ok(LoopExit::Returned(val));
            }
            Ok(None) => continue,
            Err(e) => {
                // Checked before the catchability test on purpose: the
                // sentinel crosses the registry boundary as an ordinary
                // `VmError` and would otherwise look catchable and be handed
                // to a `guard`. See `VmState::pending_escape`.
                if let Some(value) = state.pending_escape.take() {
                    let left = state.reentry_kept.is_some_and(|kept| kept < reentry_level);
                    if left || state.frames.len() <= exit_depth {
                        // Still in flight: this loop does not own the frame
                        // the escape landed in, so the one that does must
                        // still find it parked. Returning it in `Escaped`
                        // alone is not enough — `run_loop_until` flattens
                        // that to a plain value, and the boundaries above
                        // (`across_reentry`) unwind on the parked state.
                        state.pending_escape = Some(value);
                        return Ok(LoopExit::Escaped(value));
                    }
                    // Control resumed in a frame this loop still owns, so
                    // the landing (if this was one) is about to run and no
                    // boundary further out is owed the news. Nor further in:
                    // every boundary the arrival left has already unwound,
                    // or this loop could not be running.
                    state.pending_transfer = false;
                    state.reentry_kept = None;
                    cur_code = state.current_code()?;
                    continue;
                }

                // Attach source location from the current frame's code object if available.
                let e = attach_source_location(state, e);

                // Route catchable errors through exception handlers
                if is_catchable(&e) && !state.exception_handlers.is_empty() {
                    let (kind, message) = classify_error(&e);
                    let exception = state
                        .heap
                        .borrow_mut()
                        .alloc_exception(kind, message, vec![]);
                    // The raise pushes `raise_step_stub` and the loop drives
                    // it. Register 0 as the destination is a placeholder: this
                    // error has no result slot, and a non-continuable raise
                    // has no value to deliver — `ResumeRaise` raises the
                    // secondary rather than reaching the stub's `Return`.
                    //
                    // It used to be delivered: the old path let a returning
                    // handler write its value here, which is how `(list
                    // (car 5))` under a returning handler answered `(())`
                    // with the handler's value in r0 (Track L §6). That is
                    // closed. What is left is narrower than what it replaced
                    // — the `Return` is reachable only if the *secondary's*
                    // own handler returns a value here — and an attempt to
                    // build that (a composable continuation as the secondary's
                    // handler) raises a tertiary instead and terminates at the
                    // enclosing `guard`. Give the stub a sentinel destination
                    // if a program is ever found that reaches it.
                    vm_raise_value(state, exception, 0, false)?;
                    continue;
                }
                return Err(e);
            }
        }
    }
}

/// GC safe point: the VM's root set, handed to the shared driver.
///
/// The protocol — `(gc)` honored in every mode, only the outermost guard
/// collects, one borrow spans the collection — lives in
/// `GcController::safe_point`; this supplies only what is VM-specific.
#[inline]
fn maybe_collect(state: &VmState, is_outermost: bool) {
    GcController::safe_point(
        &state.gc,
        &state.heap,
        &state.gc_pending,
        is_outermost,
        |collect| {
            // Libraries are a root set. If a load is in flight we cannot read the
            // registry, so return without collecting rather than trace a partial
            // root set — a missing root is a use-after-free.
            let Ok(registry) = LibraryRegistry::try_roots(state.library_registry.as_deref()) else {
                return;
            };
            match &registry {
                Some(registry) => collect(&[state, &**registry]),
                None => collect(&[state]),
            }
        },
    );
}

/// Attach a source location to an error if it doesn't already have one.
/// Looks up the current frame's code object and uses the PC to find the
/// closest source location from the compiled source map.
///
/// The innermost frame is not always the one that *has* a source map. The
/// stubs the runtime builds rather than compiles carry none at all —
/// `value_wind_stub`, `value_cwv_stub`, `wind_jump_stub`, `invoke_step_stub`,
/// `abort_handler_stub`, `raise_step_stub` and `force_stub` — and any of them
/// can be the top frame when an error is raised: the value form's thunks
/// tail-call out of their own frames, leaving the stub innermost, and a
/// jump's, a composable invoke's re-entry thunks, an abort's handler, a
/// raise's handler and a promise's thunk do the same. Read
/// literally, that costs the error its caret entirely — `(dw (lambda () 1)
/// (lambda () (error "boom")) (lambda () 2))` printed a bare message where
/// head-position `dynamic-wind` printed file, line and source line.
///
/// So a source-map-less frame is *skipped*, and the location comes from the
/// call site that pushed it, which is what the reader wants: the stub is
/// machinery, not a place in the program. Only an entirely empty map counts as
/// "not a place" — a compiled frame that merely has no entry at this pc still
/// stops the search, as it always did, rather than blaming its caller.
fn attach_source_location(state: &VmState, e: VmError) -> VmError {
    // Don't double-wrap if already has a location.
    if e.source_location().is_some() {
        return e;
    }
    for frame in state.frames.iter().rev() {
        if frame.code.source_map.is_empty() {
            continue;
        }
        // PC was already advanced before dispatch, so use pc-1.
        let pc = frame.pc.saturating_sub(1);
        if let Some(loc) = frame.code.source_location(pc) {
            return e.at(loc.clone());
        }
        break;
    }
    e
}

/// Get the effective globals environment for the current frame.
///
/// If the current frame is executing a closure, returns that closure's
/// captured globals (the environment it was compiled against). Otherwise
/// returns `state.globals` (for top-level code).
pub(super) fn frame_globals(state: &VmState) -> Rc<Environment> {
    if let Some(closure_idx) = state.frames.last().and_then(|f| f.closure)
        && let Some(globals) = state.heap.borrow().get_vm_closure_globals(closure_idx)
    {
        return globals;
    }
    state.globals.clone()
}

/// Shared skeleton of every inline primitive opcode arm (Track P P3): run
/// `$fast` (which yields `Option<TaggedValue>`) only while the primitive is
/// unshadowed; on `Some`, write `$dst`; otherwise funnel the exact same
/// operands through `exec_call_primitive` — the registry handler the generic
/// path uses — and propagate a continuation escape out of
/// `dispatch_one_instruction`. Keeping the guard/fallback/escape plumbing
/// here means an opcode arm contains only its genuinely opcode-specific
/// fast path.
macro_rules! inline_primitive {
    ($state:ident, $base:ident, $exit_depth:ident, $func_id:ident, $name:ident, $dst:ident,
     [$($arg:expr),+], $fast:block) => {
        let fast = if !$state.is_primitive_shadowed($func_id.0 as usize) {
            $fast
        } else {
            None
        };
        match fast {
            Some(val) => $state.set_reg_at($base, $dst, val),
            None => {
                if let Some(escaped) = exec_call_primitive(
                    $state, $base, $func_id, $name, &[$($arg),+], $dst, $exit_depth,
                )? {
                    return Ok(Some(escaped));
                }
            }
        }
    };
}

/// Dispatch a single instruction. Returns `Ok(Some(val))` if the loop should
/// exit, `Ok(None)` to continue, or `Err` on error.
fn dispatch_one_instruction(
    state: &mut VmState,
    cur_code: &mut Rc<CodeObject>,
    exit_depth: usize,
) -> Result<Option<TaggedValue>, VmError> {
    // One frame access per dispatch: refresh the loop's cached code object
    // (no Rc clone per instruction — only when the frame's code changed),
    // read `pc` and fold in its advance (instructions that jump overwrite
    // it), and hoist the register window base for the frame-stable arms
    // (`reg_at`/`set_reg_at`).
    let (pc, base) = {
        let f = state.frames.last_mut().expect("empty frame stack");
        if !Rc::ptr_eq(cur_code, &f.code) {
            *cur_code = f.code.clone();
        }
        let pc = f.pc;
        f.pc = pc + 1;
        (pc, f.register_base)
    };
    let code: &CodeObject = cur_code;

    let instr = code.instructions.get(pc).ok_or_else(|| VmError::Runtime {
        message: format!("PC {} out of bounds in #{}", pc, code.id),
    })?;

    // ── Trace: before instruction ────────────────────────────────────
    if let Some(tracer) = state.tracer.clone() {
        let f = state.frames.last().unwrap();
        let depth = state.frames.len();
        tracer.borrow_mut().pre_instruction_with_depth(
            &state.registers,
            f,
            code.id,
            pc,
            instr,
            &state.heap,
            depth,
        );
    }

    match *instr {
        // ── Load / Store ────────────────────────────────────────────────
        Instruction::LoadImmediate { dst, val } => {
            state.set_reg_at(base, dst, val);
        }

        Instruction::LoadConst { dst, idx } => {
            let val =
                code.constants
                    .get(idx as usize)
                    .copied()
                    .ok_or_else(|| VmError::Runtime {
                        message: format!("constant index {} out of bounds", idx),
                    })?;
            state.set_reg_at(base, dst, val);
        }

        Instruction::Move { dst, src } => {
            let val = state.reg_at(base, src);
            state.set_reg_at(base, dst, val);
        }

        Instruction::LoadClosure { dst, slot } => {
            let closure_idx =
                state
                    .frames
                    .last()
                    .and_then(|f| f.closure)
                    .ok_or_else(|| VmError::Runtime {
                        message: "LoadClosure in non-closure frame".into(),
                    })?;
            let val = state
                .heap
                .borrow()
                .get_vm_closure_free_var(closure_idx, slot as usize)
                .ok_or_else(|| VmError::Runtime {
                    message: format!("closure slot {} out of range", slot),
                })?;
            state.set_reg_at(base, dst, val);
        }

        Instruction::StoreClosure { slot, src } => {
            let val = state.reg_at(base, src);
            let closure_idx =
                state
                    .frames
                    .last()
                    .and_then(|f| f.closure)
                    .ok_or_else(|| VmError::Runtime {
                        message: "StoreClosure in non-closure frame".into(),
                    })?;
            let ok =
                state
                    .heap
                    .borrow_mut()
                    .set_vm_closure_free_var(closure_idx, slot as usize, val);
            if !ok {
                return Err(VmError::Runtime {
                    message: format!("closure slot {} out of range", slot),
                });
            }
        }

        Instruction::LoadGlobal { dst, ref name } => {
            // Per-site inline cache (Track P P4): see `GlobalCacheEntry`.
            let globals = frame_globals(state);
            let val = match GlobalCacheEntry::probe(&code.global_cache[pc], &globals, name) {
                Some(slot) => globals.slot_value(slot),
                // Not a local slot. Fall back to the full lookup rather than
                // straight to the parent: `get` also follows macro-expansion
                // aliases recorded on this environment, which is how a template
                // reaches a binding private to the library that defined it.
                None => globals
                    .get(name)
                    .ok_or_else(|| VmError::UnboundVariable { name: name.clone() })?,
            };
            state.set_reg_at(base, dst, val);
        }

        Instruction::StoreGlobal { ref name, src } => {
            let val = state.reg_at(base, src);
            let globals = frame_globals(state);
            match GlobalCacheEntry::probe(&code.global_cache[pc], &globals, name) {
                Some(slot) => {
                    let old = globals.slot_value(slot);
                    mark_if_shadowing_primitive_value(state, old, val);
                    globals.set_slot_value(slot, val);
                }
                // Not a local slot. Use the full `set` rather than going
                // straight to the parent: it also follows macro-expansion
                // aliases, so a template can assign to a binding private to the
                // library that defined it, matching what `get` already does.
                None => {
                    mark_if_shadowing_primitive(state, &globals, name, val);
                    globals
                        .set(name, val)
                        .map_err(|_| VmError::UnboundVariable { name: name.clone() })?;
                }
            }
        }

        Instruction::Define { ref name, src } => {
            let val = state.reg_at(base, src);
            let globals = frame_globals(state);
            mark_if_shadowing_primitive(state, &globals, name, val);
            globals.define(Rc::clone(name), val);
        }

        // ── Closure Creation ────────────────────────────────────────────
        Instruction::MakeClosure {
            dst,
            code_id: child_id,
            ref free_vars,
        } => {
            let captured: Vec<TaggedValue> =
                free_vars.iter().map(|&r| state.reg_at(base, r)).collect();
            let globals = frame_globals(state);
            let closure_val = state
                .heap
                .borrow_mut()
                .alloc_vm_closure(child_id.0, captured, globals);
            state.note_closure_made(child_id);
            state.set_reg_at(base, dst, closure_val);
        }

        // ── Control Flow ────────────────────────────────────────────────
        Instruction::Jump { target } => {
            state.frames.last_mut().unwrap().pc = target;
        }

        Instruction::JumpIf { cond, target } => {
            let val = state.reg_at(base, cond);
            if val != TaggedValue::FALSE {
                state.frames.last_mut().unwrap().pc = target;
            }
        }

        Instruction::JumpUnless { cond, target } => {
            let val = state.reg_at(base, cond);
            if val == TaggedValue::FALSE {
                state.frames.last_mut().unwrap().pc = target;
            }
        }

        Instruction::JumpUnlessShadowed { form, target } => {
            if !state.is_control_shadowed(form) {
                state.frames.last_mut().unwrap().pc = target;
            }
        }

        // ── Function Calls ──────────────────────────────────────────────
        Instruction::Call {
            func,
            ref args,
            dst,
        } => {
            let func_val = state.reg_at(base, func);
            // Closure fast path: copy args from the caller's window straight
            // into the fresh callee window — no argument buffer at all.
            // Non-closure callees (control primitives, continuations,
            // parameters, deopt'd primitives) take the generic probing path
            // with a plain collected Vec, exactly as before the fast path.
            let closure_id = state.heap.borrow().get_vm_closure_code_id(func_val);
            if let Some(id) = closure_id {
                call_closure_from_regs(state, func_val, CodeObjectId(id), base, args, dst)?;
            } else {
                let arg_vals: Vec<TaggedValue> =
                    args.iter().map(|&r| state.reg_at(base, r)).collect();
                call_value_with_probe(state, func_val, None, &arg_vals, dst)?;
            }
        }

        Instruction::TailCall { func, ref args } => {
            let func_val = state.reg_at(base, func);
            // Closure fast path, tail shape: the frame window is reused in
            // place, so the args are staged through a stack buffer before
            // the overwrite — still no heap allocation.
            let closure_id = state.heap.borrow().get_vm_closure_code_id(func_val);
            if let Some(id) = closure_id {
                const INLINE_ARGS: usize = 16;
                if args.len() <= INLINE_ARGS {
                    let mut buf = [TaggedValue::UNSPECIFIED; INLINE_ARGS];
                    for (i, &r) in args.iter().enumerate() {
                        buf[i] = state.reg_at(base, r);
                    }
                    // Self-tail-call check while the caller's code is still
                    // in hand — same code object, no frame access needed.
                    if CodeObjectId(id) == code.id {
                        self_tail_call(state, base, code.arity, func_val, &buf[..args.len()])?;
                    } else {
                        tail_call_closure_resolved(
                            state,
                            func_val,
                            CodeObjectId(id),
                            &buf[..args.len()],
                        )?;
                    }
                } else {
                    let arg_vals: Vec<TaggedValue> =
                        args.iter().map(|&r| state.reg_at(base, r)).collect();
                    tail_call_closure_resolved(state, func_val, CodeObjectId(id), &arg_vals)?;
                }
            } else {
                let arg_vals: Vec<TaggedValue> =
                    args.iter().map(|&r| state.reg_at(base, r)).collect();
                if let Some(exit_val) =
                    tail_call_value_with_probe(state, func_val, None, &arg_vals, exit_depth)?
                {
                    return Ok(Some(exit_val));
                }
            }
        }

        Instruction::Apply {
            func,
            ref args,
            dst,
        } => {
            let func_val = state.reg_at(base, func);
            let arg_vals = spread_apply_args(state, base, args)?;
            // The same dispatcher `Call` uses, so `apply` accepts every
            // callee a direct call accepts — continuations and VM-intercepted
            // control primitives included.
            call_value(state, func_val, &arg_vals, dst)?;
        }

        Instruction::TailApply { func, ref args } => {
            let func_val = state.reg_at(base, func);
            let arg_vals = spread_apply_args(state, base, args)?;
            // Likewise the dispatcher `TailCall` uses. Shaped like `TailCall`'s
            // arm too, rather than returning outright: the non-escape path has
            // to fall out of the match to reach the post-instruction tracer
            // hook below.
            if let Some(exit_val) = tail_call_value(state, func_val, &arg_vals, exit_depth)? {
                return Ok(Some(exit_val));
            }
        }

        Instruction::Return { val } => {
            let result = state.reg_at(base, val);
            let frame = state.frames.pop().expect("Return with empty stack");
            if state.frames.len() == exit_depth || state.frames.is_empty() {
                // Reached target depth (or absolute bottom) — this loop is
                // done. Handlers installed under it close in the loop itself.
                state.free_top_registers(frame.register_base);
                return Ok(Some(result));
            }
            // Write result into caller's return_reg.
            let return_reg = frame.return_reg;
            state.set_reg(return_reg, result);
            // Free the callee's register window.
            state.free_top_registers(frame.register_base);
            pop_resolved_extents(state, exit_depth);
        }

        // ── call-with-values (instruction-level) ─────────────────────────
        Instruction::CallWithValues {
            dst,
            consumer,
            producer_result,
        } => {
            let consumer_val = state.reg_at(base, consumer);
            let produced_vals = unpack_values(state, state.reg_at(base, producer_result));
            if let Some(result) = call_any(state, consumer_val, &produced_vals, dst)? {
                state.set_reg(dst, result);
            }
        }

        Instruction::TailCallWithValues {
            consumer,
            producer_result,
        } => {
            let consumer_val = state.reg_at(base, consumer);
            let produced_vals = unpack_values(state, state.reg_at(base, producer_result));
            // A tail call of the consumer like any other, so it goes where
            // `TailCall`'s callee does: a primitive consumer runs *before*
            // this frame is popped. Popping first, as this arm did until
            // #420, put the primitive at the very depth a continuation its
            // callback invokes restores to, which is how an escape was told
            // then — `(call/cc (lambda (k) (call-with-values … member)))`
            // with a comparator that called `k` lost the escape and answered
            // `member`'s own result. The continuation's re-entry boundaries
            // tell it now (`VmState::reentry`).
            if let Some(exit_val) =
                tail_call_value(state, consumer_val, &produced_vals, exit_depth)?
            {
                return Ok(Some(exit_val));
            }
        }

        // ── dynamic-wind (instruction-level) ──────────────────────────
        Instruction::PushWind { before, after } => {
            let before_val = state.reg_at(base, before);
            let after_val = state.reg_at(base, after);
            let handlers = captured_handlers(state);
            state
                .dynamic_winds
                .push(DynamicWindRecord::new(before_val, after_val, handlers));
        }

        Instruction::PopWind => {
            // Pop the top wind record. The after-thunk is called by a
            // separate Call instruction emitted after this in the codegen.
            if !state.dynamic_winds.is_empty() {
                state.dynamic_winds.pop();
            }
        }

        Instruction::ResumePrimitive => {
            // A resumable primitive's call has returned into `resume_stub`'s
            // frame; see [`Instruction::ResumePrimitive`].
            super::control::resume_primitive(state, base)?;
        }

        Instruction::ResumeForce => {
            // A promise's thunk has returned into `force_stub`'s frame; see
            // [`Instruction::ResumeForce`].
            super::control::resume_force(state, base)?;
        }

        Instruction::ResumeRaise => {
            // The handler of a raise has returned. Its stub frame says which
            // raise this was and what that still owes; see
            // [`Instruction::ResumeRaise`].
            let handler = state.reg_at(base, raise_step::HANDLER);
            let exception = state.reg_at(base, raise_step::EXCEPTION);
            let continuable = state.reg_at(base, raise_step::CONTINUABLE);

            if continuable.is_truthy() {
                // Read here rather than above: the non-continuable branch
                // never uses it, and decoding it there would report a
                // malformed register on a path that does not read one.
                let below = state
                    .reg_at(base, raise_step::DEPTH_BELOW)
                    .as_fixnum()
                    .ok_or_else(|| VmError::Runtime {
                        message: "raise step: stub register is not a fixnum".into(),
                    })? as usize;
                // R7RS 6.11: the handler is installed again for the rest of
                // the thunk's extent once it has returned from a
                // `raise-continuable`. Its depth is restored from a distance
                // below this frame rather than from an absolute index,
                // because this frame may have been captured and replayed
                // somewhere else — resumed through a composable continuation,
                // or re-entered through the `handler-k` a declining `guard`
                // clause invokes. An absolute index would name a frame in the
                // stack the raise happened on, and after either of those the
                // live stack is a different one.
                let stub_index = state.frames.len().saturating_sub(1);
                state.exception_handlers.push(ExceptionHandler {
                    handler,
                    stack_depth: stub_index.saturating_sub(below),
                });
                // The `Return` after this one delivers the handler's value as
                // the value of `(raise-continuable …)`.
            } else {
                // R7RS 6.11: "If the handler returns, a secondary exception
                // is raised in the same dynamic environment as the handler."
                // That environment is the one standing right here — the entry
                // this raise popped has deliberately not been put back.
                let secondary = state.heap.borrow_mut().alloc_exception(
                    patina_core::ExceptionKind::Error,
                    "exception handler returned from non-continuable exception".to_string(),
                    vec![exception],
                );
                vm_raise_value(state, secondary, raise_step::RESULT, false)?;
            }
        }

        Instruction::ResumeComposableInvoke => {
            // A `before` thunk of a composable invoke's extent re-entry has
            // returned. Its stub frame holds the rest of the invoke; take it
            // and go on.
            let cont = state.reg_at(base, invoke_step::CONT);
            let value = state.reg_at(base, invoke_step::VALUE);
            // Not `unwrap_or(0)`: this window is restored from a snapshot by
            // a re-entering continuation, so "the slot does not hold what I
            // wrote" is the failure this stub has, and a default conflates it
            // with a legitimate zero. `dst = 0` would deliver the resumed
            // computation into the invoke site's r0 — often a `MutableCell` —
            // with no message.
            let malformed = || VmError::Runtime {
                message: "composable invoke: stub register is not a fixnum".into(),
            };
            let dst = state
                .reg_at(base, invoke_step::DST)
                .as_fixnum()
                .ok_or_else(malformed)? as u16;
            let index = state
                .reg_at(base, invoke_step::INDEX)
                .as_fixnum()
                .ok_or_else(malformed)? as usize;
            state
                .frames
                .pop()
                .expect("ResumeComposableInvoke runs in its own frame");
            // Freed, unlike `ResumeWindJump`'s — and copying that rule without
            // its reason is what made this leak. A jump's arrival *replaces*
            // the register file, so an un-freed window costs one travel; a
            // composable invoke *extends* it, so the window is stranded under
            // everything appended after it and is reclaimed only when the
            // enclosing frame returns — which for a tail-recursive loop of
            // invokes is never. Measured: 27 MB against `main`'s 10 at 400k
            // invokes, growing without bound.
            //
            // Safe because the four values are already in Rust locals, and
            // nothing between here and the next write of `cont` — into the
            // next step's window, or nowhere if this was the last — reaches a
            // GC safe point. Those live in `run_loop_until_outcome`'s loop,
            // one dispatch out.
            state.free_top_registers(base);

            let dc =
                state
                    .get_vm_delimited_continuation(cont)
                    .ok_or_else(|| VmError::TypeError {
                        message: "ResumeComposableInvoke: not a delimited continuation".into(),
                    })?;

            // The record is pushed only now, after its thunk returned: while
            // the thunk runs its extent is not entered, so a jump out of it
            // does not run the matching after-thunk.
            let record = dc
                .dynamic_winds
                .get(index)
                .ok_or_else(|| VmError::Runtime {
                    message: "composable invoke: entered wind record is gone".into(),
                })?;
            state.dynamic_winds.push(record.clone());

            match dc.dynamic_winds.get(index + 1) {
                Some(next) => push_invoke_step(state, cont, value, dst, index + 1, next.before)?,
                None => finish_delimited_invoke(state, &dc, value, dst)?,
            }
        }

        Instruction::ResumeWindJump => {
            // A wind thunk of a jump has returned. Its stub frame holds the
            // rest of the jump; take it and go on travelling.
            let target = state.reg_at(base, wind_step::TARGET);
            let value = state.reg_at(base, wind_step::VALUE);
            let entering = state.reg_at(base, wind_step::ENTERING);
            state
                .frames
                .pop()
                .expect("ResumeWindJump runs in its own frame");
            // The stub's register window is deliberately **not** freed here.
            // It is the only root for `target` and `value` — and `target` is a
            // `VmContinuationRef` whose payload lives in a *weak* store
            // (`gc_roots.rs`), so an unrooted one at a collecting safe point
            // would have its payload pruned. Freeing it would leave both
            // reachable from Rust locals alone until the next step writes them
            // back. No safe point runs in that window today, which is what
            // makes this cheap insurance rather than a fix; the same reasoning
            // roots `pending_escape`. The window costs `NUM_REGS` per step and
            // is reclaimed wholesale when the travel ends: arrival replaces
            // the register file, and so does any jump that abandons this one.

            // A before-thunk's record is pushed only now, after it returned:
            // while the thunk runs, its extent is not entered, so a jump out
            // of the thunk does not run the matching after-thunk (Track L §6,
            // the row where a declining `guard` re-enters and `after` still
            // appears once).
            if let Some(index) = entering.as_fixnum() {
                let cc = state
                    .get_vm_continuation(target)
                    .ok_or_else(|| VmError::TypeError {
                        message: "continuation jump: not a full continuation".into(),
                    })?;
                let record =
                    cc.dynamic_winds
                        .get(index as usize)
                        .ok_or_else(|| VmError::Runtime {
                            // Unreachable: `index` was in range when the step
                            // was pushed, and a continuation's wind stack is
                            // immutable. Stated as an error rather than an
                            // `if let` with no `else`, because doing nothing
                            // here would leave `step_wind_jump` to pick the
                            // same record again — a silent infinite loop with
                            // side effects instead of a diagnosis.
                            message: "continuation jump: entered wind record is gone".into(),
                        })?;
                state.dynamic_winds.push(record.clone());
            }

            // An abort's travel is a transfer at every step. This loop cleared
            // `pending_transfer` when it resumed into the thunk that has just
            // returned, and the landing the step may now arrive at can sit at
            // exactly the depth a callback's return leaves: parked as a plain
            // escape, `force` read it as its thunk returning (#342). Read
            // before the step, which can replace the machine.
            let abort_landing = state
                .get_vm_continuation(target)
                .is_some_and(|cc| cc.abort_landing);
            step_wind_jump(state, target, value)?;
            // Whether that pushed the next thunk's frames or arrived and
            // replaced the stack, the loop that owns what is now on the stack
            // decides — the same signal every continuation invoke sends.
            return Err(if abort_landing {
                park_transfer(state, value)
            } else {
                park_escape(state, value)
            });
        }

        // ── Continuations ───────────────────────────────────────────────
        Instruction::AbortToPrompt { tag, val, dst } => {
            let tag_val = state.reg_at(base, tag);
            let abort_val = state.reg_at(base, val);
            let prompt_idx = find_prompt(state, tag_val)?;
            // Never returns: the travel either pushed a thunk's frames or
            // landed, and either way the loop that owns what is on the stack
            // decides, as for every other continuation invoke.
            return Err(abort_to_prompt(state, prompt_idx, abort_val, dst));
        }

        Instruction::CaptureComposable { dst, tag } => {
            let tag_val = state.reg_at(base, tag);
            let prompt_idx = find_prompt(state, tag_val)?;
            // `dst` is both where the continuation object goes and the hole
            // it delivers into: invoking it makes *this* call return again,
            // with the delivered value in place of the continuation.
            let cont = capture_delimited(state, prompt_idx, dst);
            let cont_tv = state.alloc_vm_delimited_continuation(cont);
            state.set_reg_at(base, dst, cont_tv);
        }

        Instruction::InvokeContinuation {
            cont,
            val,
            composable,
        } => {
            let cont_tv = state.reg_at(base, cont);
            let deliver_val = state.reg_at(base, val);

            if composable {
                // Composable (delimited): append the captured frames. As with
                // the arm below, no pass emits this — the live composable
                // invokes are `call_value`, `tail_call_value` and `call_any` —
                // and having no `dst` operand to name a destination, it can
                // only be the tail form. It shares that path, so no second
                // copy of the append can drift.
                let dc = state
                    .get_vm_delimited_continuation(cont_tv)
                    .ok_or_else(|| VmError::TypeError {
                        message: "InvokeContinuation: not a delimited continuation".into(),
                    })?;
                return tail_invoke_delimited(state, cont_tv, dc, deliver_val, exit_depth);
            } else {
                // Non-composable (call/cc): travel the winds and replace the
                // stack. No pass emits this instruction — the live invoke
                // paths are `call_value` and `call_any` — but it goes through
                // the same travel so no second copy of the jump can drift.
                // `step_wind_jump` makes the not-a-continuation check itself;
                // repeating it here would only buy a second wording of the
                // same error that no test can reach.
                step_wind_jump(state, cont_tv, deliver_val)?;
                return Err(park_escape(state, deliver_val));
            }
        }

        // ── Primitives ──────────────────────────────────────────────────
        Instruction::CallPrimitive {
            func_id,
            ref name,
            ref args,
            dst,
        } => {
            // Gather args into the reusable scratch buffer (taken out of the
            // state so a re-entrant primitive cannot alias it).
            let mut arg_vals = std::mem::take(&mut state.scratch_args);
            arg_vals.clear();
            arg_vals.extend(args.iter().map(|&r| state.reg_at(base, r)));
            let result =
                exec_call_primitive(state, base, func_id, name, &arg_vals, dst, exit_depth);
            state.scratch_args = arg_vals;
            if let Some(escaped) = result? {
                return Ok(Some(escaped));
            }
        }

        Instruction::CallPrimitiveDirect {
            func_id,
            ref args,
            dst,
        } => {
            let mut arg_vals = std::mem::take(&mut state.scratch_args);
            arg_vals.clear();
            arg_vals.extend(args.iter().map(|&r| state.reg_at(base, r)));
            let result = exec_call_primitive_direct(state, base, func_id, &arg_vals, dst);
            state.scratch_args = arg_vals;
            result?;
        }

        // ── Inline primitive opcodes (Track P P3) ───────────────────────
        //
        // Contract shared by every arm (owned by `inline_primitive!`): the
        // fast path fires only when the primitive is unshadowed AND the
        // operands fit the trivial case; every other case funnels through
        // exec_call_primitive, the same registry handler the CallPrimitive
        // path uses — identical results, promotion behavior, and error
        // messages by construction.
        Instruction::Add {
            a,
            b,
            dst,
            func_id,
            ref name,
        } => {
            let (x, y) = (state.reg_at(base, a), state.reg_at(base, b));
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x, y], {
                // Overflow returns None from fixnum_add; the handler promotes.
                (x.is_fixnum() && y.is_fixnum())
                    .then(|| x.fixnum_add(y))
                    .flatten()
            });
        }

        Instruction::Sub {
            a,
            b,
            dst,
            func_id,
            ref name,
        } => {
            let (x, y) = (state.reg_at(base, a), state.reg_at(base, b));
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x, y], {
                (x.is_fixnum() && y.is_fixnum())
                    .then(|| x.fixnum_sub(y))
                    .flatten()
            });
        }

        Instruction::Mul {
            a,
            b,
            dst,
            func_id,
            ref name,
        } => {
            let (x, y) = (state.reg_at(base, a), state.reg_at(base, b));
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x, y], {
                (x.is_fixnum() && y.is_fixnum())
                    .then(|| x.fixnum_mul(y))
                    .flatten()
            });
        }

        Instruction::Lt {
            a,
            b,
            dst,
            func_id,
            ref name,
        } => {
            let (x, y) = (state.reg_at(base, a), state.reg_at(base, b));
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x, y], {
                (x.is_fixnum() && y.is_fixnum()).then(|| TaggedValue::boolean(x.fixnum_lt(y)))
            });
        }

        Instruction::NumEq {
            a,
            b,
            dst,
            func_id,
            ref name,
        } => {
            let (x, y) = (state.reg_at(base, a), state.reg_at(base, b));
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x, y], {
                (x.is_fixnum() && y.is_fixnum()).then(|| TaggedValue::boolean(x.fixnum_eq(y)))
            });
        }

        Instruction::Eq {
            a,
            b,
            dst,
            func_id,
            ref name,
        } => {
            let (x, y) = (state.reg_at(base, a), state.reg_at(base, b));
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x, y], {
                Some(TaggedValue::boolean(state.heap.borrow().values_eq(x, y)))
            });
        }

        Instruction::Cons {
            a,
            b,
            dst,
            func_id,
            ref name,
        } => {
            let (x, y) = (state.reg_at(base, a), state.reg_at(base, b));
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x, y], {
                Some(state.heap.borrow_mut().alloc_pair(x, y))
            });
        }

        Instruction::Car {
            src,
            dst,
            func_id,
            ref name,
        } => {
            let x = state.reg_at(base, src);
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x], {
                // Native pairs only; boxed pairs and type errors go to the handler.
                x.is_pair().then(|| state.heap.borrow().car(x))
            });
        }

        Instruction::Cdr {
            src,
            dst,
            func_id,
            ref name,
        } => {
            let x = state.reg_at(base, src);
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x], {
                x.is_pair().then(|| state.heap.borrow().cdr(x))
            });
        }

        Instruction::Not {
            src,
            dst,
            func_id,
            ref name,
        } => {
            let x = state.reg_at(base, src);
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x], {
                Some(TaggedValue::boolean(!x.is_truthy()))
            });
        }

        Instruction::TestJumpUnless {
            test,
            a,
            b,
            dst,
            target,
            func_id,
            ref name,
        } => {
            // Emission contract (see the If arm in pass 5): the plain
            // `JumpUnless dst` must sit at the next pc — it is the deopt
            // landing, the slow path's branch, and the reason the false
            // fast path may skip to pc+2.
            debug_assert!(matches!(
                code.instructions.get(pc + 1),
                Some(Instruction::JumpUnless { cond, .. }) if *cond == dst
            ));
            let x = state.reg_at(base, a);
            // `None` = this test can't answer here (non-fixnum comparison);
            // fall through to the shared slow path below, exactly like the
            // unfused opcodes' `inline_primitive!` fallback.
            // `not` gets a compare-and-branch ahead of the jump table the
            // rest compiles to, because it is the predicate that sits in
            // tight recursive loops — leaving it behind the table measured
            // +0.9% on tak. Hoisting more predicates by *emission* count
            // (`null?` leads at 150 sites, `pair?` 71) measured worse on
            // tak and bought nothing on a `null?`-driven loop: dynamic
            // position beats static frequency here, so the chain stays at
            // one.
            let verdict = if state.is_primitive_shadowed(func_id.0 as usize) {
                None
            } else if test == TestOp::Not {
                Some(!x.is_truthy())
            } else {
                match test {
                    TestOp::NullP => Some(x.is_null()),
                    TestOp::PairP => Some(x.is_pair()),
                    TestOp::VectorP => Some(x.is_vector()),
                    TestOp::Eq => {
                        let y = state.reg_at(base, b);
                        Some(state.heap.borrow().values_eq(x, y))
                    }
                    TestOp::Lt => {
                        let y = state.reg_at(base, b);
                        (x.is_fixnum() && y.is_fixnum()).then(|| x.fixnum_lt(y))
                    }
                    TestOp::NumEq => {
                        let y = state.reg_at(base, b);
                        (x.is_fixnum() && y.is_fixnum()).then(|| x.fixnum_eq(y))
                    }
                    // Unreachable: hoisted above. Spelling the expression
                    // out here instead measured +1.7% on tak — it keeps a
                    // live switch case that LLVM otherwise prunes.
                    TestOp::Not => unreachable!("hoisted above"),
                }
            };
            match verdict {
                Some(truthy) => {
                    // The fused pair in one dispatch: write dst (jumps into
                    // this pc rely on it), then branch — to the else target
                    // when the test is false, over the kept `JumpUnless`
                    // otherwise.
                    state.set_reg_at(base, dst, TaggedValue::boolean(truthy));
                    let f = state.frames.last_mut().expect("empty frame stack");
                    f.pc = if truthy { pc + 2 } else { target };
                }
                None => {
                    // Slow path — rebound predicate, or operands the fast
                    // path can't judge. The registry handler writes dst
                    // (identical result and error message to the unfused
                    // opcode), then control falls through to the kept
                    // `JumpUnless dst`, which branches.
                    let operands = [x, state.reg_at(base, b)];
                    let args = &operands[..test.arity()];
                    if let Some(escaped) =
                        exec_call_primitive(state, base, func_id, name, args, dst, exit_depth)?
                    {
                        return Ok(Some(escaped));
                    }
                }
            }
        }

        Instruction::AddImm {
            a,
            imm,
            dst,
            func_id,
            ref name,
        } => {
            // The compiler only absorbs fixnum literals (`primitive_operands`),
            // so `imm.is_fixnum()` holds by construction — asserted, not
            // re-checked on the hot path. Same for the other *Imm arms.
            debug_assert!(imm.is_fixnum());
            let x = state.reg_at(base, a);
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x, imm], {
                // Overflow returns None from fixnum_add; the handler promotes.
                x.is_fixnum().then(|| x.fixnum_add(imm)).flatten()
            });
        }

        Instruction::SubImm {
            a,
            imm,
            dst,
            func_id,
            ref name,
        } => {
            debug_assert!(imm.is_fixnum());
            let x = state.reg_at(base, a);
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x, imm], {
                x.is_fixnum().then(|| x.fixnum_sub(imm)).flatten()
            });
        }

        Instruction::LtImm {
            a,
            imm,
            dst,
            func_id,
            ref name,
        } => {
            let x = state.reg_at(base, a);
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x, imm], {
                (x.is_fixnum() && imm.is_fixnum()).then(|| TaggedValue::boolean(x.fixnum_lt(imm)))
            });
        }

        Instruction::NumEqImm {
            a,
            imm,
            dst,
            func_id,
            ref name,
        } => {
            let x = state.reg_at(base, a);
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x, imm], {
                (x.is_fixnum() && imm.is_fixnum()).then(|| TaggedValue::boolean(x.fixnum_eq(imm)))
            });
        }

        Instruction::NullP {
            src,
            dst,
            func_id,
            ref name,
        } => {
            let x = state.reg_at(base, src);
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x], {
                Some(TaggedValue::boolean(x.is_null()))
            });
        }

        Instruction::PairP {
            src,
            dst,
            func_id,
            ref name,
        } => {
            let x = state.reg_at(base, src);
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x], {
                Some(TaggedValue::boolean(x.is_pair()))
            });
        }

        Instruction::VectorP {
            src,
            dst,
            func_id,
            ref name,
        } => {
            let x = state.reg_at(base, src);
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [x], {
                Some(TaggedValue::boolean(x.is_vector()))
            });
        }

        Instruction::VectorRef {
            v,
            i,
            dst,
            func_id,
            ref name,
        } => {
            let (vec, idx) = (state.reg_at(base, v), state.reg_at(base, i));
            inline_primitive!(state, base, exit_depth, func_id, name, dst, [vec, idx], {
                // Out-of-bounds (`get` misses, including negative indices)
                // falls back so the handler's error message is used.
                (vec.is_vector() && idx.is_fixnum())
                    .then(|| {
                        let i = usize::try_from(idx.as_fixnum_unchecked()).ok()?;
                        state.heap.borrow().vector_slice(vec).get(i).copied()
                    })
                    .flatten()
            });
        }

        Instruction::VectorSet {
            v,
            i,
            val,
            dst,
            func_id,
            ref name,
        } => {
            let (vec, idx, x) = (
                state.reg_at(base, v),
                state.reg_at(base, i),
                state.reg_at(base, val),
            );
            inline_primitive!(
                state,
                base,
                exit_depth,
                func_id,
                name,
                dst,
                [vec, idx, x],
                {
                    // Out-of-bounds falls back so the handler's error message is
                    // used. Writes `dst ← unspecified`, matching the handler.
                    (vec.is_vector() && idx.is_fixnum())
                        .then(|| {
                            let i = usize::try_from(idx.as_fixnum_unchecked()).ok()?;
                            let mut heap = state.heap.borrow_mut();
                            let slot = heap.vector_slice_mut(vec).get_mut(i)?;
                            *slot = x;
                            Some(TaggedValue::UNSPECIFIED)
                        })
                        .flatten()
                }
            );
        }

        Instruction::AllocCell { dst, src } => {
            let val = state.reg_at(base, src);
            let cell = state.heap.borrow_mut().alloc_mutable_cell(val);
            state.set_reg_at(base, dst, cell);
        }

        Instruction::ReadCell { dst, cell } => {
            let cell_tv = state.reg_at(base, cell);
            let val = state
                .heap
                .borrow()
                .read_mutable_cell(cell_tv)
                .ok_or_else(|| VmError::Runtime {
                    message: "ReadCell: not a MutableCell".into(),
                })?;
            state.set_reg_at(base, dst, val);
        }

        Instruction::WriteCell { cell, src } => {
            let cell_tv = state.reg_at(base, cell);
            let val = state.reg_at(base, src);
            let ok = state.heap.borrow().write_mutable_cell(cell_tv, val);
            if !ok {
                return Err(VmError::Runtime {
                    message: "WriteCell: not a MutableCell".into(),
                });
            }
        }

        Instruction::Nop => {}
    }

    // ── Trace: after instruction ─────────────────────────────────────
    if let Some(tracer) = state.tracer.clone() {
        tracer
            .borrow_mut()
            .post_instruction(&state.registers, state.frames.last(), &state.heap);
    }

    Ok(None)
}

/// A rebind that overwrites a primitive binding with a *different* value
/// must deoptimize every `CallPrimitive` site compiled against it, or
/// already-compiled callers would keep calling the old primitive.
///
/// Every Rust-side writer that can overwrite a global binding must call this
/// *before* replacing it: the `Define`/`StoreGlobal` handlers. The import
/// machinery in both this file and `backend.rs` does the same job through
/// `import_export`/`import_staged` (PRD P8.1), which compare the value before
/// with the value after, since what an import installs is a binding rather
/// than a value they hold. Rebinding a name to the value it already has is a
/// no-op and does not deoptimize, so re-importing a library never pays for
/// this.
pub(crate) fn mark_if_shadowing_primitive(
    state: &mut VmState,
    globals: &Rc<Environment>,
    name: &str,
    new_val: TaggedValue,
) {
    let Some(old) = globals.get(name) else { return };
    mark_if_shadowing_primitive_value(state, old, new_val);
}

/// Value-taking core of [`mark_if_shadowing_primitive`], for callers that
/// already hold the binding's current value (the cached `StoreGlobal` path)
/// and need no name lookup.
pub(crate) fn mark_if_shadowing_primitive_value(
    state: &mut VmState,
    old: TaggedValue,
    new_val: TaggedValue,
) {
    if old == new_val {
        return;
    }
    let proc = state.heap.borrow().get_procedure(old);
    let Some(proc) = proc else { return };
    let Procedure::Primitive {
        qualified_name,
        registry_index,
        ..
    } = proc.as_ref()
    else {
        return;
    };
    if let Some(form) = super::control::control_form(qualified_name) {
        state.mark_shadowed_control(form);
    }
    let index = state
        .primitive_registry
        .resolve_index_cached(qualified_name, registry_index);
    if let Some(index) = index {
        state.mark_shadowed_primitive(index);
    }
}

/// Import-path install: make `name` in `env` the library's binding of
/// `export`, marking the shadow bit when that rebinds a primitive (see
/// `mark_if_shadowing_primitive`). Both import-set resolvers (this file's and
/// `backend.rs`'s) funnel every binding they install through here or through
/// [`import_staged`]. Installs into fresh staging/library environments find
/// no existing binding and mark nothing; over-marking is possible only when a
/// library env genuinely rebinds a primitive name, which costs a deopt, never
/// a wrong result.
///
/// What gets installed is `Library::import_into`'s business — the binding
/// itself, not the value it held (#406) — so the value compared against is
/// read back afterwards rather than assumed.
pub(crate) fn import_export(
    state: &mut VmState,
    env: &Rc<Environment>,
    name: String,
    library: &Library,
    export: &str,
) {
    let old = env.get(&name);
    library.import_into(env, name.as_str(), export);
    mark_if_import_rebound(state, env, &name, old);
}

/// [`import_export`] for a binding being brought out of the staging
/// environment an `only`/`except`/`prefix`/`rename` set was resolved into.
pub(crate) fn import_staged(
    state: &mut VmState,
    env: &Rc<Environment>,
    name: String,
    staging: &Rc<Environment>,
    staged_name: &str,
) {
    let old = env.get(&name);
    env.copy_binding(name.as_str(), staging, staged_name);
    mark_if_import_rebound(state, env, &name, old);
}

fn mark_if_import_rebound(
    state: &mut VmState,
    env: &Rc<Environment>,
    name: &str,
    old: Option<TaggedValue>,
) {
    // Nothing was bound before — every install into a fresh staging or
    // library environment — so there is nothing to have shadowed, and no
    // reason to look the name up a second time.
    let Some(old) = old else { return };
    if let Some(new) = env.get(name) {
        mark_if_shadowing_primitive_value(state, old, new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Code that returns register 0, compiled by no one.
    fn code() -> CodeObject {
        let instructions = vec![Instruction::Return { val: 0 }];
        CodeObject {
            id: CodeObjectId::label(),
            name: None,
            global_cache: GlobalCacheEntry::table(&instructions),
            instructions,
            constants: Vec::new(),
            num_regs: 1,
            arity: Arity::Fixed(0),
            source_map: Vec::new(),
            live_closures: Cell::new(0),
        }
    }

    /// A slot that has used every generation stays empty, and the next code
    /// loaded takes a new slot, so no id is ever given out twice (#352).
    #[test]
    fn a_slot_whose_generations_have_run_out_is_not_given_out_again() {
        let mut state = VmState::new(Rc::new(Environment::new()));
        let first = state.load_unit(code(), Vec::new());
        state.release_unit_if_unused(first);
        assert_eq!(state.free_code_ids, vec![CodeObjectId::new(0, 1)]);

        // As if slot 0 had been given out every generation but its last.
        state.free_code_ids = vec![CodeObjectId::new(0, u32::MAX - 1)];
        let last = state.load_unit(code(), Vec::new());
        assert_eq!(last, CodeObjectId::new(0, u32::MAX - 1));
        state.release_unit_if_unused(last);
        assert!(
            state.free_code_ids.is_empty(),
            "slot 0 went back on the free list with {:?}",
            state.free_code_ids
        );

        let next = state.load_unit(code(), Vec::new());
        assert_eq!(next, CodeObjectId::new(1, 0));
        assert!(state.code_object(last).is_err());
    }
}
