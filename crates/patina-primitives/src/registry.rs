//! Primitive procedure registry for backend-agnostic primitives

use crate::apply_context::ApplyContext;
use patina_runtime::environment::Environment;
use patina_runtime::{Arity, EvalError, SharedHeap, TaggedValue};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

/// Handler for heap-only primitives (~260 primitives)
/// These only need the heap for allocation/inspection. Arguments are borrowed:
/// heap handlers never re-enter the evaluator, so the slice can safely point at
/// caller-owned storage (eventually the VM register file itself).
pub type TaggedHandler = fn(&SharedHeap, &[TaggedValue]) -> Result<TaggedValue, EvalError>;

/// Handler for higher-order primitives (~25 primitives)
/// These need ApplyContext to call back into the evaluator (or reach the
/// filesystem). They take an owned Vec: re-entering the VM may reallocate the
/// register file, which would invalidate a borrowed argument slice.
pub type HOTaggedHandler =
    fn(&dyn ApplyContext, Vec<TaggedValue>) -> Result<TaggedValue, EvalError>;

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
pub enum Step {
    /// Finished, with this value.
    Done(TaggedValue),
    /// Call `callee` with `args`; when it returns `v`, resume the primitive
    /// with `state` and `v`. `state` is whatever the rest of the primitive
    /// needs — a value the machine keeps where the collector sees it, and
    /// which a continuation captured in the call carries along.
    Call {
        callee: TaggedValue,
        args: CallArgs,
        state: TaggedValue,
    },
    /// Evaluate the datum `expr` in `env`, as `eval` does, and resume the
    /// primitive with `state` and its value. The machine expands and runs it
    /// as its own code — the VM a closure over the compiled form, the
    /// tree-walker a CPS expression on the running trampoline — so a
    /// continuation captured in it carries the rest of the primitive, as one
    /// captured in a [`Step::Call`] does (#477).
    Eval {
        expr: TaggedValue,
        env: Rc<Environment>,
        state: TaggedValue,
    },
}

/// A resume that is done with what the call returned: the second half of a
/// primitive that makes one call and answers its value.
pub fn done_with_result(
    _ctx: &dyn ApplyContext,
    _state: TaggedValue,
    result: TaggedValue,
) -> Result<Step, EvalError> {
    Ok(Step::Done(result))
}

/// The arguments of a [`Step::Call`]: held inline up to three, which covers
/// every call a resumable primitive makes, so asking for one allocates
/// nothing.
pub type CallArgs = smallvec::SmallVec<[TaggedValue; 3]>;

/// A resumable primitive's first half: its arguments in, a [`Step`] out.
pub type ResumableStart = fn(&dyn ApplyContext, &[TaggedValue]) -> Result<Step, EvalError>;

/// A resumable primitive's continuation: the `state` its last [`Step::Call`]
/// asked to keep, and what the call returned.
pub type ResumableResume =
    fn(&dyn ApplyContext, TaggedValue, TaggedValue) -> Result<Step, EvalError>;

/// Handler variant for a primitive
pub enum PrimitiveHandler {
    /// Heap-only: only needs SharedHeap
    Heap(TaggedHandler),
    /// Higher-order: needs ApplyContext (for apply_proc, eval_expr, load_scheme_library)
    HigherOrder(HOTaggedHandler),
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
    mut step: Step,
    resume: ResumableResume,
) -> Result<TaggedValue, EvalError> {
    loop {
        match step {
            Step::Done(value) => return Ok(value),
            Step::Call {
                callee,
                args,
                state,
            } => {
                let result = ctx.apply_proc(callee, args.into_vec())?;
                step = resume(ctx, state, result)?;
            }
            Step::Eval { expr, env, state } => {
                let result = ctx.eval_expr(expr, &env)?;
                step = resume(ctx, state, result)?;
            }
        }
    }
}

/// Handler-side exact-arity check: the standard `WrongArity` error every
/// hand-rolled handler produces. (Registry dispatch also checks the declared
/// `Arity` up front via `PrimitiveFn::check_arity`; handlers keep their own
/// check so they stay correct when called directly.)
pub(crate) fn expect_arity(args: &[TaggedValue], n: usize) -> Result<(), EvalError> {
    if args.len() != n {
        return Err(EvalError::WrongArity {
            expected: n.to_string(),
            actual: args.len(),
        });
    }
    Ok(())
}

pub struct PrimitiveFn {
    pub library: &'static str,
    pub name: &'static str,
    pub arity: Arity,
    #[allow(dead_code)]
    pub help: &'static str,
    pub handler: PrimitiveHandler,
}

impl PrimitiveFn {
    pub fn new_heap(
        library: &'static str,
        name: &'static str,
        arity: Arity,
        help: &'static str,
        handler: TaggedHandler,
    ) -> Self {
        PrimitiveFn {
            library,
            name,
            arity,
            help,
            handler: PrimitiveHandler::Heap(handler),
        }
    }

    pub fn new_higher_order(
        library: &'static str,
        name: &'static str,
        arity: Arity,
        help: &'static str,
        handler: HOTaggedHandler,
    ) -> Self {
        PrimitiveFn {
            library,
            name,
            arity,
            help,
            handler: PrimitiveHandler::HigherOrder(handler),
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
        }
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

    pub fn register(&mut self, primitive: PrimitiveFn) {
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
            PrimitiveHandler::Heap(h) => h(ctx.heap(), args),
            PrimitiveHandler::HigherOrder(h) => h(ctx, args.to_vec()),
            PrimitiveHandler::Resumable { start, resume } => {
                run_synchronously(ctx, start(ctx, args)?, *resume)
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
            PrimitiveHandler::Heap(h) => h(ctx.heap(), &args),
            PrimitiveHandler::HigherOrder(h) => h(ctx, args),
            PrimitiveHandler::Resumable { start, resume } => {
                run_synchronously(ctx, start(ctx, &args)?, *resume)
            }
        }
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
    /// other's value as [`Step::Done`] — so one dispatch serves both, where
    /// [`Self::apply_by_index`] would run the calls from Rust.
    pub fn start(
        &self,
        index: usize,
        args: &[TaggedValue],
        ctx: &dyn ApplyContext,
    ) -> Result<Step, EvalError> {
        let primitive = self.entry(index)?;
        primitive.check_arity(args.len())?;
        match &primitive.handler {
            PrimitiveHandler::Heap(h) => h(ctx.heap(), args).map(Step::Done),
            PrimitiveHandler::HigherOrder(h) => h(ctx, args.to_vec()).map(Step::Done),
            PrimitiveHandler::Resumable { start, .. } => start(ctx, args),
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
    ) -> Result<Step, EvalError> {
        match &self.entry(index)?.handler {
            PrimitiveHandler::Resumable { resume, .. } => resume(ctx, state, result),
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
