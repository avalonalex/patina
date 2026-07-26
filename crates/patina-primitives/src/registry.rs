//! Primitive procedure registry for backend-agnostic primitives

use crate::apply_context::ApplyContext;
use patina_runtime::{Arity, EvalError, SharedHeap, TaggedValue};
use std::cell::Cell;
use std::collections::HashMap;

/// Handler for heap-only primitives (~290 primitives)
/// These only need the heap for allocation/inspection
pub type TaggedHandler = fn(&SharedHeap, Vec<TaggedValue>) -> Result<TaggedValue, EvalError>;

/// Handler for higher-order primitives (~8 primitives)
/// These need to call back into the evaluator
pub type HOTaggedHandler =
    fn(&dyn ApplyContext, Vec<TaggedValue>) -> Result<TaggedValue, EvalError>;

/// Handler variant for a primitive
pub enum PrimitiveHandler {
    /// Heap-only: only needs SharedHeap
    Heap(TaggedHandler),
    /// Higher-order: needs ApplyContext (for apply_proc, eval_expr, load_scheme_library)
    HigherOrder(HOTaggedHandler),
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

    /// Apply a primitive by its stable registry index — the hot dispatch
    /// path, no name hashing involved
    pub fn apply_by_index(
        &self,
        index: usize,
        args: Vec<TaggedValue>,
        ctx: &dyn ApplyContext,
    ) -> Result<TaggedValue, EvalError> {
        let primitive = self.entries.get(index).ok_or_else(|| {
            EvalError::InternalError(format!("invalid primitive index {}", index))
        })?;
        primitive.check_arity(args.len())?;
        match &primitive.handler {
            PrimitiveHandler::Heap(h) => h(ctx.heap(), args),
            PrimitiveHandler::HigherOrder(h) => h(ctx, args),
        }
    }

    /// Apply a primitive through a cached index slot (the one stored on
    /// `Procedure::Primitive`), resolving the name and filling the slot on
    /// first use. After the first call, dispatch is a direct array access.
    pub fn apply_cached(
        &self,
        qualified_name: &str,
        index_cache: &Cell<Option<usize>>,
        args: Vec<TaggedValue>,
        ctx: &dyn ApplyContext,
    ) -> Result<TaggedValue, EvalError> {
        let index = match index_cache.get() {
            Some(i) => i,
            None => {
                let i = self
                    .resolve_index(qualified_name)
                    .ok_or_else(|| EvalError::UndefinedVariable(qualified_name.to_string()))?;
                index_cache.set(Some(i));
                i
            }
        };
        // Split at the FIRST '/', matching resolve_index: the short name may
        // itself contain '/' (the division primitive is "library//")
        debug_assert_eq!(
            self.get_by_index(index).map(|p| p.name),
            Some(
                qualified_name
                    .split_once('/')
                    .map(|(_, n)| n)
                    .unwrap_or(qualified_name)
            ),
            "stale primitive index cache for {}",
            qualified_name
        );
        self.apply_by_index(index, args, ctx)
    }

    /// Apply a primitive by qualified name, using the provided context
    pub fn apply_tagged(
        &self,
        qualified_name: &str,
        args: Vec<TaggedValue>,
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
