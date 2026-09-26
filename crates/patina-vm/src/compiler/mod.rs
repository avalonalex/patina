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

/// The constructor resolver [`patina_frontend::lower_quasiquotes`] needs, over
/// this backend's registry.
///
/// Built the way `VmState::install_primitives` builds every primitive,
/// registry index included, so a call through it dispatches by index like a
/// call through the global of the same name would. It differs from that
/// global in one way only: nothing the program imports or defines can
/// redirect it — which is the whole reason the lowering references these as
/// values rather than by name (Larceny triage family 34).
fn registry_constructors<'a>(
    heap: &'a SharedHeap,
    registry: &'a PrimitiveRegistry,
) -> impl Fn(&str) -> Option<patina_core::TaggedValue> + 'a {
    move |name| {
        let qualified_name = format!("scheme.base/{name}");
        let index = registry.resolve_index(&qualified_name)?;
        let prim = registry.get_by_index(index)?;
        let proc = patina_core::procedure::Procedure::primitive(
            prim.name,
            prim.arity.clone(),
            Rc::from(qualified_name.as_str()),
            Some(index),
        );
        Some(heap.borrow_mut().alloc_procedure(proc))
    }
}

fn compile_pipeline(
    expr: &CoreExpr,
    resolver: PrimitiveResolver<'_>,
) -> Result<(CodeObject, Vec<CodeObject>), CompileError> {
    let alpha_rename::Renamed {
        expr: renamed,
        introduced_globals,
    } = alpha_rename::alpha_rename(expr, resolver.map(|(_, env, _)| env))?;

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
    // `Renamed::introduced_globals` for the identities later forms resolve.
    //
    // Without an environment there is nothing to install into; that path
    // (`compile`) compiles hand-built `CoreExpr` trees, which have no macro
    // expansion and so no such definitions.
    match resolver {
        Some((_, env, _)) => {
            // Only global environments hold these identities; lexical
            // definitions are resolved within the form being compiled.
            debug_assert!(
                env.parent().is_none(),
                "introduced globals need a root environment"
            );
            for (name, renamed) in introduced_globals {
                env.define_introduced_global(name, renamed.scopes, renamed.name);
            }
        }
        None => debug_assert!(
            introduced_globals.is_empty(),
            "compile() has no environment to record introduced globals in, and its \
             hand-built CoreExpr trees are not macro-expanded — a tree that \
             introduced globals came from somewhere that needs compile_with_qq_resolving"
        ),
    }
    Ok(code)
}

/// Compile a `CoreExpr` into a `CodeObject` (plus any nested `CodeObject`s).
///
/// Returns `(top_level_code, nested_codes)`. The caller should load them
/// together with `VmState::load_unit` before executing. No environment is
/// available, so primitive calls stay on the generic `Call` path.
pub fn compile(expr: &CoreExpr) -> Result<(CodeObject, Vec<CodeObject>), CompileError> {
    compile_pipeline(expr, None)
}

/// Compile with quasiquote lowering *and* compile-time primitive resolution:
/// `Quasiquote` nodes are first lowered into equivalent `App` calls of the
/// registry's `list`, `append` and `list->vector` (allocated on `heap`), then
/// callees that resolve to registry primitives in `env` emit `CallPrimitive`
/// (see `primitive_calls`). This is the entry the VM backend uses.
pub fn compile_with_qq_resolving(
    expr: &CoreExpr,
    heap: &SharedHeap,
    env: &Rc<Environment>,
    registry: &PrimitiveRegistry,
) -> Result<(CodeObject, Vec<CodeObject>), CompileError> {
    let expanded = patina_frontend::lower_quasiquotes(expr, &registry_constructors(heap, registry))
        .map_err(|e| CompileError::Internal(e.0))?;
    compile_pipeline(&expanded, Some((heap, env, registry)))
}

/// The lowering itself lives in `patina_frontend`, which cannot name
/// `PrimitiveRegistry` — `patina-primitives` depends on it. These exercise it
/// through this backend's resolver, which is the only place both are in scope.
#[cfg(test)]
mod quasiquote_lowering_tests {
    use super::*;
    use patina_core::core_expr::{CoreExpr, CoreExprKind};
    use patina_core::heap::Heap;
    use patina_core::procedure::Procedure;
    use patina_core::tagged_value::TaggedValue;
    use patina_core::{QuasiConstructor, QuasiTemplate};
    use patina_frontend::lower_quasiquotes;
    use std::cell::RefCell;

    fn make_heap() -> SharedHeap {
        Rc::new(RefCell::new(Heap::new()))
    }

    fn make_registry() -> PrimitiveRegistry {
        let mut registry = PrimitiveRegistry::new();
        patina_primitives::register_all(&mut registry);
        registry
    }

    fn lower(heap: &SharedHeap, template: QuasiTemplate) -> CoreExpr {
        let expr = CoreExpr::new(CoreExprKind::Quasiquote(template));
        lower_quasiquotes(&expr, &registry_constructors(heap, &make_registry()))
            .expect("the constructors resolve")
    }

    #[test]
    fn lower_self_evaluating() {
        let heap = make_heap();
        let lowered = lower(&heap, QuasiTemplate::Datum(TaggedValue::fixnum(42)));

        match &lowered.kind {
            CoreExprKind::Quote(v) => assert_eq!(v.as_fixnum(), Some(42)),
            other => panic!("expected Quote, got {:?}", other),
        }
    }

    #[test]
    fn lower_symbol() {
        let heap = make_heap();
        let sym = heap.borrow_mut().intern_symbol("foo");
        let lowered = lower(&heap, QuasiTemplate::Datum(sym));

        match &lowered.kind {
            CoreExprKind::Quote(v) => assert!(heap.borrow().is_symbol(*v)),
            other => panic!("expected Quote, got {:?}", other),
        }
    }

    #[test]
    fn lower_list_no_unquotes() {
        let heap = make_heap();
        let parts = ["a", "b", "c"]
            .iter()
            .map(|name| QuasiTemplate::Datum(heap.borrow_mut().intern_symbol(name)))
            .collect();
        let lowered = lower(&heap, QuasiTemplate::Build(QuasiConstructor::List, parts));

        // Should become (<list primitive> 'a 'b 'c) — the primitive itself,
        // not a reference to whatever `list` names where the template sits.
        match &lowered.kind {
            CoreExprKind::App { func, args } => {
                assert_eq!(args.len(), 3);
                match &func.kind {
                    CoreExprKind::Literal(v) => {
                        let proc = heap.borrow().get_procedure(*v).expect("a procedure");
                        match proc.as_ref() {
                            Procedure::Primitive { qualified_name, .. } => {
                                assert_eq!(&**qualified_name, "scheme.base/list")
                            }
                            other => panic!("expected the list primitive, got {other:?}"),
                        }
                    }
                    other => panic!("expected Literal(<list primitive>), got {:?}", other),
                }
            }
            other => panic!("expected App, got {:?}", other),
        }
    }
}
