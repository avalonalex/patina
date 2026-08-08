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

/// Names (by env binding) that pass 5 may compile to `CallPrimitive` (or an
/// inline opcode, when `inline` is set and the call site has the right arity).
pub type PrimitiveCallMap = FxHashMap<Symbol, ResolvedPrimitive>;

/// A callee name resolved to a registry primitive at compile time.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedPrimitive {
    pub id: PrimitiveFnId,
    /// Set when this primitive has a specialized inline opcode (Track P P3).
    /// Keyed on the registry entry's own qualified name — not the env
    /// binding name, so import renames still inline correctly, and not the
    /// bare short name, so an unrelated library registering e.g. its own `+`
    /// can never pick up scheme.base's fast-path semantics.
    pub inline: Option<InlineOp>,
}

/// The primitives with specialized inline opcodes. Each maps to one
/// fixed-arity `Instruction` variant; pass 5 emits it only when the call
/// site's argument count matches (`arity()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineOp {
    Add,
    Sub,
    Mul,
    Lt,
    NumEq,
    Eq,
    Cons,
    Car,
    Cdr,
    Not,
    NullP,
    PairP,
    VectorP,
    VectorRef,
    VectorSet,
}

impl InlineOp {
    /// The exact argument count the inline opcode handles.
    pub fn arity(self) -> usize {
        match self {
            InlineOp::Car
            | InlineOp::Cdr
            | InlineOp::Not
            | InlineOp::NullP
            | InlineOp::PairP
            | InlineOp::VectorP => 1,
            InlineOp::Add
            | InlineOp::Sub
            | InlineOp::Mul
            | InlineOp::Lt
            | InlineOp::NumEq
            | InlineOp::Eq
            | InlineOp::Cons
            | InlineOp::VectorRef => 2,
            InlineOp::VectorSet => 3,
        }
    }

    /// Ops with an `*Imm` right-operand instruction form (Track P P5).
    /// The single owner of that set: `primitive_operands` absorbs a literal
    /// only for these, and `primitive_call_instruction`'s imm branch covers
    /// exactly these.
    pub fn has_imm_form(self) -> bool {
        matches!(
            self,
            InlineOp::Add | InlineOp::Sub | InlineOp::Lt | InlineOp::NumEq
        )
    }
}

/// Map a registry primitive's qualified name to its inline opcode, if any.
fn inline_op_for(qualified_name: &str) -> Option<InlineOp> {
    Some(match qualified_name {
        "scheme.base/+" => InlineOp::Add,
        "scheme.base/-" => InlineOp::Sub,
        "scheme.base/*" => InlineOp::Mul,
        "scheme.base/<" => InlineOp::Lt,
        "scheme.base/=" => InlineOp::NumEq,
        "scheme.base/eq?" => InlineOp::Eq,
        "scheme.base/cons" => InlineOp::Cons,
        "scheme.base/car" => InlineOp::Car,
        "scheme.base/cdr" => InlineOp::Cdr,
        "scheme.base/not" => InlineOp::Not,
        "scheme.base/null?" => InlineOp::NullP,
        "scheme.base/pair?" => InlineOp::PairP,
        "scheme.base/vector?" => InlineOp::VectorP,
        "scheme.base/vector-ref" => InlineOp::VectorRef,
        "scheme.base/vector-set!" => InlineOp::VectorSet,
        _ => return None,
    })
}

/// Primitives that must keep the generic `Call` path. Everything in
/// `patina.internal.control` is either intercepted by the VM for control flow
/// (call/cc, dynamic-wind, values, prompts, apply) or lives next to code that
/// is; same for the exception machinery in `patina.internal.errors`
/// (raise/raise-continuable/error/with-exception-handler are VM-intercepted).
/// The test below cross-checks this against `VM_INTERCEPTED_PRIMITIVES` — if
/// an intercepted primitive ever moves outside these prefixes, that test
/// fails instead of the interception being silently bypassed.
pub(crate) fn is_excluded(qualified_name: &str) -> bool {
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
        let Some(index) = registry.resolve_index_cached(qualified_name, registry_index) else {
            continue;
        };
        let Some(entry) = registry.get_by_index(index) else {
            continue;
        };
        // The binding's qualified name may be an alias for the registry entry
        // (e.g. "patina.internal.numbers/<" for "scheme.base/<", matched via
        // the short-name fallback), so the inline key uses the entry's own
        // name per the `ResolvedPrimitive::inline` contract. Both exclusion
        // checks are needed: VM interception matches the binding's name
        // (`vm_control_primitive`), while index dispatch reaches the entry.
        let canonical = entry.qualified_name();
        if is_excluded(&canonical) {
            continue;
        }
        map.insert(
            name,
            ResolvedPrimitive {
                id: PrimitiveFnId(index as u32),
                inline: inline_op_for(&canonical),
            },
        );
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

#[cfg(test)]
mod tests {
    use super::is_excluded;
    use crate::runtime::vm_state::VM_INTERCEPTED_PRIMITIVES;

    /// `is_excluded` may be a superset of the intercepted set (everything in
    /// the control/errors libraries stays on the generic path), but it must
    /// never miss an intercepted name: a `CallPrimitive`-compiled intercepted
    /// primitive would dispatch straight to the registry handler and bypass
    /// the VM's continuation/exception cooperation.
    #[test]
    fn excluded_covers_every_intercepted_primitive() {
        for (name, _) in VM_INTERCEPTED_PRIMITIVES {
            assert!(
                is_excluded(name),
                "VM-intercepted primitive {name} is not excluded from CallPrimitive emission"
            );
        }
    }
}
