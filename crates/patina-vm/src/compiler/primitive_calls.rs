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
//! holds, never what its spelling means in the environment. A callee
//! relinking linked is never eligible, whatever its record holds: it stays
//! on the generic `Call` path.
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

use super::pass4_registers::{AllocatedExpr, RegExpr, RegExprKind};
use super::{LinkedOperand, OperandKey};
use crate::types::instruction::{ControlForm, PrimitiveFnId};
use patina_core::core_expr::Symbol;

/// The callees pass 5 may compile to a primitive call instead of a generic
/// `Call`.
#[derive(Debug, Default)]
pub struct PrimitiveCallMap {
    /// Callees (by env binding, or by the record early binding linked)
    /// that compile to `CallPrimitive` (or an inline opcode, when `inline`
    /// is set and the call site has the right arity). Never a relinked one.
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
    /// links (#438, #837), still gets the sequence: an early-bound one by
    /// its record's value. A relinked one never does.
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
/// excluded control primitives; a callee early binding linked (#837) when
/// its record's cell holds one; a callee relinking linked, never. Anything
/// unresolvable simply stays on the generic `Call` path — at worst slower,
/// never wrong.
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
            // A relinked callee (#837's second PR) is an ordinary call: a
            // private or an introduced binding, whose fast paths #837
            // leaves as they were. A rule of scope, not of soundness: a
            // record's read changes only by a write the shadow marks see,
            // as above. Making one eligible is deleting this arm and
            // `OperandKey::Relinked`.
            Some(operand) if operand.relinked => continue,
            Some(operand) => operand.link.value(&heap.borrow()),
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
/// environment's own binding for this reason (`define_global`). Imports are
/// slots of their own and keep their fast paths; an early-bound reference is
/// linked (#837) and judged by its record, not here.
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
    names: FxHashMap<OperandKey, (Symbol, Option<LinkedOperand>)>,
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
        let key = |link| {
            OperandKey::of(
                &Rc::from("car"),
                Some(crate::compiler::LinkedOperand::of(&literals, link)),
            )
        };
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

    /// A machine with its primitives, `call-with-values` and
    /// `dynamic-wind` bound as the library loader binds them, a library
    /// that imports the global `car`, `length` and those two, and a pool:
    /// what the two tests below link against.
    fn with_importer() -> (
        crate::runtime::VmState,
        std::rc::Rc<patina_core::environment::Environment>,
        patina_core::LiteralPool,
    ) {
        use patina_core::environment::Environment;
        let mut state = crate::runtime::VmState::on_a_fresh_heap();
        state.install_primitives();
        let heap = state.heap().clone();
        for (name, arity) in [("call-with-values", 2), ("dynamic-wind", 3)] {
            state.global_env().define_primitive(
                &heap,
                name,
                patina_core::Arity::Exact(arity),
                vec!["patina".into(), "internal".into(), "control".into()],
            );
        }
        let importer = std::rc::Rc::new(Environment::with_heap(&heap));
        for name in ["car", "length", "call-with-values", "dynamic-wind"] {
            let cell = state.global_env().cell_of(&heap, name).unwrap();
            importer.import_cell(&heap, name, cell);
        }
        let literals = patina_core::LiteralPool::new(&heap.borrow());
        (state, importer, literals)
    }

    /// `(lambda (p) (begin call ...))`, each call a linked callee applied
    /// to `arity` copies of `p`.
    fn a_lambda_calling(
        calls: &[(&str, patina_core::Link, usize)],
    ) -> patina_core::core_expr::CoreExpr {
        use patina_core::core_expr::{CoreExpr, CoreExprKind, Formals, ScopedParam};
        use patina_core::{ScopeId, ScopeSet};
        use std::rc::Rc;

        let scopes = ScopeSet::from_iter([ScopeId(500)]);
        let p = || CoreExpr::new(CoreExprKind::var(Rc::from("p"), ScopeSet::new()));
        let calls = calls
            .iter()
            .map(|&(name, link, arity)| {
                CoreExpr::new(CoreExprKind::App {
                    func: Rc::new(CoreExpr::new(CoreExprKind::linked_var(
                        Rc::from(name),
                        scopes.clone(),
                        Some(link),
                    ))),
                    args: (0..arity).map(|_| p()).collect(),
                })
            })
            .collect();
        CoreExpr::new(CoreExprKind::Lambda {
            params: Formals::Fixed(vec![ScopedParam::simple(Rc::from("p"))]),
            body: vec![CoreExpr::new(CoreExprKind::Begin(calls))],
            binding_scopes: Rc::new(ScopeSet::new()),
        })
    }

