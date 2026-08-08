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
    /// Side channel for multiple return values (`values` / `call-with-values`).
    pub value_buffer: Vec<TaggedValue>,
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
    /// call returns (contrast `value_buffer`, which is a value channel).
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
            value_buffer: Vec::new(),
            prompt_stack: Vec::new(),
            dynamic_winds: Vec::new(),
            exception_handlers: Vec::new(),
            code_store: Vec::new(),
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

    /// Install all registered primitives into the global environment.
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
            let proc = Rc::new(Procedure::Primitive {
                name,
                arity,
                qualified_name: Rc::from(qualified_name.as_str()),
                registry_index: std::cell::Cell::new(Some(index)),
            });
            let tv = self.heap.borrow_mut().alloc_procedure(proc);
            self.globals.define(name.to_string(), tv);
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
use patina_runtime::library_loader::{ExportSpec, ImportSet};
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

    let desugarer = Desugarer::with_env(lib_env.clone()).with_fs(state.fs.clone());
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

    // Step 3: Collect exports
    let mut library = Library::with_env(parsed.name.clone(), lib_env.clone());
    if let Some(source) = parsed.source {
        library.set_source(source);
    }

    for spec in &parsed.exports {
        match spec {
            ExportSpec::Identifier(name) => {
                if let Some(value) = lib_env.get(name) {
                    library.export_tagged(name.clone(), value);
                } else {
                    return Err(LibraryError::ParseError {
                        file: library
                            .source
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default(),
                        message: format!("Exported identifier '{}' not defined", name),
                    });
                }
            }
            ExportSpec::Rename { internal, external } => {
                if let Some(value) = lib_env.get(internal) {
                    library.export_tagged(external.clone(), value);
                } else {
                    return Err(LibraryError::ParseError {
                        file: library
                            .source
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default(),
                        message: format!("Exported identifier '{}' not defined", internal),
                    });
                }
            }
        }
    }

    Ok(library)
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
                if let Some(value) = temp_env.get(id) {
                    import_define(state, lib_env, id.clone(), value);
                } else {
                    return Err(LibraryError::ParseError {
                        file: String::new(),
                        message: format!("Identifier '{}' not found in import set", id),
                    });
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

    run_loop_until(state, 0)
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

/// The main dispatch loop. Runs until `state.frames.len() == exit_depth`.
///
/// Use `exit_depth = 0` to run until the frame stack is fully empty (top-level).
/// Use `exit_depth = N` to run a nested thunk until it returns to depth N.
fn run_loop_until(state: &mut VmState, exit_depth: usize) -> Result<TaggedValue, VmError> {
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

    loop {
        // GC safe point: all live state is on `VmState`, capture temporaries
        // are dead, buffers are restored, and no heap borrow is outstanding.
        maybe_collect(state, is_outermost);

        match dispatch_one_instruction(state, &mut cur_code, exit_depth) {
            Ok(Some(val)) => return Ok(val),
            Ok(None) => continue,
            Err(e) => {
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
fn attach_source_location(state: &VmState, e: VmError) -> VmError {
    // Don't double-wrap if already has a location.
    if e.source_location().is_some() {
        return e;
    }
    if let Some(frame) = state.frames.last() {
        // PC was already advanced before dispatch, so use pc-1.
        let pc = frame.pc.saturating_sub(1);
        if let Some(loc) = frame.code.source_location(pc) {
            return e.at(loc.clone());
        }
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
                // Not local to this environment: parent-chain lookup.
                None => globals
                    .parent()
                    .and_then(|p| p.get(name))
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
                // Not local to this environment: parent-chain set.
                None => {
                    mark_if_shadowing_primitive(state, &globals, name, val);
                    globals
                        .parent()
                        .ok_or(())
                        .and_then(|p| p.set(name, val).map_err(|_| ()))
                        .map_err(|_| VmError::UnboundVariable { name: name.clone() })?;
                }
            }
        }

        Instruction::Define { ref name, src } => {
            let val = state.reg_at(base, src);
            let globals = frame_globals(state);
            mark_if_shadowing_primitive(state, &globals, name, val);
            globals.define(name.to_string(), val);
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
            let mut arg_vals: Vec<TaggedValue> = args[..args.len() - 1]
                .iter()
                .map(|&r| state.reg_at(base, r))
                .collect();
            let last = state.reg_at(base, *args.last().unwrap());
            let spread = state
                .heap
                .borrow()
                .list_to_vec(last)
                .ok_or_else(|| VmError::Runtime {
                    message: "apply: last argument is not a proper list".into(),
                })?;
            arg_vals.extend(spread);
            if let Some(prim) = primitive_procedure(state, func_val) {
                let result = call_primitive_proc(state, &prim, &arg_vals);
                state.set_reg(dst, result?);
            } else if let Some(result) = try_call_parameter(state, func_val, &arg_vals) {
                state.set_reg(dst, result?);
            } else {
                call_closure(state, func_val, &arg_vals, dst)?;
            }
        }

        Instruction::TailApply { func, ref args } => {
            let func_val = state.reg_at(base, func);
            let mut arg_vals: Vec<TaggedValue> = args[..args.len() - 1]
                .iter()
                .map(|&r| state.reg_at(base, r))
                .collect();
            let last = state.reg_at(base, *args.last().unwrap());
            let spread = state
                .heap
                .borrow()
                .list_to_vec(last)
                .ok_or_else(|| VmError::Runtime {
                    message: "apply: last argument is not a proper list".into(),
                })?;
            arg_vals.extend(spread);

            if let Some(prim) = primitive_procedure(state, func_val) {
                let result = call_primitive_proc(state, &prim, &arg_vals)?;
                let frame = state.frames.pop().expect("TailApply with empty stack");
                if state.frames.len() == exit_depth {
                    state.free_top_registers(frame.register_base);
                    return Ok(Some(result));
                }
                let return_reg = frame.return_reg;
                state.set_reg(return_reg, result);
                state.free_top_registers(frame.register_base);
                return Ok(None);
            }

            if let Some(result) = try_call_parameter(state, func_val, &arg_vals) {
                let result = result?;
                let frame = state
                    .frames
                    .pop()
                    .expect("TailApply param with empty stack");
                if state.frames.len() == exit_depth {
                    state.free_top_registers(frame.register_base);
                    return Ok(Some(result));
                }
                let return_reg = frame.return_reg;
                state.set_reg(return_reg, result);
                state.free_top_registers(frame.register_base);
                return Ok(None);
            }

            let new_code_id = resolve_closure(state, func_val)?;
            tail_call_closure_resolved(state, func_val, new_code_id, &arg_vals)?;
        }

        Instruction::Return { val } => {
            let result = state.reg_at(base, val);
            let frame = state.frames.pop().expect("Return with empty stack");
            if state.frames.len() == exit_depth || state.frames.is_empty() {
                // Reached target depth (or absolute bottom) — this loop is done.
                state.free_top_registers(frame.register_base);
                return Ok(Some(result));
            }
            // Write result into caller's return_reg.
            let return_reg = frame.return_reg;
            state.set_reg(return_reg, result);
            // Free the callee's register window.
            state.free_top_registers(frame.register_base);
            // Pop any PromptFrames whose body just returned normally.
            pop_resolved_prompts(state);
            // Pop exception handlers whose thunk returned normally.
            pop_exception_handlers(state);
            // Pop dynamic-wind records whose body returned, running after-thunks.
            pop_resolved_winds(state)?;
        }

        // ── call-with-values (instruction-level) ─────────────────────────
        Instruction::CallWithValues {
            dst,
            consumer,
            producer_result,
        } => {
            let consumer_val = state.reg_at(base, consumer);
            let produced_vals = if !state.value_buffer.is_empty() {
                std::mem::take(&mut state.value_buffer)
            } else if let Some(vals) = state
                .heap
                .borrow()
                .get_values_as_tagged(state.reg_at(base, producer_result))
            {
                vals
            } else {
                vec![state.reg_at(base, producer_result)]
            };
            if let Some(result) = call_any(state, consumer_val, &produced_vals, dst)? {
                state.set_reg(dst, result);
            }
        }

        Instruction::TailCallWithValues {
            consumer,
            producer_result,
        } => {
            let consumer_val = state.reg_at(base, consumer);
            let produced_vals = if !state.value_buffer.is_empty() {
                std::mem::take(&mut state.value_buffer)
            } else if let Some(vals) = state
                .heap
                .borrow()
                .get_values_as_tagged(state.reg_at(base, producer_result))
            {
                vals
            } else {
                vec![state.reg_at(base, producer_result)]
            };
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
            }
        }

        // ── dynamic-wind (instruction-level) ──────────────────────────
        Instruction::PushWind { before, after } => {
            let before_val = state.reg_at(base, before);
            let after_val = state.reg_at(base, after);
            state.dynamic_winds.push(DynamicWindRecord {
                before: before_val,
                after: after_val,
                // stack_depth = 0: sentinel meaning "managed by instruction
                // sequence, not auto-popped by pop_resolved_winds".
                stack_depth: 0,
            });
        }

        Instruction::PopWind => {
            // Pop the top wind record. The after-thunk is called by a
            // separate Call instruction emitted after this in the codegen.
            if !state.dynamic_winds.is_empty() {
                state.dynamic_winds.pop();
            }
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

            // Run dynamic-wind exit thunks for the unwound portion.
            let exit_winds: Vec<_> = state.dynamic_winds[prompt.dynamic_wind_depth..]
                .iter()
                .rev()
                .cloned()
                .collect();
            for record in exit_winds {
                run_thunk(state, record.after)?;
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
                // Non-composable (call/cc): replace entire stack.
                let cc = state
                    .get_vm_continuation(cont_tv)
                    .ok_or_else(|| VmError::TypeError {
                        message: "InvokeContinuation: not a full continuation".into(),
                    })?;

                // Wind transition (full continuation — force re-enter).
                let target_winds = cc.dynamic_winds.clone();
                run_wind_transition(state, &target_winds, true)?;

                // Restore the full state snapshot.
                state.registers = cc.registers.clone();
                state.frames = cc.frames.clone();
                state.dynamic_winds = cc.dynamic_winds.clone();
                state.prompt_stack = cc.prompt_stack.clone();
                state.exception_handlers = cc.exception_handlers.clone();

                // Deliver value into deliver_reg of the top frame (the
                // dispatch-level `base` is stale here — the whole register
                // file was just replaced).
                if let Some(top) = state.frames.last() {
                    let top_base = top.register_base;
                    state.registers[top_base + cc.deliver_reg as usize] = deliver_val;
                }
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

/// Call any callable value (VmClosure or Primitive). For VmClosures, pushes a
/// frame and returns `Ok(None)` — the caller must continue in the run loop.
/// For primitives, calls immediately and returns `Ok(Some(result))`.
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
    run_loop_until(state, depth_before)
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

/// Run a thunk (0-arg closure) to completion, returning its result.
///
/// Pushes the thunk's frame, runs the execution loop until that frame returns,
/// then returns the result. Used by `dynamic-wind` and wind-transition hooks.
fn run_thunk(state: &mut VmState, thunk: TaggedValue) -> Result<TaggedValue, VmError> {
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
        return Ok(result);
    }
    // VM closure was pushed; run until it returns.
    run_loop_until(state, depth_before)
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
    _is_tail: bool,
) -> Result<Option<TaggedValue>, VmError> {
    match ctrl {
        VmControlPrimitive::DynamicWind => {
            if args.len() != 3 {
                return Err(VmError::ArityMismatch {
                    expected: "3".into(),
                    got: args.len(),
                });
            }
            let before_result = run_thunk(state, args[0])?;
            if state.frames.is_empty() {
                return Ok(Some(before_result));
            }
            let wind_depth = state.dynamic_winds.len();
            state.dynamic_winds.push(DynamicWindRecord {
                before: args[0],
                after: args[2],
                stack_depth: state.frames.len(),
            });
            let result = run_thunk(state, args[1])?;
            // If a continuation escape emptied the frame stack, the result
            // was already delivered — don't try to write to a dead frame.
            if state.frames.is_empty() {
                return Ok(Some(result));
            }
            // If the body aborted, the abort already ran exit thunks and
            // truncated dynamic_winds. Only do cleanup if our record is still there.
            if state.dynamic_winds.len() > wind_depth {
                state.dynamic_winds.pop();
                run_thunk(state, args[2])?;
            }
            if state.frames.is_empty() {
                return Ok(Some(result));
            }
            state.set_reg(dst, result);
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

            let exit_winds: Vec<_> = state.dynamic_winds[prompt.dynamic_wind_depth..]
                .iter()
                .rev()
                .cloned()
                .collect();
            for record in exit_winds {
                run_thunk(state, record.after)?;
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
            // (values v1 v2 ...) — return multiple values via value_buffer
            // Also allocate a heap Values object so the display layer can
            // detect and show all values (matches tree-walker behaviour).
            state.value_buffer.clear();
            state.value_buffer.extend_from_slice(args);
            if args.len() == 1 {
                state.set_reg(dst, args[0]);
            } else {
                let vals_tv = state.heap.borrow_mut().alloc_values(args.to_vec());
                state.set_reg(dst, vals_tv);
            }
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
            // Clear any stale value_buffer before running the producer,
            // so we only see values produced by this thunk.
            state.value_buffer.clear();
            // Run producer (0 args), collect multiple values from value_buffer
            let primary = run_thunk(state, producer)?;
            let produced_vals = if !state.value_buffer.is_empty() {
                std::mem::take(&mut state.value_buffer)
            } else if let Some(vals) = state.heap.borrow().get_values_as_tagged(primary) {
                // Primitives like exact-integer-sqrt return a #<values> heap
                // object directly (without going through the VM's `values`
                // control intercept), so unpack it here.
                vals
            } else {
                vec![primary]
            };
            // Consumer may be a primitive (e.g. `list`) or a VM closure.
            if let Some(result) = call_any(state, consumer, &produced_vals, dst)? {
                state.set_reg(dst, result);
            }
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

            // Verify both are callable
            {
                let heap = state.heap.borrow();
                if !heap.is_procedure(handler_proc) {
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

            // Push exception handler (captures current dynamic winds and frame depth).
            // The handler is popped when the thunk returns (via pop_exception_handlers)
            // or when raise invokes it.
            state.exception_handlers.push(ExceptionHandler {
                handler: handler_proc,
                dynamic_winds: state.dynamic_winds.clone(),
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
            // First arg must be a string (the message)
            let message = state
                .heap
                .borrow()
                .get_string_contents(args[0])
                .ok_or_else(|| VmError::TypeError {
                    message: "error: first argument must be a string".into(),
                })?;
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
/// expected to escape via `call/cc` continuation or similar; if it returns
/// normally, the Return instruction path detects the non-continuable case.
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
        // Unwind dynamic-wind after-thunks back to the handler's installation point
        let target_wind_depth = handler_entry.dynamic_winds.len();
        let exit_winds: Vec<_> = state.dynamic_winds[target_wind_depth..]
            .iter()
            .rev()
            .cloned()
            .collect();
        for record in exit_winds {
            // Best-effort: ignore errors from wind thunks during exception unwinding
            let _ = run_thunk(state, record.after);
        }
        state.dynamic_winds.truncate(target_wind_depth);

        if continuable {
            // Continuable: run handler synchronously, return its value
            let handler_result = match call_any(state, handler_entry.handler, &[exception], dst)? {
                Some(result) => result,
                None => {
                    // Handler is a VM closure — run it
                    let depth_before = state.frames.len() - 1;
                    run_loop_until(state, depth_before)?
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
                    // The handler is expected to escape (via call/cc or abort).
                    // If it returns normally via Return instruction, that path
                    // needs to handle the non-continuable case.
                    Ok(())
                }
            }
        }
    } else {
        // No handler — format and propagate as Rust error
        use patina_primitives::primitives::io::datum_writer::format_display_tagged;
        let display = format_display_tagged(exception, &state.heap);
        Err(VmError::SchemeException {
            message: if continuable {
                format!("unhandled continuable exception: {}", display)
            } else {
                format!("unhandled exception: {}", display)
            },
        })
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
        VmError::StackOverflow | VmError::Compile(_) => false,
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
        VmError::StackOverflow | VmError::Compile(_) => (ExceptionKind::Error, err.to_string()),
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

/// Pop dynamic-wind records whose body has returned (stack shrank below their
/// depth), running each after-thunk.  This handles the case where a continuation
/// invocation resumes inside a dynamic-wind body and the body then returns
/// normally — the Rust call-stack cleanup in `handle_control_primitive` is no
/// longer on the stack, so we must do the cleanup here.
fn pop_resolved_winds(state: &mut VmState) -> Result<(), VmError> {
    while let Some(dw) = state.dynamic_winds.last() {
        if dw.stack_depth >= state.frames.len() {
            let after = dw.after;
            state.dynamic_winds.pop();
            run_thunk(state, after)?;
        } else {
            break;
        }
    }
    Ok(())
}

/// Run wind-transition thunks when moving from current dynamic-wind state
/// to `target_winds`.
fn run_wind_transition(
    state: &mut VmState,
    target_winds: &[DynamicWindRecord],
    force_reenter: bool,
) -> Result<(), VmError> {
    // Find the common prefix (matched by `before` thunk identity).
    // For full call/cc continuations, force_reenter=true means we must exit
    // all current winds and re-enter all target winds (R7RS semantics).
    let common = if force_reenter {
        0
    } else {
        state
            .dynamic_winds
            .iter()
            .zip(target_winds.iter())
            .take_while(|(a, b)| a.before == b.before)
            .count()
    };

    // Run 'after' thunks for records being exited (innermost first).
    let exit_winds: Vec<_> = state.dynamic_winds[common..]
        .iter()
        .rev()
        .cloned()
        .collect();
    for record in exit_winds {
        run_thunk(state, record.after)?;
    }

    // Run 'before' thunks for records being entered (outermost first).
    let enter_winds: Vec<_> = target_winds[common..].to_vec();
    for record in enter_winds {
        run_thunk(state, record.before)?;
    }

    Ok(())
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
        run_apply_proc(state, proc, &args)
            .map_err(|e| patina_primitives::EvalError::InternalError(e.to_string()))
    }

    fn eval_expr(
        &self,
        expr: TaggedValue,
        env: &Rc<patina_core::environment::Environment>,
    ) -> Result<TaggedValue, patina_primitives::EvalError> {
        let state = unsafe { &mut *self.state };
        vm_eval_expr(state, expr, env)
            .map_err(|e| patina_primitives::EvalError::InternalError(e.to_string()))
    }

    fn load_scheme_library(
        &self,
        name: &[String],
    ) -> Result<Rc<patina_core::library::Library>, patina_primitives::EvalError> {
        let state = unsafe { &mut *self.state };
        vm_load_library(state, name)
            .map(Rc::new)
            .map_err(|e| patina_primitives::EvalError::InternalError(e.to_string()))
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
    WithExceptionHandler,
    Raise,
    RaiseContinuable,
    Error,
}

/// The single source of truth for which qualified names the VM intercepts
/// for control flow. `resolve_primitive_calls` must never emit
/// `CallPrimitive` for any of these (its exclusion is cross-checked against
/// this table by `excluded_covers_every_intercepted_primitive` in
/// `compiler/primitive_calls.rs`) — a directly dispatched registry handler
/// would bypass the VM's continuation/exception cooperation.
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
) -> Result<bool, VmError> {
    let deliver_val = args.first().copied().unwrap_or(TaggedValue::UNSPECIFIED);

    // Full (call/cc) continuation?
    if let Some(cc) = state.get_vm_continuation(func_val) {
        let target_winds = cc.dynamic_winds.clone();
        run_wind_transition(state, &target_winds, true)?;
        state.registers = cc.registers.clone();
        state.frames = cc.frames.clone();
        state.dynamic_winds = cc.dynamic_winds.clone();
        state.prompt_stack = cc.prompt_stack.clone();
        state.exception_handlers = cc.exception_handlers.clone();
        // Deliver value into deliver_reg of the top frame (where call/cc's
        // result was expected).
        if let Some(top) = state.frames.last() {
            let base = top.register_base;
            state.registers[base + cc.deliver_reg as usize] = deliver_val;
        }
        // When invoked with multiple values, populate value_buffer so
        // call-with-values can unpack them.
        state.value_buffer.clear();
        if args.len() > 1 {
            state.value_buffer.extend_from_slice(args);
        }
        return Ok(true);
    }

    // Delimited (composable) continuation?
    if let Some(dc) = state.get_vm_delimited_continuation(func_val) {
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
        // Populate value_buffer for multi-value delivery
        state.value_buffer.clear();
        if args.len() > 1 {
            state.value_buffer.extend_from_slice(args);
        }
        return Ok(true);
    }

    Ok(false)
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
            return handle_control_primitive(state, ctrl, arg_vals, dst, false);
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
        if try_invoke_continuation(state, func_val, arg_vals)? {
            // Continuation was invoked — stack has been replaced/appended.
            // Safety net: if frames dropped to or below exit_depth, the
            // continuation escaped past a synchronous run_thunk boundary
            // (e.g. dynamic-wind body, with-exception-handler thunk, or
            // indirect call-with-values). Return the primary value so the
            // enclosing run_loop_until exits correctly.
            if state.frames.len() <= exit_depth {
                let primary = arg_vals
                    .first()
                    .copied()
                    .unwrap_or(TaggedValue::UNSPECIFIED);
                return Ok(Some(primary));
            }
            return Ok(None);
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
            // Now at depth N-1. Handle with dst = return_reg (slot in frame
            // N-2). (At exit depth, return_reg is still the right dst — not
            // 0, which could clobber live registers like MutableCell
            // pointers.)
            if let Some(escaped) =
                handle_control_primitive(state, ctrl, arg_vals, return_reg, false)?
            {
                return Ok(Some(escaped));
            }
            // The control primitive has completed. If it wrote its result and
            // returned immediately (no new frames pushed past exit_depth), the
            // enclosing loop is done; otherwise let it drive the new frames.
            if state.frames.len() == exit_depth {
                let result = state.reg(return_reg);
                return Ok(Some(result));
            }
            return Ok(None);
        }

        // Continuation invocation in tail position.
        if try_invoke_continuation(state, func_val, arg_vals)? {
            // Safety net: if frames dropped to or below exit_depth, the
            // continuation escaped past a synchronous run_thunk boundary.
            if state.frames.len() <= exit_depth {
                let primary = arg_vals
                    .first()
                    .copied()
                    .unwrap_or(TaggedValue::UNSPECIFIED);
                return Ok(Some(primary));
            }
            return Ok(None);
        }

        // Primitives in tail position: call them, write result to current
        // frame's return_reg, then simulate a Return.
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
            return Ok(None);
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
    let registry = Rc::clone(&state.primitive_registry);
    let ctx = VmApplyContext {
        state: state as *mut VmState,
    };
    let result = registry
        .apply_by_index(func_id.0 as usize, arg_vals, &ctx)
        .map_err(|e| VmError::Runtime {
            message: e.to_string(),
        })?;
    // Primitives are frame-neutral on the `Ok` path (a re-entrant call runs
    // its nested frames to completion), so the hoisted base is still the
    // top frame's — the write the plain `set_reg` here already assumed.
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
