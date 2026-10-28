//! Primitive procedure registry for backend-agnostic primitives

use crate::apply_context::ApplyContext;
use patina_core::{CollectKind, Cx, Value};
use patina_runtime::environment::Environment;
use patina_runtime::{Arity, EvalError, TaggedValue};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

/// Handler for a primitive that takes the context (#706; `PRD/GC_PRD.md`
/// §11.3's `Prim`): it reads and allocates through the [`Cx`], and its
/// arguments and value are values of it, so the borrow checker refuses one
/// held where a collection could free it. It cannot call back into the
/// evaluator: it has no [`ApplyContext`]. The machine makes the context for
/// the call ([`ApplyContext::call_cx`]). Every primitive that calls nothing
/// back is one: the kind that took the heap went with the last of its files.
/// Its arguments are borrowed, since it never re-enters the evaluator.
///
/// The cases below pin the brand at the handler (#709). A primitive's
/// arguments are values of its call's context, so it cannot keep one past the
/// call. What it keeps is plain data:
///
/// ```
/// use std::cell::Cell;
/// use patina_core::{Cx, Value};
/// use patina_primitives::{CxHandler, EvalError};
///
/// thread_local! {
///     static LAST: Cell<Option<i64>> = const { Cell::new(None) };
/// }
///
/// fn remember<'gc>(_: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
///     LAST.with(|last| last.set(args[0].as_fixnum()));
///     Ok(args[0])
/// }
/// let _: CxHandler = remember;
/// ```
///
/// Its argument kept in a thread-local, an error with no code to pin:
///
/// ```compile_fail
/// use std::cell::Cell;
/// use patina_core::{Cx, Value};
/// use patina_primitives::{CxHandler, EvalError};
///
/// thread_local! {
///     static LAST: Cell<Option<Value<'static>>> = const { Cell::new(None) };
/// }
///
/// fn remember<'gc>(_: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
///     LAST.with(|last| last.set(Some(args[0])));
///     Ok(args[0])
/// }
/// let _: CxHandler = remember;
/// ```
///
/// Nor can it reach the heap, through which it could collect. Its context
/// reads, allocates and stores, and leads to no heap:
///
/// ```
/// use patina_core::{Cx, Value};
/// use patina_primitives::{CxHandler, EvalError};
///
/// fn pair<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
///     Ok(cx.alloc_pair(args[0], args[1]))
/// }
/// let _: CxHandler = pair;
/// ```
///
/// Asking it for the heap:
///
/// ```compile_fail,E0599
/// use patina_core::{Cx, Value};
/// use patina_primitives::{CxHandler, EvalError};
///
/// fn pair<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
///     let _heap = cx.heap();
///     Ok(cx.alloc_pair(args[0], args[1]))
/// }
/// let _: CxHandler = pair;
/// ```
pub type CxHandler = for<'gc> fn(&mut Cx<'gc>, &[Value<'gc>]) -> Result<Value<'gc>, EvalError>;

/// Handler for a higher-order primitive (9 of them): one that calls back
/// into the evaluator from Rust ([`ApplyContext::apply_in`]) or loads a
/// library. It takes the context (#706), and the machine's [`ApplyContext`]
/// beside it to call back through. Its values stay live across a call back,
/// which runs on a nested loop that cannot collect. The registry hands it a
/// copy of its arguments: re-entering the VM may reallocate the register
/// file, which would invalidate a borrowed argument slice.
pub type HigherOrderHandler =
    for<'gc> fn(&mut Cx<'gc>, &dyn ApplyContext, &[Value<'gc>]) -> Result<Value<'gc>, EvalError>;

/// What a resumable primitive asks of the machine running it.
///
/// A primitive that has to call a procedure the program gave it, and then
/// carry on with the result, cannot do the call itself from Rust: a
/// continuation captured inside the procedure cannot carry the Rust frame, so
/// re-entered after the primitive returned it has nothing to return into
/// (#471, #476–#478). So it hands the call to the machine instead — the VM
/// runs it in a stub frame, the tree-walker as a continuation — and is
/// resumed with the result, with the state it asked to keep. Nothing of the
/// primitive is on the Rust stack while the procedure runs.
///
/// A collection is the same shape ([`Step::Collect`], #639): a primitive
/// cannot collect from Rust, where the collector sees neither its frame nor
/// its caller's, so it asks the machine, which suspends the call where its
/// roots see everything and collects there.
///
/// A primitive builds its step in its context, of the context's values
/// (#706); the machine takes it as words as the context closes
/// ([`Step::tagged`], [`TaggedStep`]), and keeps them where its collector
/// sees them.
pub enum Step<'gc> {
    /// Finished, with this value.
    Done(Value<'gc>),
    /// Call `callee` with `args`; when it returns `v`, resume the primitive
    /// with `state` and `v`. `state` is whatever the rest of the primitive
    /// needs — a value the machine keeps where the collector sees it, and
    /// which a continuation captured in the call carries along.
    Call {
        callee: Value<'gc>,
        args: CallArgs<'gc>,
        state: Value<'gc>,
    },
    /// Evaluate the datum `expr` in `env`, as `eval` does, and resume the
    /// primitive with `state` and its value. The machine expands and runs it
    /// as its own code — the VM a closure over the compiled form, the
    /// tree-walker a CPS expression on the running trampoline — so a
    /// continuation captured in it carries the rest of the primitive, as one
    /// captured in a [`Step::Call`] does (#477).
    Eval {
        expr: Value<'gc>,
        env: Rc<Environment>,
        state: Value<'gc>,
    },
    /// Run a collection of `kind` at this call, then resume the primitive
    /// with `state` and whether the collection ran (`#t`, or `#f` where it
    /// was posted instead). `(gc)` asks for one and is done when resumed;
    /// an open that ran out of descriptors asks for one and opens again
    /// when resumed (#607, `primitives/io/file.rs`), its file name kept in
    /// `state`, and `load` reads its file again the same way.
    ///
    /// The machine suspends the caller at the call's return pc, clears the
    /// register the call's value goes to (the VM's liveness maps keep a
    /// call's destination from the call on), and collects before the
    /// caller's next instruction: the VM in `resume_stub`'s frame, which
    /// holds `state`; the tree-walker at the top of its trampoline, under the
    /// primitive's `ResumePrimitive` continuation. Both go through
    /// `GcController::collect_at_call`, which collects only where a safe
    /// point may, in every GC mode; where collection is deferred (a nested
    /// loop, a library body loaded under a holder's guard) it posts the
    /// collection for the next safe point that may collect and counts it in
    /// `(gc-stats)`'s `deferred-collections`.
    ///
    /// To be called again, a primitive keeps what the retry needs in `state`
    /// and repeats its work in its resume half: the machine keeps none of the
    /// call's arguments (the VM clears the stub frame's argument registers,
    /// and the tree-walker's `ResumePrimitive` keeps only `state`). For
    /// #607's opens that is the one file name, which costs no allocation
    /// (`(patina internal io)`'s `call-with-*-file` keep their procedure
    /// too, in a pair, and `load` its environment). Allocating before the step can cross the collection
    /// trigger, and then the safe point before the collection runs one of
    /// its own first, so the call pays for two. Where the answer is `#f` —
    /// deferred, or no machine at all (`run_synchronously`, which posts it)
    /// — nothing was freed, and a retry fails as the first attempt did: the
    /// opens and `load` make it anyway, for the error it raises.
    Collect {
        kind: CollectKind,
        state: Value<'gc>,
    },
}

impl Step<'_> {
    /// This step as words, for the machine that takes it.
    pub fn tagged(self) -> TaggedStep {
        match self {
            Step::Done(value) => TaggedStep::Done(value.tagged()),
            Step::Call {
                callee,
                args,
                state,
            } => TaggedStep::Call {
                callee: callee.tagged(),
                args: args.iter().map(|arg| arg.tagged()).collect(),
                state: state.tagged(),
            },
            Step::Eval { expr, env, state } => TaggedStep::Eval {
                expr: expr.tagged(),
                env,
                state: state.tagged(),
            },
            Step::Collect { kind, state } => TaggedStep::Collect {
                kind,
                state: state.tagged(),
            },
        }
    }
}

/// One half of a resumable primitive, or a higher-order primitive, as the
/// registry hands it to a machine ([`ApplyContext::step_cx`]): run in a
/// context over its words, it answers its step as words.
pub type StepFn<'a> =
    &'a mut dyn for<'gc> FnMut(&mut Cx<'gc>, &[Value<'gc>]) -> Result<TaggedStep, EvalError>;

