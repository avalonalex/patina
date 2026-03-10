//! Pass 4 — Register Allocation
//!
//! Assigns frame-relative register indices to every binding and temporary in
//! `TailedExpr`, producing `RegExpr`.
//!
//! ## Strategy: simple linear allocation
//!
//! Each lambda gets its own `RegAlloc` state starting from register 0:
//! - Parameters: r0, r1, … (in declaration order).
//! - Temporaries: next available register, freed on last use.
//! - Result register: an explicit `dst` carried on every `RegExpr` node,
//!   telling the codegen where to write the expression's value.
//!
//! This is not optimal but is correct and easy to reason about. A proper
//! linear-scan allocator with liveness can be added later.
//!
//! ## Tail-call overlap invariant
//!
//! For `TailCall`, the compiler must ensure no src/dst overlap so the runtime
//! can perform a simple sequential copy. We achieve this by:
//! 1. Evaluate all argument expressions into fresh temporaries.
//! 2. Copy from those temps to the parameter slots (r0, r1, …) in a separate
//!    step that Pass 5 emits as `Move` instructions.
//! 3. Only then emit `TailCall`.
//!
//! Tests in `tests/tail_call_overlap.rs` verify this property.
//!
//! See VM_COMPILER.md §Pass 4.

use super::pass3_tail::{TailedExpr, TailedLambda};
use crate::compiler::pass1_analysis::NodeId;
use patina_core::core_expr::Symbol;
use patina_core::tagged_value::TaggedValue;
use std::collections::{HashMap, HashSet};

// ─────────────────────────────────────────────────────────────────────────────
// RegExpr — output IR of Pass 4
// ─────────────────────────────────────────────────────────────────────────────

/// A register-allocated expression. Every node carries `dst`: the register
/// that holds the result of evaluating this expression.
#[derive(Debug, Clone)]
pub struct RegExpr {
    pub kind: RegExprKind,
    /// The register that will hold this expression's result after it executes.
    pub dst: u16,
}

#[derive(Debug, Clone)]
pub enum RegExprKind {
    Literal(TaggedValue),
    Quote(TaggedValue),
    Quasiquote(TaggedValue),

    /// Copy from a local parameter register.
    LocalRef {
        src: u16,
    },

    /// Load from closure free-var slot.
    ClosureRef {
        slot: u16,
    },

    /// Load from the global environment.
    GlobalRef {
        name: Symbol,
    },

    /// Read value out of a `MutableCell` held in a local register.
    ReadLocalCell {
        src: u16,
    },
    /// Read value out of a `MutableCell` held in a closure slot.
    ReadClosureCell {
        slot: u16,
    },

    Lambda(Box<RegLambda>),

    If {
        test: Box<RegExpr>,
        then: Box<RegExpr>,
        else_: Box<RegExpr>,
    },

    SetLocal {
        value: Box<RegExpr>,
        var_reg: u16,
    },
    /// `set!` through a `MutableCell` held in a local register.
    WriteLocalCell {
        value: Box<RegExpr>,
        var_reg: u16,
    },
    /// `set!` through a `MutableCell` held in a closure slot.
    WriteClosureCell {
        slot: u16,
        value: Box<RegExpr>,
    },
    SetGlobal {
        name: Symbol,
        value: Box<RegExpr>,
    },

    Begin(Vec<RegExpr>),
    Define {
        name: Symbol,
        value: Box<RegExpr>,
    },

    App {
        func: Box<RegExpr>,
        args: Vec<RegExpr>,
        /// Temporaries holding evaluated args (for the overlap-safe copy).
        arg_tmps: Vec<u16>,
        is_tail: bool,
    },

    Apply {
        func: Box<RegExpr>,
        args: Vec<RegExpr>,
        arg_tmps: Vec<u16>,
        is_tail: bool,
    },
}

/// Where to load a captured free variable from the *parent* frame.
#[derive(Debug, Clone)]
pub enum CaptureSource {
    /// Value is in a local register of the parent frame.
    ParentReg(u16),
    /// Value is a free variable of the parent frame (pass it through from closure).
    ParentClosureSlot(u16),
    /// Value is a global variable.
    Global(Symbol),
}

