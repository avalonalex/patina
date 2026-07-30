//! Compile-time resolution of primitive callees for `CallPrimitive` emission.
//!
//! Walks the register-allocated tree for `App` nodes whose callee is a
//! `GlobalRef`, resolves each name against the global environment the code
//! will run under, and maps names bound to registry primitives to their
//! stable `PrimitiveFnId`. Pass 5 consults the map to emit `CallPrimitive`
//! instead of `LoadGlobal` + `Call`.
//!
//! Lexically shadowed names never reach this map: pass 2 compiles them to
//! `LocalRef`/`ClosureRef`, so only true global references are candidates.
//! Rebinding a primitive name *after* compilation is handled at runtime, not
//! here: the `Define`/`StoreGlobal` handlers mark the overwritten primitive in
//! `VmState::shadowed_primitives`, and marked ids fall back to the full
//! name-lookup call path (see the `CallPrimitive` arm in `vm_state.rs`).

use std::rc::Rc;

use patina_core::environment::Environment;
use patina_core::heap::SharedHeap;
use patina_core::procedure::Procedure;
use patina_primitives::PrimitiveRegistry;
use rustc_hash::{FxHashMap, FxHashSet};

use super::pass4_registers::{AllocatedExpr, RegExpr, RegExprKind};
use crate::types::instruction::PrimitiveFnId;
use patina_core::core_expr::Symbol;

/// Names (by env binding) that pass 5 may compile to `CallPrimitive`.
pub type PrimitiveCallMap = FxHashMap<Symbol, PrimitiveFnId>;

/// Primitives that must keep the generic `Call` path. Everything in
/// `patina.internal.control` is either intercepted by the VM for control flow
/// (call/cc, dynamic-wind, values, prompts, apply) or lives next to code that
/// is; same for the exception machinery in `patina.internal.errors`
/// (raise/raise-continuable/error/with-exception-handler are VM-intercepted).
fn is_excluded(qualified_name: &str) -> bool {
    qualified_name.starts_with("patina.internal.control/")
        || qualified_name.starts_with("patina.internal.errors/")
}

/// Build the `CallPrimitive` emission map for one compilation unit.
///
/// A name maps to a `PrimitiveFnId` only when it is used in callee position,
/// currently binds to a registry primitive in `env`, and is not one of the
/// excluded control primitives. Anything unresolvable simply stays on the
/// generic `Call` path — at worst slower, never wrong.
pub fn resolve_primitive_calls(
    allocated: &AllocatedExpr,
    heap: &SharedHeap,
    env: &Rc<Environment>,
    registry: &PrimitiveRegistry,
) -> PrimitiveCallMap {
    let mut callees = FxHashSet::default();
    collect_callee_names(&allocated.expr, &mut callees);

    let mut map = PrimitiveCallMap::default();
    for name in callees {
        let Some(val) = env.get(&name) else { continue };
        let proc = heap.borrow().get_procedure(val);
        let Some(proc) = proc else { continue };
        let Procedure::Primitive {
            qualified_name,
            registry_index,
            ..
        } = proc.as_ref()
        else {
            continue;
        };
        if is_excluded(qualified_name) {
            continue;
        }
        let index = registry_index
            .get()
            .or_else(|| registry.resolve_index(qualified_name));
        if let Some(index) = index {
            map.insert(name, PrimitiveFnId(index as u32));
        }
    }
    map
}

/// Collect every name used as a `GlobalRef` callee of an `App`.
fn collect_callee_names(expr: &RegExpr, out: &mut FxHashSet<Symbol>) {
    match &expr.kind {
        RegExprKind::App { func, args, .. } => {
            if let RegExprKind::GlobalRef { name } = &func.kind {
                out.insert(name.clone());
            } else {
                collect_callee_names(func, out);
            }
            for arg in args {
                collect_callee_names(arg, out);
            }
        }
        RegExprKind::Apply { func, args, .. } => {
            collect_callee_names(func, out);
            for arg in args {
                collect_callee_names(arg, out);
            }
        }
        RegExprKind::Lambda(lam) => {
            for e in &lam.body {
                collect_callee_names(e, out);
            }
        }
        RegExprKind::If { test, then, else_ } => {
            collect_callee_names(test, out);
            collect_callee_names(then, out);
            collect_callee_names(else_, out);
        }
        RegExprKind::Begin(exprs) => {
            for e in exprs {
                collect_callee_names(e, out);
            }
        }
        RegExprKind::SetLocal { value, .. }
        | RegExprKind::WriteLocalCell { value, .. }
        | RegExprKind::WriteClosureCell { value, .. }
        | RegExprKind::SetGlobal { value, .. }
        | RegExprKind::Define { value, .. } => collect_callee_names(value, out),
        RegExprKind::Literal(_)
        | RegExprKind::Quote(_)
        | RegExprKind::Quasiquote(_)
        | RegExprKind::LocalRef { .. }
        | RegExprKind::ClosureRef { .. }
        | RegExprKind::GlobalRef { .. }
        | RegExprKind::ReadLocalCell { .. }
        | RegExprKind::ReadClosureCell { .. } => {}
    }
}
