//! Compile-time resolution of primitive callees for `CallPrimitive` emission.
//!
//! Walks the register-allocated tree for `App` nodes whose callee is a
//! `GlobalRef`, resolves each name against the global environment the code
//! will run under, and maps names bound to registry primitives to their
//! stable `PrimitiveFnId`. Pass 5 consults the map to emit `CallPrimitive`
//! instead of `LoadGlobal` + `Call`.
//!
//! It also finds the operators bound to `call-with-values` and `dynamic-wind`,
//! which pass 5 compiles to an instruction sequence of their own behind a
//! guard (`PrimitiveCallMap::control`).
//!
//! Lexically shadowed names never reach this map: pass 2 compiles them to
//! `LocalRef`/`ClosureRef`, so only true global references are candidates.
//! A callee the desugarer linked to a binding record (#837) is keyed by the
//! record (`OperandKey`), and its eligibility is the record's: what it
//! holds, never what its spelling means in the environment.
//! Rebinding a primitive name *after* compilation is handled at runtime, not
//! here: the `Define`/`StoreGlobal` handlers, the import path and a host's
//! definitions (`Backend::define`, #673) mark the overwritten primitive in
//! `VmState::shadowed_primitives`, and marked ids fall back to the full
//! name-lookup call path (see the `CallPrimitive` arm in `vm_state.rs`).

use std::rc::Rc;

use patina_core::NamespaceView;
use patina_core::environment::Environment;
use patina_core::heap::SharedHeap;
use patina_core::procedure::Procedure;
use patina_primitives::PrimitiveRegistry;
use rustc_hash::{FxHashMap, FxHashSet};

use super::OperandKey;
use super::pass4_registers::{AllocatedExpr, RegExpr, RegExprKind};
use crate::types::instruction::{ControlForm, PrimitiveFnId};
use patina_core::BindingLink;
use patina_core::core_expr::Symbol;

/// The callees pass 5 may compile to a primitive call instead of a generic
/// `Call`.
#[derive(Debug, Default)]
pub struct PrimitiveCallMap {
    /// Callees (by env binding, or by the record linked) that compile to
    /// `CallPrimitive` (or an inline opcode, when `inline` is set and the
    /// call site has the right arity).
    pub by_name: FxHashMap<OperandKey, ResolvedPrimitive>,
    /// Primitive procedure *values* in operator position — the quasiquote
    /// pre-pass's constructors — keyed by the literal's raw bits; these
    /// compile to `CallPrimitiveDirect`, which never deoptimizes because no
    /// global binding stands behind the site.
    pub by_value: FxHashMap<u64, PrimitiveFnId>,
    /// Names bound to `call-with-values` or `dynamic-wind`, and which. Pass
    /// 5 compiles a call through one to the form's instruction sequence,
    /// behind a guard that sends it to an ordinary call once a binding of the
    /// form's procedure has been given another value
    /// (`Instruction::JumpUnlessShadowed`).
    ///
    /// Keyed on the binding, not on the spelling, since #442: a program's
    /// own `call-with-values` is a call of the program's procedure, and an
    /// import renamed, or a library template's reference early binding
    /// links (#438, #837), still gets the sequence: a linked one by its
    /// record's value.
    pub control: FxHashMap<OperandKey, ControlForm>,
}

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
/// Two intercepted primitives are bound elsewhere, so each is named on its
/// own: `exit`, in `patina.internal.system` beside `emergency-exit`, and
/// `force`, in `patina.internal.lazy` beside the promise constructors (#476).
/// The test below cross-checks this against `VM_INTERCEPTED_PRIMITIVES` — if
/// an intercepted primitive ever moves outside these prefixes, that test
/// fails instead of the interception being silently bypassed.
pub(crate) fn is_excluded(qualified_name: &str) -> bool {
    qualified_name.starts_with("patina.internal.control/")
        || qualified_name.starts_with("patina.internal.errors/")
        || qualified_name == "patina.internal.system/exit"
        || qualified_name == "patina.internal.lazy/force"
}

/// Build the `CallPrimitive` emission map for one compilation unit.
///
/// A name maps to a `PrimitiveFnId` only when it is used in callee position,
/// currently binds to a registry primitive in `env`, and is not one of the
/// excluded control primitives; a linked callee (#837) when its record's
/// cell holds one. Anything unresolvable simply stays on the generic `Call`
/// path — at worst slower, never wrong.
pub fn resolve_primitive_calls(
    allocated: &AllocatedExpr,
    heap: &SharedHeap,
    env: &Rc<Environment>,
    registry: &PrimitiveRegistry,
) -> PrimitiveCallMap {
    let mut callees = Callees::default();
    collect_callees(&allocated.expr, &mut callees);

    let mut map = PrimitiveCallMap::default();
    let view = NamespaceView::of(env);
    for (key, (name, link)) in callees.names {
        // A linked callee is the binding its record names, whatever its
        // spelling means in `env` (#837). What the record reads changes
        // only by a store into its cell or a re-point of its slot, each of
        // which marks the primitive it replaces, as for any binding of the
        // environment's own, so the site deoptimizes as theirs do.
        let val = match link {
            Some(link) => link.value(&heap.borrow()),
            None => resolved_by_name(view, heap, &name),
        };
        let Some(val) = val else {
            continue;
        };
        let Some(proc) = heap.borrow().get_procedure(val) else {
            continue;
        };
        if let Some(form) = control_form(&proc) {
            map.control.insert(key, form);
            continue;
        }
        let Some((index, canonical)) = primitive_entry(&proc, registry) else {
            continue;
        };
        map.by_name.insert(
            key,
            ResolvedPrimitive {
                id: PrimitiveFnId(index as u32),
                inline: inline_op_for(&canonical),
            },
        );
    }
    for val in callees.values {
        let proc = heap.borrow().get_procedure(val);
        if let Some((index, _)) = proc.and_then(|proc| primitive_entry(&proc, registry)) {
            map.by_value
                .insert(val.raw_bits(), PrimitiveFnId(index as u32));
        }
    }
    map
}

