//! VM instruction set.
//!
//! Each variant is one instruction. The VM does NOT use a binary wire format in
//! Phase 2A — instructions live as `Vec<Instruction>` inside `CodeObject`.
//! A compact binary encoding can be added later without changing semantics.
//!
//! See VM_ISA.md §3 and §4 for the full specification.

use super::{CodeObjectId, ConstIdx, Reg};
use patina_core::tagged_value::TaggedValue;
use patina_core::{CollectKind, LinkIdx};

/// Which predicate a [`Instruction::TestJumpUnless`] evaluates. Folding the
/// test into the branch instruction (rather than minting one fused variant
/// per predicate) keeps a single fused-branch arm, deopt path, and emission
/// site as more predicates become fusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestOp {
    /// `(not a)` — total truthiness test.
    Not,
    /// `(null? a)`, `(pair? a)`, `(vector? a)` — total bit tests.
    NullP,
    PairP,
    VectorP,
    /// `(eq? a b)` — total identity test.
    Eq,
    /// `(< a b)` / `(= a b)` — fixnum fast path, else the registry handler.
    Lt,
    NumEq,
}

impl TestOp {
    /// Operand count — the single owner of which tests read `b`. The
    /// dispatch arm's deopt argument list and the disassembler both read
    /// it, so a new unary predicate can't be mistaken for a binary one.
    #[inline]
    pub fn arity(self) -> usize {
        match self {
            TestOp::Not | TestOp::NullP | TestOp::PairP | TestOp::VectorP => 1,
            TestOp::Eq | TestOp::Lt | TestOp::NumEq => 2,
        }
    }
}

/// A control form with an instruction sequence of its own in head position
/// (#442), guarded by [`Instruction::JumpUnlessShadowed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlForm {
    /// `(call-with-values producer consumer)`.
    CallWithValues,
    /// `(dynamic-wind before body after)`.
    DynamicWind,
}

impl ControlForm {
    /// The argument count the sequence is for. Any other count is compiled
    /// as an ordinary call, whose arity error is the procedure's to raise.
    pub fn arity(self) -> usize {
        match self {
            ControlForm::CallWithValues => 2,
            ControlForm::DynamicWind => 3,
        }
    }

    /// This form's bit in `VmState::shadowed_controls`.
    pub fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// A single VM instruction.
///
/// Operands use frame-relative register indices (`Reg = u16`). All values are
/// `TaggedValue` — the same type used throughout the tree-walker and
/// `patina-primitives`. No conversion overhead. (VM_DECISIONS.md §2, §10)
#[derive(Debug, Clone)]
pub enum Instruction {
    // ── Load / Store ──────────────────────────────────────────────────────────
    /// `dst ← constants[idx]`
    LoadConst { dst: Reg, idx: ConstIdx },

    /// `dst ← val`  (inline immediate: fixnum, bool, char, null, eof)
    ///
    /// Used when the value fits in a `TaggedValue` immediate so no constant-pool
    /// entry is needed.
    LoadImmediate { dst: Reg, val: TaggedValue },

    /// `dst ← src`  (register copy within the same frame)
    Move { dst: Reg, src: Reg },

    /// `dst ← current_closure.free_vars[slot]`
    LoadClosure { dst: Reg, slot: u16 },

    /// `dst ←` the global the unit's link table names at `link` (error if
    /// unbound).
    LoadGlobal { dst: Reg, link: LinkIdx },

    /// The global the unit's link table names at `link` `← src`.
    StoreGlobal { link: LinkIdx, src: Reg },

    // ── MutableCell ──────────────────────────────────────────────────────────
    /// Allocate a `MutableCell` wrapping `reg[src]`, store the heap pointer in `dst`.
    ///
    /// Emitted in the lambda prologue for each `boxed_param`.
    AllocCell { dst: Reg, src: Reg },

    /// `dst ← *reg[cell]`  (read through a `MutableCell` pointer)
    ReadCell { dst: Reg, cell: Reg },

    /// `*reg[cell] ← reg[src]`  (write through a `MutableCell` pointer)
    WriteCell { cell: Reg, src: Reg },

