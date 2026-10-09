//! Pass 5 — Code Generation
//!
//! Walks `RegExpr` and emits `Instruction`s into `CodeObject`s. Nested lambdas
//! are compiled recursively; each is named by its own `CodeObjectId::label`,
//! which a `MakeClosure` instruction in the parent refers to it by until
//! `VmState::load_unit` gives it an id.
//!
//! Label patching: forward jumps (`JumpIf`, `JumpUnless`, `Jump`) are emitted
//! with target `0` and patched after the target instruction is known.
//!
//! Two-pass top-level define: `Begin([Define, ...])` pre-scans all `Define`
//! names so forward references within the same compilation unit work.
//! (In A2 this is a no-op because globals are resolved at runtime.)
//!
//! See VM_COMPILER.md §Pass 5.

use super::pass4_registers::{AllocatedExpr, CaptureSource, RegExpr, RegExprKind, RegLambda};
use super::primitive_calls::{InlineOp, PrimitiveCallMap, ResolvedPrimitive};
use crate::error::CompileError;
use crate::types::code_object::{Arity, CodeObject, CodeObjectId, GlobalCacheEntry};
use crate::types::instruction::{ControlForm, Instruction, TestOp};
use patina_core::core_expr::Symbol;
use patina_core::error::SourceLocation;
use patina_core::tagged_value::TaggedValue;
use std::rc::Rc;

// ─────────────────────────────────────────────────────────────────────────────
// Codegen context (per CodeObject being built)
// ─────────────────────────────────────────────────────────────────────────────

struct Codegen {
    name: Option<Symbol>,
    instructions: Vec<Instruction>,
    constants: Vec<TaggedValue>,
    /// All nested CodeObjects emitted during this compilation unit.
    /// Returned alongside the top-level CodeObject.
    nested: Vec<CodeObject>,
    /// Source location map: (pc, source_location).
    source_map: Vec<(usize, SourceLocation)>,
    /// Global callee names that resolved to registry primitives at compile
    /// time; `App`s on these emit `CallPrimitive` instead of `LoadGlobal` +
    /// `Call`. Empty when compiling without an environment.
    prim_calls: Rc<PrimitiveCallMap>,
    /// Expression lifetime ends at instruction boundaries; these generate
    /// GC metadata, not instructions on the execution path (#423).
    retirements: Vec<(usize, std::ops::Range<u16>)>,
}

impl Codegen {
    fn new(name: Option<Symbol>, prim_calls: Rc<PrimitiveCallMap>) -> Self {
        Self {
            name,
            instructions: Vec::new(),
            constants: Vec::new(),
            nested: Vec::new(),
            source_map: Vec::new(),
            prim_calls,
            retirements: Vec::new(),
        }
    }

    fn emit(&mut self, instr: Instruction) -> usize {
        let idx = self.instructions.len();
        self.instructions.push(instr);
        idx
    }

    /// Emit a placeholder jump and return its index for later patching.
    fn emit_jump_placeholder(&mut self) -> usize {
        self.emit(Instruction::Jump { target: 0 })
    }

    /// Emit a conditional jump placeholder.
    fn emit_jump_unless_placeholder(&mut self, cond: u16) -> usize {
        self.emit(Instruction::JumpUnless { cond, target: 0 })
    }

    /// Patch a previously emitted jump at `idx` to point to `target`.
    fn patch_jump(&mut self, idx: usize, target: usize) {
        match &mut self.instructions[idx] {
            Instruction::Jump { target: t } => *t = target,
            Instruction::JumpUnless { target: t, .. } => *t = target,
            Instruction::JumpIf { target: t, .. } => *t = target,
            Instruction::TestJumpUnless { target: t, .. } => *t = target,
            Instruction::JumpUnlessShadowed { target: t, .. } => *t = target,
            _ => panic!("patch_jump called on non-jump instruction at {}", idx),
        }
    }

    fn current_pc(&self) -> usize {
        self.instructions.len()
    }

    fn add_constant(&mut self, val: TaggedValue) -> u16 {
        // Deduplicate by value.
        if let Some(i) = self.constants.iter().position(|c| *c == val) {
            return i as u16;
        }
        let idx = self.constants.len() as u16;
        self.constants.push(val);
        idx
    }

