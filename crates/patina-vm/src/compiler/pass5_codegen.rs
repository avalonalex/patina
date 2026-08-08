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
use super::primitive_calls::{InlineOp, PrimitiveCallMap, ResolvedPrimitive};
use crate::error::CompileError;
use crate::types::code_object::{Arity, CodeObject, CodeObjectId, GlobalCacheEntry};
use crate::types::instruction::Instruction;
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

    /// Record a source location for the instruction about to be emitted.
    fn record_source(&mut self, source: &Option<SourceLocation>) {
        if let Some(loc) = source {
            let pc = self.current_pc();
            // Avoid duplicate entries for the same pc.
            if self
                .source_map
                .last()
                .is_none_or(|(last_pc, _)| *last_pc != pc)
            {
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
    /// The caller should load all of them into `VmState::code_store`.
    pub fn run(
        allocated: &AllocatedExpr,
        prim_calls: PrimitiveCallMap,
    ) -> Result<(CodeObject, Vec<CodeObject>), CompileError> {
        let expr = &allocated.expr;
        let id = CodeObjectId::fresh();
        let mut cg = Codegen::new(None, Rc::new(prim_calls));
        gen_expr(expr, &mut cg)?;
        // Top-level: emit a Return of the expression's result.
        cg.emit(Instruction::Return { val: expr.dst });
        let nested = cg.nested;
        let code = CodeObject {
            id,
            name: cg.name,
            global_cache: GlobalCacheEntry::table(cg.instructions.len()),
            instructions: cg.instructions,
            constants: cg.constants,
            // Use the Pass 4 high-water mark so all temps are covered.
            num_regs: allocated.num_regs.max(1),
            arity: Arity::Fixed(0),
            source_map: cg.source_map,
        };
        Ok((code, nested))
    }
}

/// Build the instruction for a call to a compile-time-resolved primitive:
/// the specialized inline opcode when the primitive has one and the call
/// site has exactly the fixed arity (Track P P3), `CallPrimitive` otherwise.
fn primitive_call_instruction(
    resolved: ResolvedPrimitive,
    name: Symbol,
    arg_tmps: &[u16],
    dst: u16,
) -> Instruction {
    let func_id = resolved.id;
    let inline = resolved.inline.filter(|op| op.arity() == arg_tmps.len());
    let Some(op) = inline else {
        return Instruction::CallPrimitive {
            func_id,
            name,
            args: arg_tmps.to_vec(),
            dst,
        };
    };
    match op {
        InlineOp::Add => Instruction::Add {
            a: arg_tmps[0],
            b: arg_tmps[1],
            dst,
            func_id,
            name,
        },
        InlineOp::Sub => Instruction::Sub {
            a: arg_tmps[0],
            b: arg_tmps[1],
            dst,
            func_id,
            name,
        },
        InlineOp::Mul => Instruction::Mul {
            a: arg_tmps[0],
            b: arg_tmps[1],
            dst,
            func_id,
            name,
        },
        InlineOp::Lt => Instruction::Lt {
            a: arg_tmps[0],
            b: arg_tmps[1],
            dst,
            func_id,
            name,
        },
        InlineOp::NumEq => Instruction::NumEq {
            a: arg_tmps[0],
            b: arg_tmps[1],
            dst,
            func_id,
            name,
        },
        InlineOp::Eq => Instruction::Eq {
            a: arg_tmps[0],
            b: arg_tmps[1],
            dst,
            func_id,
            name,
        },
        InlineOp::Cons => Instruction::Cons {
            a: arg_tmps[0],
            b: arg_tmps[1],
            dst,
            func_id,
            name,
        },
        InlineOp::Car => Instruction::Car {
            src: arg_tmps[0],
            dst,
            func_id,
            name,
        },
        InlineOp::Cdr => Instruction::Cdr {
            src: arg_tmps[0],
            dst,
            func_id,
            name,
        },
        InlineOp::Not => Instruction::Not {
            src: arg_tmps[0],
            dst,
            func_id,
            name,
        },
        InlineOp::NullP => Instruction::NullP {
            src: arg_tmps[0],
            dst,
            func_id,
            name,
        },
        InlineOp::PairP => Instruction::PairP {
            src: arg_tmps[0],
            dst,
            func_id,
            name,
        },
        InlineOp::VectorP => Instruction::VectorP {
            src: arg_tmps[0],
            dst,
            func_id,
            name,
        },
        InlineOp::VectorRef => Instruction::VectorRef {
            v: arg_tmps[0],
            i: arg_tmps[1],
            dst,
            func_id,
            name,
        },
        InlineOp::VectorSet => Instruction::VectorSet {
            v: arg_tmps[0],
            i: arg_tmps[1],
            val: arg_tmps[2],
            dst,
            func_id,
            name,
        },
    }
}

/// Generate instructions for `expr` into `cg`.
fn gen_expr(expr: &RegExpr, cg: &mut Codegen) -> Result<(), CompileError> {
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

        RegExprKind::Quasiquote(_v) => {
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
            // Recognize (call-with-values producer consumer) and emit
            // instruction-level sequence instead of a runtime-intercepted Call.
            // This avoids run_thunk, making call/cc inside the producer safe.
            if args.len() == 2
                && let RegExprKind::GlobalRef { name } = &func.kind
                && name.as_ref() == "call-with-values"
            {
                // Evaluate producer and consumer into their temps.
                gen_expr(&args[0], cg)?;
                gen_expr(&args[1], cg)?;
                let producer_reg = arg_tmps[0];
                let consumer_reg = arg_tmps[1];
                // Call producer (0 args), result in expr.dst.
                cg.emit(Instruction::Call {
                    func: producer_reg,
                    args: vec![],
                    dst: expr.dst,
                });
                // Call consumer with value_buffer or [producer_result].
                if *is_tail {
                    cg.emit(Instruction::TailCallWithValues {
                        consumer: consumer_reg,
                        producer_result: expr.dst,
                    });
                } else {
                    cg.emit(Instruction::CallWithValues {
                        dst: expr.dst,
                        consumer: consumer_reg,
                        producer_result: expr.dst,
                    });
                }
                return Ok(());
            }

            // Recognize (dynamic-wind before body after) and emit
            // instruction-level sequence to avoid run_thunk for the body.
            if args.len() == 3
                && let RegExprKind::GlobalRef { name } = &func.kind
                && name.as_ref() == "dynamic-wind"
            {
                gen_expr(&args[0], cg)?;
                gen_expr(&args[1], cg)?;
                gen_expr(&args[2], cg)?;
                let before_reg = arg_tmps[0];
                let body_reg = arg_tmps[1];
                let after_reg = arg_tmps[2];
                // Call before-thunk (result discarded).
                cg.emit(Instruction::Call {
                    func: before_reg,
                    args: vec![],
                    dst: expr.dst,
                });
                // Push wind record.
                cg.emit(Instruction::PushWind {
                    before: before_reg,
                    after: after_reg,
                });
                // Call body-thunk, result in expr.dst.
                cg.emit(Instruction::Call {
                    func: body_reg,
                    args: vec![],
                    dst: expr.dst,
                });
                // Pop wind record (does not call after-thunk).
                cg.emit(Instruction::PopWind);
                // Call after-thunk (result discarded, goes into before_reg).
                cg.emit(Instruction::Call {
                    func: after_reg,
                    args: vec![],
                    dst: before_reg,
                });
                // expr.dst still holds body result.
                if *is_tail {
                    cg.emit(Instruction::Return { val: expr.dst });
                }
                return Ok(());
            }

            // Statically-known primitive: skip the callee LoadGlobal and the
            // frame push entirely. In tail position a primitive cannot capture
            // the continuation (control primitives are excluded from the map),
            // so the emitted call + Return is equivalent to TailCall. The
            // shadow-bit deopt path relies on this exact `<op> dst; Return
            // dst` pair to recognize tail sites and keep them proper tail
            // calls when the rebound callee is a closure (PRD P8.2, see
            // `exec_call_primitive`).
            if let RegExprKind::GlobalRef { name } = &func.kind
                && let Some(&resolved) = cg.prim_calls.get(name)
            {
                for arg in args {
                    gen_expr(arg, cg)?;
                }
                cg.emit(primitive_call_instruction(
                    resolved,
                    name.clone(),
                    arg_tmps,
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
    let child_id = CodeObjectId::fresh();
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
        global_cache: GlobalCacheEntry::table(child_cg.instructions.len()),
        instructions: child_cg.instructions,
        constants: child_cg.constants,
        num_regs: lam.num_regs,
        arity,
        source_map: child_cg.source_map,
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
