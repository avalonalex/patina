//! `VmState` — the complete mutable state of the VM during execution.
//!
//! See VM_RUNTIME.md §core-data-structures.

use crate::error::VmError;
use crate::types::code_object::{Arity, CodeObject, GlobalCacheEntry};
use crate::types::continuation::{
    DynamicWindRecord, ExceptionHandler, PromptFrame, VmContinuation, VmDelimitedContinuation,
};
use crate::types::instruction::{Instruction, TestOp};
use crate::types::{CallFrame, CodeObjectId};
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
    /// parked between [`across_reentry`] and the dispatch loop that resumes
    /// with it. A field rather than a return value because the
    /// `ApplyContext` methods return `Result<T, EvalError>` and cannot say
    /// "escaped". Mirrors the tree-walker's `set_pending_escape` /
    /// `take_pending_escape` pair (`cps_eval/types.rs`) — same split, same
    /// reason, and rooted for the same reason (`gc_roots.rs`).
    pub(crate) pending_escape: Option<TaggedValue>,
    /// Stack of active continuation prompts (SRFI-226).
    pub prompt_stack: Vec<PromptFrame>,
    /// Stack of active `dynamic-wind` records.
    pub dynamic_winds: Vec<DynamicWindRecord>,
    /// Stack of installed exception handlers (`with-exception-handler`).
    pub exception_handlers: Vec<ExceptionHandler>,
    /// All compiled `CodeObject`s, indexed densely by `CodeObjectId` (ids
    /// are process-wide sequential — see `CodeObjectId::fresh`). Slots for
    /// ids loaded into other `VmState`s stay `None`.
    pub(crate) code_store: Vec<Option<Rc<CodeObject>>>,
    /// Id of the one-instruction stub each step of a continuation jump runs
    /// in ([`wind_jump_stub`]). Built on the first jump that has a wind thunk
    /// to run; most states never build one. The id, not the `Rc` — the code
    /// object has exactly one owner, `code_store`, and this is a note of
    /// where to find it.
    pub(crate) wind_jump_code: Option<CodeObjectId>,
    /// Id of the six-instruction stub the *value* form of `dynamic-wind` runs
    /// in ([`value_wind_stub`]). Built on the first such call; a program that
    /// only ever calls `dynamic-wind` in head position never builds one.
    pub(crate) value_wind_code: Option<CodeObjectId>,
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
        Self {
            registers: Vec::new(),
            frames: Vec::new(),
            pending_escape: None,
            prompt_stack: Vec::new(),
            dynamic_winds: Vec::new(),
            exception_handlers: Vec::new(),
            code_store: Vec::new(),
            wind_jump_code: None,
            value_wind_code: None,
            globals,
            heap,
            primitive_registry: Rc::new(registry),
            shadowed_primitives: Vec::new(),
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

    /// Load a `CodeObject` (and nested ones) into the code store.
    pub fn load(&mut self, code: CodeObject) {
        let idx = code.id.index();
        if idx >= self.code_store.len() {
            self.code_store.resize(idx + 1, None);
        }
        self.code_store[idx] = Some(Rc::new(code));
    }

    pub fn load_all(&mut self, codes: impl IntoIterator<Item = CodeObject>) {
        for c in codes {
            self.load(c);
        }
    }

    /// Fetch a loaded `CodeObject` by id.
    #[inline(always)]
    fn code_object(&self, id: CodeObjectId) -> Result<Rc<CodeObject>, VmError> {
        self.code_store
            .get(id.index())
            .and_then(Option::clone)
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
    fn reg_at(&self, base: usize, reg: u16) -> TaggedValue {
        debug_assert_eq!(base, self.frame_base());
        self.registers[base + reg as usize]
    }

    #[inline(always)]
    fn set_reg_at(&mut self, base: usize, reg: u16, val: TaggedValue) {
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
use crate::types::instruction::PrimitiveFnId;
use patina_core::core_expr::Symbol;
use patina_frontend::Desugarer;
use patina_runtime::Library;
use patina_runtime::library_loader::{ImportSet, build_library};
use patina_runtime::library_registry::LibraryError;

/// Load a Scheme library by name using the shared registries on `VmState`.
///
/// This mirrors `VmBackend::load_library()` but operates through the
/// `Rc<RefCell<...>>` registries stored on `VmState`, so it can be called
/// from `VmApplyContext` during eval primitive execution.
fn vm_load_library(state: &mut VmState, name: &[String]) -> Result<Library, LibraryError> {
    let library_registry = state
        .library_registry
        .as_ref()
        .ok_or_else(|| LibraryError::NotFound(name.to_vec()))?
        .clone();
    let loader_registry = state
        .loader_registry
        .as_ref()
        .ok_or_else(|| LibraryError::NotFound(name.to_vec()))?
        .clone();

    // Check if already loaded
    {
        let registry = library_registry.borrow();
        if let Some(lib) = registry.get(name) {
            return Ok(lib.clone());
        }
    }

    // Circular dependency detection
    {
        let mut registry = library_registry.borrow_mut();
        registry.begin_loading(name)?;
    }

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
        let search_paths_for_checker = search_paths.clone();
        let loader_reg_clone = loader_registry.clone();
        let can_load_library = |lib_name: &[String]| {
            let loaders = loader_reg_clone.borrow();
            loaders.can_load_with_paths(lib_name, &search_paths_for_checker)
        };

        let parsed = {
            let loaders = loader_registry.borrow();
            loaders.try_parse_with_heap_and_library_checker(
                name,
                &search_paths,
                heap.clone(),
                &can_load_library,
            )?
        };

        if let Some(parsed) = parsed {
            vm_evaluate_parsed_library(state, parsed)?
        } else {
            library_registry.borrow_mut().end_loading(name);
            return Err(LibraryError::NotFound(name.to_vec()));
        }
    };

    // End loading tracking
    library_registry.borrow_mut().end_loading(name);

    // Register the library
    let _ = library_registry.borrow_mut().register(lib);

    // Return from registry
    library_registry
        .borrow()
        .get(name)
        .cloned()
        .ok_or_else(|| LibraryError::NotFound(name.to_vec()))
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

            let top_id = top.id;
            state.load(top);
            state.load_all(nested);

            execute_nested(state, top_id).map_err(|e| LibraryError::ParseError {
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
            for (name, value) in imported_lib.exports_iter_tagged() {
                import_define(state, lib_env, name.clone(), value);
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
                match temp_env.get(id) {
                    Some(value) => import_define(state, lib_env, id.clone(), value),
                    None => {
                        return Err(LibraryError::parse(
                            None,
                            format!("Identifier '{}' not found in import set", id),
                        ));
                    }
                }
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
            for (name, value) in temp_env.bindings() {
                if !exclude.contains(&name) {
                    import_define(state, lib_env, name, value);
                }
            }
            Ok(())
        }
        ImportSet::Prefix { import_set, prefix } => {
            let temp_env = Rc::new(Environment::with_heap(state.globals.heap().clone()));
            vm_process_import_set(state, import_set, &temp_env)?;
            for (name, value) in temp_env.bindings() {
                import_define(state, lib_env, format!("{}{}", prefix, name), value);
            }
            Ok(())
        }
        ImportSet::Rename {
            import_set,
            renames,
        } => {
            let temp_env = Rc::new(Environment::with_heap(state.globals.heap().clone()));
            vm_process_import_set(state, import_set, &temp_env)?;
            let rename_map: std::collections::HashMap<_, _> = renames
                .iter()
                .map(|(o, n)| (o.clone(), n.clone()))
                .collect();
            for (name, value) in temp_env.bindings() {
                let exported_name = rename_map.get(&name).cloned().unwrap_or(name);
                import_define(state, lib_env, exported_name, value);
            }
            Ok(())
        }
    }
}

/// Evaluate a datum expression in the given environment using the VM.
///
/// Used by the `eval` primitive. Swaps `state.globals` to the given
/// environment, executes in the main VmState using `execute_nested`
/// (which respects the current frame depth), then restores globals.
fn vm_eval_expr(
    state: &mut VmState,
    expr: TaggedValue,
    env: &Rc<Environment>,
) -> Result<TaggedValue, VmError> {
    let desugarer = Desugarer::with_env(env.clone()).with_fs(state.fs.clone());
    let heap = state.globals.heap().clone();

    let core_expr = desugarer
        .desugar_tagged(expr, &heap)
        .map_err(|e| VmError::Runtime {
            message: format!("eval: desugar error: {}", e),
        })?;

    let (top, nested) =
        compile_with_qq_resolving(&core_expr, &heap, env, &state.primitive_registry).map_err(
            |e| VmError::Runtime {
                message: format!("eval: compile error: {}", e),
            },
        )?;

    // Swap globals to the eval environment, execute in the main state,
    // then restore. This keeps continuations and code objects valid.
    //
    // `saved_globals` is reachable only from this Rust frame while the swap
    // is in effect, so defer for its extent rather than relying on this
    // always being reached from inside a dispatch loop.
    let _gc_defer = GcDeferGuard::new(&state.heap);

    let saved_globals = state.globals.clone();
    state.globals = env.clone();

    let top_id = top.id;
    state.load(top);
    state.load_all(nested);

    let result = execute_nested(state, top_id);

    // Always restore globals, even on error
    state.globals = saved_globals;

    result
}

// ─────────────────────────────────────────────────────────────────────────────
// Execution loop
// ─────────────────────────────────────────────────────────────────────────────

/// Outlined error constructor for a `code_object` miss — `#[cold]` keeps the
/// formatting machinery out of the callers that inline the lookup.
#[cold]
#[inline(never)]
fn missing_code_object(id: CodeObjectId) -> VmError {
    VmError::Runtime {
        message: format!("missing CodeObject {:?}", id),
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
    if result.is_err() {
        // An error that reaches the top level abandons whatever the machine
        // was doing: the frames it was running, the handlers and wind
        // records their extents installed, an escape parked mid-flight. None
        // of it can be resumed, and the next `execute` runs "until the frame
        // stack is empty" — left in place, the abandoned frames would be
        // where that next form *returns to*, and a REPL or resilient-mode
        // script would run the dead frames after it, re-reporting the old
        // error. Winds are dropped, not unwound: their after-thunks were
        // never owed a run by an abort, and one that raised would abort the
        // recovery.
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

/// Call a closure (heap index) with `args`, returning its result.
///
/// Used by the VM internally for calls to Scheme closures from the execution
/// loop and for `CallWithPrompt` body thunks.
fn call_closure(
    state: &mut VmState,
    closure_val: TaggedValue,
    args: &[TaggedValue],
    return_reg: u16,
) -> Result<(), VmError> {
    // Resolve the closure to its code id (free vars stay on the heap).
    let code_id = resolve_closure(state, closure_val)?;
    call_closure_resolved(state, closure_val, code_id, args, return_reg)
}

/// `call_closure` for a callee whose code id is already resolved — call
/// sites that probed the closure type up front pass the id along instead of
/// paying a second heap lookup.
fn call_closure_resolved(
    state: &mut VmState,
    closure_val: TaggedValue,
    code_id: CodeObjectId,
    args: &[TaggedValue],
    return_reg: u16,
) -> Result<(), VmError> {
    let code = state.code_object(code_id)?;

    check_arity(code.arity, args.len())?;

    let base = state.alloc_registers(code.num_regs);
    store_args_in_window(state, base, code.arity, args);

    state.frames.push(CallFrame {
        pc: 0,
        register_base: base,
        num_regs: code.num_regs,
        closure: closure_heap_index(closure_val),
        return_reg,
        code,
    });
    Ok(())
}

/// `call_closure_resolved` sourcing arguments directly from the caller's
/// registers — the `Call` instruction's closure fast path. The callee
/// window is freshly allocated above the caller's, so the copy cannot
/// overlap and no intermediate argument buffer of any kind is needed.
/// (Tail calls can't use this: they reuse the caller's window in place.)
fn call_closure_from_regs(
    state: &mut VmState,
    closure_val: TaggedValue,
    code_id: CodeObjectId,
    caller_base: usize,
    arg_regs: &[u16],
    return_reg: u16,
) -> Result<(), VmError> {
    let code = state.code_object(code_id)?;

    check_arity(code.arity, arg_regs.len())?;

    let base = state.alloc_registers(code.num_regs);
    if let Arity::Variadic(fixed) = &code.arity {
        let fixed = *fixed as usize;
        for (i, &r) in arg_regs.iter().take(fixed).enumerate() {
            state.registers[base + i] = state.registers[caller_base + r as usize];
        }
        // Cons the rest list straight from the caller's registers — no
        // staging Vec. Variadic calls are hot in practice: `map` and every
        // rest-arg stdlib procedure land here per call.
        let regs = &state.registers;
        let rest = state.heap.borrow_mut().list_from_iter(
            arg_regs[fixed..]
                .iter()
                .map(|&r| regs[caller_base + r as usize]),
        );
        state.registers[base + fixed] = rest;
    } else {
        for (i, &r) in arg_regs.iter().enumerate() {
            state.registers[base + i] = state.registers[caller_base + r as usize];
        }
    }

    state.frames.push(CallFrame {
        pc: 0,
        register_base: base,
        num_regs: code.num_regs,
        closure: closure_heap_index(closure_val),
        return_reg,
        code,
    });
    Ok(())
}

/// How a dispatch loop ended.
#[derive(Clone, Copy)]
enum LoopExit {
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
fn run_loop_until(state: &mut VmState, exit_depth: usize) -> Result<TaggedValue, VmError> {
    run_loop_until_outcome(state, exit_depth).map(LoopExit::value)
}

/// The main dispatch loop. Runs until `state.frames.len() == exit_depth`.
///
/// Use `exit_depth = 0` to run until the frame stack is fully empty (top-level).
/// Use `exit_depth = N` to run a nested thunk until it returns to depth N.
///
/// This is the only place that decides whether a continuation invocation is
/// this loop's business: the frames it restored are either ones this loop
/// still owns (resume) or ones further out (exit, reporting `Escaped`). Every
/// synchronous boundary below reports the invocation as
/// [`VmError::ContinuationEscape`] and lets the decision happen here once.
fn run_loop_until_outcome(state: &mut VmState, exit_depth: usize) -> Result<LoopExit, VmError> {
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
    // also where a handler this loop was *started under* sits when the
    // control primitive that started it tail-replaced its thunk's frame —
    // `(with-exception-handler h (lambda () (call-with-values p c)))` runs
    // `p` on a nested loop with `h` at that depth and still owed a raise from
    // `c`. So the exit-depth `Return` pops nothing (see
    // `pop_resolved_extents`) and the loop closes its own on the way out.
    //
    // The example used to be the value form of `dynamic-wind`, whose thunks
    // ran on nested loops until 2026-09-02 (issue #157). `call-with-values`'
    // producer is now the only remaining `run_thunk_outcome` caller with a
    // result to place, and carries the invariant on its own.
    let handlers_at_entry = state.exception_handlers.len();

    loop {
        // GC safe point: all live state is on `VmState`, capture temporaries
        // are dead, buffers are restored, and no heap borrow is outstanding.
        maybe_collect(state, is_outermost);

        match dispatch_one_instruction(state, &mut cur_code, exit_depth) {
            Ok(Some(val)) => {
                state.exception_handlers.truncate(handlers_at_entry);
                return Ok(LoopExit::Returned(val));
            }
            Ok(None) => continue,
            Err(e) => {
                // Checked before the catchability test on purpose: the
                // sentinel crosses the registry boundary as an ordinary
                // `VmError` and would otherwise look catchable and be handed
                // to a `guard`. See `VmState::pending_escape`.
                if let Some(value) = state.pending_escape.take() {
                    if state.frames.len() <= exit_depth {
                        return Ok(LoopExit::Escaped(value));
                    }
                    // Control resumed in a frame this loop still owns.
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
                    // The raise will invoke the handler (push frame or escape via call/cc)
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
/// The innermost frame is not always the one that *has* a source map. The two
/// stubs the runtime builds rather than compiles — [`value_wind_stub`] and
/// [`wind_jump_stub`] — carry none at all, and either can be the top frame
/// when an error is raised: the value form's thunks tail-call out of their own
/// frames, leaving the stub innermost, and a jump's thunks do the same. Read
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
fn frame_globals(state: &VmState) -> Rc<Environment> {
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
        message: format!("PC {} out of bounds in {:?}", pc, code.id),
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
                if let Some(escaped) =
                    call_value_with_probe(state, func_val, None, &arg_vals, dst, exit_depth)?
                {
                    return Ok(Some(escaped));
                }
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
            if let Some(escaped) = call_value(state, func_val, &arg_vals, dst, exit_depth)? {
                return Ok(Some(escaped));
            }
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
            // Pop current frame (tail position), then call consumer.
            let frame = state
                .frames
                .pop()
                .expect("TailCallWithValues with empty stack");
            let return_reg = frame.return_reg;
            state.free_top_registers(frame.register_base);
            if state.frames.len() == exit_depth {
                // At exit depth — call consumer; if it returns immediately,
                // return the result.
                if let Some(result) = call_any(state, consumer_val, &produced_vals, return_reg)? {
                    return Ok(Some(result));
                }
                if state.frames.len() == exit_depth {
                    let result = state.reg(return_reg);
                    return Ok(Some(result));
                }
            } else if let Some(result) = call_any(state, consumer_val, &produced_vals, return_reg)?
            {
                state.set_reg(return_reg, result);
                pop_resolved_extents(state, exit_depth);
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

            step_wind_jump(state, target, value)?;
            // Whether that pushed the next thunk's frames or arrived and
            // replaced the stack, the loop that owns what is now on the stack
            // decides — the same signal every continuation invoke sends.
            return Err(park_escape(state, value));
        }

        // ── Continuations ───────────────────────────────────────────────
        Instruction::CallWithPrompt {
            body,
            tag,
            handler,
            dst,
        } => {
            let tag_val = state.reg_at(base, tag);
            let handler_val = state.reg_at(base, handler);
            let body_val = state.reg_at(base, body);
            state.prompt_stack.push(PromptFrame {
                tag: tag_val,
                stack_depth: state.frames.len(),
                dynamic_wind_depth: state.dynamic_winds.len(),
                handler: handler_val,
                dst,
            });
            call_closure(state, body_val, &[], dst)?;
        }

        Instruction::AbortToPrompt { tag, val } => {
            let tag_val = state.reg_at(base, tag);
            let abort_val = state.reg_at(base, val);

            let prompt_idx = state
                .prompt_stack
                .iter()
                .rposition(|p| p.tag == tag_val)
                .ok_or(VmError::NoMatchingPrompt)?;
            let prompt = state.prompt_stack[prompt_idx].clone();

            // Snapshot the captured frame slice + register windows.
            let cap_frames = state.frames[prompt.stack_depth..].to_vec();
            let cap_winds = state.dynamic_winds[prompt.dynamic_wind_depth..].to_vec();
            let reg_base = if !cap_frames.is_empty() {
                cap_frames[0].register_base
            } else {
                state.registers.len()
            };
            let cap_regs = state.registers[reg_base..].to_vec();
            let cont = VmDelimitedContinuation {
                frames: cap_frames,
                dynamic_winds: cap_winds,
                registers: cap_regs,
                base_at_capture: reg_base,
            };
            let cont_tv = state.alloc_vm_delimited_continuation(cont);

            // Run dynamic-wind exit thunks for the unwound portion, popping
            // each before it runs (see `vm_raise_value`).
            while state.dynamic_winds.len() > prompt.dynamic_wind_depth {
                let after = state
                    .dynamic_winds
                    .pop()
                    .expect("loop condition guarantees a record")
                    .after;
                run_thunk(state, after)?;
            }

            // Unwind the call stack.
            state.frames.truncate(prompt.stack_depth);
            state.dynamic_winds.truncate(prompt.dynamic_wind_depth);
            state.prompt_stack.truncate(prompt_idx);
            // Reclaim register space freed by the unwind.
            if let Some(top) = state.frames.last() {
                state
                    .registers
                    .truncate(top.register_base + top.num_regs as usize);
            } else {
                state.registers.clear();
            }

            // Call handler(abort_val, captured_cont) — result goes to prompt.dst.
            // Handler may be a primitive or a VM closure.
            if let Some(result) =
                call_any(state, prompt.handler, &[abort_val, cont_tv], prompt.dst)?
            {
                state.set_reg(prompt.dst, result);
            }
        }

        Instruction::CaptureComposable { dst, tag } => {
            let tag_val = state.reg_at(base, tag);
            let prompt_idx = state
                .prompt_stack
                .iter()
                .rposition(|p| p.tag == tag_val)
                .ok_or(VmError::NoMatchingPrompt)?;
            let prompt = &state.prompt_stack[prompt_idx];

            let cap_frames = state.frames[prompt.stack_depth..].to_vec();
            let cap_winds = state.dynamic_winds[prompt.dynamic_wind_depth..].to_vec();
            let reg_base = if !cap_frames.is_empty() {
                cap_frames[0].register_base
            } else {
                state.registers.len()
            };
            let cap_regs = state.registers[reg_base..].to_vec();
            let cont = VmDelimitedContinuation {
                frames: cap_frames,
                dynamic_winds: cap_winds,
                registers: cap_regs,
                base_at_capture: reg_base,
            };
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
                // Composable (delimited): append captured frames to current stack.
                let dc = state
                    .get_vm_delimited_continuation(cont_tv)
                    .ok_or_else(|| VmError::TypeError {
                        message: "InvokeContinuation: not a delimited continuation".into(),
                    })?;

                // Run wind entry thunks for the newly-appended winds.
                let enter_winds = dc.dynamic_winds.clone();
                for record in &enter_winds {
                    run_thunk(state, record.before)?;
                }

                // Relocate captured frames onto the end of the register array.
                let new_base = state.registers.len();
                let old_base = dc.base_at_capture;
                let shift = new_base.wrapping_sub(old_base);
                state.registers.extend_from_slice(&dc.registers);
                let mut new_frames: Vec<CallFrame> = dc.frames.clone();
                for f in &mut new_frames {
                    f.register_base = f.register_base.wrapping_add(shift);
                }
                state.frames.extend(new_frames);
                state.dynamic_winds.extend(enter_winds);

                // Deliver the value into the topmost appended frame's return slot.
                // The continuation expects the value where the sub-call result would land.
                if let Some(top) = state.frames.last() {
                    let ret_reg = top.return_reg;
                    // Write into the *caller* of the top frame (below the top).
                    let n = state.frames.len();
                    if n >= 2 {
                        let caller_base = state.frames[n - 2].register_base;
                        state.registers[caller_base + ret_reg as usize] = deliver_val;
                    }
                }
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

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn resolve_closure(state: &VmState, val: TaggedValue) -> Result<CodeObjectId, VmError> {
    state
        .heap
        .borrow()
        .get_vm_closure_code_id(val)
        .map(CodeObjectId)
        .ok_or_else(|| VmError::TypeError {
            message: format!("expected a procedure, got {}", val.type_name()),
        })
}

/// Build `apply`'s argument list from a register window: every argument but
/// the last verbatim, then the last spread out of a proper list.
///
/// Shared by `Instruction::Apply`, `Instruction::TailApply` and the
/// `VmControlPrimitive::Apply` handler, which is what keeps the head-position
/// form and the value form accepting the same shapes.
fn spread_apply_args(
    state: &VmState,
    base: usize,
    args: &[u16],
) -> Result<Vec<TaggedValue>, VmError> {
    let (&last, fixed) = args.split_last().expect("apply site has no arguments");
    let spread = spread_apply_tail(state, state.reg_at(base, last))?;
    // `(apply proc lst)` is the dominant shape and has no fixed prefix, so the
    // flattened list *is* the argument vector — returning it avoids allocating
    // a second one and memcpying into it.
    if fixed.is_empty() {
        return Ok(spread);
    }
    let mut arg_vals: Vec<TaggedValue> = Vec::with_capacity(fixed.len() + spread.len());
    arg_vals.extend(fixed.iter().map(|&r| state.reg_at(base, r)));
    arg_vals.extend(spread);
    Ok(arg_vals)
}

/// `apply`'s final argument, flattened. Errors if it is not a proper list.
fn spread_apply_tail(state: &VmState, last: TaggedValue) -> Result<Vec<TaggedValue>, VmError> {
    state
        .heap
        .borrow()
        .list_to_vec(last)
        .ok_or_else(|| VmError::Runtime {
            message: "apply: last argument is not a proper list".into(),
        })
}

/// Call any callable value (VmClosure or Primitive). For VmClosures, pushes a
/// frame and returns `Ok(None)` — the caller must continue in the run loop.
/// For primitives, calls immediately and returns `Ok(Some(result))`.
///
/// **Narrower than [`call_value`]**, and knowingly so for now: it probes
/// primitive → parameter → closure, which is the probe set the `apply`
/// instructions shed when they moved to `call_value`. A VM-intercepted control
/// primitive or a continuation reached through one of this function's callers
/// — `call-with-values`' consumer, a prompt handler, an exception handler —
/// still fails, e.g.
/// `(call-with-values (lambda () (values + '(1 2))) apply)`. Pinned in
/// `patina-tests/tests/callability.rs`; the fix is to give this function
/// `call_value`'s probe set, which needs an `exit_depth` its callers do not all
/// have today.
fn call_any(
    state: &mut VmState,
    func_val: TaggedValue,
    args: &[TaggedValue],
    return_reg: u16,
) -> Result<Option<TaggedValue>, VmError> {
    // Try as primitive first
    if let Some(prim) = primitive_procedure(state, func_val) {
        return Ok(Some(call_primitive_proc(state, &prim, args)?));
    }
    // Try as parameter
    if let Some(result) = try_call_parameter(state, func_val, args) {
        return Ok(Some(result?));
    }
    // Try as VM closure
    // A continuation is callable wherever a procedure is — as an exception
    // handler, a call-with-values consumer, a wind thunk. Invoking it
    // replaces the stack, so the caller's own frame is gone: signal the
    // dispatch loop the same way the instruction-level call paths do.
    if let Some(delivered) = try_invoke_continuation(state, func_val, args)? {
        return Err(park_escape(state, delivered));
    }
    call_closure(state, func_val, args, return_reg)?;
    Ok(None)
}

/// Try to call a parameter object. Returns `Some(Ok(result))` if `func_val`
/// is a parameter, `None` otherwise.
fn try_call_parameter(
    state: &mut VmState,
    func_val: TaggedValue,
    args: &[TaggedValue],
) -> Option<Result<TaggedValue, VmError>> {
    let heap = state.heap.borrow();
    let (values, converter) = heap.get_parameter(func_val)?;
    drop(heap);
    match args.len() {
        0 => {
            // Get current value (top of stack)
            let stack = values.borrow();
            let val = stack.last().copied().unwrap_or(TaggedValue::UNSPECIFIED);
            Some(Ok(val))
        }
        1 => {
            // Set value (replace top of stack, applying converter if present)
            let new_val = if let Some(conv) = converter {
                match call_any_sync(state, conv, &[args[0]]) {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                }
            } else {
                args[0]
            };
            let mut stack = values.borrow_mut();
            if let Some(top) = stack.last_mut() {
                *top = new_val;
            }
            Some(Ok(TaggedValue::UNSPECIFIED))
        }
        _ => Some(Err(VmError::ArityMismatch {
            expected: "0 or 1".into(),
            got: args.len(),
        })),
    }
}

/// Synchronously call a callable value and return its result.
/// Used for parameter converters and similar internal callbacks.
fn call_any_sync(
    state: &mut VmState,
    func_val: TaggedValue,
    args: &[TaggedValue],
) -> Result<TaggedValue, VmError> {
    // Try as primitive first
    if let Some(prim) = primitive_procedure(state, func_val) {
        return call_primitive_proc(state, &prim, args);
    }
    // Must be a VM closure — use run_thunk-style execution.
    // Use a return_reg beyond the caller's window to avoid clobbering live regs.
    let depth_before = state.frames.len();
    let return_reg = state.frames.last().map(|f| f.num_regs).unwrap_or(0);
    if let Some(f) = state.frames.last() {
        let needed = f.register_base + return_reg as usize + 1;
        if state.registers.len() < needed {
            state.registers.resize(needed, TaggedValue::UNSPECIFIED);
        }
    }
    call_closure(state, func_val, args, return_reg)?;
    // run_loop_until returns the value directly from Return instruction dispatch.
    // Routed through the re-entry boundary: this is how a parameter converter
    // runs during `parameterize`, and a continuation can escape out of it.
    match across_reentry(
        state,
        depth_before,
        |s| run_loop_until(s, depth_before),
        |v| *v,
    ) {
        Ok(v) => Ok(v),
        Err(Reentry::Escaped) => Err(VmError::Runtime {
            message: "continuation escaped".into(),
        }),
        Err(Reentry::Failed(e)) => Err(e),
    }
}

fn closure_heap_index(val: TaggedValue) -> Option<patina_core::tagged_value::HeapIndex> {
    // VmClosures are TAG_OBJECT (generic heap objects).
    if val.is_object() {
        Some(val.heap_index())
    } else {
        None
    }
}

/// Outlined error constructor — `#[cold]` keeps the formatting machinery
/// out of the call paths that inline `check_arity`.
#[cold]
#[inline(never)]
fn arity_error(arity: Arity, n: usize) -> VmError {
    VmError::ArityMismatch {
        expected: match arity {
            Arity::Fixed(k) => format!("{}", k),
            Arity::Variadic(k) => format!("at least {}", k),
        },
        got: n,
    }
}

#[inline(always)]
fn check_arity(arity: Arity, n: usize) -> Result<(), VmError> {
    if arity.accepts(n) {
        Ok(())
    } else {
        Err(arity_error(arity, n))
    }
}

/// Fill the register window at `base` with a call's arguments per `arity`:
/// fixed args into `r0..`, a variadic rest collected into a list after them.
/// The caller has already checked arity.
fn store_args_in_window(state: &mut VmState, base: usize, arity: Arity, arg_vals: &[TaggedValue]) {
    if let Arity::Variadic(fixed) = arity {
        let fixed = fixed as usize;
        for (dst, &val) in state.registers[base..base + fixed].iter_mut().zip(arg_vals) {
            *dst = val;
        }
        let rest = state
            .heap
            .borrow_mut()
            .list_from_iter(arg_vals[fixed..].iter().copied());
        state.registers[base + fixed] = rest;
    } else {
        for (dst, &val) in state.registers[base..base + arg_vals.len()]
            .iter_mut()
            .zip(arg_vals)
        {
            *dst = val;
        }
    }
}

/// Run a thunk (0-arg closure) to completion and *require* that it returned.
///
/// The internal boundaries that only ever run bookkeeping thunks — an abort's
/// exit winds and a composable continuation's entry thunks — use this: a
/// continuation invoked inside one of those does not resume here, it unwinds,
/// so the sentinel is simply propagated. `call-with-values`' producer has a
/// result to place and uses [`run_thunk_outcome`] instead, deciding for
/// itself.
///
/// Two families of thunk have left this list, both by the same move — being
/// given an instruction to come back to instead of a Rust frame. A *jump's*
/// wind thunks run as ordinary frames under a `ResumeWindJump` stub
/// (`step_wind_jump`) since 2026-09-02, and the **value form** of
/// `dynamic-wind` runs all three of its thunks as ordinary frames of
/// [`value_wind_stub`] since 2026-09-02 as well. In both cases that is what
/// lets a continuation captured inside one be resumed.
fn run_thunk(state: &mut VmState, thunk: TaggedValue) -> Result<TaggedValue, VmError> {
    match run_thunk_outcome(state, thunk)? {
        ThunkOutcome::Returned(v) => Ok(v),
        ThunkOutcome::Escaped(v) => {
            state.pending_escape = Some(v);
            Err(VmError::ContinuationEscape)
        }
    }
}

/// How a thunk run at a synchronous boundary ended. The same distinction
/// [`LoopExit`] makes, restated for the boundary's benefit.
enum ThunkOutcome {
    Returned(TaggedValue),
    /// A continuation unwound past this boundary. The caller owns no live
    /// frame: it must not write a register, and must hand the unwind on.
    ///
    /// Nor may it run cleanup of its own. The one boundary that tried —
    /// the value form of `dynamic-wind`, deciding whether it still owed its
    /// after-thunk by comparing wind-stack *lengths* — was asking about a
    /// stack the jump had already replaced with the target's, and truncated
    /// the target's records to run its own thunk again (issue #157). Cleanup
    /// that must survive an escape belongs in an instruction, not here.
    Escaped(TaggedValue),
}

/// The values a producer handed to `call-with-values`: the elements of a
/// #<values> object (from `values` with other than one argument, from a
/// primitive such as `exact-integer-sqrt`, or from a continuation invoked
/// with several), or the single value itself.
fn unpack_values(state: &VmState, primary: TaggedValue) -> Vec<TaggedValue> {
    match state.heap.borrow().get_values_as_tagged(primary) {
        Some(vals) => vals,
        None => vec![primary],
    }
}

/// Run a thunk (0-arg closure) to completion, reporting whether it returned.
///
/// Pushes the thunk's frame, runs the execution loop until that frame returns,
/// then returns the result. Reached by `call-with-values`' producer directly,
/// and by everything [`run_thunk`] covers.
fn run_thunk_outcome(state: &mut VmState, thunk: TaggedValue) -> Result<ThunkOutcome, VmError> {
    let depth_before = state.frames.len();

    // Use a return_reg beyond the caller's register window so the thunk's
    // Return instruction doesn't clobber any live value (e.g. MutableCell in r0).
    let return_reg = state.frames.last().map(|f| f.num_regs).unwrap_or(0);
    // Ensure the register array has room for the scratch slot.
    if let Some(f) = state.frames.last() {
        let needed = f.register_base + return_reg as usize + 1;
        if state.registers.len() < needed {
            state.registers.resize(needed, TaggedValue::UNSPECIFIED);
        }
    }

    // If thunk is a primitive (rare but possible), call it directly.
    if let Some(result) = call_any(state, thunk, &[], return_reg)? {
        return Ok(ThunkOutcome::Returned(result));
    }
    // VM closure was pushed; run until it returns.
    Ok(match run_loop_until_outcome(state, depth_before)? {
        LoopExit::Returned(v) => ThunkOutcome::Returned(v),
        LoopExit::Escaped(v) => ThunkOutcome::Escaped(v),
    })
}

/// Handle a VM-intercepted control primitive call.
///
/// Returns `Ok(None)` for normal completion (result written to `dst`).
/// Returns `Ok(Some(val))` when a continuation escape completed the entire
/// computation — the caller should propagate this as the final value and
/// not attempt to access the (now-empty) frame stack.
///
/// `is_tail` is currently unused (all are handled as non-tail for A6).
fn handle_control_primitive(
    state: &mut VmState,
    ctrl: VmControlPrimitive,
    args: &[TaggedValue],
    dst: u16,
    exit_depth: usize,
) -> Result<Option<TaggedValue>, VmError> {
    match ctrl {
        VmControlPrimitive::DynamicWind => {
            if args.len() != 3 {
                return Err(VmError::ArityMismatch {
                    expected: "3".into(),
                    got: args.len(),
                });
            }
            // Only the value form reaches here; head-position `dynamic-wind`
            // compiles to `PushWind`/`PopWind`. Run the same instructions in
            // a frame of the machine's own, so that everything this call
            // still owes — its record, its after-thunk, the delivery of its
            // body's value — is a pc a re-entering continuation restores
            // along with the frame, rather than a Rust frame it cannot
            // (issue #157, and PR #156's move for a jump's wind thunks).
            let code = value_wind_stub(state)?;
            let base = state.alloc_registers(value_wind::NUM_REGS);
            state.frames.push(CallFrame {
                pc: 0,
                register_base: base,
                num_regs: value_wind::NUM_REGS,
                closure: None,
                return_reg: dst,
                code,
            });
            // Through `set_reg_at`, not the raw slice: the frame is already
            // pushed, so `base` *is* `frame_base()`, and the debug assert
            // keeps that machine-checked if these writes are ever moved above
            // the push.
            state.set_reg_at(base, value_wind::BEFORE, args[0]);
            state.set_reg_at(base, value_wind::BODY, args[1]);
            state.set_reg_at(base, value_wind::AFTER, args[2]);
        }

        VmControlPrimitive::CallWithContinuationPrompt => {
            // (call-with-continuation-prompt body tag [handler])
            if args.is_empty() {
                return Err(VmError::ArityMismatch {
                    expected: "1+".into(),
                    got: 0,
                });
            }
            let body = args[0];
            let tag = if args.len() > 1 {
                args[1]
            } else {
                // No tag: use a fresh default tag (not ideal but functional for A6)
                use patina_core::cps_expr::PromptTag;
                state
                    .heap
                    .borrow_mut()
                    .alloc_prompt_tag(std::rc::Rc::new(PromptTag::new("default")))
            };
            let handler = if args.len() > 2 {
                args[2]
            } else {
                TaggedValue::FALSE
            };
            state.prompt_stack.push(PromptFrame {
                tag,
                stack_depth: state.frames.len(),
                dynamic_wind_depth: state.dynamic_winds.len(),
                handler,
                dst,
            });
            call_closure(state, body, &[], dst)?;
            // When body returns normally, pop_resolved_prompts will clean up the prompt.
        }

        VmControlPrimitive::AbortCurrentContinuation => {
            // (abort-current-continuation tag val...)
            if args.is_empty() {
                return Err(VmError::ArityMismatch {
                    expected: "1+".into(),
                    got: 0,
                });
            }
            let tag = args[0];
            let val = if args.len() > 1 {
                args[1]
            } else {
                TaggedValue::UNSPECIFIED
            };

            let prompt_idx = state
                .prompt_stack
                .iter()
                .rposition(|p| p.tag == tag)
                .ok_or(VmError::NoMatchingPrompt)?;
            let prompt = state.prompt_stack[prompt_idx].clone();

            let cap_frames = state.frames[prompt.stack_depth..].to_vec();
            let cap_winds = state.dynamic_winds[prompt.dynamic_wind_depth..].to_vec();
            let reg_base = if !cap_frames.is_empty() {
                cap_frames[0].register_base
            } else {
                state.registers.len()
            };
            let cap_regs = state.registers[reg_base..].to_vec();
            let cont = VmDelimitedContinuation {
                frames: cap_frames,
                dynamic_winds: cap_winds,
                registers: cap_regs,
                base_at_capture: reg_base,
            };
            let cont_tv = state.alloc_vm_delimited_continuation(cont);

            // Popped before each after-thunk runs, for the reason spelled out
            // in `vm_raise_value`.
            while state.dynamic_winds.len() > prompt.dynamic_wind_depth {
                let after = state
                    .dynamic_winds
                    .pop()
                    .expect("loop condition guarantees a record")
                    .after;
                run_thunk(state, after)?;
            }

            state.frames.truncate(prompt.stack_depth);
            state.dynamic_winds.truncate(prompt.dynamic_wind_depth);
            state.prompt_stack.truncate(prompt_idx);
            if let Some(top) = state.frames.last() {
                state
                    .registers
                    .truncate(top.register_base + top.num_regs as usize);
            } else {
                state.registers.clear();
            }

            // Handler may be a primitive or a VM closure.
            if let Some(result) = call_any(state, prompt.handler, &[val, cont_tv], prompt.dst)? {
                state.set_reg(prompt.dst, result);
            }
        }

        VmControlPrimitive::CallWithCurrentContinuation => {
            // (call/cc proc) — capture the current continuation and call proc with it
            if args.len() != 1 {
                return Err(VmError::ArityMismatch {
                    expected: "1".into(),
                    got: args.len(),
                });
            }
            let proc = args[0];
            // `dst` is dead at this point — the only writes it will ever
            // see are this call's result or a value delivered through the
            // continuation — so whatever it still holds must not go into
            // the snapshot. Left in, it chains: `guard`'s expansion is
            // `((call/cc …))`, so `dst` holds the thunk the *previous* guard
            // delivered, that thunk closes over its `handler-k`, and that
            // continuation's snapshot holds the one before. A loop catching
            // one raise per iteration retained nine heap objects per
            // iteration for the life of its frame (296 MB at 160k).
            state.set_reg(dst, TaggedValue::NULL);
            // Capture a full continuation: snapshot of entire current state
            let cont = VmContinuation {
                frames: state.frames.clone(),
                dynamic_winds: state.dynamic_winds.clone(),
                prompt_stack: state.prompt_stack.clone(),
                exception_handlers: state.exception_handlers.clone(),
                registers: state.registers.clone(),
                deliver_reg: dst,
            };
            let cont_tv = state.alloc_vm_continuation(cont);
            // Call proc with the continuation object.
            // Proc could be a primitive or a VM closure.
            if let Some(result) = call_any(state, proc, &[cont_tv], dst)? {
                state.set_reg(dst, result);
            }
        }

        VmControlPrimitive::Values => {
            // (values v1 v2 ...) — one value is itself; any other count is a
            // #<values> heap object, which is how multiple values travel
            // everywhere (call-with-values unpacks it, the display layer
            // shows it, the tree-walker does the same). There is no side
            // channel: a register-only protocol cannot go stale when a
            // `values` call is discarded.
            let packed = state.heap.borrow_mut().values_from(args.to_vec());
            state.set_reg(dst, packed);
        }

        VmControlPrimitive::CallWithValues => {
            // (call-with-values producer consumer)
            if args.len() != 2 {
                return Err(VmError::ArityMismatch {
                    expected: "2".into(),
                    got: args.len(),
                });
            }
            let producer = args[0];
            let consumer = args[1];
            // Run producer (0 args); its multiple values, if any, are a
            // #<values> object in the result.
            let primary = match run_thunk_outcome(state, producer)? {
                ThunkOutcome::Returned(v) => v,
                // The producer escaped. Running the consumer here would run it
                // on the escape value and then overwrite that value with the
                // consumer's result — the whole `call-with-values` call is
                // abandoned, not completed.
                ThunkOutcome::Escaped(v) => return Err(park_escape(state, v)),
            };
            let produced_vals = unpack_values(state, primary);
            // Consumer may be a primitive (e.g. `list`) or a VM closure.
            if let Some(result) = call_any(state, consumer, &produced_vals, dst)? {
                state.set_reg(dst, result);
            }
        }

        VmControlPrimitive::Apply => {
            // (apply proc arg ... arg-list) — R7RS §6.10.
            //
            // Nothing implements `apply` in the primitive registry, because
            // the work is spreading a list into a real call and only the VM
            // can do that. Head position does not need one: the desugarer
            // lowers `(apply f xs)` to `Instruction::Apply`. Every other route
            // arrives here — `apply` as a value, and also `(apply +)`, which
            // the desugarer's own arity check hands back to the value path.
            if args.len() < 2 {
                return Err(VmError::ArityMismatch {
                    expected: "at least 2".into(),
                    got: args.len(),
                });
            }
            let callee = args[0];
            // Fixed arguments, then the final list spread onto the end.
            let mut call_args: Vec<TaggedValue> = args[1..args.len() - 1].to_vec();
            call_args.extend(spread_apply_tail(state, *args.last().unwrap())?);
            // The same dispatcher `Instruction::Apply` uses, so the value form
            // and the head-position form accept the same callees.
            return call_value(state, callee, &call_args, dst, exit_depth);
        }

        // ── Exception handling ────────────────────────────────────────────
        VmControlPrimitive::WithExceptionHandler => {
            // (with-exception-handler handler thunk)
            if args.len() != 2 {
                return Err(VmError::ArityMismatch {
                    expected: "2".into(),
                    got: args.len(),
                });
            }
            let handler_proc = args[0];
            let thunk = args[1];

            // Verify both are callable. The handler may be a continuation —
            // `(call/cc (lambda (k) (with-exception-handler k thunk)))` is
            // R7RS's idiom for capturing a raised object.
            {
                let heap = state.heap.borrow();
                if !heap.is_callable(handler_proc) {
                    return Err(VmError::TypeError {
                        message: "with-exception-handler: first argument must be a procedure"
                            .into(),
                    });
                }
                if !heap.is_procedure(thunk) {
                    return Err(VmError::TypeError {
                        message: "with-exception-handler: second argument must be a procedure"
                            .into(),
                    });
                }
            }

            // Push exception handler (captures the frame depth). The handler is
            // popped when the thunk returns (via pop_exception_handlers) or when
            // raise invokes it. It records no wind depth: a raise does not
            // unwind, so there is nothing to unwind *to*.
            state.exception_handlers.push(ExceptionHandler {
                handler: handler_proc,
                stack_depth: state.frames.len(),
            });

            // Call the thunk — push its frame and let the run loop drive it.
            // When the thunk returns, the Return instruction path will pop the
            // exception handler via pop_exception_handlers.
            call_closure(state, thunk, &[], dst)?;
        }

        VmControlPrimitive::Raise => {
            vm_raise(state, args, dst, false)?;
        }

        VmControlPrimitive::RaiseContinuable => {
            vm_raise(state, args, dst, true)?;
        }

        VmControlPrimitive::Error => {
            // (error message obj ...)
            if args.is_empty() {
                return Err(VmError::ArityMismatch {
                    expected: "1+".into(),
                    got: 0,
                });
            }
            // R7RS 6.11 says the message *should* be a string — advice, not a
            // requirement. A non-string is displayed instead of refused; see
            // the `error` primitive in patina-primitives for why.
            let message = {
                let as_string = state.heap.borrow().get_string_contents(args[0]);
                match as_string {
                    Some(s) => s,
                    None => patina_primitives::primitives::io::datum_writer::format_display_tagged(
                        args[0],
                        &state.heap,
                    ),
                }
            };
            let irritants = args[1..].to_vec();

            // Create exception object on heap
            let exception_tv = state.heap.borrow_mut().alloc_exception(
                patina_core::ExceptionKind::Error,
                message,
                irritants,
            );

            // Raise it (non-continuable)
            vm_raise_value(state, exception_tv, dst, false)?;
        }
    }
    Ok(None)
}

/// Implement `raise` / `raise-continuable`.
fn vm_raise(
    state: &mut VmState,
    args: &[TaggedValue],
    dst: u16,
    continuable: bool,
) -> Result<(), VmError> {
    if args.len() != 1 {
        return Err(VmError::ArityMismatch {
            expected: "1".into(),
            got: args.len(),
        });
    }
    vm_raise_value(state, args[0], dst, continuable)
}

/// Raise an exception value through the handler stack.
///
/// For non-continuable raise, this pushes the handler frame and returns Ok —
/// the caller's run loop will drive the handler to completion. The handler is
/// expected to escape via `call/cc` continuation or similar. **If it returns
/// normally nothing notices**: R7RS 6.11 wants a secondary exception raised
/// in the handler's dynamic environment, but no path here detects the return
/// — `Return` delivers the handler's value to `dst` as though the raise had
/// been continuable, and the popped handler is not restored. The tree-walker
/// raises the secondary; recorded in `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6
/// and pinned in `backend_divergence.rs`.
///
/// For continuable raise, this runs the handler synchronously and returns its
/// value as the result of `raise-continuable`.
fn vm_raise_value(
    state: &mut VmState,
    exception: TaggedValue,
    dst: u16,
    continuable: bool,
) -> Result<(), VmError> {
    if let Some(handler_entry) = state.exception_handlers.pop() {
        // The wind stack is left exactly as the raise found it. R7RS 6.11
        // calls the handler "in the dynamic environment of the call to
        // `raise`, except that the current exception handler is the outer
        // one" — the pop above is that exception, and it is the only one.
        //
        // A raise crosses no dynamic extent, so no after-thunk is due. This
        // used to unwind to the handler's own installation depth first, which
        // ran an after-thunk nothing had asked for and left the extent before
        // the handler could see it. `guard` then had nowhere to go back to, so
        // a declining clause could not re-raise where R7RS says it must
        // (Track L triage families 22 and 28).
        //
        // The unwind still happens for `guard` — one level up, where it
        // belongs. `guard-k` is an ordinary continuation, and jumping to it
        // runs the after-thunks through the wind machinery that already
        // handles every other control transfer.
        if continuable {
            // Continuable: run handler synchronously, return its value
            let handler_result = match call_any(state, handler_entry.handler, &[exception], dst)? {
                Some(result) => result,
                None => {
                    // Handler is a VM closure — run it
                    let depth_before = state.frames.len() - 1;
                    match run_loop_until_outcome(state, depth_before)? {
                        LoopExit::Returned(v) => v,
                        // The handler escaped: the continuation restored its
                        // own handler stack, so this entry must not be
                        // re-pushed, and `dst` names a register in a frame
                        // that is gone.
                        //
                        // Right for a jump *out* (`guard-k`), wrong for a
                        // jump back *in*: `guard`'s declining clause invokes
                        // `handler-k`, captured inside this handler with the
                        // entry popped, and the handler then returns to the
                        // raise point through the restored frames — with no
                        // re-push on that path, the `guard` is gone for the
                        // rest of its body. The tree-walker's cleanup
                        // continuation restores it. Recorded in
                        // `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6, pinned in
                        // `backend_divergence.rs`.
                        LoopExit::Escaped(v) => return Err(park_escape(state, v)),
                    }
                }
            };
            // Re-push the handler
            state.exception_handlers.push(handler_entry);
            state.set_reg(dst, handler_result);
            Ok(())
        } else {
            // Non-continuable: push handler frame and let the run loop drive it.
            // If the handler returns normally, that's an error (R7RS §6.11)
            // which would be caught by the next iteration.
            // Typically the handler escapes via call/cc.
            match call_any(state, handler_entry.handler, &[exception], dst)? {
                Some(_result) => {
                    // Handler was a primitive that returned immediately.
                    // Non-continuable handler returning is itself an error.
                    let msg = "handler returned from non-continuable exception raise";
                    let secondary = state.heap.borrow_mut().alloc_exception(
                        patina_core::ExceptionKind::Error,
                        msg.to_string(),
                        vec![exception],
                    );
                    vm_raise_value(state, secondary, dst, false)
                }
                None => {
                    // Handler frame was pushed — the run loop will drive it.
                    // The handler is expected to escape (via call/cc or
                    // abort). If it returns normally, `Return` delivers its
                    // value to `dst` unremarked — the gap the doc comment
                    // above records.
                    Ok(())
                }
            }
        }
    } else {
        // No handler — format and propagate as Rust error
        use patina_primitives::primitives::io::datum_writer::format_display_tagged;
        let display = format_display_tagged(exception, &state.heap);
        // Deliberately the same wording whether or not the raise was
        // continuable — the variant's `Display` supplies it. Continuability is
        // an implementation detail once nothing handles it, and since
        // `guard`'s re-raise is `raise-continuable` (R7RS 7.3), saying
        // "continuable" here reported a plain `(raise 'x)` whose guard
        // declined as `unhandled continuable exception: x` — naming a form
        // the user never wrote.
        Err(VmError::SchemeException { message: display })
    }
}

/// Route a VmError through the exception handler stack if possible.
/// If no handlers are installed, re-raises the error.
#[allow(dead_code)]
fn maybe_route_error(state: &mut VmState, err: VmError, dst: u16) -> Result<(), VmError> {
    // Only route catchable errors through handlers
    if !is_catchable(&err) || state.exception_handlers.is_empty() {
        return Err(err);
    }

    // Convert VmError to a heap-allocated exception object
    let (kind, message) = classify_error(&err);
    let exception = state
        .heap
        .borrow_mut()
        .alloc_exception(kind, message, vec![]);

    vm_raise_value(state, exception, dst, false)
}

/// Classify whether a VmError should be catchable by Scheme exception handlers.
#[allow(dead_code)]
fn is_catchable(err: &VmError) -> bool {
    match err {
        VmError::StackOverflow | VmError::Compile(_) | VmError::ContinuationEscape => false,
        VmError::WithLocation { error, .. } => is_catchable(error),
        _ => true,
    }
}

/// Convert a VmError to an ExceptionKind + message for Scheme-level exception objects.
#[allow(dead_code)]
fn classify_error(err: &VmError) -> (patina_core::ExceptionKind, String) {
    use patina_core::ExceptionKind;
    match err {
        VmError::UnboundVariable { name } => (
            ExceptionKind::Error,
            format!("Undefined variable: {}", name),
        ),
        VmError::ArityMismatch { expected, got } => (
            ExceptionKind::Error,
            format!(
                "Wrong number of arguments: expected {}, got {}",
                expected, got
            ),
        ),
        VmError::TypeError { message } => {
            // Strip "Type error: " prefix if present (matches tree-walker behavior)
            let msg = message
                .strip_prefix("Type error: ")
                .unwrap_or(message)
                .to_string();
            (ExceptionKind::Error, msg)
        }
        VmError::NoMatchingPrompt => (ExceptionKind::Error, "no matching prompt tag".to_string()),
        VmError::DivideByZero => (ExceptionKind::Error, "Division by zero".to_string()),
        VmError::Runtime { message } => {
            // Classify sub-errors by message content (matches tree-walker behavior)
            let kind = if message.contains("Cannot open")
                || message.contains("Cannot delete")
                || message.contains("Cannot read")
                || message.contains("Cannot write")
                || message.contains("No such file")
                || message.contains("file")
            {
                ExceptionKind::FileError
            } else if message.contains("read:") || message.contains("parse") {
                ExceptionKind::ReadError
            } else {
                ExceptionKind::Error
            };
            // Strip "Type error: " prefix if present (primitive errors come
            // through here via e.to_string() which includes the prefix)
            let msg = message
                .strip_prefix("Type error: ")
                .unwrap_or(message)
                .to_string();
            (kind, msg)
        }
        VmError::SchemeException { message } => (ExceptionKind::Error, message.clone()),
        // Unwrap location wrapper and classify the inner error.
        VmError::WithLocation { error, .. } => classify_error(error),
        // Non-catchable (shouldn't reach here due to is_catchable check)
        VmError::StackOverflow | VmError::Compile(_) | VmError::ContinuationEscape => {
            (ExceptionKind::Error, err.to_string())
        }
    }
}

/// Pop any `PromptFrame`s whose `stack_depth` now exceeds the current frame depth
/// (i.e., the prompt's body returned normally).
fn pop_resolved_prompts(state: &mut VmState) {
    while let Some(pf) = state.prompt_stack.last() {
        if pf.stack_depth >= state.frames.len() {
            state.prompt_stack.pop();
        } else {
            break;
        }
    }
}

/// Close the extents keyed on frames that are no longer on the stack. Called
/// wherever the frame stack has just shrunk and the departing frame's value
/// has been delivered: `Return`, and each tail-position branch that pops the
/// frame itself because its callee (a control primitive, a primitive, a
/// parameter, a `call-with-values` consumer) delivers straight to the caller.
///
/// A handler is live exactly while the thunk it was installed for has a
/// frame, which is `stack_depth < frames.len()`; the same holds of prompts.
/// So the pops are safe at any such point, and skipping them at one leaves a
/// stale entry that the next raise finds — the tail-position branches used to
/// skip them, and `(with-exception-handler h (lambda () (values 1)))` left
/// `h` installed for the rest of the program.
///
/// Wind records are *not* among the extents closed here, and no longer carry
/// a depth to close them by. Every record is pushed by `PushWind` and popped
/// by the `PopWind` that follows it in the same instruction sequence, or by
/// the jump that leaves its extent. A sweep by depth existed until 2026-09-02
/// as the only cleanup the **value form** of `dynamic-wind` had left once a
/// continuation abandoned its Rust frame; the value form runs `PushWind` and
/// `PopWind` itself now ([`value_wind_stub`]), and the sweep was reachable on
/// no input.
///
/// **Nothing is popped at the run loop's exit depth.** The depth test cannot
/// decide there: a handler the loop was *started under* — its thunk's frame
/// tail-replaced by the control primitive that started the loop, so it sits
/// at exactly `exit_depth` — is live, while one a tail-called
/// `with-exception-handler` installed *inside* the loop sits at the same
/// depth and is dead. `run_loop_until_outcome` closes the second kind from
/// its own entry count. Prompts at that depth belong to the Rust caller that
/// started the loop (`run_thunk_outcome`, a prompt body), which closes them
/// itself.
fn pop_resolved_extents(state: &mut VmState, exit_depth: usize) {
    if state.frames.len() == exit_depth || state.frames.is_empty() {
        return;
    }
    pop_resolved_prompts(state);
    pop_exception_handlers(state);
}

/// Pop exception handlers whose thunk has returned (stack shrank below their depth).
fn pop_exception_handlers(state: &mut VmState) {
    while let Some(eh) = state.exception_handlers.last() {
        if eh.stack_depth >= state.frames.len() {
            state.exception_handlers.pop();
        } else {
            break;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Continuation jumps and their wind thunks
// ─────────────────────────────────────────────────────────────────────────────

/// The registers of a wind-step stub frame — the jump itself, held where a
/// `call/cc` inside the thunk will snapshot it along with every other
/// register. See [`Instruction::ResumeWindJump`].
mod wind_step {
    /// The continuation being jumped to.
    pub(super) const TARGET: u16 = 0;
    /// The value it delivers on arrival.
    pub(super) const VALUE: u16 = 1;
    /// Index into the *target's* wind stack of the record whose `before`
    /// thunk this step is running, or `#f` for an `after` thunk (whose record
    /// was popped before it ran and is never pushed back).
    pub(super) const ENTERING: u16 = 2;
    /// Where the thunk returns. A wind thunk's value is discarded, but
    /// `Return` needs a slot to write into.
    pub(super) const THUNK_RESULT: u16 = 3;
    /// Window size of a stub frame.
    pub(super) const NUM_REGS: u16 = 4;
}

/// How many leading records two wind stacks share (R7RS §6.10's common
/// prefix), by the identity of the `dynamic-wind` call each stands for.
///
/// The comparison is on `DynamicWindRecord::id`, unique per `dynamic-wind`
/// *call*, which is why the VM's record grew one. Depth cannot serve — the
/// whole question is where two stacks stop agreeing — and neither can the
/// `before` thunk, since two calls may share a closure.
///
/// What was once wrong here was not the comparison but that it was skipped: a
/// full `call/cc` invoke forced the prefix to 0, so it exited and re-entered
/// every extent, the shared ones included. Invoking a continuation captured
/// inside its own extent therefore ran that extent's after and before thunks
/// for a jump that crossed nothing: `(dynamic-wind in (lambda () (call/cc
/// (lambda (k) (k #f)))) out)` logged `(in out in out)` where the
/// tree-walker, chibi and Gauche log `(in out)`.
fn common_wind_prefix(current: &[DynamicWindRecord], target: &[DynamicWindRecord]) -> usize {
    current
        .iter()
        .zip(target.iter())
        .take_while(|(a, b)| a.id == b.id)
        .count()
}

/// Take the next step of a jump to the full continuation `target`: run one
/// wind thunk between the live wind stack and the target's, or, with none
/// left, arrive — restore the target's state and deliver `value`.
///
/// This is chibi's "travel to point", one thunk per step: leave the innermost
/// extent not shared with the target (pop its record, run its `after`), until
/// the live stack is a prefix of the target's; then enter the target's
/// remaining extents outermost first (run `before`, push the record). Each
/// thunk runs under a stub frame whose only instruction comes back here
/// ([`Instruction::ResumeWindJump`]), so the thunks are ordinary frames of
/// the machine the jump was made on rather than nested Rust calls.
///
/// Each thunk also runs in the dynamic environment of its own `dynamic-wind`
/// call (R7RS §6.10): the wind stack below its record, and the handler stack
/// the record captured. So a raise in an after-thunk reaches the `guard`
/// whose escape is running it, the guard's handler fires a second time, and
/// its second jump — starting from the stack this one had got to — abandons
/// this jump and runs the after-thunks still outstanding. That is the
/// `finally` rule of Track L §6: the after-thunk's exception replaces the one
/// in flight, and unwinding continues.
///
/// Popping the record *before* running its after-thunk is what makes the
/// second jump terminate: the thunk is not on the stack it jumps from.
///
/// The caller signals the dispatch loop the way every continuation invoke
/// does — park the value, return the sentinel — and the loop decides whether
/// the frames now on the stack are its own to run.
fn step_wind_jump(
    state: &mut VmState,
    target: TaggedValue,
    value: TaggedValue,
) -> Result<(), VmError> {
    let cc = state
        .get_vm_continuation(target)
        .ok_or_else(|| VmError::TypeError {
            message: "continuation jump: not a full continuation".into(),
        })?;
    let common = common_wind_prefix(&state.dynamic_winds, &cc.dynamic_winds);

    // Leaving an extent: pop first, then run its after-thunk.
    if state.dynamic_winds.len() > common {
        let record = state
            .dynamic_winds
            .pop()
            .expect("longer than its own prefix");
        return push_wind_step(
            state,
            target,
            value,
            TaggedValue::FALSE,
            record.after,
            &record.handlers,
        );
    }

    // Entering one: run its before-thunk, and push the record only when that
    // returns — `ResumeWindJump` does it, from `ENTERING`.
    //
    // `common` is `state.dynamic_winds.len()` here (the branch above ruled
    // out longer, and a prefix cannot be longer than the stack it indexes),
    // so this is the first record of the target that the live stack lacks.
    if let Some(record) = cc.dynamic_winds.get(state.dynamic_winds.len()) {
        let entering = TaggedValue::fixnum(state.dynamic_winds.len() as i64);
        return push_wind_step(
            state,
            target,
            value,
            entering,
            record.before,
            &record.handlers,
        );
    }

    // Arrived.
    state.registers = cc.registers.clone();
    state.frames = cc.frames.clone();
    state.dynamic_winds = cc.dynamic_winds.clone();
    state.prompt_stack = cc.prompt_stack.clone();
    state.exception_handlers = cc.exception_handlers.clone();
    // Deliver into `deliver_reg` of the top frame. Any `base` the caller
    // hoisted is stale here — the whole register file was just replaced.
    if let Some(top) = state.frames.last() {
        let top_base = top.register_base;
        state.registers[top_base + cc.deliver_reg as usize] = value;
    }
    Ok(())
}

/// Push a stub frame carrying the rest of the jump, install the handler stack
/// the thunk's `dynamic-wind` call had, and call the thunk under it.
fn push_wind_step(
    state: &mut VmState,
    target: TaggedValue,
    value: TaggedValue,
    entering: TaggedValue,
    thunk: TaggedValue,
    handlers: &[ExceptionHandler],
) -> Result<(), VmError> {
    install_thunk_handlers(state, handlers);
    let code = wind_jump_stub(state)?;
    let base = state.alloc_registers(wind_step::NUM_REGS);
    state.frames.push(CallFrame {
        pc: 0,
        register_base: base,
        num_regs: wind_step::NUM_REGS,
        closure: None,
        // Nothing ever returns *into* this frame: `ResumeWindJump` pops it
        // itself and either starts the next thunk or lands the jump.
        return_reg: 0,
        code,
    });
    // `set_reg_at` for the reason the value form's stub gives: the frame is
    // pushed, so the assert holds and keeps holding.
    state.set_reg_at(base, wind_step::TARGET, target);
    state.set_reg_at(base, wind_step::VALUE, value);
    state.set_reg_at(base, wind_step::ENTERING, entering);
    // A primitive thunk returns here and now; its value is discarded either
    // way, and the stub frame is left on top for `ResumeWindJump` to run.
    call_any(state, thunk, &[], wind_step::THUNK_RESULT)?;
    Ok(())
}

/// Make `handlers` — the stack of the `dynamic-wind` call a thunk belongs to
/// — the live handler stack for the length of that thunk.
///
/// The depths inside a record's handlers are frame depths of a stack that is
/// not the live one: the record may have been made far below the jump site,
/// or, on a re-entry, above it. `pop_exception_handlers` reads them
/// literally and would drop a handler as soon as a frame at or above its
/// recorded depth returned, so each is clamped to the depth the thunk starts
/// at. Clamping against one constant preserves the stack's non-decreasing
/// order, and nothing else reads the field.
///
/// There is no restore, and none is owed. Every step of a jump installs a
/// stack and arrival installs the target's own, so the jump site's stack is
/// never the right one to come back to — R7RS 6.10 puts the thunk in its own
/// call's dynamic environment, not the jump's. That holds for a `VmError` the
/// thunk's *call* raises too (a `dynamic-wind` whose after is `42`): it is
/// routed through the record's handlers, which is where a raise from the thunk
/// belongs. A travel abandoned by an error that nothing catches abandons the
/// machine with it — `execute` clears the frames and every stack on the way
/// out.
fn install_thunk_handlers(state: &mut VmState, handlers: &[ExceptionHandler]) {
    let depth = state.frames.len();
    state.exception_handlers.clear();
    state
        .exception_handlers
        .extend(handlers.iter().map(|h| ExceptionHandler {
            handler: h.handler,
            stack_depth: h.stack_depth.min(depth),
        }));
}

/// The one-instruction code object every wind step's frame runs.
///
/// Built at most once per `VmState` and kept in `code_store`, so the GC's
/// "every frame's code came from the store" invariant (`gc_roots.rs`) holds
/// without qualification — this one has no constants to trace, but the
/// invariant is cheaper to keep than to caveat.
fn wind_jump_stub(state: &mut VmState) -> Result<Rc<CodeObject>, VmError> {
    if let Some(id) = state.wind_jump_code {
        return state.code_object(id);
    }
    let instructions = vec![Instruction::ResumeWindJump];
    let id = CodeObjectId::fresh();
    state.load(CodeObject {
        id,
        name: Some(Rc::from("wind-jump")),
        global_cache: GlobalCacheEntry::table(&instructions),
        instructions,
        constants: Vec::new(),
        num_regs: wind_step::NUM_REGS,
        arity: Arity::Fixed(0),
        source_map: Vec::new(),
    });
    state.wind_jump_code = Some(id);
    state.code_object(id)
}

/// The registers of the stub frame the **value** form of `dynamic-wind` runs
/// in — its three thunks, its body's value, and one slot for the two thunks
/// whose values are discarded. See [`value_wind_stub`].
mod value_wind {
    /// The before-thunk.
    pub(super) const BEFORE: u16 = 0;
    /// The body thunk.
    pub(super) const BODY: u16 = 1;
    /// The after-thunk.
    pub(super) const AFTER: u16 = 2;
    /// The body's value, which is the call's value. Held across the
    /// after-thunk, so it cannot share a slot with it.
    pub(super) const RESULT: u16 = 3;
    /// Where the before- and after-thunks return. Their values are discarded,
    /// but `Return` needs a slot to write into.
    pub(super) const DISCARD: u16 = 4;
    /// Window size of the stub frame.
    pub(super) const NUM_REGS: u16 = 5;
}

/// The code object the value form of `dynamic-wind` runs — the six
/// instructions `pass5_codegen` emits for head position, in a frame of their
/// own because there is no call site to emit them into.
///
/// **Kept in step by hand with `pass5_codegen.rs`'s `dynamic-wind` case**,
/// which is the price of the two entry points; a change to one is a change to
/// the other, and each names the other so an editor of either finds it. The
/// two are not textually identical and are not meant to be: pass 5 emits
/// `Return` only in tail position and lets the discarded thunk results land in
/// `expr.dst` and the before-thunk's own register, where a frame of its own
/// can afford a dedicated slot and always returns. The *order and identity* of
/// the instructions is what has to match, because that is what the two forms
/// agreeing depends on.
///
/// The point is not to save the nested Rust loop but to make this call's
/// remaining obligations *resumable*. A continuation captured in the body
/// restores the VM's frames, and this frame's pc is one of them, so a
/// re-entry that returns through here still pops the record, still runs this
/// call's own after-thunk, and still delivers the body's value to the caller
/// — none of which a Rust frame abandoned by the jump could do (issue #157).
///
/// Built at most once per `VmState` and kept in `code_store`, like
/// [`wind_jump_stub`] and for the same GC-invariant reason.
fn value_wind_stub(state: &mut VmState) -> Result<Rc<CodeObject>, VmError> {
    if let Some(id) = state.value_wind_code {
        return state.code_object(id);
    }
    let instructions = vec![
        Instruction::Call {
            func: value_wind::BEFORE,
            args: vec![],
            dst: value_wind::DISCARD,
        },
        Instruction::PushWind {
            before: value_wind::BEFORE,
            after: value_wind::AFTER,
        },
        Instruction::Call {
            func: value_wind::BODY,
            args: vec![],
            dst: value_wind::RESULT,
        },
        // Pops the record; the after-thunk is the `Call` that follows, so
        // that a jump out of *it* no longer sees this extent as entered.
        Instruction::PopWind,
        Instruction::Call {
            func: value_wind::AFTER,
            args: vec![],
            dst: value_wind::DISCARD,
        },
        Instruction::Return {
            val: value_wind::RESULT,
        },
    ];
    let id = CodeObjectId::fresh();
    state.load(CodeObject {
        id,
        name: Some(Rc::from("dynamic-wind")),
        global_cache: GlobalCacheEntry::table(&instructions),
        instructions,
        constants: Vec::new(),
        num_regs: value_wind::NUM_REGS,
        arity: Arity::Fixed(0),
        source_map: Vec::new(),
    });
    state.value_wind_code = Some(id);
    state.code_object(id)
}

/// The handler stack a `dynamic-wind` record captures.
///
/// Shared when empty: `Rc<[T]>` allocates a header even for no elements, and
/// most `dynamic-wind` calls run with no handler installed at all.
fn captured_handlers(state: &VmState) -> Rc<[ExceptionHandler]> {
    if state.exception_handlers.is_empty() {
        return EMPTY_HANDLERS.with(Rc::clone);
    }
    Rc::from(state.exception_handlers.as_slice())
}

thread_local! {
    /// The one empty handler stack every handler-free wind record shares.
    static EMPTY_HANDLERS: Rc<[ExceptionHandler]> = Rc::from(Vec::new());
}

// ─────────────────────────────────────────────────────────────────────────────
// VmApplyContext — implements ApplyContext with higher-order proc support
// ─────────────────────────────────────────────────────────────────────────────

/// `ApplyContext` implementation for the VM.
///
/// Holds a raw pointer to the `VmState` so that `apply_proc` (which takes
/// `&self`) can mutably re-enter the VM execution loop.  This is sound because
/// `apply_proc` is only called synchronously during `call_primitive_proc`, which
/// already has exclusive `&mut VmState` access, and the pointer is never shared
/// across threads.
struct VmApplyContext {
    state: *mut VmState,
}

/// How a re-entry into the VM ended, when it did not end normally.
enum Reentry {
    /// A continuation captured outside the boundary was invoked inside it.
    /// The value it carries is on [`VmState::pending_escape`].
    Escaped,
    /// The body failed on its own terms.
    Failed(VmError),
}

/// Run `body` across a re-entry into the VM, detecting a continuation that
/// escaped past the boundary.
///
/// Every `ApplyContext` method, and every synchronous nested run, is such a
/// boundary: the Rust code below it holds a stack it loses if a continuation
/// is invoked inside. On a shrink the carried value is stashed on
/// [`VmState::pending_escape`] and the caller is told to unwind rather than
/// carry on with frames it no longer owns.
///
/// `depth_before` is the caller's own frame depth *before* it pushed anything
/// for this call — passed in rather than sampled here, because a caller that
/// has already pushed a frame (`call_any_sync`) would otherwise compare
/// against the pushed depth and read every normal return as an escape.
///
/// Route new boundaries through here. The first attempt at this fix covered
/// one boundary of four, and `load` and `parameterize` stayed broken because
/// nothing made the omission visible.
fn across_reentry<T>(
    state: &mut VmState,
    depth_before: usize,
    body: impl FnOnce(&mut VmState) -> Result<T, VmError>,
    value_of: impl FnOnce(&T) -> TaggedValue,
) -> Result<T, Reentry> {
    let result = body(state);
    if state.frames.len() < depth_before {
        // Any error here belongs to a call that is being abandoned; what
        // resumes is the continuation's value, not this one's outcome.
        let carried = result.as_ref().ok().map(value_of);
        state.pending_escape = Some(carried.unwrap_or(TaggedValue::UNSPECIFIED));
        return Err(Reentry::Escaped);
    }
    result.map_err(Reentry::Failed)
}

impl Reentry {
    /// The escape sentinel is deliberately `ContinuationEscape`: it is already
    /// non-catchable, so no `guard` between the primitive and the dispatch
    /// loop can swallow it.
    fn into_eval_error(self) -> patina_primitives::EvalError {
        match self {
            Reentry::Escaped => patina_primitives::EvalError::ContinuationEscape,
            Reentry::Failed(e) => patina_primitives::EvalError::InternalError(e.to_string()),
        }
    }
}

impl patina_primitives::ApplyContext for VmApplyContext {
    fn heap(&self) -> &SharedHeap {
        // SAFETY: pointer is valid for the lifetime of the primitive call.
        unsafe { &(*self.state).heap }
    }

    fn fs(&self) -> &Arc<dyn patina_core::FileSystem> {
        // SAFETY: pointer is valid for the lifetime of the primitive call.
        unsafe { &(*self.state).fs }
    }

    fn apply_proc(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
    ) -> Result<TaggedValue, patina_primitives::EvalError> {
        // SAFETY: we have exclusive access (see struct doc comment).
        let state = unsafe { &mut *self.state };
        let depth_before = state.frames.len();
        across_reentry(
            state,
            depth_before,
            |s| run_apply_proc(s, proc, &args),
            |v| *v,
        )
        .map_err(Reentry::into_eval_error)
    }

    fn eval_expr(
        &self,
        expr: TaggedValue,
        env: &Rc<patina_core::environment::Environment>,
    ) -> Result<TaggedValue, patina_primitives::EvalError> {
        let state = unsafe { &mut *self.state };
        let depth_before = state.frames.len();
        across_reentry(state, depth_before, |s| vm_eval_expr(s, expr, env), |v| *v)
            .map_err(Reentry::into_eval_error)
    }

    fn load_scheme_library(
        &self,
        name: &[String],
    ) -> Result<Rc<patina_core::library::Library>, patina_primitives::EvalError> {
        let state = unsafe { &mut *self.state };
        // A library is not a `TaggedValue`; an escape out of a load resumes
        // with whatever the continuation carried, not with the library.
        let depth_before = state.frames.len();
        across_reentry(
            state,
            depth_before,
            |s| {
                vm_load_library(s, name).map_err(|e| VmError::Runtime {
                    message: e.to_string(),
                })
            },
            |_| TaggedValue::UNSPECIFIED,
        )
        .map(Rc::new)
        .map_err(Reentry::into_eval_error)
    }

    fn interaction_environment(&self) -> Rc<patina_core::environment::Environment> {
        // SAFETY: pointer is valid for the lifetime of the primitive call.
        unsafe { (*self.state).globals.clone() }
    }
}

/// Apply a procedure (VmClosure or primitive) to args within the VM,
/// running the execution loop if needed. Used by `VmApplyContext::apply_proc`.
fn run_apply_proc(
    state: &mut VmState,
    proc: TaggedValue,
    args: &[TaggedValue],
) -> Result<TaggedValue, VmError> {
    let depth_before = state.frames.len();

    // Use a scratch return register beyond the current frame's live registers.
    let return_reg = state.frames.last().map(|f| f.num_regs).unwrap_or(0);
    if let Some(f) = state.frames.last() {
        let needed = f.register_base + return_reg as usize + 1;
        if state.registers.len() < needed {
            state.registers.resize(needed, TaggedValue::UNSPECIFIED);
        }
    }

    if let Some(result) = call_any(state, proc, args, return_reg)? {
        // Primitive — returned immediately.
        return Ok(result);
    }
    // VM closure was pushed; run until it returns.
    run_loop_until(state, depth_before)
}

/// Recognized VM-intercepted control primitives.
#[derive(Clone, Copy)]
pub(crate) enum VmControlPrimitive {
    DynamicWind,
    CallWithContinuationPrompt,
    AbortCurrentContinuation,
    CallWithCurrentContinuation,
    CallWithValues,
    Values,
    Apply,
    WithExceptionHandler,
    Raise,
    RaiseContinuable,
    Error,
}

/// The single source of truth for which qualified names the VM intercepts.
/// `resolve_primitive_calls` must never emit `CallPrimitive` for any of these
/// (its exclusion is cross-checked against this table by
/// `excluded_covers_every_intercepted_primitive` in
/// `compiler/primitive_calls.rs`).
///
/// The predicate is "the registry cannot implement this — it needs the VM's
/// own call machinery". For eleven of the twelve the reason is control flow: a
/// directly dispatched registry handler would bypass the VM's
/// continuation/exception cooperation. `apply` is the exception and is why the
/// predicate is worded that way rather than as "control primitives": spreading
/// a list into a call is not control flow, but it is equally impossible from
/// inside a registry handler, and there is no handler to bypass — the registry
/// entry `(patina internal control)` exports has no body at all.
pub(crate) const VM_INTERCEPTED_PRIMITIVES: &[(&str, VmControlPrimitive)] = &[
    (
        "patina.internal.control/dynamic-wind",
        VmControlPrimitive::DynamicWind,
    ),
    (
        "patina.internal.control/call-with-continuation-prompt",
        VmControlPrimitive::CallWithContinuationPrompt,
    ),
    (
        "patina.internal.control/abort-current-continuation",
        VmControlPrimitive::AbortCurrentContinuation,
    ),
    (
        "patina.internal.control/call-with-current-continuation",
        VmControlPrimitive::CallWithCurrentContinuation,
    ),
    (
        "patina.internal.control/call/cc",
        VmControlPrimitive::CallWithCurrentContinuation,
    ),
    (
        "patina.internal.control/call-with-values",
        VmControlPrimitive::CallWithValues,
    ),
    ("patina.internal.control/values", VmControlPrimitive::Values),
    ("patina.internal.control/apply", VmControlPrimitive::Apply),
    (
        "patina.internal.errors/with-exception-handler",
        VmControlPrimitive::WithExceptionHandler,
    ),
    ("patina.internal.errors/raise", VmControlPrimitive::Raise),
    (
        "patina.internal.errors/raise-continuable",
        VmControlPrimitive::RaiseContinuable,
    ),
    ("patina.internal.errors/error", VmControlPrimitive::Error),
];

/// If `func_val` is a VM-intercepted control primitive, return which one.
fn vm_control_primitive(state: &VmState, func_val: TaggedValue) -> Option<VmControlPrimitive> {
    let proc = state.heap.borrow().get_procedure(func_val)?;
    let Procedure::Primitive { qualified_name, .. } = proc.as_ref() else {
        return None;
    };
    // Cheap prefix reject before the linear string scan: every intercepted
    // primitive lives under the excluded namespaces — enforced by
    // `excluded_covers_every_intercepted_primitive` in primitive_calls.rs.
    if !crate::compiler::primitive_calls::is_excluded(qualified_name) {
        return None;
    }
    VM_INTERCEPTED_PRIMITIVES
        .iter()
        .find(|(name, _)| *name == qualified_name.as_ref())
        .map(|&(_, ctrl)| ctrl)
}

/// Try to invoke `func_val` as a continuation. Returns `Ok(true)` if it was
/// a continuation and was invoked (stack has been replaced/appended),
/// `Ok(false)` if not a continuation.
fn try_invoke_continuation(
    state: &mut VmState,
    func_val: TaggedValue,
    args: &[TaggedValue],
) -> Result<Option<TaggedValue>, VmError> {
    // Full (call/cc) continuation?
    //
    // The travel may not finish here: a jump with a wind thunk to run pushes
    // that thunk's frames and comes back through `ResumeWindJump`. Either
    // way this call is over, and the caller signals the dispatch loop.
    if state.get_vm_continuation(func_val).is_some() {
        let deliver_val = deliver_value(state, args);
        step_wind_jump(state, func_val, deliver_val)?;
        return Ok(Some(deliver_val));
    }

    // Delimited (composable) continuation?
    if let Some(dc) = state.get_vm_delimited_continuation(func_val) {
        let deliver_val = deliver_value(state, args);
        let enter_winds = dc.dynamic_winds.clone();
        for record in &enter_winds {
            run_thunk(state, record.before)?;
        }
        let new_base = state.registers.len();
        let old_base = dc.base_at_capture;
        let shift = new_base.wrapping_sub(old_base);
        state.registers.extend_from_slice(&dc.registers);
        let mut new_frames: Vec<CallFrame> = dc.frames.clone();
        for f in &mut new_frames {
            f.register_base = f.register_base.wrapping_add(shift);
        }
        state.frames.extend(new_frames);
        state.dynamic_winds.extend(enter_winds);
        if let Some(top) = state.frames.last() {
            let ret_reg = top.return_reg;
            let n = state.frames.len();
            if n >= 2 {
                let caller_base = state.frames[n - 2].register_base;
                state.registers[caller_base + ret_reg as usize] = deliver_val;
            }
        }
        return Ok(Some(deliver_val));
    }

    Ok(None)
}

/// What `(k …)` delivers: `(k v)` delivers `v`; `(k)` and `(k v1 v2 …)`
/// deliver a `#<values>` object, exactly as `(values …)` would return one — so
/// a producer that escapes through a continuation still hands
/// `call-with-values` its values.
///
/// Built only once the callee is known to be a continuation. It used to be the
/// first thing `try_invoke_continuation` did, so every non-continuation callee
/// that reached the probe paid a `Vec` (and, for other than one argument, a
/// heap allocation) to decline.
fn deliver_value(state: &mut VmState, args: &[TaggedValue]) -> TaggedValue {
    state.heap.borrow_mut().values_from(args.to_vec())
}

/// Try to call `func_val` as a primitive. Returns `Some(result)` if it was a
/// primitive, `None` if it's a VM closure (caller should push a frame instead).
/// If `func_val` is a primitive procedure, return it. This is only a type
/// check — no argument copying — so call sites can hand their argument
/// vector to `call_primitive_proc` by move exactly when it will be consumed,
/// instead of cloning it defensively before knowing the callee's kind.
fn primitive_procedure(state: &VmState, func_val: TaggedValue) -> Option<Rc<Procedure>> {
    let proc = state.heap.borrow().get_procedure(func_val)?;
    matches!(proc.as_ref(), Procedure::Primitive { .. }).then_some(proc)
}

/// Park `value` for the dispatch loop that owns the resumed frame and return
/// the sentinel that unwinds the Rust frames in between.
///
/// Always an unwind, never a return. The frames now on `state` may belong to
/// any enclosing dispatch loop — or to none of them, if the escape targets a
/// frame further out — and only `run_loop_until` knows its own `exit_depth`,
/// so it is the one place allowed to decide whether to resume or exit. Every
/// synchronous boundary in between (`run_thunk`, and through it
/// `call-with-values`) sees the sentinel and learns that the value is the
/// resumed computation's, not its own — which is the whole difference between
/// writing a register in a live frame and writing one in a frame that no
/// longer exists.
///
/// `value` is what the continuation *delivers*
/// ([`deliver_value`]), which for `(k)` and `(k v1 v2 …)` is a `#<values>`
/// object rather than the first argument. The two invoke paths used to park
/// different things — `ResumeWindJump` the delivered value, the direct path
/// `args[0]` — so the same `(k 1 2)` carried different values depending on
/// whether the jump happened to cross a wind.
fn park_escape(state: &mut VmState, value: TaggedValue) -> VmError {
    state.pending_escape = Some(value);
    VmError::ContinuationEscape
}

/// Dispatch a call to an arbitrary callee value — the body of the `Call`
/// instruction, also used by `CallPrimitive`'s deopt path. Returns
/// `Some(value)` when an invoked continuation unwound to or past
/// `exit_depth` and the enclosing `run_loop_until` must exit with `value`.
fn call_value(
    state: &mut VmState,
    func_val: TaggedValue,
    arg_vals: &[TaggedValue],
    dst: u16,
    exit_depth: usize,
) -> Result<Option<TaggedValue>, VmError> {
    // The callee is almost always a plain closure, and the callable heap
    // types are mutually exclusive — so probe the closure case first and
    // let the common path pay one type check instead of failing the four
    // rarer probes below (control primitive, primitive, parameter,
    // continuation) on every call. Keep the probed code id so the closure
    // branch doesn't resolve it a second time.
    let closure_code_id = state.heap.borrow().get_vm_closure_code_id(func_val);
    call_value_with_probe(state, func_val, closure_code_id, arg_vals, dst, exit_depth)
}

/// `call_value` for a callee whose closure probe already ran — the `Call`
/// arm's non-closure branch passes `None` so the probe isn't repeated.
fn call_value_with_probe(
    state: &mut VmState,
    func_val: TaggedValue,
    closure_code_id: Option<u32>,
    arg_vals: &[TaggedValue],
    dst: u16,
    exit_depth: usize,
) -> Result<Option<TaggedValue>, VmError> {
    if closure_code_id.is_none() {
        // Intercept higher-order control primitives that need VM cooperation.
        if let Some(ctrl) = vm_control_primitive(state, func_val) {
            return handle_control_primitive(state, ctrl, arg_vals, dst, exit_depth);
        }
        if let Some(prim) = primitive_procedure(state, func_val) {
            let result = call_primitive_proc(state, &prim, arg_vals);
            state.set_reg(dst, result?);
            return Ok(None);
        }
        if let Some(result) = try_call_parameter(state, func_val, arg_vals) {
            state.set_reg(dst, result?);
            return Ok(None);
        }
        if let Some(delivered) = try_invoke_continuation(state, func_val, arg_vals)? {
            return Err(park_escape(state, delivered));
        }
    }
    let code_id = match closure_code_id {
        Some(id) => CodeObjectId(id),
        // Not callable: resolve_closure produces the standard type error.
        None => resolve_closure(state, func_val)?,
    };
    call_closure_resolved(state, func_val, code_id, arg_vals, dst)?;
    Ok(None)
}

/// Dispatch a call to an arbitrary callee value in tail position — the body
/// of the `TailCall` instruction, also used by the deopt path of tail-shaped
/// primitive sites (PRD P8.2). Control primitives, primitives, and
/// parameters pop the frame and deliver straight to the caller; closures
/// reuse the current frame's register window. Returns `Some(value)` when the
/// enclosing `run_loop_until` must exit with `value`.
fn tail_call_value(
    state: &mut VmState,
    func_val: TaggedValue,
    arg_vals: &[TaggedValue],
    exit_depth: usize,
) -> Result<Option<TaggedValue>, VmError> {
    // Same closure-first probe as `call_value` — see there for why probe
    // order is safe.
    let closure_code_id = state.heap.borrow().get_vm_closure_code_id(func_val);
    tail_call_value_with_probe(state, func_val, closure_code_id, arg_vals, exit_depth)
}

/// `tail_call_value` for a callee whose closure probe already ran — the
/// `TailCall` arm's non-closure branch passes `None`.
fn tail_call_value_with_probe(
    state: &mut VmState,
    func_val: TaggedValue,
    closure_code_id: Option<u32>,
    arg_vals: &[TaggedValue],
    exit_depth: usize,
) -> Result<Option<TaggedValue>, VmError> {
    if closure_code_id.is_none() {
        // Intercept higher-order control primitives in tail position.
        // Strategy: pop the current frame first (simulating "already
        // returned"), then dispatch the primitive as if called from the
        // parent frame.
        if let Some(ctrl) = vm_control_primitive(state, func_val) {
            let frame = state.frames.pop().expect("tail call ctrl with empty stack");
            let return_reg = frame.return_reg;
            state.free_top_registers(frame.register_base);
            let depth = state.frames.len();
            // Now at depth N-1. Handle with dst = return_reg (slot in frame
            // N-2). (At exit depth, return_reg is still the right dst — not
            // 0, which could clobber live registers like MutableCell
            // pointers.)
            //
            // The extents keyed on the popped frame stay open across the
            // dispatch: a `raise` in tail position must still find the
            // handler its thunk was called under.
            if let Some(escaped) =
                handle_control_primitive(state, ctrl, arg_vals, return_reg, exit_depth)?
            {
                return Ok(Some(escaped));
            }
            // The control primitive has completed. If it delivered its
            // result without pushing a frame, the tail call has returned and
            // the popped frame's extents close now, as after `Return`; a
            // frame it did push closes them when that frame returns. Then,
            // if this is the exit depth, the enclosing loop is done.
            if state.frames.len() == depth {
                pop_resolved_extents(state, exit_depth);
            }
            if state.frames.len() == exit_depth {
                let result = state.reg(return_reg);
                return Ok(Some(result));
            }
            return Ok(None);
        }

        // Primitives in tail position: call them, write result to current
        // frame's return_reg, then simulate a Return.
        //
        // Probed before continuations, matching `call_value_with_probe`'s
        // order. The two are interchangeable — the callable heap variants are
        // mutually exclusive, the same argument the closure-first probe above
        // rests on — and a primitive callee is far commoner than a
        // continuation, so this saves the two heap borrows
        // `try_invoke_continuation` spends before it can decline.
        if let Some(prim) = primitive_procedure(state, func_val) {
            let result = call_primitive_proc(state, &prim, arg_vals)?;
            let frame = state.frames.pop().expect("tail call with empty stack");
            if state.frames.len() == exit_depth {
                state.free_top_registers(frame.register_base);
                return Ok(Some(result));
            }
            let return_reg = frame.return_reg;
            state.set_reg(return_reg, result);
            state.free_top_registers(frame.register_base);
            // The popped frame's extents close now, as after `Return`.
            pop_resolved_extents(state, exit_depth);
            return Ok(None);
        }

        // Continuation invocation in tail position.
        if let Some(delivered) = try_invoke_continuation(state, func_val, arg_vals)? {
            return Err(park_escape(state, delivered));
        }

        // Parameters in tail position: same as primitives.
        if let Some(result) = try_call_parameter(state, func_val, arg_vals) {
            let result = result?;
            let frame = state
                .frames
                .pop()
                .expect("tail call param with empty stack");
            if state.frames.len() == exit_depth {
                state.free_top_registers(frame.register_base);
                return Ok(Some(result));
            }
            let return_reg = frame.return_reg;
            state.set_reg(return_reg, result);
            state.free_top_registers(frame.register_base);
            pop_resolved_extents(state, exit_depth);
            return Ok(None);
        }
    }

    let new_code_id = match closure_code_id {
        Some(id) => CodeObjectId(id),
        // Not callable: resolve_closure produces the standard type error.
        None => resolve_closure(state, func_val)?,
    };
    tail_call_closure_resolved(state, func_val, new_code_id, arg_vals)?;
    Ok(None)
}

/// The tail-position closure branch of `tail_call_value`, shared with the
/// `TailCall` instruction's closure fast path: reuse the current frame's
/// register window for the resolved callee.
fn tail_call_closure_resolved(
    state: &mut VmState,
    func_val: TaggedValue,
    new_code_id: CodeObjectId,
    arg_vals: &[TaggedValue],
) -> Result<(), VmError> {
    let top = state
        .frames
        .last()
        .expect("tail call with empty frame stack");
    if top.code.id == new_code_id {
        let (arity, base) = (top.code.arity, top.register_base);
        return self_tail_call(state, base, arity, func_val, arg_vals);
    }

    let new_code = state.code_object(new_code_id)?;

    check_arity(new_code.arity, arg_vals.len())?;

    // Reuse the current frame's register window.
    // If the new code needs more registers, grow the window.
    let frame = state.frames.last_mut().unwrap();
    let old_base = frame.register_base;
    let old_num = frame.num_regs;
    let new_num = new_code.num_regs;

    if new_num > old_num {
        let extra = new_num - old_num;
        state
            .registers
            .resize(state.registers.len() + extra as usize, TaggedValue::NULL);
        state.frames.last_mut().unwrap().num_regs = new_num;
    }

    store_args_in_window(state, old_base, new_code.arity, arg_vals);

    // Update frame in-place — dispatch fetches instructions from `code`.
    let frame = state.frames.last_mut().unwrap();
    frame.pc = 0;
    frame.closure = closure_heap_index(func_val);
    frame.code = new_code;
    Ok(())
}

/// Tail call whose callee is the current frame's own code object — the
/// `(define (loop …) … (loop …))` shape. No store lookup, no `Rc` churn,
/// no window grow: the window is already sized for this code. The closure
/// field still updates — a different instance of the same code (different
/// captures) tail-calls itself under the same code id.
fn self_tail_call(
    state: &mut VmState,
    base: usize,
    arity: Arity,
    func_val: TaggedValue,
    arg_vals: &[TaggedValue],
) -> Result<(), VmError> {
    check_arity(arity, arg_vals.len())?;
    store_args_in_window(state, base, arity, arg_vals);
    let frame = state
        .frames
        .last_mut()
        .expect("tail call with empty frame stack");
    frame.pc = 0;
    frame.closure = closure_heap_index(func_val);
    Ok(())
}

/// Execute a compile-time-resolved primitive call: the shared back end of
/// `CallPrimitive` and every inline opcode's slow path. Checks the shadow
/// bit — a rebound name deoptimizes to name-lookup dispatch, preserving
/// redefinition semantics — otherwise dispatches through the registry by
/// index. Returns `Some(value)` on a continuation escape, as `call_value`
/// does.
#[allow(clippy::too_many_arguments)]
fn exec_call_primitive(
    state: &mut VmState,
    base: usize,
    func_id: PrimitiveFnId,
    name: &Symbol,
    arg_vals: &[TaggedValue],
    dst: u16,
    exit_depth: usize,
) -> Result<Option<TaggedValue>, VmError> {
    if state.is_primitive_shadowed(func_id.0 as usize) {
        let globals = frame_globals(state);
        let func_val = globals
            .get(name)
            .ok_or_else(|| VmError::UnboundVariable { name: name.clone() })?;
        // Tail-shape detection (PRD P8.2): pass 5 lowers a tail-position
        // primitive site to `<prim-op> dst; Return dst`. When the deopt
        // replaces the primitive with a closure call, that shape must stay a
        // proper tail call (R7RS §3.5), or mutual tail recursion through the
        // rebound site grows the frame stack. The check reads the actual
        // next instruction — it cannot go stale, and a coincidental match is
        // semantically a tail call anyway. On the tail path the callee
        // returns directly to this frame's caller; the `Return` is never
        // executed.
        let frame = state.frames.last().expect("no active frame");
        let is_tail_site = matches!(
            frame.code.instructions.get(frame.pc),
            Some(Instruction::Return { val }) if *val == dst
        );
        if is_tail_site {
            return tail_call_value(state, func_val, arg_vals, exit_depth);
        }
        return call_value(state, func_val, arg_vals, dst, exit_depth);
    }
    exec_call_primitive_direct(state, base, func_id, arg_vals, dst)
}

/// Dispatch to the registry primitive at `func_id` by index, with no
/// shadow check: the `CallPrimitiveDirect` site names the procedure itself,
/// not a global that could since have been rebound, so there is nothing to
/// deoptimize to. Also the tail of [`exec_call_primitive`] once it has
/// established the site is not shadowed.
fn exec_call_primitive_direct(
    state: &mut VmState,
    base: usize,
    func_id: PrimitiveFnId,
    arg_vals: &[TaggedValue],
    dst: u16,
) -> Result<Option<TaggedValue>, VmError> {
    let registry = Rc::clone(&state.primitive_registry);
    let ctx = VmApplyContext {
        state: state as *mut VmState,
    };
    let result = registry
        .apply_by_index(func_id.0 as usize, arg_vals, &ctx)
        .map_err(|e| VmError::Runtime {
            message: e.to_string(),
        })?;
    // No frame check here: a continuation escaping out of a re-entrant
    // primitive is signalled at the boundary (`across_reentry`) and unwinds
    // through the `?` above, so by this point the frames are the ones this
    // call started with. An earlier fix guarded it here instead, which caught
    // only primitives in call position.
    state.set_reg_at(base, dst, result);
    Ok(None)
}

/// A rebind that overwrites a primitive binding with a *different* value
/// must deoptimize every `CallPrimitive` site compiled against it, or
/// already-compiled callers would keep calling the old primitive.
///
/// Every Rust-side writer that can overwrite a global binding must call this
/// *before* replacing it: the `Define`/`StoreGlobal` handlers, and the import
/// machinery in both this file and `backend.rs` (via `import_define` — PRD
/// P8.1). Rebinding a name to the value it already has is a no-op and does
/// not deoptimize, so re-importing a library never pays for this.
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
    let index = state
        .primitive_registry
        .resolve_index_cached(qualified_name, registry_index);
    if let Some(index) = index {
        state.mark_shadowed_primitive(index);
    }
}

/// Import-path define: overwrite `name` in `env`, first marking the shadow
/// bit when the write rebinds a primitive (see `mark_if_shadowing_primitive`).
/// Both import-set resolvers (this file's and `backend.rs`'s) funnel every
/// binding they install through here. Defines into fresh temp/library
/// environments find no existing binding and mark nothing; over-marking is
/// possible only when a library env genuinely rebinds a primitive name, which
/// costs a deopt, never a wrong result.
pub(crate) fn import_define(
    state: &mut VmState,
    env: &Rc<Environment>,
    name: String,
    value: TaggedValue,
) {
    mark_if_shadowing_primitive(state, env, &name, value);
    env.define(name, value);
}

/// Call a primitive procedure (as returned by `primitive_procedure`).
/// Arguments are borrowed: heap-tier handlers read them in place; only the
/// higher-order tier copies them into an owned Vec at the registry boundary.
fn call_primitive_proc(
    state: &mut VmState,
    proc: &Procedure,
    args: &[TaggedValue],
) -> Result<TaggedValue, VmError> {
    let Procedure::Primitive {
        qualified_name,
        registry_index,
        ..
    } = proc
    else {
        return Err(VmError::Runtime {
            message: "call_primitive_proc: not a primitive".into(),
        });
    };
    let ctx = VmApplyContext {
        state: state as *mut VmState,
    };
    state
        .primitive_registry
        .apply_cached(qualified_name, registry_index, args, &ctx)
        .map_err(|e| VmError::Runtime {
            message: e.to_string(),
        })
}
