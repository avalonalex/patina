//! Structured bytecode tracer for the VM.
//!
//! Records typed `TraceEvent`s during execution for debugging and test assertions.
//! Enable via `VmBackend::set_tracer()` or the `--vm-trace` CLI flag.
//!
//! # Usage from tests
//!
//! ```ignore
//! let tracer = StepTracer::handle();
//! backend.set_tracer(Some(tracer.clone()));
//! // ... run code ...
//! let t = tracer.borrow();
//! assert!(t.writes_to_abs(3).is_empty(), "r0 clobbered:\n{}", t.format_log());
//! ```
//!
//! # Output formats
//!
//! - `format_log()` — human-readable, indented by frame depth
//! - `format_jsonl()` — one JSON object per line for programmatic analysis

use crate::types::CallFrame;
use crate::types::code_object::CodeObjectId;
use crate::types::instruction::Instruction;
use patina_core::heap::SharedHeap;
use patina_core::tagged_value::TaggedValue;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

/// Shared handle to a StepTracer (cheap to clone via Rc).
pub type TracerHandle = Rc<RefCell<StepTracer>>;

/// Monotonically increasing step counter.
pub type StepId = u64;

// ─────────────────────────────────────────────────────────────────────────────
// Trace events
// ─────────────────────────────────────────────────────────────────────────────

/// A single trace event emitted during VM execution.
#[derive(Debug, Clone)]
pub enum TraceEvent {
    /// An instruction is about to execute.
    Instr {
        step: StepId,
        depth: usize,
        code_id: u32,
        pc: usize,
        text: String,
        base: usize,
        num_regs: u16,
    },

    /// A register was written.
    RegWrite {
        step: StepId,
        reg: u16,
        abs_idx: usize,
        old_val: String,
        new_val: String,
        source: WriteSource,
    },

    /// A new CallFrame was pushed.
    FramePush {
        step: StepId,
        depth: usize,
        code_id: u32,
        base: usize,
        num_regs: u16,
        return_reg: u16,
    },

    /// A CallFrame was popped (Return or TailCall pop).
    FramePop {
        step: StepId,
        depth: usize,
        code_id: u32,
        result: String,
        return_reg: u16,
    },

    /// A control primitive was intercepted.
    ControlPrimitive {
        step: StepId,
        depth: usize,
        name: String,
        dst: u16,
    },

    /// run_thunk entered.
    ThunkEnter {
        step: StepId,
        depth: usize,
        context: String,
    },

    /// run_thunk returned.
    ThunkExit {
        step: StepId,
        depth: usize,
        result: String,
    },

    /// MutableCell operation.
    CellAccess {
        step: StepId,
        kind: CellAccessKind,
        cell_reg: u16,
        value: String,
    },

    /// Watchpoint triggered: an absolute register index changed.
    Watchpoint {
        step: StepId,
        abs_idx: usize,
        old_val: String,
        new_val: String,
        code_id: u32,
        pc: usize,
    },

    /// Exception handler event.
    ExceptionEvent {
        step: StepId,
        kind: ExceptionEventKind,
        depth: usize,
    },

    /// Continuation event.
    ContinuationEvent {
        step: StepId,
        kind: ContinuationEventKind,
        depth: usize,
    },
}