    // ── Closure Creation ──────────────────────────────────────────────────────
    /// Allocate a `VmClosure { code_id, free_vars: regs.map(|r| reg[r]) }` on
    /// the heap and store the heap pointer in `dst`.
    ///
    /// `free_vars` is a list of register indices whose current values are captured.
    /// Determined by free-variable analysis in Pass 1.
    MakeClosure {
        dst: Reg,
        code_id: CodeObjectId,
        free_vars: Vec<Reg>,
    },

    // ── Control Flow ─────────────────────────────────────────────────────────
    /// Unconditional jump.
    Jump { target: usize },

    /// Jump if `reg[cond] != #f`.  (All values except `#f` are truthy, R7RS §6.3)
    JumpIf { cond: Reg, target: usize },

    /// Jump if `reg[cond] == #f`.
    JumpUnless { cond: Reg, target: usize },

    /// Jump to `target` — the instruction sequence of a head-position
    /// `call-with-values` or `dynamic-wind` — unless `form`'s procedure has
    /// been shadowed; fall through to an ordinary call of the operator if it
    /// has.
    ///
    /// The guard pass 5 puts in front of the sequence (#442). The site's
    /// operator was bound to the form's procedure when it was compiled; the
    /// shadow mark (`VmState::shadowed_controls`) is set when any global
    /// binding holding that procedure is given another value, which every
    /// writer of a value reports, as it does for `CallPrimitive`'s shadow
    /// bits. So while the mark is clear the operator is still that procedure,
    /// and once it is set every such site calls whatever its operator is now.
    /// Two things keep that true: the compiler gives a fast path only to a
    /// binding of the environment's own, never to a name answered through a
    /// macro-expansion alias, which can be re-pointed without a write
    /// (`resolve_primitive_calls`); and a top-level `define-syntax` over the
    /// name reports nothing, for these sites as for `CallPrimitive`'s, so one
    /// compiled before it keeps the procedure — as chibi's does, where Gauche
    /// and the tree-walker raise.
    JumpUnlessShadowed { form: ControlForm, target: usize },

    // ── Function Calls ────────────────────────────────────────────────────────
    /// Non-tail call.
    ///
    /// 1. Push a new `CallFrame` (with `return_reg = dst` recorded in the new frame).
    /// 2. Copy `args` into the callee's parameter registers (r0, r1, …).
    /// 3. Begin executing the callee at pc=0.
    /// 4. On `Return`: pop frame, write result into caller's `dst`.
    Call { func: Reg, args: Vec<Reg>, dst: Reg },

    /// Tail call — reuses the current `CallFrame`.
    ///
    /// The compiler (Pass 4) guarantees no src/dst overlap among `args` by
    /// materialising args into fresh temporaries before emitting `TailCall`.
    /// The runtime performs a simple sequential copy.
    TailCall { func: Reg, args: Vec<Reg> },

    /// `(apply func arg... list)` — non-tail.
    ///
    /// The last register in `args` holds a proper list whose elements are
    /// spread and appended to the preceding argument registers.
    Apply { func: Reg, args: Vec<Reg>, dst: Reg },

    /// `(apply func arg... list)` — tail position.
    TailApply { func: Reg, args: Vec<Reg> },

    /// Return a single value to the caller.
    ///
    /// Reads `reg[val]`, pops the current frame, and writes the value into the
    /// caller frame's `return_reg`.
    Return { val: Reg },

    /// Call a statically-known primitive without pushing a `CallFrame`.
    ///
    /// `func_id` names a registered `patina-primitives` function. The compiler
    /// emits this only when the callee is a `GlobalRef` that resolved to that
    /// primitive at compile time. `link` names, in the unit's link table, the
    /// global the callee resolved from: if the program later rebinds it
    /// (tracked per-primitive in `VmState::shadowed_primitives`), the VM
    /// deoptimizes this site back to a call of whatever that name is bound
    /// to, so redefinition semantics stay exact.
    CallPrimitive {
        func_id: PrimitiveFnId,
        link: LinkIdx,
        args: Vec<Reg>,
        dst: Reg,
    },

    /// Call a registry primitive that the *code itself* names as a value —
    /// the quasiquote pre-pass puts `list`, `append` and `list->vector` in
    /// operator position as `Literal` procedures, so that nothing the
    /// program imports or defines can redirect them. There is no global
    /// binding behind such a site, hence no `link` and no deoptimization:
    /// it always dispatches by index. Same registry handler as
    /// `CallPrimitive`, so results and errors are identical.
    CallPrimitiveDirect {
        func_id: PrimitiveFnId,
        args: Vec<Reg>,
        dst: Reg,
    },