/// What an unlinked callee `name` holds in `env`, if it is a binding of the
/// environment's own.
///
/// Only a binding of the environment's own. A name it answers through a
/// macro-expansion alias, or as the name-only view of a definition a macro
/// introduced, can be pointed at another binding without any write to the
/// one it reaches now, which is what the shadow marks watch — a second
/// expansion defining it again re-points the alias. So can a name a host
/// namespace reaches through its parent (#620): a definition there gives it
/// a binding of its own and writes nothing of the parent's. A host
/// namespace's read-through records (#835) are in no slot, so they are
/// skipped as a parent's binding is. A definition marks by the
/// environment's own binding for this reason (`define_global`). Imports,
/// and the aliases early binding makes until 4b.10's switch links them
/// (#837), are slots of their own and keep their fast paths.
fn resolved_by_name(
    view: NamespaceView<'_>,
    heap: &SharedHeap,
    name: &Symbol,
) -> Option<patina_core::tagged_value::TaggedValue> {
    view.local_slot(name)?;
    view.get(heap, name)
}

/// The control form `proc` is the procedure of, if it has an instruction
/// sequence. Asked by the runtime's own classification, so a procedure the
/// VM intercepts as `call-with-values` is the one compiled as it.
fn control_form(proc: &Procedure) -> Option<ControlForm> {
    let Procedure::Primitive { qualified_name, .. } = proc else {
        return None;
    };
    crate::runtime::control::control_form(qualified_name)
}

/// The registry index and canonical qualified name of the primitive `proc`
/// is, if it is one that may be called by index.
fn primitive_entry(proc: &Procedure, registry: &PrimitiveRegistry) -> Option<(usize, String)> {
    let Procedure::Primitive {
        qualified_name,
        registry_index,
        ..
    } = proc
    else {
        return None;
    };
    if is_excluded(qualified_name) {
        return None;
    }
    let index = registry.resolve_index_cached(qualified_name, registry_index)?;
    let entry = registry.get_by_index(index)?;
    // The binding's qualified name may be an alias for the registry entry
    // (e.g. "patina.internal.numbers/<" for "scheme.base/<", matched via
    // the short-name fallback), so the inline key uses the entry's own
    // name per the `ResolvedPrimitive::inline` contract. Both exclusion
    // checks are needed: VM interception matches the binding's name
    // (`vm_control_primitive`), while index dispatch reaches the entry.
    let canonical = entry.qualified_name();
    if is_excluded(&canonical) {
        return None;
    }
    Some((index, canonical))
}

/// What `collect_callees` gathers: global names in operator position, and
/// literal values there. Both dedupe — one compilation unit can hold a
/// hundred quasiquotes over the same three constructor procedures, and
/// resolving each costs a heap borrow, a registry lookup and a `format!`.
#[derive(Default)]
struct Callees {
    /// Each callee once by its key, with its spelling and its link.
    names: FxHashMap<OperandKey, (Symbol, Option<BindingLink>)>,
    values: FxHashSet<patina_core::tagged_value::TaggedValue>,
}

/// Collect every `GlobalRef` name and every `Literal` value used as the
/// callee of an `App`.
fn collect_callees(expr: &RegExpr, out: &mut Callees) {
    // Grown with the depth of the code (#617).
    patina_core::walk::ensure_sufficient_stack(|| collect_callees_inner(expr, out))
}