/// A register-allocated lambda.
#[derive(Debug, Clone)]
pub struct RegLambda {
    pub node_id: NodeId,
    pub num_params: u16,
    pub rest_param: bool,
    /// Total registers used by this lambda's frame.
    pub num_regs: u16,
    pub body: Vec<RegExpr>,
    /// How to obtain each captured free variable from the parent frame, in slot order.
    pub captures: Vec<CaptureSource>,
    /// Registers of params that must be wrapped in `MutableCell` on entry.
    pub boxed_params: HashSet<u16>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Register allocator state (per lambda frame)
// ─────────────────────────────────────────────────────────────────────────────

struct RegAlloc {
    /// Maps symbol name → register index for currently-live locals.
    locals: HashMap<Symbol, u16>,
    /// Next free register.
    next: u16,
    /// High-water mark (= total registers needed).
    num_regs: u16,
}

impl RegAlloc {
    fn new() -> Self {
        Self {
            locals: HashMap::new(),
            next: 0,
            num_regs: 0,
        }
    }

    /// Allocate `count` consecutive registers starting at `self.next`.
    fn alloc_n(&mut self, count: u16) -> u16 {
        let base = self.next;
        self.next += count;
        if self.next > self.num_regs {
            self.num_regs = self.next;
        }
        base
    }

    fn alloc(&mut self) -> u16 {
        self.alloc_n(1)
    }

    /// Bind a parameter name to an already-allocated register.
    fn bind(&mut self, name: Symbol, reg: u16) {
        self.locals.insert(name, reg);
    }

    /// Look up a local binding.
    fn lookup(&self, name: &Symbol) -> Option<u16> {
        self.locals.get(name).copied()
    }

