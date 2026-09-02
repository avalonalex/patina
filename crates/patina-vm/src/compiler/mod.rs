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

pub(crate) mod alpha_rename;
mod body_defines;
pub mod pass1_analysis;
pub mod pass2_closure;
pub mod pass3_tail;
pub mod pass4_registers;
pub mod pass5_codegen;
pub mod primitive_calls;
pub mod quasiquote_expand;

pub(crate) use body_defines::for_each_define;

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
    let alpha_rename::Renamed {
        expr: renamed,
        global_aliases,
    } = alpha_rename::alpha_rename(expr)?;

    let analysis = pass1_analysis::Pass1Analysis::run(&renamed);
    let closed = pass2_closure::Pass2Closure::run(&renamed, &analysis);
    let tailed = pass3_tail::Pass3Tail::run(&closed);
    let allocated = pass4_registers::Pass4Registers::run(&tailed);
    let prim_calls = resolver
        .map(|(heap, env, registry)| {
            primitive_calls::resolve_primitive_calls(&allocated, heap, env, registry)
        })
        .unwrap_or_default();
    let code = pass5_codegen::Pass5Codegen::run(&allocated, prim_calls)?;

    // Install last, so the passes above stay a pure function of their input
    // and a compile that fails leaves the environment untouched. See
    // `Renamed::global_aliases` for what these are and why they are aliases.
    //
    // Without an environment there is nothing to install into; that path
    // (`compile`) compiles hand-built `CoreExpr` trees, which have no macro
    // expansion and so no such definitions.
    match resolver {
        Some((_, env, _)) => {
            // The bare-name alias contract holds only in a parentless
            // environment — `get` returns on an alias hit rather than falling
            // through, so one whose target is unbound would eclipse a parent's
            // binding. Every environment this path compiles against is a
            // parentless global one; the assertion is what keeps that true.
            debug_assert!(
                env.parent().is_none(),
                "bare-name aliases need a parentless environment"
            );
            for (bare, renamed_to) in global_aliases {
                env.define_alias(bare, env.clone(), renamed_to);
            }
        }
        None => debug_assert!(
            global_aliases.is_empty(),
            "compile() has no environment to install aliases into, and its \
             hand-built CoreExpr trees are not macro-expanded — a tree that \
             produced aliases came from somewhere that needs compile_with_qq_resolving"
        ),
    }
    Ok(code)
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
/// `Quasiquote` nodes are first expanded into equivalent `App` calls of the
/// registry's `list`, `append` and `list->vector` (requires heap access to
/// walk TaggedValue templates), then callees that resolve to registry
/// primitives in `env` emit `CallPrimitive` (see `primitive_calls`). This is
/// the entry the VM backend uses.
pub fn compile_with_qq_resolving(
    expr: &CoreExpr,
    heap: &SharedHeap,
    env: &Rc<Environment>,
    registry: &PrimitiveRegistry,
) -> Result<(CodeObject, Vec<CodeObject>), CompileError> {
    let expanded = quasiquote_expand::expand_quasiquotes(expr, heap, env, registry)?;
    compile_pipeline(&expanded, Some((heap, env, registry)))
}
