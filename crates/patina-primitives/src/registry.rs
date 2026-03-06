//! Primitive procedure registry for backend-agnostic primitives

use crate::apply_context::ApplyContext;
use patina_runtime::{Arity, EvalError, SharedHeap, TaggedValue};
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

pub struct PrimitiveRegistry {
    primitives: HashMap<String, PrimitiveFn>,
    name_index: HashMap<&'static str, String>,
}

impl PrimitiveRegistry {
    pub fn new() -> Self {
        PrimitiveRegistry {
            primitives: HashMap::new(),
            name_index: HashMap::new(),
        }
    }

    pub fn register(&mut self, primitive: PrimitiveFn) {
        let qualified_name = primitive.qualified_name();
        self.name_index
            .entry(primitive.name)
            .or_insert_with(|| qualified_name.clone());
        self.primitives.insert(qualified_name, primitive);
    }

    pub fn get(&self, qualified_name: &str) -> Option<&PrimitiveFn> {
        self.primitives.get(qualified_name)
    }

    pub fn get_by_name(&self, name: &str) -> Option<&PrimitiveFn> {
        let qn = self.name_index.get(name)?;
        self.primitives.get(qn.as_str())
    }

    pub fn get_from_library(&self, library: &str, name: &str) -> Option<&PrimitiveFn> {
        let qualified = format!("{}/{}", library, name);
        self.primitives.get(&qualified)
    }

    pub fn get_library_primitives(&self, library: &str) -> impl Iterator<Item = &PrimitiveFn> {
        let prefix = format!("{}/", library);
        self.primitives.iter().filter_map(move |(qn, pf)| {
            if qn.starts_with(&prefix) {
                Some(pf)
            } else {
                None
            }
        })
    }

    pub fn contains(&self, qualified_name: &str) -> bool {
        self.primitives.contains_key(qualified_name)
    }

    pub fn len(&self) -> usize {
        self.primitives.len()
    }

    pub fn is_empty(&self) -> bool {
        self.primitives.is_empty()
    }

    pub fn list_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.primitives.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    pub fn primitives(&self) -> impl Iterator<Item = &PrimitiveFn> {
        self.primitives.values()
    }

    /// Apply a primitive by qualified name, using the provided context
    pub fn apply_tagged(
        &self,
        qualified_name: &str,
        args: Vec<TaggedValue>,
        ctx: &dyn ApplyContext,
    ) -> Result<TaggedValue, EvalError> {
        let primitive = self.lookup_primitive(qualified_name)?;
        primitive.check_arity(args.len())?;
        match &primitive.handler {
            PrimitiveHandler::Heap(h) => h(ctx.heap(), args),
            PrimitiveHandler::HigherOrder(h) => h(ctx, args),
        }
    }

    fn lookup_primitive(&self, qualified_name: &str) -> Result<&PrimitiveFn, EvalError> {
        if let Some(prim) = self.get(qualified_name) {
            return Ok(prim);
        }
        let name = match qualified_name.split_once('/') {
            Some((_, n)) => n,
            None => qualified_name,
        };
        self.get_by_name(name)
            .ok_or_else(|| EvalError::UndefinedVariable(qualified_name.to_string()))
    }
}

impl Default for PrimitiveRegistry {
    fn default() -> Self {
        Self::new()
    }
}