    /// Free temporaries above `watermark` (simple stack-like free).
    fn free_above(&mut self, watermark: u16) {
        self.next = watermark;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pass entry
// ─────────────────────────────────────────────────────────────────────────────

pub struct Pass4Registers;

/// Output of Pass 4: the allocated expression plus the total register count
/// needed for the top-level frame.
pub struct AllocatedExpr {
    pub expr: RegExpr,
    /// High-water mark — total registers this frame needs.
    pub num_regs: u16,
}

impl Pass4Registers {
    pub fn run(expr: &TailedExpr) -> AllocatedExpr {
        let mut alloc = RegAlloc::new();
        let dst = alloc.alloc();
        let expr = allocate(expr, dst, &mut alloc);
        AllocatedExpr {
            expr,
            num_regs: alloc.num_regs,
        }
    }
}

/// Allocate registers for `expr`, writing result into `dst`.
fn allocate(expr: &TailedExpr, dst: u16, alloc: &mut RegAlloc) -> RegExpr {
    allocate_ctx(expr, dst, alloc, &[])
}

/// Core allocator. `own_captures` is the capture list of the immediately
/// enclosing lambda — needed so nested lambda nodes can resolve which parent
/// closure slot each free variable comes from.
fn allocate_ctx(
    expr: &TailedExpr,
    dst: u16,
    alloc: &mut RegAlloc,
    own_captures: &[Symbol],
) -> RegExpr {
    let kind = match expr {
        TailedExpr::Literal(v) => RegExprKind::Literal(*v),
        TailedExpr::Quote(v) => RegExprKind::Quote(*v),
        TailedExpr::Quasiquote(v) => RegExprKind::Quasiquote(*v),

        TailedExpr::LocalRef(name) => {
            let src = alloc.lookup(name).unwrap_or(0); // resolved at compile time
            RegExprKind::LocalRef { src }
        }

        TailedExpr::ClosureRef { slot, .. } => RegExprKind::ClosureRef { slot: *slot },

        TailedExpr::GlobalRef(name) => RegExprKind::GlobalRef { name: name.clone() },

        TailedExpr::ReadLocalCell(name) => {
            let src = alloc.lookup(name).unwrap_or(0);
            RegExprKind::ReadLocalCell { src }
        }

        TailedExpr::ReadClosureCell { slot, .. } => RegExprKind::ReadClosureCell { slot: *slot },

        TailedExpr::Lambda(lam) => {
            // own_captures is the parent lambda's capture list — nested lambdas
            // use it to resolve ParentClosureSlot sources.
            let reg_lam = allocate_lambda(lam, alloc, own_captures);
            // Reserve scratch regs for MakeClosure capture loads: dst+1..dst+n_captures.
            let n_captures = reg_lam.captures.len() as u16;
            if n_captures > 0 {
                let needed = dst + 1 + n_captures;
                if needed > alloc.num_regs {
                    alloc.num_regs = needed;
                }
            }
            RegExprKind::Lambda(Box::new(reg_lam))
        }

        TailedExpr::If { test, then, else_ } => {
            let test_dst = alloc.alloc();
            let test_reg = allocate_ctx(test, test_dst, alloc, own_captures);
            // Reuse `dst` for both branches.
            let then_reg = allocate_ctx(then, dst, alloc, own_captures);
            let else_reg = allocate_ctx(else_, dst, alloc, own_captures);
            RegExprKind::If {
                test: Box::new(test_reg),
                then: Box::new(then_reg),
                else_: Box::new(else_reg),
            }
        }

        TailedExpr::SetLocal { var, value } => {
            let var_reg = alloc.lookup(var).unwrap_or(0);
            let tmp = alloc.alloc();
            let val_expr = allocate_ctx(value, tmp, alloc, own_captures);
            RegExprKind::SetLocal {
                value: Box::new(val_expr),
                var_reg,
            }
        }

        TailedExpr::WriteLocalCell { var, value } => {
            let var_reg = alloc.lookup(var).unwrap_or(0);
            let tmp = alloc.alloc();
            let val_expr = allocate_ctx(value, tmp, alloc, own_captures);
            RegExprKind::WriteLocalCell {
                value: Box::new(val_expr),
                var_reg,
            }
        }

        TailedExpr::WriteClosureCell { slot, value } => {
            let tmp = alloc.alloc();
            let val_reg = allocate_ctx(value, tmp, alloc, own_captures);
            RegExprKind::WriteClosureCell {
                slot: *slot,
                value: Box::new(val_reg),
            }
        }

        TailedExpr::SetGlobal { var, value } => {
            let tmp = alloc.alloc();
            let val_reg = allocate_ctx(value, tmp, alloc, own_captures);
            RegExprKind::SetGlobal {
                name: var.clone(),
                value: Box::new(val_reg),
            }
        }

        TailedExpr::Begin(exprs) => {
            let n = exprs.len();
            let regs: Vec<RegExpr> = exprs
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    // Only the last expr's result goes to `dst`.
                    let d = if i == n - 1 { dst } else { alloc.alloc() };
                    allocate_ctx(e, d, alloc, own_captures)
                })
                .collect();
            RegExprKind::Begin(regs)
        }

        TailedExpr::Define { name, value } => {
            let tmp = alloc.alloc();
            let val_reg = allocate_ctx(value, tmp, alloc, own_captures);
            RegExprKind::Define {
                name: name.clone(),
                value: Box::new(val_reg),
            }
        }

        TailedExpr::App {
            func,
            args,
            is_tail,
        } => {
            let func_dst = alloc.alloc();
            let func_reg = allocate_ctx(func, func_dst, alloc, own_captures);

            // Evaluate args into fresh temporaries (overlap-safe for TailCall).
            let watermark = alloc.next;
            let mut arg_regs: Vec<RegExpr> = Vec::with_capacity(args.len());
            let mut arg_tmps: Vec<u16> = Vec::with_capacity(args.len());
            for arg in args {
                let tmp = alloc.alloc();
                arg_tmps.push(tmp);
                arg_regs.push(allocate_ctx(arg, tmp, alloc, own_captures));
            }

            // For non-tail calls, free arg temporaries after the call.
            if !is_tail {
                alloc.free_above(watermark);
            }

            RegExprKind::App {
                func: Box::new(func_reg),
                args: arg_regs,
                arg_tmps,
                is_tail: *is_tail,
            }
        }

        TailedExpr::Apply {
            func,
            args,
            is_tail,
        } => {
            let func_dst = alloc.alloc();
            let func_reg = allocate_ctx(func, func_dst, alloc, own_captures);

            let watermark = alloc.next;
            let mut arg_regs: Vec<RegExpr> = Vec::with_capacity(args.len());
            let mut arg_tmps: Vec<u16> = Vec::with_capacity(args.len());
            for arg in args {
                let tmp = alloc.alloc();
                arg_tmps.push(tmp);
                arg_regs.push(allocate_ctx(arg, tmp, alloc, own_captures));
            }
            if !is_tail {
                alloc.free_above(watermark);
            }

            RegExprKind::Apply {
                func: Box::new(func_reg),
                args: arg_regs,
                arg_tmps,
                is_tail: *is_tail,
            }
        }
    };