    // ── Inline primitive opcodes (Track P P3) ───────────────────────────────
    //
    // Fixed-arity fast paths for the hottest primitives, executed directly in
    // the dispatch loop. Every opcode carries the same `(func_id, link)` pair
    // as `CallPrimitive` and obeys the same contract: the fast path fires only
    // when `!is_primitive_shadowed(func_id)` AND the operands fit the trivial
    // case (fixnums that don't overflow, a native pair, an in-bounds vector
    // index). Everything else — floats, bignums, overflow, type errors,
    // rebound names — falls back to `exec_call_primitive`, i.e. the exact
    // registry handler the generic path calls, so results and error messages
    // are byte-for-byte identical to `CallPrimitive`/`Call`.
    //
    // The compiler emits these only for the fixed-arity shape (2-arg `+`,
    // 1-arg `car`, …); other arities stay on `CallPrimitive`.
    /// 2-arg `+`. Fast path: fixnum add; `None` (overflow) → handler promotes.
    Add {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 2-arg `-`.
    Sub {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 2-arg `*`.
    Mul {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 2-arg `<`.
    Lt {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 2-arg `=`.
    NumEq {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 2-arg `eq?` — total, one `values_eq` heap call.
    Eq {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 2-arg `cons` — total, one `alloc_pair`.
    Cons {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 1-arg `car`. Fast path: native pair only; boxed pairs and type errors
    /// go to the handler.
    Car {
        src: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 1-arg `cdr`.
    Cdr {
        src: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 1-arg `not` — total truthiness test (only #f is falsy).
    Not {
        src: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// Fused test + branch (Track P P5). Emitted in place of a predicate
    /// opcode whose result feeds the `JumpUnless` that *must still follow
    /// at the next pc*: the fast path writes `dst` and branches directly
    /// (to `target` when the test is false, over the following `JumpUnless`
    /// otherwise), saving one dispatch.
    ///
    /// Every slow case — a non-shadowed test whose operands miss the fast
    /// path (`(< 1.5 2)`), or a shadowed/rebound predicate — funnels into
    /// `exec_call_primitive`, which writes `dst` exactly as the unfused
    /// opcode would, and then *falls through* to the kept `JumpUnless`,
    /// which performs the branch. So the fused and unfused forms agree by
    /// construction, and the kept instruction is both the deopt landing and
    /// the fallback's branch.
    ///
    /// `b` is unused for the unary tests (`Not`/`NullP`/`PairP`/`VectorP`).
    TestJumpUnless {
        test: TestOp,
        a: Reg,
        b: Reg,
        dst: Reg,
        target: usize,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// `Add` with a literal right operand (Track P P5): absorbs the
    /// `LoadImmediate` at sites like `(+ x 1)`. Right operand only, even
    /// though `+` commutes: the shadow-bit deopt passes `[a, imm]` to
    /// whatever the name is bound to at that point, and a user rebind need
    /// not be commutative — operand order must survive exactly.
    AddImm {
        a: Reg,
        imm: TaggedValue,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 2-arg `-` with a literal right operand: `(- x 1)`-shaped sites only.
    SubImm {
        a: Reg,
        imm: TaggedValue,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 2-arg `<` with a literal right operand: `(< i n)`-shaped sites only.
    LtImm {
        a: Reg,
        imm: TaggedValue,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 2-arg `=` with a literal right operand: `(= x 0)`-shaped sites only.
    NumEqImm {
        a: Reg,
        imm: TaggedValue,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 1-arg `null?` — total bit test.
    NullP {
        src: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 1-arg `pair?` — total bit test.
    PairP {
        src: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 1-arg `vector?` — total bit test.
    VectorP {
        src: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 2-arg `vector-ref`. Fast path: vector + in-bounds fixnum index;
    /// out-of-bounds falls back so the error message comes from the handler.
    VectorRef {
        v: Reg,
        i: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },
    /// 3-arg `vector-set!`. Writes `dst ← unspecified` on the fast path,
    /// matching the handler's return value.
    VectorSet {
        v: Reg,
        i: Reg,
        val: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        link: LinkIdx,
    },

    // ── call-with-values (instruction-level) ────────────────────────────────
    /// Call `consumer` with the values produced by a preceding producer call.
    ///
    /// If `value_buffer` is non-empty (producer used `values`), pass those as
    /// args. Otherwise pass `[reg[producer_result]]` as a single arg.
    /// Non-tail variant: result written to `dst`.
    CallWithValues {
        dst: Reg,
        consumer: Reg,
        producer_result: Reg,
    },

    /// Tail-call variant of `CallWithValues`.
    TailCallWithValues { consumer: Reg, producer_result: Reg },

    // ── dynamic-wind (instruction-level) ────────────────────────────────────
    /// Push a dynamic-wind record onto `ExecutionState::dynamic_winds`.
    ///
    /// The only site that *mints* a record (`DynamicWindRecord::new`, which is
    /// where its `id` comes from). Head-position `dynamic-wind` compiles to
    /// this; the value form runs the same instructions in a runtime-pushed
    /// stub frame (`value_wind_stub` in `runtime/control.rs`). The two other
    /// sites that grow `dynamic_winds` re-push records that already exist:
    /// `ResumeWindJump`, entering an extent of the continuation being jumped
    /// to, and the composable-continuation invokes, appending the extents they
    /// captured. A record is popped by the `PopWind` that follows it in its
    /// own sequence, or by a jump that leaves its extent — never swept by
    /// depth, and it carries none.
    PushWind { before: Reg, after: Reg },

    /// Pop the top dynamic-wind record from `ExecutionState::dynamic_winds`.
    /// Does NOT call the after-thunk — that is handled by a separate `Call`
    /// instruction emitted after `PopWind` in the codegen sequence.
    PopWind,

    /// Take the next step of a continuation jump that is running wind thunks.
    ///
    /// Never emitted by the compiler. It is the whole body of the one-frame
    /// stub the runtime pushes under each wind thunk of a jump
    /// (`push_wind_step` in `runtime/control.rs`), so that "the rest of the
    /// jump" is an ordinary frame: a continuation captured inside the thunk
    /// captures it, and re-entering that continuation resumes the thunk and
    /// then continues the jump, instead of falling back into whatever the
    /// jump site was doing.
    ///
    /// Its frame's registers carry the jump: the target continuation, the
    /// value it delivers, and which of the target's records this step is
    /// entering (see the `wind_step` module in `runtime/control.rs`).
    ResumeWindJump,

    /// Take the next step of a composable-continuation invoke that is running
    /// the `before` thunks of the extents it re-enters.
    ///
    /// Never emitted by the compiler, and the exact analogue of
    /// [`Instruction::ResumeWindJump`]: it is the whole body of the one-frame
    /// stub the runtime pushes under each of those thunks
    /// (`push_invoke_step`), so that "the rest of the invoke" — the remaining
    /// thunks, then appending the captured frames and delivering the value —
    /// is a **pc** a re-entering continuation restores, rather than a Rust
    /// frame it abandons.
    ///
    /// A jump gets this from `ResumeWindJump`; the value form of
    /// `dynamic-wind` from `value_wind_stub`; an abort from travelling to a
    /// landing continuation. This is the fourth and last place that owed it
    /// (issue #167). It cannot be the third: a jump's target *replaces* the
    /// machine, and a composable invoke's *extends* it, which is why routing
    /// these thunks through `step_wind_jump` was tried and reverted — see
    /// `install_thunk_handlers`.
    ///
    /// Its frame's registers carry the invoke: the continuation, the value it
    /// delivers, where that value's computation returns, and which captured
    /// extent this step is entering (see the `invoke_step` module in
    /// `runtime/control.rs`).
    ResumeComposableInvoke,

    /// The bookkeeping a `raise` still owes once its handler has returned.
    ///
    /// The middle instruction of `raise_step_stub`'s three — `Call handler`,
    /// this, `Return` — and the reason a raise's remainder is a **pc** rather
    /// than Rust code after a nested dispatch loop. What it does depends on
    /// the raise it stands for, which its frame's `CONTINUABLE` register says:
    ///
    /// - **continuable**: re-push the handler, which R7RS 6.11 keeps
    ///   installed for the rest of the thunk once the handler has returned,
    ///   and let the `Return` deliver its value as the value of
    ///   `(raise-continuable …)`.
    /// - **non-continuable**: raise a secondary exception, because R7RS 6.11
    ///   says a handler that returns from `raise` must not simply return.
    ///
    /// Both used to live in `vm_raise_value`, after a nested
    /// `run_loop_until_outcome`, and both were therefore invisible to a
    /// continuation captured inside the handler: resuming one replayed the
    /// handler's frames and returned straight past the Rust that owed the
    /// work. That cost three divergences — a `guard` silently uninstalled
    /// after a declining clause, a composable continuation that lost its
    /// handler, and a non-continuable handler allowed to return (issue #178).
    /// A frame is captured with the rest of them, so the debt is carried
    /// wherever the continuation goes.
    ///
    /// Its frame's registers are the `raise_step` module in
    /// `runtime/control.rs`.
    ResumeRaise,

    /// What `force` owes once a promise's thunk has returned: R7RS 7.3's
    /// bookkeeping, one turn of it. Never emitted by the compiler — the
    /// middle instruction of `force_stub`, `Call thunk` / `ResumeForce` /
    /// `Return`. If the promise is done — the thunk forced it re-entrantly —
    /// its value stands; if the thunk returned a promise (`delay-force`), this
    /// one takes its state and, if that is not done, the frame goes round
    /// again from the top with its thunk; otherwise this one is done, holding
    /// what the thunk returned. The value lands in the register the `Return`
    /// reads.
    ///
    /// A frame rather than Rust (#476): a continuation captured inside the
    /// thunk carries it, so re-entered after `force` returned it still settles
    /// the promise and returns its first value. The primitive `force` ran the
    /// thunk across a re-entry boundary, which no continuation can carry, and
    /// answered a stray internal value there. Its frame's registers are the
    /// `force_step` module in `runtime/control.rs`.
    ResumeForce,

    /// A resumable primitive's call has returned: resume the primitive
    /// (`PrimitiveRegistry::resume`) with the state it kept and the result.
    /// Never emitted by the compiler — the middle instruction of
    /// `resume_stub`, `Call callee(args…)` / `ResumePrimitive` / `Return`
    /// (`Apply callee args` past three arguments). If
    /// the primitive is done, its value lands in the register the `Return`
    /// reads; if it asks for another call, the frame goes round again from
    /// the top with the new callee, arguments and state, in the stub for
    /// their count.
    ///
    /// The general form of what `ResumeForce` does for `force` (#477, #478):
    /// a primitive that has to call a procedure the program gave it and then
    /// carry on hands the call to the machine (`patina_primitives::Step`),
    /// and this frame is where the rest of the primitive waits. A
    /// continuation captured in the call carries the frame, so re-entered
    /// after the primitive returned it resumes the primitive again. Its
    /// registers are the `resume_step` module in `runtime/control.rs`.
    ///
    /// After a `CollectAtCall` it resumes the primitive the same way, with
    /// whether the collection ran in place of a call's result.
    ResumePrimitive,

    /// Collect at a resumable primitive's call (`patina_primitives::Step::
    /// Collect`, #639): `(gc)`'s full collection, and the collection an open
    /// that ran out of descriptors retries after (#607). Never emitted by the
    /// compiler — the first instruction of `collect_stub`, `CollectAtCall` /
    /// `ResumePrimitive` / `Return`, the variant of `resume_stub`'s frame
    /// that collects.
    ///
    /// The caller is suspended at the call's return pc beneath the frame,
    /// which holds the primitive's state, so every live value is on the
    /// machine. It clears the register the primitive's value goes to (a
    /// liveness map keeps a call's destination from the call on), then runs
    /// the collection through `GcController::collect_at_call`, which
    /// collects only where a safe point may and otherwise posts it for the
    /// next safe point that may, counting it as deferred; and it leaves
    /// whether a collection ran in the frame's `RESULT` register for
    /// `ResumePrimitive`. So `(gc)` collects before its caller's next
    /// instruction whether or not a safe point comes between: since #700
    /// one rarely does, the VM polling at frame entries and transfer returns
    /// rather than before every instruction.
    CollectAtCall { kind: CollectKind },

    // ── Global Definitions ────────────────────────────────────────────────────
    /// Top-level `define` of the global the unit's link table names at
    /// `link`, to `reg[src]`.
    Define { link: LinkIdx, src: Reg },

    // ── Continuations (SRFI-226) ──────────────────────────────────────────────
    /// Maps to `abort-current-continuation`.
    ///
    /// Searches prompt stack for nearest frame matching `tag`, clones the
    /// intervening frames as a `VmDelimitedContinuation`, unwinds the stack,
    /// then calls the prompt's handler with `(val, captured_continuation)`.
    ///
    /// `dst` is the register this call's own result would have landed in. The
    /// abort never returns one, which is exactly why the operand is needed:
    /// that register is the hole the captured continuation delivers into if
    /// the handler invokes it.
    AbortToPrompt { tag: Reg, val: Reg, dst: Reg },

    /// Maps to `call-with-composable-continuation`.
    ///
    /// Captures frames from the matching prompt to the current top into a
    /// `VmDelimitedContinuation` stored in `dst`. Does NOT unwind the stack.
    CaptureComposable { dst: Reg, tag: Reg },

    /// Invoke a captured continuation.
    ///
    /// - `composable = true`: append captured frames to current stack.
    /// - `composable = false`: run dynamic-wind exit/entry hooks and replace
    ///   the call stack with the captured frames.
    InvokeContinuation {
        cont: Reg,
        val: Reg,
        composable: bool,
    },

    // ── Miscellaneous ─────────────────────────────────────────────────────────
    /// No operation. Used as a placeholder during label patching.
    Nop,
}

impl Instruction {
    /// The global operand, an index into the unit's link table, of an
    /// instruction that names a global: `LoadGlobal`, `StoreGlobal`,
    /// `Define`, `CallPrimitive` and the 20 inline opcodes. Exhaustive, so a
    /// new variant says whether it names one.
    pub fn link(&self) -> Option<LinkIdx> {
        use Instruction::*;
        match *self {
            LoadGlobal { link, .. }
            | StoreGlobal { link, .. }
            | Define { link, .. }
            | CallPrimitive { link, .. }
            | Add { link, .. }
            | Sub { link, .. }
            | Mul { link, .. }
            | Lt { link, .. }
            | NumEq { link, .. }
            | Eq { link, .. }
            | Cons { link, .. }
            | Car { link, .. }
            | Cdr { link, .. }
            | Not { link, .. }
            | TestJumpUnless { link, .. }
            | AddImm { link, .. }
            | SubImm { link, .. }
            | LtImm { link, .. }
            | NumEqImm { link, .. }
            | NullP { link, .. }
            | PairP { link, .. }
            | VectorP { link, .. }
            | VectorRef { link, .. }
            | VectorSet { link, .. } => Some(link),
            LoadConst { .. }
            | LoadImmediate { .. }
            | Move { .. }
            | LoadClosure { .. }
            | AllocCell { .. }
            | ReadCell { .. }
            | WriteCell { .. }
            | MakeClosure { .. }
            | Jump { .. }
            | JumpIf { .. }
            | JumpUnless { .. }
            | JumpUnlessShadowed { .. }
            | Call { .. }
            | TailCall { .. }
            | Apply { .. }
            | TailApply { .. }
            | Return { .. }
            | CallPrimitiveDirect { .. }
            | CallWithValues { .. }
            | TailCallWithValues { .. }
            | PushWind { .. }
            | PopWind
            | ResumeWindJump
            | ResumeComposableInvoke
            | ResumeRaise
            | ResumeForce
            | ResumePrimitive
            | CollectAtCall { .. }
            | AbortToPrompt { .. }
            | CaptureComposable { .. }
            | InvokeContinuation { .. }
            | Nop => None,
        }
    }
}

// Every dispatched instruction is fetched by value from this enum, so its
// size is hot-loop-critical. 40 bytes is the current high-water mark, held by
// `CallPrimitive` (a `Vec` beside its `dst`, `func_id` and `link`) and
// `MakeClosure` (a `Vec` beside its `dst` and `code_id`); the next variants
// down are at 32, and new variants must fit under it. It was 48 while the 24
// global-naming variants carried their name, an `Rc<str>`, in place of a
// link index (#836).
const _: () = assert!(std::mem::size_of::<Instruction>() <= 40);

/// Identifier for a registered primitive function.
///
/// In Phase 2A this is an index into a flat `Vec` of primitive descriptors
/// built at startup. The VM resolves it to a function pointer at load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrimitiveFnId(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    /// `link` answers the operand of each of the 24 variants that name a
    /// global, and nothing for a variant that names none, the two beside
    /// them that carry no link (`CallPrimitiveDirect`, `MakeClosure`) among
    /// them (#836).
    #[test]
    fn link_answers_the_operand_of_every_variant_that_names_a_global() {
        let f = PrimitiveFnId(0);
        let imm = TaggedValue::fixnum(1);
        let naming = [
            Instruction::LoadGlobal { dst: 0, link: 0 },
            Instruction::StoreGlobal { link: 1, src: 0 },
            Instruction::Define { link: 2, src: 0 },
            Instruction::CallPrimitive {
                func_id: f,
                link: 3,
                args: vec![0],
                dst: 0,
            },
            Instruction::Add {
                a: 0,
                b: 0,
                dst: 0,
                func_id: f,
                link: 4,
            },
            Instruction::Sub {
                a: 0,
                b: 0,
                dst: 0,
                func_id: f,
                link: 5,
            },
            Instruction::Mul {
                a: 0,
                b: 0,
                dst: 0,
                func_id: f,
                link: 6,
            },
            Instruction::Lt {
                a: 0,
                b: 0,
                dst: 0,
                func_id: f,
                link: 7,
            },
            Instruction::NumEq {
                a: 0,
                b: 0,
                dst: 0,
                func_id: f,
                link: 8,
            },
            Instruction::Eq {
                a: 0,
                b: 0,
                dst: 0,
                func_id: f,
                link: 9,
            },
            Instruction::Cons {
                a: 0,
                b: 0,
                dst: 0,
                func_id: f,
                link: 10,
            },
            Instruction::Car {
                src: 0,
                dst: 0,
                func_id: f,
                link: 11,
            },
            Instruction::Cdr {
                src: 0,
                dst: 0,
                func_id: f,
                link: 12,
            },
            Instruction::Not {
                src: 0,
                dst: 0,
                func_id: f,
                link: 13,
            },
            Instruction::TestJumpUnless {
                test: TestOp::NullP,
                a: 0,
                b: 0,
                dst: 0,
                target: 0,
                func_id: f,
                link: 14,
            },
            Instruction::AddImm {
                a: 0,
                imm,
                dst: 0,
                func_id: f,
                link: 15,
            },
            Instruction::SubImm {
                a: 0,
                imm,
                dst: 0,
                func_id: f,
                link: 16,
            },
            Instruction::LtImm {
                a: 0,
                imm,
                dst: 0,
                func_id: f,
                link: 17,
            },
            Instruction::NumEqImm {
                a: 0,
                imm,
                dst: 0,
                func_id: f,
                link: 18,
            },
            Instruction::NullP {
                src: 0,
                dst: 0,
                func_id: f,
                link: 19,
            },
            Instruction::PairP {
                src: 0,
                dst: 0,
                func_id: f,
                link: 20,
            },
            Instruction::VectorP {
                src: 0,
                dst: 0,
                func_id: f,
                link: 21,
            },
            Instruction::VectorRef {
                v: 0,
                i: 0,
                dst: 0,
                func_id: f,
                link: 22,
            },
            Instruction::VectorSet {
                v: 0,
                i: 0,
                val: 0,
                dst: 0,
                func_id: f,
                link: 23,
            },
        ];
        for (k, instr) in naming.iter().enumerate() {
            assert_eq!(instr.link(), Some(k as LinkIdx), "{instr:?}");
        }
        let not_naming = [
            Instruction::CallPrimitiveDirect {
                func_id: f,
                args: vec![0],
                dst: 0,
            },
            Instruction::MakeClosure {
                dst: 0,
                code_id: CodeObjectId::label(),
                free_vars: vec![0],
            },
            Instruction::LoadConst { dst: 0, idx: 0 },
            Instruction::LoadClosure { dst: 0, slot: 0 },
            Instruction::Call {
                func: 0,
                args: vec![0],
                dst: 0,
            },
            Instruction::JumpUnless { cond: 0, target: 0 },
            Instruction::Return { val: 0 },
            Instruction::Nop,
        ];
        for instr in &not_naming {
            assert_eq!(instr.link(), None, "{instr:?}");
        }
    }
}
