//! VM instruction set.
//!
//! Each variant is one instruction. The VM does NOT use a binary wire format in
//! Phase 2A — instructions live as `Vec<Instruction>` inside `CodeObject`.
//! A compact binary encoding can be added later without changing semantics.
//!
//! See VM_ISA.md §3 and §4 for the full specification.

use super::{CodeObjectId, ConstIdx, Reg};
use patina_core::core_expr::Symbol;
use patina_core::tagged_value::TaggedValue;

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

    /// `current_closure.free_vars[slot] ← src`
    ///
    /// Used for `set!` on mutable captured variables. The slot holds a
    /// `MutableCell` heap pointer; this instruction writes through the cell.
    StoreClosure { slot: u16, src: Reg },

    /// `dst ← globals[name]`  (error if unbound)
    LoadGlobal { dst: Reg, name: Symbol },

    /// `globals[name] ← src`
    StoreGlobal { name: Symbol, src: Reg },

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
    /// primitive at compile time. `name` is the global the callee resolved
    /// from: if the program later rebinds it (tracked per-primitive in
    /// `VmState::shadowed_primitives`), the VM deoptimizes this site back to
    /// the name-lookup `Call` path so redefinition semantics stay exact.
    CallPrimitive {
        func_id: PrimitiveFnId,
        name: Symbol,
        args: Vec<Reg>,
        dst: Reg,
    },

    // ── Inline primitive opcodes (Track P P3) ───────────────────────────────
    //
    // Fixed-arity fast paths for the hottest primitives, executed directly in
    // the dispatch loop. Every opcode carries the same `(func_id, name)` pair
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
        name: Symbol,
    },
    /// 2-arg `-`.
    Sub {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 2-arg `*`.
    Mul {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 2-arg `<`.
    Lt {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 2-arg `=`.
    NumEq {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 2-arg `eq?` — total, one `values_eq` heap call.
    Eq {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 2-arg `cons` — total, one `alloc_pair`.
    Cons {
        a: Reg,
        b: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 1-arg `car`. Fast path: native pair only; boxed pairs and type errors
    /// go to the handler.
    Car {
        src: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 1-arg `cdr`.
    Cdr {
        src: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 1-arg `not` — total truthiness test (only #f is falsy).
    Not {
        src: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// Fused `not` + branch (Track P P5). Emitted in place of a `Not` whose
    /// result feeds the `JumpUnless` that *must still follow at the next pc*:
    /// the fast path writes `dst ← not(src)` and branches directly (to
    /// `target` when `src` is truthy, over the following `JumpUnless`
    /// otherwise), saving one dispatch. When `not` is shadowed it deopts to
    /// exactly the un-fused pair: call the current binding into `dst` and
    /// fall through to the kept `JumpUnless`, which performs the branch.
    NotJumpUnless {
        src: Reg,
        dst: Reg,
        target: usize,
        func_id: PrimitiveFnId,
        name: Symbol,
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
        name: Symbol,
    },
    /// 2-arg `-` with a literal right operand: `(- x 1)`-shaped sites only.
    SubImm {
        a: Reg,
        imm: TaggedValue,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 2-arg `<` with a literal right operand: `(< i n)`-shaped sites only.
    LtImm {
        a: Reg,
        imm: TaggedValue,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 2-arg `=` with a literal right operand: `(= x 0)`-shaped sites only.
    NumEqImm {
        a: Reg,
        imm: TaggedValue,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 1-arg `null?` — total bit test.
    NullP {
        src: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 1-arg `pair?` — total bit test.
    PairP {
        src: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 1-arg `vector?` — total bit test.
    VectorP {
        src: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 2-arg `vector-ref`. Fast path: vector + in-bounds fixnum index;
    /// out-of-bounds falls back so the error message comes from the handler.
    VectorRef {
        v: Reg,
        i: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
    },
    /// 3-arg `vector-set!`. Writes `dst ← unspecified` on the fast path,
    /// matching the handler's return value.
    VectorSet {
        v: Reg,
        i: Reg,
        val: Reg,
        dst: Reg,
        func_id: PrimitiveFnId,
        name: Symbol,
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
    /// Push a dynamic-wind record onto `VmState::dynamic_winds`.
    ///
    /// Uses `stack_depth = 0` so `pop_resolved_winds` won't auto-pop it;
    /// the explicit `PopWind` instruction handles cleanup instead.
    PushWind { before: Reg, after: Reg },

    /// Pop the top dynamic-wind record from `VmState::dynamic_winds`.
    /// Does NOT call the after-thunk — that is handled by a separate `Call`
    /// instruction emitted after `PopWind` in the codegen sequence.
    PopWind,

    // ── Multiple Values ───────────────────────────────────────────────────────
    /// Return multiple values to the caller via `VmState::value_buffer`.
    ReturnMulti { vals: Vec<Reg> },

    /// Bind incoming multiple values from `VmState::value_buffer` into registers.
    /// Errors if the value count doesn't match `dsts.len()`.
    ReceiveValues { dsts: Vec<Reg> },

    // ── Global Definitions ────────────────────────────────────────────────────
    /// Top-level `define`: `globals[name] ← reg[src]`.
    Define { name: Symbol, src: Reg },

    // ── Continuations (SRFI-226) ──────────────────────────────────────────────
    /// Maps to `call-with-continuation-prompt`.
    ///
    /// 1. Push a `PromptFrame` onto `VmState::prompt_stack`.
    /// 2. Call `body` thunk as a normal `Call`.
    /// 3. Normal return: pop prompt, write result to `dst`.
    /// 4. `AbortToPrompt` with matching tag: unwind, invoke `handler`.
    CallWithPrompt {
        body: Reg,
        tag: Reg,
        handler: Reg,
        dst: Reg,
    },

    /// Maps to `abort-current-continuation`.
    ///
    /// Searches prompt stack for nearest frame matching `tag`, clones the
    /// intervening frames as a `VmDelimitedContinuation`, unwinds the stack,
    /// then calls the prompt's handler with `(val, captured_continuation)`.
    AbortToPrompt { tag: Reg, val: Reg },

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

/// Identifier for a registered primitive function.
///
/// In Phase 2A this is an index into a flat `Vec` of primitive descriptors
/// built at startup. The VM resolves it to a function pointer at load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrimitiveFnId(pub u32);