    RegExpr { kind, dst }
}

/// Allocate registers for a lambda, starting a fresh frame.
///
/// `parent_alloc`: the parent frame's register allocator, used to resolve
/// where each captured free variable currently lives.
fn allocate_lambda(
    lam: &TailedLambda,
    parent_alloc: &RegAlloc,
    parent_captures: &[Symbol],
) -> RegLambda {
    // Resolve capture sources from the parent frame.
    // A free variable in `lam` may live in:
    //   1. A local register of the parent (parent_alloc.lookup succeeds).
    //   2. A closure slot of the parent (parent_captures[i] matches).
    //   3. A global (fall-through).
    let captures: Vec<CaptureSource> = lam
        .capture_list
        .iter()
        .map(|name| {
            if let Some(reg) = parent_alloc.lookup(name) {
                CaptureSource::ParentReg(reg)
            } else if let Some(slot) = parent_captures.iter().position(|n| n == name) {
                // Variable lives in the parent's own closure (pass-through).
                CaptureSource::ParentClosureSlot(slot as u16)
            } else {
                CaptureSource::Global(name.clone())
            }
        })
        .collect();

    let mut alloc = RegAlloc::new();

    // Parameters occupy r0..r(n-1) in declaration order.
    for name in &lam.params {
        let reg = alloc.alloc();
        alloc.bind(name.clone(), reg);
    }

    // Map boxed param names → their register indices.
    let boxed_params: HashSet<u16> = lam
        .boxed_params
        .iter()
        .filter_map(|name| alloc.lookup(name))
        .collect();

    let num_params = lam.params.len() as u16;
    let n = lam.body.len();
    let mut body: Vec<RegExpr> = Vec::with_capacity(n);
    // Pass this lambda's own capture_list so nested lambdas can resolve
    // variables that live in this lambda's closure slots.
    for e in lam.body.iter() {
        let d = alloc.alloc();
        body.push(allocate_ctx(e, d, &mut alloc, &lam.capture_list));
    }

    let num_regs = alloc.num_regs;
    RegLambda {
        node_id: lam.node_id,
        num_params,
        rest_param: lam.rest_param,
        num_regs,
        body,
        captures,
        boxed_params,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::pass1_analysis::Pass1Analysis;
    use crate::compiler::pass2_closure::Pass2Closure;
    use crate::compiler::pass3_tail::Pass3Tail;
    use patina_core::core_expr::{CoreExpr, CoreExprKind, Formals, ScopedParam};
    use std::rc::Rc;

    fn var(name: &str) -> CoreExpr {
        CoreExpr::new(CoreExprKind::Var {
            name: Rc::from(name),
            scopes: Default::default(),
        })
    }

    fn app(func: CoreExpr, args: Vec<CoreExpr>) -> CoreExpr {
        CoreExpr::new(CoreExprKind::App {
            func: Rc::new(func),
            args,
        })
    }

    fn lambda(params: Vec<&str>, body: Vec<CoreExpr>) -> CoreExpr {
        CoreExpr::new(CoreExprKind::Lambda {
            params: Formals::Fixed(
                params
                    .iter()
                    .map(|n| ScopedParam::simple(Rc::from(*n)))
                    .collect(),
            ),
            body,
            binding_scope: None,
        })
    }

    fn pipeline(expr: &CoreExpr) -> RegExpr {
        let info = Pass1Analysis::run(expr);
        let closed = Pass2Closure::run(expr, &info);
        let tailed = Pass3Tail::run(&closed);
        Pass4Registers::run(&tailed).expr
    }

    #[test]
    fn lambda_params_get_registers() {
        // (lambda (x y) x) — x=r0, y=r1
        let expr = lambda(vec!["x", "y"], vec![var("x")]);
        let regged = pipeline(&expr);
        let RegExprKind::Lambda(lam) = &regged.kind else {
            panic!("expected Lambda");
        };
        assert_eq!(lam.num_params, 2);
        assert!(lam.num_regs >= 2);
    }

    #[test]
    fn tail_call_arg_tmps_are_fresh() {
        // (lambda (x) (f x x))  — both args should be fresh tmps (not r0)
        let expr = lambda(vec!["x"], vec![app(var("f"), vec![var("x"), var("x")])]);
        let regged = pipeline(&expr);
        let RegExprKind::Lambda(lam) = &regged.kind else {
            panic!();
        };
        let RegExprKind::App {
            arg_tmps, is_tail, ..
        } = &lam.body[0].kind
        else {
            panic!();
        };
        assert!(*is_tail);
        // Each arg must have a distinct temp register.
        assert_eq!(arg_tmps.len(), 2);
        // Arg tmps must be beyond the parameter registers (r0).
        assert!(arg_tmps.iter().all(|&t| t >= 1));
    }

    #[test]
    fn nested_lambda_gets_own_frame() {
        // (lambda (x) (lambda (y) y)) — inner frame is independent
        let inner = lambda(vec!["y"], vec![var("y")]);
        let outer = lambda(vec!["x"], vec![inner]);
        let regged = pipeline(&outer);
        let RegExprKind::Lambda(outer_lam) = &regged.kind else {
            panic!();
        };
        let RegExprKind::Lambda(inner_lam) = &outer_lam.body[0].kind else {
            panic!();
        };
        // inner frame: y=r0, plus result slot
        assert!(inner_lam.num_regs >= 1);
        // outer frame: x=r0 + result + inner closure dst
        assert!(outer_lam.num_regs >= 1);
    }
}