    /// Record a source location for the instruction about to be emitted.
    fn record_source(&mut self, source: &Option<SourceLocation>) {
        if let Some(loc) = source {
            let pc = self.current_pc();
            // Parent and child can start at the same instruction. The
            // child is the expression that instruction actually evaluates.
            if let Some((last_pc, last_source)) = self.source_map.last_mut()
                && *last_pc == pc
            {
                *last_source = loc.clone();
            } else {
                self.source_map.push((pc, loc.clone()));
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pass entry
// ─────────────────────────────────────────────────────────────────────────────

pub struct Pass5Codegen;

impl Pass5Codegen {
    /// Compile the top-level `AllocatedExpr` into a `CodeObject`.
    ///
    /// Returns the primary `CodeObject` plus any nested ones (from lambdas).
    /// The caller should load them together with `VmState::load_unit`.
    pub fn run(
        allocated: &AllocatedExpr,
        prim_calls: PrimitiveCallMap,
    ) -> Result<(CodeObject, Vec<CodeObject>), CompileError> {
        let expr = &allocated.expr;
        let id = CodeObjectId::label();
        let mut cg = Codegen::new(None, Rc::new(prim_calls));
        gen_expr(expr, &mut cg)?;
        // Top-level: emit a Return of the expression's result.
        cg.emit(Instruction::Return { val: expr.dst });
        let global_cache = finalize_instructions(&mut cg.instructions);
        let register_roots = register_root_maps(&cg, allocated.num_regs.max(1), 0);
        let nested = cg.nested;
        let code = CodeObject {
            id,
            name: cg.name,
            global_cache,
            live_closures: std::cell::Cell::new(0),
            instructions: cg.instructions,
            constants: cg.constants,
            // Use the Pass 4 high-water mark so all temps are covered.
            num_regs: allocated.num_regs.max(1),
            arity: Arity::Fixed(0),
            source_map: cg.source_map,
            register_roots: Some(register_roots),
        };
        Ok((code, nested))
    }
}

/// Thread branch tails straight to their `Return` (Track P P5). Two
/// in-place rewrites, run to fixpoint after all jump patching:
///
/// - `Jump L` where `L` holds `Return v`        ⇒ `Return v`
/// - `Move d ← s; Jump L` where `L` holds `Return d` ⇒ `Return s`
///   (the following `Jump` becomes unreachable via fallthrough but keeps
///   its slot, so no pc shifts anywhere)
///
/// Both rewrites preserve the behavior of every path *into* the rewritten
/// pc, so jump targets need no adjustment; instructions are only replaced,
/// never added or removed. This turns the `Move`-to-join-then-`Return`
/// shape every `if` at function tail produces into a single dispatch, and
/// extends proper tail behavior to sites the P8.2 deopt's `Return`-shape
/// check can now recognize.
///
/// Invariant this pass (and any future in-place rewrite) must not break:
/// a `TestJumpUnless` at pc `i` requires the `JumpUnless` at `i + 1` to
/// survive untouched — it is the fused site's deopt landing (and the pc+2
/// skip target). Neither rewrite here touches conditional jumps.
fn thread_returns(instructions: &mut [Instruction]) {
    loop {
        let mut changed = false;
        for i in 0..instructions.len() {
            if let Instruction::Jump { target } = instructions[i]
                && let Instruction::Return { val } = instructions[target]
            {
                instructions[i] = Instruction::Return { val };
                changed = true;
                continue;
            }
            if i + 1 < instructions.len()
                && let Instruction::Move { dst, src } = instructions[i]
                && let Instruction::Jump { target } = instructions[i + 1]
                && let Instruction::Return { val } = instructions[target]
                && val == dst
            {
                instructions[i] = Instruction::Return { val: src };
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

/// Finalize a finished instruction stream: run the in-place rewrites, then
/// build the pc-aligned global cache table. One owner for the ordering so
/// both CodeObject construction sites (top-level and lambda) stay in step.
fn finalize_instructions(
    instructions: &mut [Instruction],
) -> Vec<std::cell::Cell<GlobalCacheEntry>> {
    thread_returns(instructions);
    GlobalCacheEntry::table(instructions)
}

/// Forward may-root analysis over the final bytecode. A write makes its slot
/// a possible root; an expression's lifetime end retires its temporaries.
/// Union at joins preserves a value needed on either path. This deliberately
/// keeps local bindings through the frame's lifetime, rather than predicting
/// their last use. No instructions are added to the normal execution path.
fn register_root_maps(cg: &Codegen, num_regs: u16, num_params: u16) -> Vec<Vec<u64>> {
    use std::collections::VecDeque;
    let len = cg.instructions.len();
    let mut retire = vec![Vec::new(); len + 1];
    for (pc, range) in &cg.retirements {
        retire[*pc].push(range.clone());
    }
    let mut incoming = vec![vec![0u64; (num_regs as usize).div_ceil(64)]; len + 1];
    for reg in 0..num_params as usize {
        incoming[0][reg / 64] |= 1 << (reg % 64);
    }
    let mut reached = vec![false; len + 1];
    reached[0] = true;
    let mut queue = VecDeque::from([0]);
    while let Some(pc) = queue.pop_front() {
        let mut roots = incoming[pc].clone();
        for range in &retire[pc] {
            retire_root_range(&mut roots, range);
        }
        let Some(instruction) = cg.instructions.get(pc) else {
            continue;
        };
        if let Some(dst) = written_register(instruction) {
            roots[dst as usize / 64] |= 1 << (dst % 64);
        }
        // A fused predicate has three successors: fast true skips the kept
        // branch, fast false jumps, and a slow/deopt result uses that branch.
        let successors: &[usize] = match *instruction {
            Instruction::Jump { target } => &[target],
            Instruction::JumpIf { target, .. }
            | Instruction::JumpUnless { target, .. }
            | Instruction::JumpUnlessShadowed { target, .. } => &[pc + 1, target],
            Instruction::TestJumpUnless { target, .. } => &[pc + 1, pc + 2, target],
            Instruction::Return { .. }
            | Instruction::TailCall { .. }
            | Instruction::TailApply { .. }
            | Instruction::TailCallWithValues { .. } => &[],
            _ => &[pc + 1],
        };
        for &next in successors {
            let mut changed = !reached[next];
            reached[next] = true;
            for (dest, &root) in incoming[next].iter_mut().zip(&roots) {
                changed |= root & !*dest != 0;
                *dest |= root;
            }
            if changed {
                queue.push_back(next);
            }
        }
    }
    for (pc, roots) in incoming.iter_mut().enumerate() {
        if !reached[pc] {
            // Runtime-created control landings are not compiler CFG edges.
            // Be conservative if a future one lands in otherwise dead code.
            roots.fill(u64::MAX);
        } else {
            for range in &retire[pc] {
                retire_root_range(roots, range);
            }
        }
    }
    incoming
}

fn retire_root_range(roots: &mut [u64], range: &std::ops::Range<u16>) {
    for reg in range.clone() {
        roots[reg as usize / 64] &= !(1 << (reg % 64));
    }
}

/// Exhaustive so a new destination-bearing opcode cannot silently lose a
/// root. Calls mark their result even while waiting for the callee: its old
/// slot is conservative until the result (or continuation value) arrives.
fn written_register(instruction: &Instruction) -> Option<u16> {
    use Instruction::*;
    match *instruction {
        LoadConst { dst, .. }
        | LoadImmediate { dst, .. }
        | Move { dst, .. }
        | LoadClosure { dst, .. }
        | LoadGlobal { dst, .. }
        | AllocCell { dst, .. }
        | ReadCell { dst, .. }
        | MakeClosure { dst, .. }
        | Call { dst, .. }
        | Apply { dst, .. }
        | CallPrimitive { dst, .. }
        | CallPrimitiveDirect { dst, .. }
        | Add { dst, .. }
        | Sub { dst, .. }
        | Mul { dst, .. }
        | Lt { dst, .. }
        | NumEq { dst, .. }
        | Eq { dst, .. }
        | Cons { dst, .. }
        | Car { dst, .. }
        | Cdr { dst, .. }
        | Not { dst, .. }
        | TestJumpUnless { dst, .. }
        | AddImm { dst, .. }
        | SubImm { dst, .. }
        | LtImm { dst, .. }
        | NumEqImm { dst, .. }
        | NullP { dst, .. }
        | PairP { dst, .. }
        | VectorP { dst, .. }
        | VectorRef { dst, .. }
        | VectorSet { dst, .. }
        | CallWithValues { dst, .. }
        | AbortToPrompt { dst, .. }
        | CaptureComposable { dst, .. } => Some(dst),
        StoreClosure { .. }
        | StoreGlobal { .. }
        | WriteCell { .. }
        | Jump { .. }
        | JumpIf { .. }
        | JumpUnless { .. }
        | JumpUnlessShadowed { .. }
        | TailCall { .. }
        | TailApply { .. }
        | Return { .. }
        | TailCallWithValues { .. }
        | PushWind { .. }
        | PopWind
        | Define { .. }
        | InvokeContinuation { .. }
        | Nop => None,
        ResumeWindJump
        | ResumeComposableInvoke
        | ResumeRaise
        | ResumeForce
        | ResumePrimitive
        | CollectAtCall { .. } => {
            unreachable!("runtime stubs use conservative register roots")
        }
    }
}

/// Fold a just-emitted predicate opcode into a fused test+branch
/// (Track P P5 wave 2), when the last instruction is a fusable test writing
/// the register the branch is about to read. Returns the fused
/// instruction's index for jump patching, or `None` when no fusion applies
/// (any other test shape simply keeps the unfused opcode + `JumpUnless`).
///
/// The caller must still emit the plain `JumpUnless` immediately after:
/// the fused instruction's slow and deopt paths fall through to it.
fn fuse_test_into_branch(cond: u16, cg: &mut Codegen) -> Option<usize> {
    // Which opcodes are fusable, and their operands. Or-patterns bind the
    // same names across each shape, so adding a predicate is one alternative
    // in the matching group; anything else (arithmetic, `car`, vector ops)
    // never produces a branch condition and stays unfused.
    let (test, a, b, dst, func_id) = match cg.instructions.last()? {
        Instruction::Not {
            src, dst, func_id, ..
        } => (TestOp::Not, *src, 0, *dst, *func_id),
        Instruction::NullP {
            src, dst, func_id, ..
        } => (TestOp::NullP, *src, 0, *dst, *func_id),
        Instruction::PairP {
            src, dst, func_id, ..
        } => (TestOp::PairP, *src, 0, *dst, *func_id),
        Instruction::VectorP {
            src, dst, func_id, ..
        } => (TestOp::VectorP, *src, 0, *dst, *func_id),
        Instruction::Eq {
            a, b, dst, func_id, ..
        } => (TestOp::Eq, *a, *b, *dst, *func_id),
        Instruction::Lt {
            a, b, dst, func_id, ..
        } => (TestOp::Lt, *a, *b, *dst, *func_id),
        Instruction::NumEq {
            a, b, dst, func_id, ..
        } => (TestOp::NumEq, *a, *b, *dst, *func_id),
        _ => return None,
    };
    if dst != cond {
        return None;
    }
    // Pop by value so the predicate's `name` moves into the fused
    // instruction rather than being cloned.
    let name = match cg.instructions.pop() {
        Some(
            Instruction::Not { name, .. }
            | Instruction::NullP { name, .. }
            | Instruction::PairP { name, .. }
            | Instruction::VectorP { name, .. }
            | Instruction::Eq { name, .. }
            | Instruction::Lt { name, .. }
            | Instruction::NumEq { name, .. },
        ) => name,
        _ => unreachable!("just matched a fusable test"),
    };
    Some(cg.emit(Instruction::TestJumpUnless {
        test,
        a,
        b,
        dst,
        target: 0,
        func_id,
        name,
    }))
}

/// Emit whatever code the arguments of a resolved-primitive call need and
/// decide how they reach the instruction (Track P P5): the operand
/// registers, plus at most one absorbed literal — always the *right*
/// operand of one of the `InlineOp::has_imm_form` ops.
///
/// When every argument is an atom (`writes_only_dst` — no argument's
/// evaluation can clobber another's source), `LocalRef`s are read in place
/// (no staging `Move`; both `CallPrimitive` and the inline opcodes accept
/// arbitrary registers). Any non-atomic argument (a nested call, `set!`,
/// `begin`, …) falls the whole call back to fully staged temps, preserving
/// left-to-right evaluation and its side-effect ordering.
///
/// Why only the right operand, even for commutative ops: the shadow-bit
/// deopt passes `[a, imm]` to whatever the name is bound to at that point,
/// and a user rebind need not be commutative — operand order must survive
/// exactly. Why only fixnum literals: instructions must never embed
/// heap-referencing `TaggedValue`s (the constant pool is what roots those —
/// see the `is_immediate` split in the `Literal` arm); `is_immediate` would
/// be the widest safe bound, and fixnums are the profitable subset.
fn primitive_operands(
    args: &[RegExpr],
    arg_tmps: &[u16],
    inline: Option<InlineOp>,
    cg: &mut Codegen,
) -> Result<(Vec<u16>, Option<TaggedValue>), CompileError> {
    if !args.iter().all(|a| writes_only_dst(&a.kind)) {
        for arg in args {
            gen_expr(arg, cg)?;
        }
        return Ok((arg_tmps.to_vec(), None));
    }
    let absorbed = match (inline, args.last()) {
        (Some(op), Some(last)) if op.has_imm_form() => match &last.kind {
            RegExprKind::Literal(v) if v.is_fixnum() => Some(*v),
            _ => None,
        },
        _ => None,
    };
    let reg_args = match absorbed {
        Some(_) => &args[..args.len() - 1],
        None => args,
    };
    let regs = reg_args
        .iter()
        .enumerate()
        .map(|(i, arg)| {
            if let RegExprKind::LocalRef { src } = &arg.kind {
                return Ok(*src);
            }
            gen_expr(arg, cg)?;
            Ok(arg_tmps[i])
        })
        .collect::<Result<Vec<u16>, CompileError>>()?;
    Ok((regs, absorbed))
}

/// Build the instruction for a call to a compile-time-resolved primitive:
/// the specialized inline opcode when the primitive has one and the call
/// site has exactly the fixed arity (Track P P3) — in its imm form when
/// `primitive_operands` absorbed a literal (Track P P5) — and
/// `CallPrimitive` otherwise. `inline` must be the same arity-filtered
/// value handed to `primitive_operands`.
fn primitive_call_instruction(
    inline: Option<InlineOp>,
    resolved: ResolvedPrimitive,
    name: Symbol,
    regs: &[u16],
    imm: Option<TaggedValue>,
    dst: u16,
) -> Instruction {
    let func_id = resolved.id;
    if let Some(imm) = imm {
        let a = regs[0];
        // `primitive_operands` absorbs a literal only for the
        // `has_imm_form` ops, so this match covers exactly that set.
        return match inline.expect("absorbed literal implies an inline op") {
            InlineOp::Add => Instruction::AddImm {
                a,
                imm,
                dst,
                func_id,
                name,
            },
            InlineOp::Sub => Instruction::SubImm {
                a,
                imm,
                dst,
                func_id,
                name,
            },
            InlineOp::Lt => Instruction::LtImm {
                a,
                imm,
                dst,
                func_id,
                name,
            },
            InlineOp::NumEq => Instruction::NumEqImm {
                a,
                imm,
                dst,
                func_id,
                name,
            },
            op => unreachable!("no imm form for {:?}", op),
        };
    }
    let Some(op) = inline else {
        return Instruction::CallPrimitive {
            func_id,
            name,
            args: regs.to_vec(),
            dst,
        };
    };
    match op {
        InlineOp::Add => Instruction::Add {
            a: regs[0],
            b: regs[1],
            dst,
            func_id,
            name,
        },
        InlineOp::Sub => Instruction::Sub {
            a: regs[0],
            b: regs[1],
            dst,
            func_id,
            name,
        },
        InlineOp::Mul => Instruction::Mul {
            a: regs[0],
            b: regs[1],
            dst,
            func_id,
            name,
        },
        InlineOp::Lt => Instruction::Lt {
            a: regs[0],
            b: regs[1],
            dst,
            func_id,
            name,
        },
        InlineOp::NumEq => Instruction::NumEq {
            a: regs[0],
            b: regs[1],
            dst,
            func_id,
            name,
        },
        InlineOp::Eq => Instruction::Eq {
            a: regs[0],
            b: regs[1],
            dst,
            func_id,
            name,
        },
        InlineOp::Cons => Instruction::Cons {
            a: regs[0],
            b: regs[1],
            dst,
            func_id,
            name,
        },
        InlineOp::Car => Instruction::Car {
            src: regs[0],
            dst,
            func_id,
            name,
        },
        InlineOp::Cdr => Instruction::Cdr {
            src: regs[0],
            dst,
            func_id,
            name,
        },
        InlineOp::Not => Instruction::Not {
            src: regs[0],
            dst,
            func_id,
            name,
        },
        InlineOp::NullP => Instruction::NullP {
            src: regs[0],
            dst,
            func_id,
            name,
        },
        InlineOp::PairP => Instruction::PairP {
            src: regs[0],
            dst,
            func_id,
            name,
        },
        InlineOp::VectorP => Instruction::VectorP {
            src: regs[0],
            dst,
            func_id,
            name,
        },
        InlineOp::VectorRef => Instruction::VectorRef {
            v: regs[0],
            i: regs[1],
            dst,
            func_id,
            name,
        },
        InlineOp::VectorSet => Instruction::VectorSet {
            v: regs[0],
            i: regs[1],
            val: regs[2],
            dst,
            func_id,
            name,
        },
    }
}

/// True when `gen_expr` for this kind emits code that writes *only* the
/// expression's own `dst` register — no other local can be clobbered.
/// This is the atomicity gate for in-place operands: `primitive_operands`
/// may read a `LocalRef`'s home register at op-execution time only if no
/// sibling argument's code can have written it in between.
///
/// Every kind listed here is a claim about the corresponding `gen_expr`
/// arm below; an arm that grows a scratch write to any register other than
/// `expr.dst` must be removed from this list (`ReadClosureCell` writes
/// `expr.dst` twice — its own dst as scratch — which is exactly the limit
/// of what remains safe).
fn writes_only_dst(kind: &RegExprKind) -> bool {
    matches!(
        kind,
        RegExprKind::Literal(_)
            | RegExprKind::Quote(_)
            | RegExprKind::LocalRef { .. }
            | RegExprKind::ClosureRef { .. }
            | RegExprKind::GlobalRef { .. }
            | RegExprKind::ReadLocalCell { .. }
            | RegExprKind::ReadClosureCell { .. }
    )
}

/// Generate instructions for `expr` into `cg`.
fn gen_expr(expr: &RegExpr, cg: &mut Codegen) -> Result<(), CompileError> {
    gen_expr_value(expr, cg)?;
    // Tail transfers replace the frame. Its callee starts with only the
    // parameter slots live, so no caller retirement point is needed.
    if !matches!(
        expr.kind,
        RegExprKind::App { is_tail: true, .. } | RegExprKind::Apply { is_tail: true, .. }
    ) {
        retire_temporaries(expr.dst + 1, expr.temp_end, cg);
    }
    Ok(())
}

fn retire_temporaries(start: u16, end: u16, cg: &mut Codegen) {
    if start < end {
        cg.retirements.push((cg.current_pc(), start..end));
    }
}

/// A sequence discards the expression's result as well as its temporaries.
fn gen_discarded_expr(expr: &RegExpr, cg: &mut Codegen) -> Result<(), CompileError> {
    gen_expr_value(expr, cg)?;
    retire_temporaries(expr.dst, expr.temp_end, cg);
    Ok(())
}

fn gen_expr_value(expr: &RegExpr, cg: &mut Codegen) -> Result<(), CompileError> {
    // Grown with the depth of the code (#617).
    patina_core::walk::ensure_sufficient_stack(|| gen_expr_value_inner(expr, cg))
}

fn gen_expr_value_inner(expr: &RegExpr, cg: &mut Codegen) -> Result<(), CompileError> {
    // Record source location before emitting instructions for this expression.
    cg.record_source(&expr.source);

    match &expr.kind {
        RegExprKind::Literal(v) => {
            // Immediates inline; heap values go via constant pool.
            if v.is_immediate() {
                cg.emit(Instruction::LoadImmediate {
                    dst: expr.dst,
                    val: *v,
                });
            } else {
                let idx = cg.add_constant(*v);
                cg.emit(Instruction::LoadConst { dst: expr.dst, idx });
            }
        }

        RegExprKind::Quote(v) => {
            let idx = cg.add_constant(*v);
            cg.emit(Instruction::LoadConst { dst: expr.dst, idx });
        }

        RegExprKind::Quasiquote => {
            // Quasiquotes must be expanded before codegen; callers should use
            // `compile_with_qq_resolving` rather than the lower-level `compile`.
            return Err(CompileError::Internal(
                "Quasiquote reached codegen unexpanded; use compile_with_qq_resolving".into(),
            ));
        }

        RegExprKind::LocalRef { src } => {
            cg.emit(Instruction::Move {
                dst: expr.dst,
                src: *src,
            });
        }

        RegExprKind::ClosureRef { slot } => {
            cg.emit(Instruction::LoadClosure {
                dst: expr.dst,
                slot: *slot,
            });
        }

        RegExprKind::GlobalRef { name } => {
            cg.emit(Instruction::LoadGlobal {
                dst: expr.dst,
                name: name.clone(),
            });
        }

        RegExprKind::Lambda(lam) => {
            gen_lambda(lam, expr.dst, cg)?;
        }

        RegExprKind::If { test, then, else_ } => {
            // Evaluate test.
            gen_expr_value(test, cg)?;
            // A predicate feeding the branch fuses into `TestJumpUnless`,
            // which branches directly on the fast path (Track P P5). The
            // plain `JumpUnless` is still emitted right after it: the fused
            // fast path skips it, while the deopt and slow paths fall
            // through to it (see the instruction's doc). Jumps into the
            // fused pc are safe — it writes `dst` and branches exactly like
            // the pair it replaced.
            let fused = fuse_test_into_branch(test.dst, cg);
            // Jump to else if false.
            let jump_else = cg.emit_jump_unless_placeholder(test.dst);
            // The test is consumed on either edge. Keep the predicate and
            // branch adjacent so fused tests retain their deopt landing.
            retire_temporaries(test.dst, test.temp_end, cg);
            // Then branch.
            gen_expr(then, cg)?;
            // Jump over else.
            let jump_end = cg.emit_jump_placeholder();
            // Else branch.
            let else_start = cg.current_pc();
            cg.patch_jump(jump_else, else_start);
            if let Some(idx) = fused {
                cg.patch_jump(idx, else_start);
            }
            retire_temporaries(test.dst, test.temp_end, cg);
            gen_expr(else_, cg)?;
            let end = cg.current_pc();
            cg.patch_jump(jump_end, end);
        }

        RegExprKind::ReadLocalCell { src } => {
            cg.emit(Instruction::ReadCell {
                dst: expr.dst,
                cell: *src,
            });
        }

        RegExprKind::ReadClosureCell { slot } => {
            // Load the cell from the closure slot into dst, then read through it.
            // We use dst as scratch for the cell pointer itself, then overwrite with content.
            cg.emit(Instruction::LoadClosure {
                dst: expr.dst,
                slot: *slot,
            });
            cg.emit(Instruction::ReadCell {
                dst: expr.dst,
                cell: expr.dst,
            });
        }

        RegExprKind::SetLocal { value, var_reg } => {
            gen_expr(value, cg)?;
            cg.emit(Instruction::Move {
                dst: *var_reg,
                src: value.dst,
            });
            cg.emit(Instruction::LoadImmediate {
                dst: expr.dst,
                val: TaggedValue::UNSPECIFIED,
            });
        }

        RegExprKind::WriteLocalCell { value, var_reg } => {
            gen_expr(value, cg)?;
            cg.emit(Instruction::WriteCell {
                cell: *var_reg,
                src: value.dst,
            });
            cg.emit(Instruction::LoadImmediate {
                dst: expr.dst,
                val: TaggedValue::UNSPECIFIED,
            });
        }

        RegExprKind::WriteClosureCell { slot, value } => {
            gen_expr(value, cg)?;
            // Load cell pointer from closure slot into a scratch reg (expr.dst).
            cg.emit(Instruction::LoadClosure {
                dst: expr.dst,
                slot: *slot,
            });
            cg.emit(Instruction::WriteCell {
                cell: expr.dst,
                src: value.dst,
            });
            cg.emit(Instruction::LoadImmediate {
                dst: expr.dst,
                val: TaggedValue::UNSPECIFIED,
            });
        }

        RegExprKind::SetGlobal { name, value } => {
            gen_expr(value, cg)?;
            cg.record_source(&expr.source);
            cg.emit(Instruction::StoreGlobal {
                name: name.clone(),
                src: value.dst,
            });
            cg.emit(Instruction::LoadImmediate {
                dst: expr.dst,
                val: TaggedValue::UNSPECIFIED,
            });
        }

        RegExprKind::Begin(exprs) => {
            for (i, e) in exprs.iter().enumerate() {
                if i + 1 == exprs.len() {
                    gen_expr(e, cg)?;
                } else {
                    gen_discarded_expr(e, cg)?;
                }
            }
        }

        RegExprKind::Define { name, value } => {
            gen_expr(value, cg)?;
            cg.emit(Instruction::Define {
                name: name.clone(),
                src: value.dst,
            });
            cg.emit(Instruction::LoadImmediate {
                dst: expr.dst,
                val: TaggedValue::UNSPECIFIED,
            });
        }

        RegExprKind::App {
            func,
            args,
            arg_tmps,
            is_tail,
        } => {
            // `call-with-values` and `dynamic-wind`, where the operator is
            // bound to them: see `gen_inline_control`.
            if let RegExprKind::GlobalRef { name } = &func.kind
                && let Some(&form) = cg.prim_calls.control.get(name)
                && form.arity() == args.len()
            {
                return gen_inline_control(form, func, args, arg_tmps, *is_tail, expr.dst, cg);
            }

            // Statically-known primitive: skip the callee LoadGlobal and the
            // frame push entirely. In tail position a primitive cannot capture
            // the continuation (control primitives are excluded from the map),
            // so the emitted call + Return is equivalent to TailCall. The
            // shadow-bit deopt path relies on this exact `<op> dst; Return
            // dst` pair to recognize tail sites and keep them proper tail
            // calls when the rebound callee is a closure (PRD P8.2, see
            // `exec_call_primitive`).
            // A primitive the code names as a value (see
            // `Instruction::CallPrimitiveDirect`): the same skip, with
            // nothing to deoptimize to.
            if let RegExprKind::Literal(v) = &func.kind
                && let Some(&func_id) = cg.prim_calls.by_value.get(&v.raw_bits())
            {
                let (regs, _) = primitive_operands(args, arg_tmps, None, cg)?;
                cg.record_source(&expr.source);
                cg.emit(Instruction::CallPrimitiveDirect {
                    func_id,
                    args: regs,
                    dst: expr.dst,
                });
                if *is_tail {
                    cg.emit(Instruction::Return { val: expr.dst });
                }
                return Ok(());
            }

            if let RegExprKind::GlobalRef { name } = &func.kind
                && let Some(&resolved) = cg.prim_calls.by_name.get(name)
            {
                let inline = resolved.inline.filter(|op| op.arity() == args.len());
                let (regs, imm) = primitive_operands(args, arg_tmps, inline, cg)?;
                cg.record_source(&expr.source);
                cg.emit(primitive_call_instruction(
                    inline,
                    resolved,
                    name.clone(),
                    &regs,
                    imm,
                    expr.dst,
                ));
                if *is_tail {
                    cg.emit(Instruction::Return { val: expr.dst });
                }
                return Ok(());
            }

            // General case: evaluate function and arguments, emit Call/TailCall.
            gen_expr(func, cg)?;
            for arg in args {
                gen_expr(arg, cg)?;
            }
            cg.record_source(&expr.source);
            let arg_regs: Vec<u16> = arg_tmps.clone();
            if *is_tail {
                cg.emit(Instruction::TailCall {
                    func: func.dst,
                    args: arg_regs,
                });
            } else {
                cg.emit(Instruction::Call {
                    func: func.dst,
                    args: arg_regs,
                    dst: expr.dst,
                });
            }
        }

        RegExprKind::Apply {
            func,
            args,
            arg_tmps,
            is_tail,
        } => {
            gen_expr(func, cg)?;
            for arg in args {
                gen_expr(arg, cg)?;
            }
            cg.record_source(&expr.source);
            let arg_regs: Vec<u16> = arg_tmps.clone();
            if *is_tail {
                cg.emit(Instruction::TailApply {
                    func: func.dst,
                    args: arg_regs,
                });
            } else {
                cg.emit(Instruction::Apply {
                    func: func.dst,
                    args: arg_regs,
                    dst: expr.dst,
                });
            }
        }
    }
    Ok(())
}

/// A call whose operator was bound to `call-with-values` or `dynamic-wind`
/// when it was compiled: the form's own instruction sequence, which calls
/// the thunks as frames of this procedure rather than through the value
/// form's stub, behind a guard that the operator still is that (#442).
///
/// ```text
///     …operands…
///     JumpUnlessShadowed form → SEQ
///     LoadGlobal   f ← name          ; an ordinary call, once the form's
///     Call / TailCall f(operands)    ; procedure has been rebound anywhere
///     Jump         → END             ; (not in tail position, which returns)
/// SEQ:
///     …the form's sequence…
/// END:
/// ```
///
/// The guard is what lets the sequence follow the binding and not the
/// spelling. Until #442 the sequence was emitted for any global *spelled*
/// `call-with-values`: a program's own definition was never called, and the
/// procedure reached under another name went the slower value path. Now a
/// renamed import, or the alias early binding gives a library template's
/// reference (#438), gets the sequence, and a program that defines or
/// `set!`s the name after the site was compiled gets its own procedure
/// called.
///
/// Why a shadow mark and not the operator's value. Measured on loops that do
/// nothing but the form, against the sequence with no guard at all: calling
/// every one through the value form's stub instead was 16–38% slower (19% on
/// SRFI 1's multi-list `fold`); loading the operator and comparing it with
/// the procedure the name was bound to, 4–6%; this, which reads one bit and
/// loads the operator only when it has to call it, at most about 3%.
///
/// **The value form runs the same instructions** from a stub the runtime
/// builds — `value_wind_stub` and `value_cwv_stub` in `runtime/control.rs` —
/// which is what makes the two forms agree, and it is kept in step by hand.
/// Changing a sequence here means changing it there. See those functions for
/// the deliberate differences (a dedicated discard slot, and the stub's
/// always being in tail position).
fn gen_inline_control(
    form: ControlForm,
    func: &RegExpr,
    args: &[RegExpr],
    arg_tmps: &[u16],
    is_tail: bool,
    dst: u16,
    cg: &mut Codegen,
) -> Result<(), CompileError> {
    for arg in args {
        gen_expr(arg, cg)?;
    }
    let to_sequence = cg.emit(Instruction::JumpUnlessShadowed { form, target: 0 });
    // The operator is loaded after the operands, and only here: `func.dst` is
    // live across the operands in the general case, so it is none of theirs.
    // Every other call loads it first, and so does the tree-walker; an
    // operand that rebinds the operator can tell the difference, which R7RS
    // leaves unspecified (§4.1.3), and loading it only on this branch is the
    // cost the sequence saves.
    gen_expr(func, cg)?;
    let arg_regs = arg_tmps.to_vec();
    let to_end = if is_tail {
        cg.emit(Instruction::TailCall {
            func: func.dst,
            args: arg_regs,
        });
        None
    } else {
        cg.emit(Instruction::Call {
            func: func.dst,
            args: arg_regs,
            dst,
        });
        Some(cg.emit_jump_placeholder())
    };
    let sequence = cg.current_pc();
    cg.patch_jump(to_sequence, sequence);
    match form {
        ControlForm::CallWithValues => {
            let producer_reg = arg_tmps[0];
            let consumer_reg = arg_tmps[1];
            // Call producer (0 args), result in dst.
            cg.emit(Instruction::Call {
                func: producer_reg,
                args: vec![],
                dst,
            });
            // Call consumer with the values the producer returned.
            if is_tail {
                cg.emit(Instruction::TailCallWithValues {
                    consumer: consumer_reg,
                    producer_result: dst,
                });
            } else {
                cg.emit(Instruction::CallWithValues {
                    dst,
                    consumer: consumer_reg,
                    producer_result: dst,
                });
            }
        }
        ControlForm::DynamicWind => {
            let before_reg = arg_tmps[0];
            let body_reg = arg_tmps[1];
            let after_reg = arg_tmps[2];
            // Call before-thunk (result discarded).
            cg.emit(Instruction::Call {
                func: before_reg,
                args: vec![],
                dst,
            });
            // Push wind record.
            cg.emit(Instruction::PushWind {
                before: before_reg,
                after: after_reg,
            });
            // Call body-thunk, result in dst.
            cg.emit(Instruction::Call {
                func: body_reg,
                args: vec![],
                dst,
            });
            // Pop wind record (does not call after-thunk).
            cg.emit(Instruction::PopWind);
            // Call after-thunk (result discarded, goes into before_reg).
            cg.emit(Instruction::Call {
                func: after_reg,
                args: vec![],
                dst: before_reg,
            });
            // dst still holds body result.
            if is_tail {
                cg.emit(Instruction::Return { val: dst });
            }
        }
    }
    if let Some(to_end) = to_end {
        let end = cg.current_pc();
        cg.patch_jump(to_end, end);
    }
    Ok(())
}

/// Compile a nested lambda, emit `MakeClosure` into `cg`, result in `dst`.
fn gen_lambda(lam: &RegLambda, dst: u16, cg: &mut Codegen) -> Result<(), CompileError> {
    let child_id = CodeObjectId::label();
    let mut child_cg = Codegen::new(None, Rc::clone(&cg.prim_calls));

    // Prologue: wrap each boxed param register in a MutableCell.
    // The param already lives in reg[r]; emit AllocCell r←r to box it in-place.
    for &reg in &lam.boxed_params {
        // Internal define regs that are boxed: initialize to UNSPECIFIED first,
        // then AllocCell will box that value.
        if lam.internal_define_regs.contains(&reg) {
            child_cg.emit(Instruction::LoadImmediate {
                dst: reg,
                val: TaggedValue::UNSPECIFIED,
            });
        }
        child_cg.emit(Instruction::AllocCell { dst: reg, src: reg });
    }

    // Initialize non-boxed internal-define registers to UNSPECIFIED.
    for &reg in &lam.internal_define_regs {
        if !lam.boxed_params.contains(&reg) {
            child_cg.emit(Instruction::LoadImmediate {
                dst: reg,
                val: TaggedValue::UNSPECIFIED,
            });
        }
    }

    // Generate body instructions for the child.
    let body_len = lam.body.len();
    for (i, e) in lam.body.iter().enumerate() {
        if i == body_len - 1 {
            gen_expr(e, &mut child_cg)?;
            // Return last result.
            child_cg.emit(Instruction::Return { val: e.dst });
        } else {
            gen_discarded_expr(e, &mut child_cg)?;
        }
    }

    let arity = if lam.rest_param {
        Arity::Variadic(lam.num_params.saturating_sub(1))
    } else {
        Arity::Fixed(lam.num_params)
    };

    let global_cache = finalize_instructions(&mut child_cg.instructions);
    let register_roots = register_root_maps(&child_cg, lam.num_regs, lam.num_params);
    let child_code = CodeObject {
        id: child_id,
        name: None,
        global_cache,
        live_closures: std::cell::Cell::new(0),
        instructions: child_cg.instructions,
        constants: child_cg.constants,
        num_regs: lam.num_regs,
        arity,
        source_map: child_cg.source_map,
        register_roots: Some(register_roots),
    };

    // Collect nested from child.
    cg.nested.push(child_code);
    cg.nested.extend(child_cg.nested);

    // Emit instructions to load each captured free variable into a scratch
    // register in the *parent* frame, then emit MakeClosure.
    let mut capture_regs: Vec<u16> = Vec::with_capacity(lam.captures.len());
    for (i, src) in lam.captures.iter().enumerate() {
        let scratch = dst + 1 + i as u16;
        match src {
            CaptureSource::ParentReg(reg) => {
                cg.emit(Instruction::Move {
                    dst: scratch,
                    src: *reg,
                });
            }
            CaptureSource::ParentClosureSlot(slot) => {
                cg.emit(Instruction::LoadClosure {
                    dst: scratch,
                    slot: *slot,
                });
            }
            CaptureSource::Global(name) => {
                cg.emit(Instruction::LoadGlobal {
                    dst: scratch,
                    name: name.clone(),
                });
            }
        }
        capture_regs.push(scratch);
    }

    cg.emit(Instruction::MakeClosure {
        dst,
        code_id: child_id,
        free_vars: capture_regs,
    });
    Ok(())
}