    /// The primitive-call map of `expr` compiled in `state`'s globals.
    fn primitive_calls_of(
        state: &crate::runtime::VmState,
        literals: &patina_core::LiteralPool,
        expr: &patina_core::core_expr::CoreExpr,
    ) -> super::PrimitiveCallMap {
        use crate::compiler::{pass1_analysis, pass2_closure, pass3_tail, pass4_registers};
        let env = state.global_env();
        let renamed = crate::compiler::alpha_rename::alpha_rename(expr, Some(env))
            .unwrap()
            .expr;
        let analysis = pass1_analysis::Pass1Analysis::run(&renamed);
        let closed = pass2_closure::Pass2Closure::run(&renamed, &analysis, literals);
        let tailed = pass3_tail::Pass3Tail::run(&closed);
        let allocated = pass4_registers::Pass4Registers::run(&tailed);
        super::resolve_primitive_calls(&allocated, state.heap(), env, state.primitive_registry())
    }

    /// A callee relinking linked is no fast path (#837's second PR),
    /// whatever its record's cell holds: over the records an early-bound
    /// `car` gets the inline opcode through, an early-bound `length` its
    /// `CallPrimitive`, and an early-bound `call-with-values` and
    /// `dynamic-wind` their instruction sequences, the relinked callee is
    /// in neither map, and pass 5 compiles each call through it to a load
    /// of its pinned operand and a `Call`, or a `TailCall` for the last.
    #[test]
    fn a_relinked_callee_is_no_fast_path() {
        use super::OperandKey;
        use crate::compiler::{LinkedOperand, link_to, relink_to};
        use crate::types::instruction::Instruction;
        use std::rc::Rc;

        let (state, importer, literals) = with_importer();
        let heap = state.heap().clone();
        const CALLEES: [(&str, usize); 4] = [
            ("car", 1),
            ("length", 1),
            ("call-with-values", 2),
            ("dynamic-wind", 3),
        ];
        let early = CALLEES.map(|(name, arity)| {
            let link = link_to(&heap, &importer, name, &literals);
            (name, link, arity)
        });
        let relinked = CALLEES.map(|(name, arity)| {
            let link = relink_to(&heap, &importer, name, &literals);
            (name, link, arity)
        });
        let key = |name: &str, link| {
            OperandKey::of(&Rc::from(name), Some(LinkedOperand::of(&literals, link)))
        };

        // The positive control: the same four records, early-bound, are
        // two primitive calls, one inline and one not, and two control
        // forms.
        let map = primitive_calls_of(&state, &literals, &a_lambda_calling(&early));
        for (name, link, _) in &early[..2] {
            assert!(map.by_name.contains_key(&key(name, *link)), "{name}");
        }
        for (name, link, _) in &early[2..] {
            assert!(map.control.contains_key(&key(name, *link)), "{name}");
        }

        let relinked_car = relinked[0].1;
        let relinked = a_lambda_calling(&relinked);
        let map = primitive_calls_of(&state, &literals, &relinked);
        assert!(
            map.by_name.is_empty() && map.control.is_empty(),
            "a relinked callee is in neither map: {map:?}"
        );
        assert_eq!(
            key("car", relinked_car),
            OperandKey::Relinked(literals.link(relinked_car).key())
        );

        let (_, nested) = crate::compiler::compile_with_qq_resolving(
            &relinked,
            &literals,
            &heap,
            state.global_env(),
            state.primitive_registry(),
        )
        .unwrap();
        let code = &nested[0];
        let loads: Vec<&str> = code
            .instructions
            .iter()
            .filter_map(|instr| match instr {
                Instruction::LoadGlobal { link, .. } => {
                    assert!(code.links.is_pinned(*link), "a relinked operand is pinned");
                    Some(&**code.links.name(*link))
                }
                _ => None,
            })
            .collect();
        assert_eq!(loads, CALLEES.map(|(name, _)| name));
        let calls = code
            .instructions
            .iter()
            .filter(|instr| {
                matches!(
                    instr,
                    Instruction::Call { .. } | Instruction::TailCall { .. }
                )
            })
            .count();
        assert_eq!(calls, CALLEES.len(), "each an ordinary call");
        assert!(
            !code.instructions.iter().any(|instr| matches!(
                instr,
                Instruction::Car { .. }
                    | Instruction::CallPrimitive { .. }
                    | Instruction::JumpUnlessShadowed { .. }
            )),
            "no inline opcode, primitive call or control sequence"
        );
    }