/// A [`Step`] as the machine takes it: its values as words, out of the
/// context the primitive ran in. The machine keeps them where its collector
/// sees them — the VM in `resume_stub`'s frame, the tree-walker in a
/// `ResumePrimitive` continuation — and each variant means what [`Step`]'s
/// does.
pub enum TaggedStep {
    Done(TaggedValue),
    Call {
        callee: TaggedValue,
        args: TaggedCallArgs,
        state: TaggedValue,
    },
    Eval {
        expr: TaggedValue,
        env: Rc<Environment>,
        state: TaggedValue,
    },
    Collect {
        kind: CollectKind,
        state: TaggedValue,
    },
}

/// A resume that is done with what the call returned: the second half of a
/// primitive that makes one call and answers its value.
pub fn done_with_result<'gc>(
    _cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    _state: Value<'gc>,
    result: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    Ok(Step::Done(result))
}

/// The arguments of a [`Step::Call`]: held inline up to three, which covers
/// every call a resumable primitive makes, so asking for one allocates
/// nothing.
pub type CallArgs<'gc> = smallvec::SmallVec<[Value<'gc>; 3]>;

/// [`CallArgs`] as words, in a [`TaggedStep`].
pub type TaggedCallArgs = smallvec::SmallVec<[TaggedValue; 3]>;

