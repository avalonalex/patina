//! 5-pass compiler pipeline: `CoreExpr → CodeObject`.
//!
//! Each pass is a pure stateless transformation. Passes are run sequentially;
//! each feeds its output into the next.
//!
//! ```text
//! CoreExpr
//!   → Pass1Analysis  (NodeId assignment + free-var analysis + mutation detection)
//!   → Pass2Closure   (closure conversion: explicit capture lists, Box/Unbox)
//!   → Pass3Tail      (tail-position annotation)
//!   → Pass4Registers (linear-scan register allocation)
//!   → Pass5Codegen   (code generation → CodeObject)
//! ```
//!
//! See VM_COMPILER.md for the full specification.

pub mod alpha_rename;
pub mod pass1_analysis;
pub mod pass2_closure;
pub mod pass3_tail;
pub mod pass4_registers;
pub mod pass5_codegen;
pub mod primitive_calls;
pub mod quasiquote_expand;

use crate::error::CompileError;
use crate::types::CodeObject;
use patina_core::core_expr::CoreExpr;
use patina_core::environment::Environment;
use patina_core::heap::SharedHeap;
use patina_primitives::PrimitiveRegistry;
use std::rc::Rc;

/// The global environment + registry a compilation unit resolves primitive
/// callees against. `None` disables `CallPrimitive` emission entirely.
type PrimitiveResolver<'a> = Option<(&'a SharedHeap, &'a Rc<Environment>, &'a PrimitiveRegistry)>;

fn compile_pipeline(
    expr: &CoreExpr,
    resolver: PrimitiveResolver<'_>,
) -> Result<(CodeObject, Vec<CodeObject>), CompileError> {
    let renamed = alpha_rename::alpha_rename(expr);
    let analysis = pass1_analysis::Pass1Analysis::run(&renamed);
    let closed = pass2_closure::Pass2Closure::run(&renamed, &analysis);
    let tailed = pass3_tail::Pass3Tail::run(&closed);
    let allocated = pass4_registers::Pass4Registers::run(&tailed);
    let prim_calls = resolver
        .map(|(heap, env, registry)| {
            primitive_calls::resolve_primitive_calls(&allocated, heap, env, registry)
        })
        .unwrap_or_default();
    pass5_codegen::Pass5Codegen::run(&allocated, prim_calls)
}

/// Compile a `CoreExpr` into a `CodeObject` (plus any nested `CodeObject`s).
///
/// Returns `(top_level_code, nested_codes)`. The caller should load all of
/// them into `VmState::code_store` before executing. No environment is
/// available, so primitive calls stay on the generic `Call` path.
pub fn compile(expr: &CoreExpr) -> Result<(CodeObject, Vec<CodeObject>), CompileError> {
    compile_pipeline(expr, None)
}

/// Compile with quasiquote expansion *and* compile-time primitive resolution:
/// `Quasiquote` nodes are first expanded into equivalent `App` calls (list,
/// cons, append — requires heap access to walk TaggedValue templates), then
/// callees that resolve to registry primitives in `env` emit `CallPrimitive`
/// (see `primitive_calls`). This is the entry the VM backend uses.
pub fn compile_with_qq_resolving(
    expr: &CoreExpr,
    heap: &SharedHeap,
    env: &Rc<Environment>,
    registry: &PrimitiveRegistry,
) -> Result<(CodeObject, Vec<CodeObject>), CompileError> {
    let expanded = quasiquote_expand::expand_quasiquotes(expr, heap, env);
    compile_pipeline(&expanded, Some((heap, env, registry)))
}