/// What caused a register write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteSource {
    /// Normal instruction execution.
    Instruction,
    /// Return instruction writing to caller's return_reg.
    Return,
    /// Continuation invocation delivering a value.
    ContinuationDeliver,
    /// Call/TailCall copying args into callee frame.
    ArgCopy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellAccessKind {
    Alloc,
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionEventKind {
    Push,
    Pop,
    Raise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationEventKind {
    CaptureFullCC,
    InvokeFull,
    InvokeDelimited,
}

/// Discriminant-only version for filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TraceEventKind {
    Instr,
    RegWrite,
    FramePush,
    FramePop,
    ControlPrimitive,
    ThunkEnter,
    ThunkExit,
    CellAccess,
    Watchpoint,
    ExceptionEvent,
    ContinuationEvent,
}

impl TraceEvent {
    pub fn kind(&self) -> TraceEventKind {
        match self {
            TraceEvent::Instr { .. } => TraceEventKind::Instr,
            TraceEvent::RegWrite { .. } => TraceEventKind::RegWrite,
            TraceEvent::FramePush { .. } => TraceEventKind::FramePush,
            TraceEvent::FramePop { .. } => TraceEventKind::FramePop,
            TraceEvent::ControlPrimitive { .. } => TraceEventKind::ControlPrimitive,
            TraceEvent::ThunkEnter { .. } => TraceEventKind::ThunkEnter,
            TraceEvent::ThunkExit { .. } => TraceEventKind::ThunkExit,
            TraceEvent::CellAccess { .. } => TraceEventKind::CellAccess,
            TraceEvent::Watchpoint { .. } => TraceEventKind::Watchpoint,
            TraceEvent::ExceptionEvent { .. } => TraceEventKind::ExceptionEvent,
            TraceEvent::ContinuationEvent { .. } => TraceEventKind::ContinuationEvent,
        }
    }

    pub fn step(&self) -> StepId {
        match self {
            TraceEvent::Instr { step, .. }
            | TraceEvent::RegWrite { step, .. }
            | TraceEvent::FramePush { step, .. }
            | TraceEvent::FramePop { step, .. }
            | TraceEvent::ControlPrimitive { step, .. }
            | TraceEvent::ThunkEnter { step, .. }
            | TraceEvent::ThunkExit { step, .. }
            | TraceEvent::CellAccess { step, .. }
            | TraceEvent::Watchpoint { step, .. }
            | TraceEvent::ExceptionEvent { step, .. }
            | TraceEvent::ContinuationEvent { step, .. } => *step,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Filter
// ─────────────────────────────────────────────────────────────────────────────

/// Controls which events are recorded. Empty sets mean "pass everything."
#[derive(Debug, Clone, Default)]
pub struct TraceFilter {
    /// Only record events for these code object IDs. Empty = all.
    pub code_ids: HashSet<u32>,
    /// Only record RegWrite events for these absolute register indices.
    pub watch_regs: HashSet<usize>,
    /// Absolute register indices to monitor. Triggers Watchpoint events
    /// when any instruction writes to these positions.
    pub watchpoints: HashSet<usize>,
    /// Event type filter. If non-empty, only record events of these kinds.
    pub event_kinds: HashSet<TraceEventKind>,
    /// Maximum number of events to record (0 = unlimited).
    pub max_events: usize,
}

impl TraceFilter {
    fn accepts_code(&self, code_id: u32) -> bool {
        self.code_ids.is_empty() || self.code_ids.contains(&code_id)
    }

    fn accepts_kind(&self, kind: TraceEventKind) -> bool {
        self.event_kinds.is_empty() || self.event_kinds.contains(&kind)
    }

    fn is_full(&self, count: usize) -> bool {
        self.max_events > 0 && count >= self.max_events
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// StepTracer
// ─────────────────────────────────────────────────────────────────────────────

/// The main tracer. Records `TraceEvent`s and provides query/formatting methods.
pub struct StepTracer {
    /// Recorded events.
    pub events: Vec<TraceEvent>,
    /// Filter controlling what gets recorded.
    pub filter: TraceFilter,
    /// Also print events to stderr as they happen.
    pub print_live: bool,
    /// Current step counter.
    step: StepId,
    /// Register snapshot before the current instruction (for diffing).
    pre_regs: Vec<TaggedValue>,
    /// Full register array snapshot (for watchpoints).
    pre_all_regs: Vec<TaggedValue>,
    /// Code ID of the current instruction (for post-instruction).
    pre_code_id: u32,
    /// PC of the current instruction.
    pre_pc: usize,
}

impl StepTracer {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            filter: TraceFilter::default(),
            print_live: false,
            step: 0,
            pre_regs: Vec::new(),
            pre_all_regs: Vec::new(),
            pre_code_id: 0,
            pre_pc: 0,
        }
    }

    pub fn with_filter(filter: TraceFilter) -> Self {
        Self {
            filter,
            ..Self::new()
        }
    }

    /// Create a `TracerHandle` (convenience for tests).
    pub fn handle() -> TracerHandle {
        Rc::new(RefCell::new(Self::new()))
    }

    /// Create a filtered `TracerHandle`.
    pub fn handle_with_filter(filter: TraceFilter) -> TracerHandle {
        Rc::new(RefCell::new(Self::with_filter(filter)))
    }

    pub fn reset(&mut self) {
        self.events.clear();
        self.step = 0;
    }

    // ── Pre/post instruction hooks ───────────────────────────────────────

    /// Called before each instruction dispatch. Snapshots registers.
    #[allow(clippy::too_many_arguments)]
    pub fn pre_instruction_with_depth(
        &mut self,
        registers: &[TaggedValue],
        frame: &CallFrame,
        code_id: CodeObjectId,
        pc: usize,
        instr: &Instruction,
        _heap: &SharedHeap,
        depth: usize,
    ) {
        self.step += 1;

        self.pre_code_id = code_id.0;
        self.pre_pc = pc;

        // Snapshot current frame's registers
        let base = frame.register_base;
        let end = (base + frame.num_regs as usize).min(registers.len());
        self.pre_regs = registers[base..end].to_vec();

        // Snapshot all registers for watchpoints
        if !self.filter.watchpoints.is_empty() {
            self.pre_all_regs = registers.to_vec();
        }

        if self.filter.is_full(self.events.len()) {
            return;
        }

        // Emit Instr event
        if self.filter.accepts_code(code_id.0) && self.filter.accepts_kind(TraceEventKind::Instr) {
            let mut dummy = Vec::new();
            let text = crate::disasm::format_instruction(instr, &mut dummy);
            let event = TraceEvent::Instr {
                step: self.step,
                depth,
                code_id: code_id.0,
                pc,
                text,
                base,
                num_regs: frame.num_regs,
            };
            self.record(event);
        }
    }

    /// Called after each instruction dispatch. Diffs registers and checks watchpoints.
    pub fn post_instruction(
        &mut self,
        registers: &[TaggedValue],
        frame: Option<&CallFrame>,
        heap: &SharedHeap,
    ) {
        if self.filter.is_full(self.events.len()) {
            return;
        }

        // Collect events first (avoids borrow conflict with self.pre_regs)
        let mut pending: Vec<TraceEvent> = Vec::new();

        // Diff frame registers
        if let Some(f) = frame
            && self.filter.accepts_code(self.pre_code_id)
            && self.filter.accepts_kind(TraceEventKind::RegWrite)
        {
            let base = f.register_base;
            let end = (base + f.num_regs as usize).min(registers.len());
            let after = &registers[base..end];
            for (i, (&old, &new)) in self.pre_regs.iter().zip(after.iter()).enumerate() {
                if old != new {
                    let abs = base + i;
                    if self.filter.watch_regs.is_empty() || self.filter.watch_regs.contains(&abs) {
                        pending.push(TraceEvent::RegWrite {
                            step: self.step,
                            reg: i as u16,
                            abs_idx: abs,
                            old_val: format_value(old, heap),
                            new_val: format_value(new, heap),
                            source: WriteSource::Instruction,
                        });
                    }
                }
            }
        }

        // Check watchpoints
        if !self.filter.watchpoints.is_empty() {
            for &idx in &self.filter.watchpoints {
                let old = self.pre_all_regs.get(idx).copied();
                let new = registers.get(idx).copied();
                if old != new {
                    pending.push(TraceEvent::Watchpoint {
                        step: self.step,
                        abs_idx: idx,
                        old_val: old
                            .map(|v| format_value(v, heap))
                            .unwrap_or_else(|| "<freed>".into()),
                        new_val: new
                            .map(|v| format_value(v, heap))
                            .unwrap_or_else(|| "<freed>".into()),
                        code_id: self.pre_code_id,
                        pc: self.pre_pc,
                    });
                }
            }
        }

        // Record all pending events
        for event in pending {
            self.record(event);
        }
    }

    // ── Emit helpers ─────────────────────────────────────────────────────

    /// Record an event (applies kind filter and max_events cap).
    pub fn emit(&mut self, event: TraceEvent) {
        if !self.filter.is_full(self.events.len()) && self.filter.accepts_kind(event.kind()) {
            self.record(event);
        }
    }

    /// Emit with the current step counter.
    pub fn emit_with_step(&mut self, f: impl FnOnce(StepId) -> TraceEvent) {
        if !self.filter.is_full(self.events.len()) {
            let event = f(self.step);
            if self.filter.accepts_kind(event.kind()) {
                self.record(event);
            }
        }
    }

    fn record(&mut self, event: TraceEvent) {
        if self.print_live {
            eprintln!("{}", format_event(&event));
        }
        self.events.push(event);
    }

    // ── Query methods for test assertions ────────────────────────────────

    /// All events of a given kind.
    pub fn events_of_kind(&self, kind: TraceEventKind) -> Vec<&TraceEvent> {
        self.events.iter().filter(|e| e.kind() == kind).collect()
    }

    /// All RegWrite events for a specific absolute register index.
    pub fn writes_to_abs(&self, abs_idx: usize) -> Vec<&TraceEvent> {
        self.events
            .iter()
            .filter(|e| matches!(e, TraceEvent::RegWrite { abs_idx: a, .. } if *a == abs_idx))
            .collect()
    }

    /// All RegWrite events for a specific frame-relative register.
    pub fn writes_to_reg(&self, reg: u16) -> Vec<&TraceEvent> {
        self.events
            .iter()
            .filter(|e| matches!(e, TraceEvent::RegWrite { reg: r, .. } if *r == reg))
            .collect()
    }

    /// All events within a specific code object.
    pub fn events_for_code(&self, code_id: u32) -> Vec<&TraceEvent> {
        self.events
            .iter()
            .filter(|e| match e {
                TraceEvent::Instr { code_id: c, .. }
                | TraceEvent::Watchpoint { code_id: c, .. } => *c == code_id,
                TraceEvent::FramePush { code_id: c, .. }
                | TraceEvent::FramePop { code_id: c, .. } => *c == code_id,
                _ => false,
            })
            .collect()
    }

    /// All Watchpoint events.
    pub fn watchpoints(&self) -> Vec<&TraceEvent> {
        self.events_of_kind(TraceEventKind::Watchpoint)
    }

    /// Total number of recorded events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    // ── Formatting ───────────────────────────────────────────────────────

    /// Human-readable log of all events.
    pub fn format_log(&self) -> String {
        self.events
            .iter()
            .map(format_event)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// JSONL format (one JSON object per line).
    pub fn format_jsonl(&self) -> String {
        self.events
            .iter()
            .map(format_event_json)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for StepTracer {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Value formatting
// ─────────────────────────────────────────────────────────────────────────────

/// Format a TaggedValue for trace output.
pub fn format_value(tv: TaggedValue, heap: &SharedHeap) -> String {
    if tv == TaggedValue::UNSPECIFIED {
        "#<void>".to_string()
    } else if tv == TaggedValue::NULL {
        "()".to_string()
    } else if tv == TaggedValue::TRUE {
        "#t".to_string()
    } else if tv == TaggedValue::FALSE {
        "#f".to_string()
    } else if let Some(n) = tv.as_fixnum() {
        format!("{}", n)
    } else if tv.is_char() {
        format!("#\\{}", tv.as_char().unwrap_or('?'))
    } else if tv.is_object() {
        let h = heap.borrow();
        let ty = h.get_object_type(tv);
        match ty {
            patina_core::heap::HeapObjectType::MutableCell => {
                let inner = h
                    .read_mutable_cell(tv)
                    .map(|v| {
                        if let Some(n) = v.as_fixnum() {
                            format!("{}", n)
                        } else {
                            format!("{:?}", v)
                        }
                    })
                    .unwrap_or_else(|| "???".into());
                format!("MutableCell({})", inner)
            }
            patina_core::heap::HeapObjectType::VmClosure => {
                if let Some((code_id, fv)) = h.get_vm_closure(tv) {
                    format!("VmClosure(#{}, {}cap)", code_id, fv.len())
                } else {
                    "VmClosure(?)".to_string()
                }
            }
            patina_core::heap::HeapObjectType::Symbol => {
                let name = h.get_symbol_name(tv).unwrap_or("?");
                format!("'{}", name)
            }
            patina_core::heap::HeapObjectType::Procedure => "#<prim>".to_string(),
            _ => format!("{:?}", ty),
        }
    } else {
        format!("{:?}", tv)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Event formatting
// ─────────────────────────────────────────────────────────────────────────────

fn format_event(e: &TraceEvent) -> String {
    match e {
        TraceEvent::Instr {
            step,
            depth,
            code_id,
            pc,
            text,
            base,
            num_regs,
        } => {
            format!(
                "[{:04}] D={} #{} pc={:<3} {}  (base={} regs={})",
                step, depth, code_id, pc, text, base, num_regs
            )
        }

        TraceEvent::RegWrite {
            step: _,
            reg,
            abs_idx,
            old_val,
            new_val,
            source,
        } => {
            let src = match source {
                WriteSource::Instruction => "",
                WriteSource::Return => " [return]",
                WriteSource::ContinuationDeliver => " [cont-deliver]",
                WriteSource::ArgCopy => " [arg-copy]",
            };
            format!(
                "       r{}[{}] = {}  (was {}){}",
                reg, abs_idx, new_val, old_val, src
            )
        }

        TraceEvent::FramePush {
            step,
            depth,
            code_id,
            base,
            num_regs,
            return_reg,
        } => {
            format!(
                "[{:04}]   >> PUSH D={} #{} base={} regs={} ret=r{}",
                step, depth, code_id, base, num_regs, return_reg
            )
        }

        TraceEvent::FramePop {
            step,
            depth,
            code_id,
            result,
            return_reg,
        } => {
            format!(
                "[{:04}]   << POP  D={} #{} result={} -> r{}",
                step, depth, code_id, result, return_reg
            )
        }

        TraceEvent::ControlPrimitive {
            step,
            depth,
            name,
            dst,
        } => {
            format!("[{:04}]   ** CTRL D={} {} dst=r{}", step, depth, name, dst)
        }

        TraceEvent::ThunkEnter {
            step,
            depth,
            context,
        } => {
            format!("[{:04}]   -> THUNK D={} {}", step, depth, context)
        }

        TraceEvent::ThunkExit {
            step,
            depth,
            result,
        } => {
            format!("[{:04}]   <- THUNK D={} result={}", step, depth, result)
        }

        TraceEvent::CellAccess {
            step,
            kind,
            cell_reg,
            value,
        } => {
            let op = match kind {
                CellAccessKind::Alloc => "ALLOC",
                CellAccessKind::Read => "READ",
                CellAccessKind::Write => "WRITE",
            };
            format!("[{:04}]   CELL {} r{} val={}", step, op, cell_reg, value)
        }

        TraceEvent::Watchpoint {
            step,
            abs_idx,
            old_val,
            new_val,
            code_id,
            pc,
        } => {
            format!(
                "[{:04}]   !! WATCH regs[{}] changed: {} -> {} (in #{} pc={})",
                step, abs_idx, old_val, new_val, code_id, pc
            )
        }

        TraceEvent::ExceptionEvent { step, kind, depth } => {
            let op = match kind {
                ExceptionEventKind::Push => "PUSH",
                ExceptionEventKind::Pop => "POP",
                ExceptionEventKind::Raise => "RAISE",
            };
            format!("[{:04}]   EXN {} D={}", step, op, depth)
        }

        TraceEvent::ContinuationEvent { step, kind, depth } => {
            let op = match kind {
                ContinuationEventKind::CaptureFullCC => "CAPTURE call/cc",
                ContinuationEventKind::InvokeFull => "INVOKE call/cc",
                ContinuationEventKind::InvokeDelimited => "INVOKE delimited",
            };
            format!("[{:04}]   CONT {} D={}", step, op, depth)
        }
    }
}

fn format_event_json(e: &TraceEvent) -> String {
    match e {
        TraceEvent::Instr {
            step,
            depth,
            code_id,
            pc,
            text,
            ..
        } => {
            format!(
                r#"{{"type":"instr","step":{},"depth":{},"code":{},"pc":{},"text":"{}"}}"#,
                step,
                depth,
                code_id,
                pc,
                text.replace('\\', "\\\\").replace('"', "\\\"")
            )
        }

        TraceEvent::RegWrite {
            step,
            reg,
            abs_idx,
            old_val,
            new_val,
            source,
        } => {
            let src = match source {
                WriteSource::Instruction => "instr",
                WriteSource::Return => "return",
                WriteSource::ContinuationDeliver => "cont",
                WriteSource::ArgCopy => "arg",
            };
            format!(
                r#"{{"type":"reg_write","step":{},"reg":{},"abs":{},"old":"{}","new":"{}","source":"{}"}}"#,
                step, reg, abs_idx, old_val, new_val, src
            )
        }

        TraceEvent::FramePush {
            step,
            depth,
            code_id,
            base,
            num_regs,
            return_reg,
        } => {
            format!(
                r#"{{"type":"frame_push","step":{},"depth":{},"code":{},"base":{},"regs":{},"ret":{}}}"#,
                step, depth, code_id, base, num_regs, return_reg
            )
        }

        TraceEvent::FramePop {
            step,
            depth,
            code_id,
            result,
            return_reg,
        } => {
            format!(
                r#"{{"type":"frame_pop","step":{},"depth":{},"code":{},"result":"{}","ret":{}}}"#,
                step, depth, code_id, result, return_reg
            )
        }

        TraceEvent::Watchpoint {
            step,
            abs_idx,
            old_val,
            new_val,
            code_id,
            pc,
        } => {
            format!(
                r#"{{"type":"watchpoint","step":{},"abs":{},"old":"{}","new":"{}","code":{},"pc":{}}}"#,
                step, abs_idx, old_val, new_val, code_id, pc
            )
        }

        TraceEvent::ControlPrimitive {
            step,
            depth,
            name,
            dst,
        } => {
            format!(
                r#"{{"type":"ctrl","step":{},"depth":{},"name":"{}","dst":{}}}"#,
                step, depth, name, dst
            )
        }

        TraceEvent::ThunkEnter {
            step,
            depth,
            context,
        } => {
            format!(
                r#"{{"type":"thunk_enter","step":{},"depth":{},"ctx":"{}"}}"#,
                step, depth, context
            )
        }

        TraceEvent::ThunkExit {
            step,
            depth,
            result,
        } => {
            format!(
                r#"{{"type":"thunk_exit","step":{},"depth":{},"result":"{}"}}"#,
                step, depth, result
            )
        }

        TraceEvent::CellAccess {
            step,
            kind,
            cell_reg,
            value,
        } => {
            let op = match kind {
                CellAccessKind::Alloc => "alloc",
                CellAccessKind::Read => "read",
                CellAccessKind::Write => "write",
            };
            format!(
                r#"{{"type":"cell","step":{},"op":"{}","reg":{},"val":"{}"}}"#,
                step, op, cell_reg, value
            )
        }

        TraceEvent::ExceptionEvent { step, kind, depth } => {
            let op = match kind {
                ExceptionEventKind::Push => "push",
                ExceptionEventKind::Pop => "pop",
                ExceptionEventKind::Raise => "raise",
            };
            format!(
                r#"{{"type":"exception","step":{},"op":"{}","depth":{}}}"#,
                step, op, depth
            )
        }

        TraceEvent::ContinuationEvent { step, kind, depth } => {
            let op = match kind {
                ContinuationEventKind::CaptureFullCC => "capture_cc",
                ContinuationEventKind::InvokeFull => "invoke_cc",
                ContinuationEventKind::InvokeDelimited => "invoke_del",
            };
            format!(
                r#"{{"type":"continuation","step":{},"op":"{}","depth":{}}}"#,
                step, op, depth
            )
        }
    }
}