/// A resumable primitive's first half: its arguments in, a [`Step`] out. It
/// takes the context (#706), and the machine's [`ApplyContext`] beside it,
/// which `load` asks for the interaction environment.
pub type ResumableStart =
    for<'gc> fn(&mut Cx<'gc>, &dyn ApplyContext, &[Value<'gc>]) -> Result<Step<'gc>, EvalError>;

/// A resumable primitive's continuation: the `state` its last [`Step::Call`]
/// asked to keep, and what the call returned.
pub type ResumableResume = for<'gc> fn(
    &mut Cx<'gc>,
    &dyn ApplyContext,
    Value<'gc>,
    Value<'gc>,
) -> Result<Step<'gc>, EvalError>;

/// Handler variant for a primitive
pub enum PrimitiveHandler {
    /// Takes the context (#706): every primitive that calls nothing back.
    Cx(CxHandler),
    /// Higher-order: takes the context, and calls back through the
    /// `ApplyContext` beside it ([`HigherOrderHandler`]).
    HigherOrder(HigherOrderHandler),
    /// Calls a procedure and carries on with the result through the machine,
    /// not from Rust ([`Step`]).
    Resumable {
        start: ResumableStart,
        resume: ResumableResume,
    },
}

/// Run a resumable primitive's calls from Rust, for a caller that has no
/// machine to hand them to. What every call site did before [`Step`]
/// existed, and still right wherever no continuation can be captured in the
/// call; the machines take the other route.
fn run_synchronously(
    ctx: &dyn ApplyContext,
    mut step: TaggedStep,
    resume: ResumableResume,
) -> Result<TaggedValue, EvalError> {
    loop {
        match step {
            TaggedStep::Done(value) => return Ok(value),
            TaggedStep::Call {
                callee,
                args,
                state,
            } => {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "holds `state`, the value the primitive kept (a converter, a \
                              parameter, `load`'s remaining forms), across the call, and the \
                              caller holds the primitive's arguments; `resume` reads `state` \
                              afterwards. Both machines start a resumable primitive themselves \
                              (the VM's `start_resumable`, the tree-walker's \
                              `apply_other_primitive`), so this runs only for a context with no \
                              machine: the tree-walker's detached `ApplyContext for Evaluator`, \
                              whose `apply_proc` holds `GcDeferGuard::holding`, so the call \
                              cannot collect; an embedder's own context must defer likewise \
                              (docs/GC_DESIGN.md §7)"
                )]
                let result = ctx.apply_proc(callee, args.into_vec())?;
                step = resume_in(ctx, resume, state, result)?;
            }
            TaggedStep::Eval { expr, env, state } => {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "holds `state` and the environment `env` across the evaluation, \
                              as the `Step::Call` arm above holds `state`, and deferred for the \
                              same reason: no machine reaches this loop, and the detached \
                              context's `eval_expr` holds `GcDeferGuard::holding`"
                )]
                let result = ctx.eval_expr(expr, &env)?;
                step = resume_in(ctx, resume, state, result)?;
            }
            TaggedStep::Collect { kind: _, state } => {
                // No machine, so no roots to collect with: posted for the
                // next safe point that may collect, as where a machine's
                // collection is deferred (`Cx::defer_collection`), from the
                // context the primitive resumes in.
                step = ctx.step_cx(&[state, TaggedValue::FALSE], &mut |cx, words| {
                    cx.defer_collection();
                    resume(cx, ctx, words[0], words[1]).map(Step::tagged)
                })?;
            }
        }
    }
}

