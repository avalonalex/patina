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
    /// Keyed on the registry entry's own short name, so import renames still
    /// inline correctly.
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
}

/// Map a registry primitive's short name to its inline opcode, if any.
/// (`not` is Scheme-defined and never resolves to a primitive, so it has no
/// opcode despite appearing in early drafts of the P3 list.)
fn inline_op_for(short_name: &str) -> Option<InlineOp> {
    Some(match short_name {
        "+" => InlineOp::Add,
        "-" => InlineOp::Sub,
        "*" => InlineOp::Mul,
        "<" => InlineOp::Lt,
        "=" => InlineOp::NumEq,
        "eq?" => InlineOp::Eq,
        "cons" => InlineOp::Cons,
        "car" => InlineOp::Car,
        "cdr" => InlineOp::Cdr,
        "null?" => InlineOp::NullP,
        "pair?" => InlineOp::PairP,
        "vector?" => InlineOp::VectorP,
        "vector-ref" => InlineOp::VectorRef,
        "vector-set!" => InlineOp::VectorSet,
        _ => return None,
    })
}

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
            name: short_name,
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
            map.insert(
                name,
                ResolvedPrimitive {
                    id: PrimitiveFnId(index as u32),
                    inline: inline_op_for(short_name),
                },
            );
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