    /// An early-bound and a relinked `car` over one record, in one unit,
    /// are two operands (#837's second PR): two keys, so the early one
    /// keeps its inline opcode and the relinked one is a call, and two
    /// entries of the unit's link table, both pinned, both reading the one
    /// record. Sharing a key would give the two sites one answer: both the
    /// fast path, or neither.
    #[test]
    fn a_relinked_and_an_early_operand_of_one_record_stay_apart() {
        use super::OperandKey;
        use crate::compiler::{LinkedOperand, link_to, relink_to};
        use crate::types::instruction::Instruction;
        use std::rc::Rc;

        let (state, importer, literals) = with_importer();
        let heap = state.heap().clone();
        let early = link_to(&heap, &importer, "car", &literals);
        let relinked = relink_to(&heap, &importer, "car", &literals);
        assert_eq!(literals.link(early), literals.link(relinked), "one record");
        let key = |link| OperandKey::of(&Rc::from("car"), Some(LinkedOperand::of(&literals, link)));
        assert_ne!(key(early), key(relinked));

        // In either order, so that neither can answer for the other by
        // being met first.
        for calls in [
            [("car", early, 1), ("car", relinked, 1)],
            [("car", relinked, 1), ("car", early, 1)],
        ] {
            let expr = a_lambda_calling(&calls);
            let map = primitive_calls_of(&state, &literals, &expr);
            assert!(map.by_name.contains_key(&key(early)));
            assert!(!map.by_name.contains_key(&key(relinked)));
            assert_eq!(map.by_name.len(), 1);

            let (_, nested) = crate::compiler::compile_with_qq_resolving(
                &expr,
                &literals,
                &heap,
                state.global_env(),
                state.primitive_registry(),
            )
            .unwrap();
            let code = &nested[0];
            let names: Vec<&str> = code.links.names().iter().map(|name| &**name).collect();
            assert_eq!(names, ["car", "car"], "two entries, one spelling");
            assert_eq!(code.links.pinned(), &[0, 1], "both pinned");
            let h = heap.borrow();
            assert!(code.links.get(&h, 0).is_some());
            assert_eq!(code.links.get(&h, 0), code.links.get(&h, 1), "one record");
            let inline: Vec<_> = code
                .instructions
                .iter()
                .filter_map(|instr| match instr {
                    Instruction::Car { link, .. } => Some(*link),
                    _ => None,
                })
                .collect();
            let loaded: Vec<_> = code
                .instructions
                .iter()
                .filter_map(|instr| match instr {
                    Instruction::LoadGlobal { link, .. } => Some(*link),
                    _ => None,
                })
                .collect();
            assert_eq!((inline.len(), loaded.len()), (1, 1));
            assert_ne!(inline[0], loaded[0], "each its own entry");
        }
    }
}