/// Start a resumable primitive on `args`, in a context the machine makes for
/// it ([`ApplyContext::step_cx`]): its step, as words.
fn start_in(
    ctx: &dyn ApplyContext,
    start: ResumableStart,
    args: &[TaggedValue],
) -> Result<TaggedStep, EvalError> {
    ctx.step_cx(args, &mut |cx, args| start(cx, ctx, args).map(Step::tagged))
}

/// Call a higher-order primitive on `args`, in a context the machine makes
/// for it ([`ApplyContext::step_cx`]), as the step that is done with its
/// value. `args` is the registry's own copy, which a call back cannot move.
fn call_higher_order(
    ctx: &dyn ApplyContext,
    handler: HigherOrderHandler,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let step = ctx.step_cx(&args, &mut |cx, args| {
        handler(cx, ctx, args).map(|value| TaggedStep::Done(value.tagged()))
    })?;
    match step {
        TaggedStep::Done(value) => Ok(value),
        _ => Err(EvalError::InternalError(
            "a higher-order primitive answered a step".into(),
        )),
    }
}

/// Resume a resumable primitive with the `state` it kept and the `result` of
/// what it asked for, in a context the machine makes for it.
fn resume_in(
    ctx: &dyn ApplyContext,
    resume: ResumableResume,
    state: TaggedValue,
    result: TaggedValue,
) -> Result<TaggedStep, EvalError> {
    ctx.step_cx(&[state, result], &mut |cx, words| {
        resume(cx, ctx, words[0], words[1]).map(Step::tagged)
    })
}

/// Handler-side exact-arity check: the standard `WrongArity` error every
/// hand-rolled handler produces. (Registry dispatch also checks the declared
/// `Arity` up front via `PrimitiveFn::check_arity`; handlers keep their own
/// check so they stay correct when called directly.)
pub(crate) fn expect_arity<T>(args: &[T], n: usize) -> Result<(), EvalError> {
    if args.len() != n {
        return Err(EvalError::WrongArity {
            expected: n.to_string(),
            actual: args.len(),
        });
    }
    Ok(())
}

// C11 (`PRD/GC_PRD.md` §18.6): the registry is `Send + Sync`, entries of fn
// pointers and names, so under `threaded` one registry can serve every
// carrier. What holds one today does not need it to be: the VM's `Rc`, and the
// index each procedure caches in a `Cell`.
const _: () = {
    const fn send_and_sync<T: Send + Sync>() {}
    send_and_sync::<PrimitiveRegistry>();
};

