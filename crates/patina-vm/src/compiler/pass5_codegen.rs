//! Pass 5 — Code Generation
//!
//! Walks `RegExpr` and emits `Instruction`s into `CodeObject`s. Nested lambdas
//! are compiled recursively; each gets its own `CodeObjectId` and is referenced
//! by a `MakeClosure` instruction in the parent.
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
use crate::error::CompileError;
use crate::types::code_object::{Arity, CodeObject, CodeObjectId};
use crate::types::instruction::Instruction;
use patina_core::core_expr::Symbol;
use patina_core::tagged_value::TaggedValue;
use std::sync::atomic::{AtomicU32, Ordering};

// ─────────────────────────────────────────────────────────────────────────────
// Global CodeObjectId counter (one per process — fine for tests)
// ─────────────────────────────────────────────────────────────────────────────

static CODE_ID_COUNTER: AtomicU32 = AtomicU32::new(0);

fn fresh_code_id() -> CodeObjectId {
    CodeObjectId(CODE_ID_COUNTER.fetch_add(1, Ordering::Relaxed))
}

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
}

impl Codegen {
    fn new(_id: CodeObjectId, name: Option<Symbol>) -> Self {
        Self {
            name,
            instructions: Vec::new(),
            constants: Vec::new(),
            nested: Vec::new(),
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
}

// ─────────────────────────────────────────────────────────────────────────────
// Pass entry
// ─────────────────────────────────────────────────────────────────────────────

pub struct Pass5Codegen;

impl Pass5Codegen {
    /// Compile the top-level `AllocatedExpr` into a `CodeObject`.
    ///
    /// Returns the primary `CodeObject` plus any nested ones (from lambdas).
    /// The caller should load all of them into `VmState::code_store`.
    pub fn run(allocated: &AllocatedExpr) -> Result<(CodeObject, Vec<CodeObject>), CompileError> {
        let expr = &allocated.expr;
        let id = fresh_code_id();
        let mut cg = Codegen::new(id, None);
        gen_expr(expr, &mut cg)?;
        // Top-level: emit a Return of the expression's result.
        cg.emit(Instruction::Return { val: expr.dst });
        let nested = cg.nested;
        let code = CodeObject {
            id,
            name: cg.name,
            instructions: cg.instructions,
            constants: cg.constants,
            // Use the Pass 4 high-water mark so all temps are covered.
            num_regs: allocated.num_regs.max(1),
            arity: Arity::Fixed(0),
            source_map: vec![],
        };
        Ok((code, nested))
    }
}

/// Generate instructions for `expr` into `cg`.
fn gen_expr(expr: &RegExpr, cg: &mut Codegen) -> Result<(), CompileError> {
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

        RegExprKind::Quasiquote(_v) => {
            // TODO(A3): implement quasiquote lowering
            cg.emit(Instruction::LoadImmediate {
                dst: expr.dst,
                val: TaggedValue::UNSPECIFIED,
            });
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
            gen_expr(test, cg)?;
            // Jump to else if false.
            let jump_else = cg.emit_jump_unless_placeholder(test.dst);
            // Then branch.
            gen_expr(then, cg)?;
            // Jump over else.
            let jump_end = cg.emit_jump_placeholder();
            // Else branch.
            let else_start = cg.current_pc();
            cg.patch_jump(jump_else, else_start);
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
            for e in exprs {
                gen_expr(e, cg)?;
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
            // Evaluate function.
            gen_expr(func, cg)?;
            // Evaluate each argument into its temp.
            for arg in args {
                gen_expr(arg, cg)?;
            }
            let arg_regs: Vec<u16> = arg_tmps.clone();
            if *is_tail {
                // The VM's TailCall dispatch reads arg values from `arg_tmps`,
                // collects them all first, then writes to the new frame's r0..r(n-1).
                // This means we can pass arg_tmps directly without pre-moving —
                // and avoids clobbering func.dst if it falls in the param slot range.
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

/// Compile a nested lambda, emit `MakeClosure` into `cg`, result in `dst`.
fn gen_lambda(lam: &RegLambda, dst: u16, cg: &mut Codegen) -> Result<(), CompileError> {
    let child_id = fresh_code_id();
    let mut child_cg = Codegen::new(child_id, None);

    // Prologue: wrap each boxed param register in a MutableCell.
    // The param already lives in reg[r]; emit AllocCell r←r to box it in-place.
    for &reg in &lam.boxed_params {
        child_cg.emit(Instruction::AllocCell { dst: reg, src: reg });
    }

    // Generate body instructions for the child.
    let body_len = lam.body.len();
    for (i, e) in lam.body.iter().enumerate() {
        gen_expr(e, &mut child_cg)?;
        if i == body_len - 1 {
            // Return last result.
            child_cg.emit(Instruction::Return { val: e.dst });
        }
    }

    let arity = if lam.rest_param {
        Arity::Variadic(lam.num_params.saturating_sub(1))
    } else {
        Arity::Fixed(lam.num_params)
    };

    let child_code = CodeObject {
        id: child_id,
        name: None,
        instructions: child_cg.instructions,
        constants: child_cg.constants,
        num_regs: lam.num_regs,
        arity,
        source_map: vec![],
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