fn collect_callees_inner(expr: &RegExpr, out: &mut Callees) {
    match &expr.kind {
        RegExprKind::App { func, args, .. } => {
            match &func.kind {
                RegExprKind::GlobalRef { name, link } => {
                    out.names
                        .entry(OperandKey::of(name, *link))
                        .or_insert_with(|| (name.clone(), *link));
                }
                RegExprKind::Literal(v) => {
                    out.values.insert(*v);
                }
                _ => collect_callees(func, out),
            }
            for arg in args {
                collect_callees(arg, out);
            }
        }
        RegExprKind::Apply { func, args, .. } => {
            collect_callees(func, out);
            for arg in args {
                collect_callees(arg, out);
            }
        }
        RegExprKind::Lambda(lam) => {
            for e in &lam.body {
                collect_callees(e, out);
            }
        }
        RegExprKind::If { test, then, else_ } => {
            collect_callees(test, out);
            collect_callees(then, out);
            collect_callees(else_, out);
        }
        RegExprKind::Begin(exprs) => {
            for e in exprs {
                collect_callees(e, out);
            }
        }
        RegExprKind::SetLocal { value, .. }
        | RegExprKind::WriteLocalCell { value, .. }
        | RegExprKind::WriteClosureCell { value, .. }
        | RegExprKind::SetGlobal { value, .. }
        | RegExprKind::Define { value, .. } => collect_callees(value, out),
        RegExprKind::Literal(_)
        | RegExprKind::Quote(_)
        | RegExprKind::Quasiquote
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
    use crate::runtime::control::VM_INTERCEPTED_PRIMITIVES;

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

    /// A linked and an unlinked `car` in one tree get two keys (#837): the
    /// unlinked one by its spelling, eligible through the environment's own
    /// binding; a linked one by its record, eligible when the record's cell
    /// holds the primitive, whatever `car` means in the environment, and
    /// not when it holds anything else.
    #[test]
    fn a_linked_callee_is_keyed_and_judged_by_its_record() {
        use super::{OperandKey, resolve_primitive_calls};
        use crate::compiler::{
            link_to, pass1_analysis, pass2_closure, pass3_tail, pass4_registers,
        };
        use crate::runtime::VmState;
        use crate::types::instruction::Instruction;
        use patina_core::core_expr::{CoreExpr, CoreExprKind, Formals, ScopedParam};
        use patina_core::environment::Environment;
        use patina_core::{LiteralPool, ScopeId, ScopeSet, TaggedValue};
        use std::rc::Rc;

        let mut state = VmState::on_a_fresh_heap();
        state.install_primitives();
        let heap = state.heap().clone();
        let env = Rc::clone(state.global_env());
        let literals = LiteralPool::new(&heap.borrow());
        // A library that imports the global `car`, and one that binds its
        // own `car` to a number.
        let importer = Rc::new(Environment::with_heap(&heap));
        importer.import_cell(&heap, "car", env.cell_of(&heap, "car").unwrap());
        let shadower = Rc::new(Environment::with_heap(&heap));
        shadower.define(&heap, "car", TaggedValue::fixnum(7));
        let to_primitive = link_to(&heap, &importer, "car", &literals);
        let to_number = link_to(&heap, &shadower, "car", &literals);

        let scopes = ScopeSet::from_iter([ScopeId(500)]);
        let p = || CoreExpr::new(CoreExprKind::var(Rc::from("p"), ScopeSet::new()));
        let call = |func: CoreExprKind| {
            CoreExpr::new(CoreExprKind::App {
                func: Rc::new(CoreExpr::new(func)),
                args: vec![p()],
            })
        };
        let linked = |link| CoreExprKind::linked_var(Rc::from("car"), scopes.clone(), Some(link));
        let expr = CoreExpr::new(CoreExprKind::Lambda {
            params: Formals::Fixed(vec![ScopedParam::simple(Rc::from("p"))]),
            body: vec![CoreExpr::new(CoreExprKind::Begin(vec![
                call(linked(to_primitive)),
                call(linked(to_number)),
                call(CoreExprKind::var(Rc::from("car"), ScopeSet::new())),
            ]))],
            binding_scopes: Rc::new(ScopeSet::new()),
        });

        let renamed = crate::compiler::alpha_rename::alpha_rename(&expr, Some(&env))
            .unwrap()
            .expr;
        let analysis = pass1_analysis::Pass1Analysis::run(&renamed);
        let closed = pass2_closure::Pass2Closure::run(&renamed, &analysis, &literals);
        let tailed = pass3_tail::Pass3Tail::run(&closed);
        let allocated = pass4_registers::Pass4Registers::run(&tailed);
        let map = resolve_primitive_calls(&allocated, &heap, &env, state.primitive_registry());
        let key = |link| OperandKey::of(&Rc::from("car"), Some(literals.link(link)));
        assert!(map.by_name.contains_key(&OperandKey::Name(Rc::from("car"))));
        assert!(
            map.by_name.contains_key(&key(to_primitive)),
            "its record holds the primitive"
        );
        assert!(
            !map.by_name.contains_key(&key(to_number)),
            "its record holds a number"
        );
        assert_eq!(map.by_name.len(), 2);

        // And pass 5 compiles the two eligible calls to the inline `car`,
        // one of them on the pinned operand, and the third to a call.
        let (_, nested) = crate::compiler::compile_with_qq_resolving(
            &expr,
            &literals,
            &heap,
            &env,
            state.primitive_registry(),
        )
        .unwrap();
        let code = &nested[0];
        let cars: Vec<bool> = code
            .instructions
            .iter()
            .filter_map(|instr| match instr {
                Instruction::Car { link, .. } => Some(code.links.is_pinned(*link)),
                _ => None,
            })
            .collect();
        assert_eq!(cars, [true, false]);
        assert!(
            code.instructions.iter().any(|instr| matches!(
                instr,
                Instruction::Call { .. } | Instruction::TailCall { .. }
            )),
            "the linked car whose record holds a number is an ordinary call"
        );
    }
}