/// A machine a host registers its primitives in (#710): both backends are
/// one, and `Interpreter::register_primitive` asks for one.
pub trait HostPrimitives {
    /// Add `primitive` to the machine's registry, and export it from the
    /// host library `library`, whose name `primitive.library` is, joined with
    /// dots: a program reaches it by importing that library. Between
    /// evaluations only.
    fn register_host_primitive(
        &mut self,
        library: &[String],
        primitive: PrimitiveFn,
    ) -> Result<(), patina_runtime::RegisterError>;
}

/// The helper class a primitive declares for the JIT (`PRD/GC_PRD.md`
/// §11.5, decision 13): whether it may block. No machine reads it yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimClass {
    /// Never blocks. Its context can neither collect nor call Scheme, so a
    /// `Leaf` may still allocate: the JIT treats every host primitive as
    /// allocating, whatever its class.
    Leaf,
    /// May block, or calls back into the program: a resumable primitive
    /// always is one.
    Transfer,
}

pub struct PrimitiveFn {
    pub library: &'static str,
    pub name: &'static str,
    pub arity: Arity,
    #[allow(dead_code)]
    pub help: &'static str,
    pub handler: PrimitiveHandler,
    /// The class a host declared at registration (#710); a built-in
    /// primitive's follows from its handler ([`PrimitiveFn::class`]).
    pub declared_class: Option<PrimClass>,
}

impl PrimitiveFn {
    /// A primitive that takes the context ([`CxHandler`], #706).
    pub fn new_cx(
        library: &'static str,
        name: &'static str,
        arity: Arity,
        help: &'static str,
        handler: CxHandler,
    ) -> Self {
        PrimitiveFn {
            library,
            name,
            arity,
            help,
            handler: PrimitiveHandler::Cx(handler),
            declared_class: None,
        }
    }

    pub fn new_higher_order(
        library: &'static str,
        name: &'static str,
        arity: Arity,
        help: &'static str,
        handler: HigherOrderHandler,
    ) -> Self {
        PrimitiveFn {
            library,
            name,
            arity,
            help,
            handler: PrimitiveHandler::HigherOrder(handler),
            declared_class: None,
        }
    }

    /// A primitive that calls a procedure by asking the machine to (see
    /// [`Step`]): `start` runs on the arguments, and `resume` each time a call
    /// it asked for returns.
    pub fn new_resumable(
        library: &'static str,
        name: &'static str,
        arity: Arity,
        help: &'static str,
        start: ResumableStart,
        resume: ResumableResume,
    ) -> Self {
        PrimitiveFn {
            library,
            name,
            arity,
            help,
            handler: PrimitiveHandler::Resumable { start, resume },
            declared_class: None,
        }
    }

    /// This primitive with the class a host declared (#710).
    pub fn with_class(mut self, class: PrimClass) -> Self {
        self.declared_class = Some(class);
        self
    }

    /// The primitive's helper class: the one a host declared, or for a
    /// built-in primitive, until stage 6, what its handler says. One that
    /// takes the context cannot run Scheme or collect, so it is a `Leaf`;
    /// one that calls back is a `Transfer`.
    pub fn class(&self) -> PrimClass {
        self.declared_class.unwrap_or(match self.handler {
            PrimitiveHandler::Cx(_) => PrimClass::Leaf,
            PrimitiveHandler::HigherOrder(_) | PrimitiveHandler::Resumable { .. } => {
                PrimClass::Transfer
            }
        })
    }

    pub fn qualified_name(&self) -> String {
        format!("{}/{}", self.library, self.name)
    }

    pub fn check_arity(&self, arg_count: usize) -> Result<(), EvalError> {
        match self.arity {
            Arity::Exact(n) => {
                if arg_count != n {
                    return Err(EvalError::InvalidSyntax(format!(
                        "{} expects exactly {} arguments, got {}",
                        self.name, n, arg_count
                    )));
                }
            }
            Arity::Min(n) => {
                if arg_count < n {
                    return Err(EvalError::InvalidSyntax(format!(
                        "{} expects at least {} arguments, got {}",
                        self.name, n, arg_count
                    )));
                }
            }
            Arity::Range(min, max) => {
                if arg_count < min || arg_count > max {
                    return Err(EvalError::InvalidSyntax(format!(
                        "{} expects {}-{} arguments, got {}",
                        self.name, min, max, arg_count
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Primitive storage is index-addressable: entries live in a `Vec` and the
/// name maps only translate names to indices. Hot call paths cache the index
/// (see `Procedure::Primitive::registry_index`) so they dispatch with a
/// bounds-checked array access instead of hashing the qualified name on
/// every call. Indices are stable: registration only appends or replaces in
/// place, never removes or reorders.
pub struct PrimitiveRegistry {
    entries: Vec<PrimitiveFn>,
    by_qualified: HashMap<String, usize>,
    name_index: HashMap<&'static str, usize>,
}

impl PrimitiveRegistry {
    pub fn new() -> Self {
        PrimitiveRegistry {
            entries: Vec::new(),
            by_qualified: HashMap::new(),
            name_index: HashMap::new(),
        }
    }

    /// Register `primitive`, answering its index: a new one is appended,
    /// and one of a name already registered replaces that entry in place.
    pub fn register(&mut self, primitive: PrimitiveFn) -> usize {
        let qualified_name = primitive.qualified_name();
        let index = match self.by_qualified.get(&qualified_name) {
            Some(&existing) => {
                self.entries[existing] = primitive;
                existing
            }
            None => {
                self.entries.push(primitive);
                let index = self.entries.len() - 1;
                self.by_qualified.insert(qualified_name, index);
                index
            }
        };
        self.name_index
            .entry(self.entries[index].name)
            .or_insert(index);
        index
    }

    pub fn get(&self, qualified_name: &str) -> Option<&PrimitiveFn> {
        self.by_qualified
            .get(qualified_name)
            .map(|&i| &self.entries[i])
    }

    pub fn get_by_name(&self, name: &str) -> Option<&PrimitiveFn> {
        self.name_index.get(name).map(|&i| &self.entries[i])
    }

    pub fn get_from_library(&self, library: &str, name: &str) -> Option<&PrimitiveFn> {
        let qualified = format!("{}/{}", library, name);
        self.get(&qualified)
    }

    pub fn get_library_primitives(&self, library: &str) -> impl Iterator<Item = &PrimitiveFn> {
        self.entries.iter().filter(move |pf| pf.library == library)
    }

    pub fn contains(&self, qualified_name: &str) -> bool {
        self.by_qualified.contains_key(qualified_name)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn list_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.by_qualified.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    pub fn primitives(&self) -> impl Iterator<Item = &PrimitiveFn> {
        self.entries.iter()
    }

    /// Iterate primitives together with their stable registry index
    pub fn primitives_indexed(&self) -> impl Iterator<Item = (usize, &PrimitiveFn)> {
        self.entries.iter().enumerate()
    }

    /// Look up the stable index for a qualified name, falling back to the
    /// short name (the part after '/') like `apply_tagged` does
    pub fn resolve_index(&self, qualified_name: &str) -> Option<usize> {
        if let Some(&i) = self.by_qualified.get(qualified_name) {
            return Some(i);
        }
        let name = match qualified_name.split_once('/') {
            Some((_, n)) => n,
            None => qualified_name,
        };
        self.name_index.get(name).copied()
    }

    /// Get a primitive by its stable registry index
    pub fn get_by_index(&self, index: usize) -> Option<&PrimitiveFn> {
        self.entries.get(index)
    }

    /// Resolve a primitive's index through its cache slot (the one stored on
    /// `Procedure::Primitive`), filling the slot on first use so the
    /// name-hash fallback runs at most once per procedure. Every dispatch or
    /// lookup site that holds a `Procedure::Primitive` should go through
    /// this rather than re-deriving the get-or-resolve dance.
    pub fn resolve_index_cached(
        &self,
        qualified_name: &str,
        index_cache: &Cell<Option<usize>>,
    ) -> Option<usize> {
        match index_cache.get() {
            Some(i) => Some(i),
            None => {
                let i = self.resolve_index(qualified_name)?;
                index_cache.set(Some(i));
                Some(i)
            }
        }
    }

    fn entry(&self, index: usize) -> Result<&PrimitiveFn, EvalError> {
        self.entries
            .get(index)
            .ok_or_else(|| EvalError::InternalError(format!("invalid primitive index {}", index)))
    }

    /// Apply a primitive by its stable registry index — the hot dispatch
    /// path, no name hashing involved. Borrows the arguments; the
    /// higher-order tier copies them into an owned Vec at this boundary.
    pub fn apply_by_index(
        &self,
        index: usize,
        args: &[TaggedValue],
        ctx: &dyn ApplyContext,
    ) -> Result<TaggedValue, EvalError> {
        let primitive = self.entry(index)?;
        primitive.check_arity(args.len())?;
        match &primitive.handler {
            PrimitiveHandler::Cx(h) => ctx.call_cx(*h, args),
            PrimitiveHandler::HigherOrder(h) => call_higher_order(ctx, *h, args.to_vec()),
            #[expect(
                clippy::disallowed_methods,
                reason = "holds `args`, the primitive's arguments, across its calls. A machine \
                          never comes here for a resumable primitive (the VM's \
                          `start_resumable`, the tree-walker's `apply_other_primitive` start it \
                          themselves), so `ctx` is the tree-walker's detached `ApplyContext for \
                          Evaluator`, whose methods defer each call as a holder, or an \
                          embedder's own: see `run_synchronously`"
            )]
            PrimitiveHandler::Resumable { start, resume } => {
                run_synchronously(ctx, start_in(ctx, *start, args)?, *resume)
            }
        }
    }

    /// `apply_by_index` for callers that own their argument vector: the
    /// higher-order tier takes the Vec by move instead of re-copying it.
    pub fn apply_by_index_owned(
        &self,
        index: usize,
        args: Vec<TaggedValue>,
        ctx: &dyn ApplyContext,
    ) -> Result<TaggedValue, EvalError> {
        let primitive = self.entry(index)?;
        primitive.check_arity(args.len())?;
        match &primitive.handler {
            PrimitiveHandler::Cx(h) => ctx.call_cx(*h, &args),
            PrimitiveHandler::HigherOrder(h) => call_higher_order(ctx, *h, args),
            #[expect(
                clippy::disallowed_methods,
                reason = "holds `args`, the primitive's arguments, across its calls. Reached \
                          only as in `apply_by_index`: never by a machine, so `ctx` is the \
                          detached `ApplyContext for Evaluator`, whose methods defer each call \
                          as a holder, or an embedder's own: see `run_synchronously`"
            )]
            PrimitiveHandler::Resumable { start, resume } => {
                run_synchronously(ctx, start_in(ctx, *start, &args)?, *resume)
            }
        }
    }

    /// The handler of the primitive at `index`, if it takes the context —
    /// for a machine that calls it itself, with no [`ApplyContext`] and no
    /// [`Step`] between, since it can neither call back nor suspend (#706).
    /// `None` for any other kind, which [`Self::start`] dispatches. The
    /// caller checks the arity first ([`Self::check_arity`]).
    #[inline]
    pub fn cx_handler(&self, index: usize) -> Option<CxHandler> {
        match self.entries.get(index)?.handler {
            PrimitiveHandler::Cx(handler) => Some(handler),
            _ => None,
        }
    }

    /// Check `arg_count` against the arity of the primitive at `index`, as
    /// every dispatch does before it calls the handler.
    pub fn check_arity(&self, index: usize, arg_count: usize) -> Result<(), EvalError> {
        self.entry(index)?.check_arity(arg_count)
    }

    /// The resumable primitive at `index`'s two halves, if it is one — for a
    /// machine that runs its calls itself rather than through
    /// [`Self::apply_by_index`].
    pub fn resumable(&self, index: usize) -> Option<(ResumableStart, ResumableResume)> {
        match &self.entries.get(index)?.handler {
            PrimitiveHandler::Resumable { start, resume } => Some((*start, *resume)),
            _ => None,
        }
    }

    /// Apply the primitive at `index` as a machine that makes a resumable
    /// primitive's calls itself: a resumable one's first [`Step`], and any
    /// other's value as [`TaggedStep::Done`] — so one dispatch serves both,
    /// where [`Self::apply_by_index`] would run the calls from Rust.
    pub fn start(
        &self,
        index: usize,
        args: &[TaggedValue],
        ctx: &dyn ApplyContext,
    ) -> Result<TaggedStep, EvalError> {
        let primitive = self.entry(index)?;
        primitive.check_arity(args.len())?;
        match &primitive.handler {
            PrimitiveHandler::Cx(h) => ctx.call_cx(*h, args).map(TaggedStep::Done),
            PrimitiveHandler::HigherOrder(h) => {
                call_higher_order(ctx, *h, args.to_vec()).map(TaggedStep::Done)
            }
            PrimitiveHandler::Resumable { start, .. } => start_in(ctx, *start, args),
        }
    }

    /// Resume the resumable primitive at `index` with the state its last
    /// call kept and what that call returned.
    pub fn resume(
        &self,
        index: usize,
        state: TaggedValue,
        result: TaggedValue,
        ctx: &dyn ApplyContext,
    ) -> Result<TaggedStep, EvalError> {
        match &self.entry(index)?.handler {
            PrimitiveHandler::Resumable { resume, .. } => resume_in(ctx, *resume, state, result),
            _ => Err(EvalError::InternalError(format!(
                "primitive {index} is not resumable"
            ))),
        }
    }

    /// Apply a primitive through its cached index slot (see
    /// `resolve_index_cached`). After the first call, dispatch is a direct
    /// array access.
    pub fn apply_cached(
        &self,
        qualified_name: &str,
        index_cache: &Cell<Option<usize>>,
        args: &[TaggedValue],
        ctx: &dyn ApplyContext,
    ) -> Result<TaggedValue, EvalError> {
        let index = self
            .resolve_index_cached(qualified_name, index_cache)
            .ok_or_else(|| EvalError::UndefinedVariable(qualified_name.to_string()))?;
        debug_assert_eq!(
            self.resolve_index(qualified_name),
            Some(index),
            "stale primitive index cache for {}",
            qualified_name
        );
        self.apply_by_index(index, args, ctx)
    }

    /// `apply_cached` for callers that own their argument vector.
    pub fn apply_cached_owned(
        &self,
        qualified_name: &str,
        index_cache: &Cell<Option<usize>>,
        args: Vec<TaggedValue>,
        ctx: &dyn ApplyContext,
    ) -> Result<TaggedValue, EvalError> {
        let index = self
            .resolve_index_cached(qualified_name, index_cache)
            .ok_or_else(|| EvalError::UndefinedVariable(qualified_name.to_string()))?;
        self.apply_by_index_owned(index, args, ctx)
    }

    /// Apply a primitive by qualified name, using the provided context
    pub fn apply_tagged(
        &self,
        qualified_name: &str,
        args: &[TaggedValue],
        ctx: &dyn ApplyContext,
    ) -> Result<TaggedValue, EvalError> {
        let index = self
            .resolve_index(qualified_name)
            .ok_or_else(|| EvalError::UndefinedVariable(qualified_name.to_string()))?;
        self.apply_by_index(index, args, ctx)
    }
}

impl Default for PrimitiveRegistry {
    fn default() -> Self {
        Self::new()
    }
}
