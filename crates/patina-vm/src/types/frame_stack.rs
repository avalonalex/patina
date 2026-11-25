//! The VM's register stack (#908, GC stage 4d.7b): frames and their
//! registers interleaved in one buffer of tagged words.
//!
//! Until 4d.7b the machine kept a vector of `CallFrame`s beside a vector of
//! registers, and a continuation cloned both. Here a frame is a header of
//! five words followed by its registers, in the layout `PRD/GC_PRD.md`
//! §11.1 gives the interpreter, JIT code and captured continuations alike:
//!
//! ```text
//! fp+0   ret      0 in an interpreter's frame
//! fp+8   link     the distance to the caller in bytes [3–34] | nregs [35–50] | flags [51–62]
//! fp+16  code     the code object's address
//! fp+24  meta     pc [3–31] | return_reg [35–50]
//! fp+32  closure  a value
//! fp+40  r0 … rₙ₋₁
//! ```
//!
//! **Every word is a value.** `ret`, `link` and `meta` are fixnums, and so
//! is `code` as the collector reads it: an 8-aligned address has a fixnum's
//! tag. So a copy of the buffer can be traced word by word, with no
//! knowledge of frames.
//!
//! **`link` is a distance,** so a run of frames can be copied to another
//! place in a buffer, or into a buffer of its own, and still be walked: a
//! delimited continuation's frames are appended with one word fixed up, the
//! outermost frame's distance to its new caller. The bottom frame's
//! distance is its header's size, as if a caller's registers began at the
//! buffer's base, so that a pop is the same subtraction for every frame.
//!
//! **Code liveness until 4e.** The `code` word is an address nothing
//! counts. Beside the buffer is a vector of the frames' code objects, which
//! keeps each frame's unit loaded, reports each frame's code id to the
//! collector, and is what a frame's view hands out. It holds the object
//! once for each run, a stretch of consecutive frames that run the same
//! code, and the first frame of a run says so in a flag of its `link`
//! word. A recursion a million frames deep is one run, so a frame costs
//! its 40 bytes and its registers and nothing more, and a capture of it
//! copies one reference, where a reference a frame cost a count up at
//! every capture and a count down at every drop (measured 2026-11-24: 14%
//! of `samedepth1000`'s instructions). A frame that joins the run below it
//! touches the vector neither when it is pushed nor when it is popped. 4e
//! replaces the address with a reference to a code descriptor, and the
//! vector and the flag go.
//!
//! **A run that ends is spent, not dropped.** When the first frame of a
//! run returns, the run's entry stays in the vector, past the entries of
//! the frames there are (the field `runs` counts those). The next run
//! started there takes it back if it is of the same code, as the callee of
//! a loop is at every call, and then no reference is counted up or down: a
//! push is given its code borrowed from the store and takes one of its own
//! only for a run whose place holds other code. A spent entry still holds
//! its code, so whatever asks whether a frame holds a code object drops
//! them first ([`FrameStack::drop_spent`]): the store, before it lets a
//! unit go (`unit_in_use`), at a form's end and after every collection. No
//! copy has one. (Measured 2026-11-24 against a vector pushed and popped
//! with every run, each run counting its frames: the bookkeeping cost a
//! call and its return about 20 instructions within one procedure and 30
//! between two.)
//!
//! **The top frame's pc is not in its header.** The dispatch loop reads and
//! advances a pc once an instruction, and here that is a field of this
//! type, [`FrameStack::pc`], beside the address of the top frame's code and
//! the index of its first register: three loads from fixed offsets, with
//! no bounds check and no decoding. A frame's header holds its pc from the
//! moment another frame is pushed above it, and the top frame's header
//! holds whatever it held last. Everything that reads a pc goes through
//! [`FrameStack::frames`] or [`FrameStack::top`], which answer the field
//! for the top frame, and the type has no other reader, so no caller can
//! see the stale word. A copy is a clone and carries the field with it.

use super::code_object::CodeObject;
use super::{CallFrame, FrameCode, Reg};
use patina_core::Trace;
use patina_core::tagged_value::TaggedValue;
use std::cell::Cell;
use std::rc::Rc;

/// The words in a frame's header.
pub(crate) const HEADER_WORDS: usize = 5;

/// The bytes in a word of the stack.
pub(crate) const WORD_BYTES: usize = std::mem::size_of::<TaggedValue>();

/// The bytes in a frame's header: `PRD/GC_PRD.md` §11.1's 40.
pub(crate) const FRAME_HEADER_BYTES: usize = HEADER_WORDS * WORD_BYTES;

// A header word's place below its frame's first register.
const LINK: usize = 4;
const CODE: usize = 3;
const META: usize = 2;
const CLOSURE: usize = 1;

/// What a pc is below: it is 29 bits of `meta` (decision A3 on #782). A
/// code object has fewer instructions than this, checked where it is
/// loaded, so that the pc one past its last fits too.
pub(crate) const MAX_INSTRUCTIONS: usize = 1 << 29;

/// The pc's bits of a `meta` word, as a fixnum's payload.
const PC_MASK: u64 = (MAX_INSTRUCTIONS as u64) - 1;

/// Where the second field of a header word starts, in its payload: `nregs`
/// in `link` and `return_reg` in `meta`, bit 35 of the word.
const HIGH_SHIFT: u32 = 32;

/// The distance's bits of a `link` word, as a fixnum's payload: 32, in
/// bytes.
const DISTANCE_MASK: u64 = (1 << HIGH_SHIFT) - 1;

/// A header word's payload: the bits above a fixnum's tag.
#[inline(always)]
fn payload(word: TaggedValue) -> u64 {
    word.raw() >> 3
}

/// The one flag of a `link` word the interpreter sets before stage 7, as
/// a fixnum's payload: the frame is the first of its run of code, the one
/// that starts an entry of the side vector (the module's comment). The
/// last of the flags' bits, bit 62 of the word; it goes with the vector at
/// 4e.
const RUN_FLAG: u64 = 1 << 59;

/// A `link` word: the distance to the caller in words, the window's
/// registers, and whether the frame starts a run of code. The other flags
/// are zero until stage 7 has one to set.
#[inline(always)]
fn link_word(distance: usize, num_regs: u16, starts_run: bool) -> TaggedValue {
    TaggedValue::fixnum(
        ((distance * WORD_BYTES) as u64
            | (num_regs as u64) << HIGH_SHIFT
            | if starts_run { RUN_FLAG } else { 0 }) as i64,
    )
}

/// Whether a `link` word's frame starts a run of code: 1 or 0, which a pop
/// takes from the count of runs.
#[inline(always)]
fn link_starts_run(link: TaggedValue) -> usize {
    ((payload(link) & RUN_FLAG) != 0) as usize
}

/// The distance a `link` word holds, in words.
#[inline(always)]
fn link_distance(link: TaggedValue) -> usize {
    (payload(link) & DISTANCE_MASK) as usize / WORD_BYTES
}

/// The registers a `link` word counts.
#[inline(always)]
fn link_num_regs(link: TaggedValue) -> u16 {
    (payload(link) >> HIGH_SHIFT) as u16
}

/// A `meta` word.
#[inline(always)]
fn meta_word(pc: usize, return_reg: Reg) -> TaggedValue {
    debug_assert!(pc < MAX_INSTRUCTIONS);
    TaggedValue::fixnum((pc as u64 | (return_reg as u64) << HIGH_SHIFT) as i64)
}

/// `meta` with its pc replaced: the word's high half kept, and the low one
/// written, as the one 32-bit store decision A3 describes.
#[inline(always)]
fn meta_with_pc(meta: TaggedValue, pc: usize) -> TaggedValue {
    debug_assert!(pc < MAX_INSTRUCTIONS);
    TaggedValue::fixnum((((meta.raw() >> 35) << HIGH_SHIFT) | pc as u64) as i64)
}

/// The pc a `meta` word holds.
#[inline(always)]
fn meta_pc(meta: TaggedValue) -> usize {
    (payload(meta) & PC_MASK) as usize
}

/// The return register a `meta` word holds.
#[inline(always)]
fn meta_return_reg(meta: TaggedValue) -> Reg {
    (payload(meta) >> HIGH_SHIFT) as Reg
}

/// A code object's address, which is what a frame's `code` word holds and
/// what [`FrameStack::top_code`] compares.
#[inline(always)]
fn code_address(code: &Rc<CodeObject>) -> usize {
    Rc::as_ptr(code) as usize
}

/// A `code` word: the address, which a fixnum's tag fits, since a code
/// object is 8-aligned.
#[inline(always)]
fn code_word(code: &Rc<CodeObject>) -> TaggedValue {
    const { assert!(std::mem::align_of::<CodeObject>() >= 8) };
    TaggedValue::fixnum((code_address(code) >> 3) as i64)
}

/// The address a `code` word holds: [`code_word`]'s inverse. Compared with
/// a code object's address, and never read through.
#[inline(always)]
fn word_address(word: TaggedValue) -> usize {
    // An 8-aligned address is its own fixnum: the tag's three bits are 0.
    word.raw() as usize
}

/// Put a copy of `words` on the end of `buffer`, as one run of stores.
/// `<[T]>::to_vec` and `extend_from_slice` clone a slice of cells an
/// element at a time under a guard, which costs a capture several
/// instructions a word.
#[inline(always)]
fn extend_words(buffer: &mut Vec<Cell<TaggedValue>>, words: &[Cell<TaggedValue>]) {
    buffer.extend(words.iter().map(|word| Cell::new(word.get())));
}

/// A copy of `words`, in a buffer of its size.
fn copy_words(words: &[Cell<TaggedValue>]) -> Vec<Cell<TaggedValue>> {
    let mut copy = Vec::with_capacity(words.len());
    extend_words(&mut copy, words);
    copy
}

/// The least buffer, in words, that an arrival keeps whatever it needs:
/// below it a buffer is too small for its size to matter.
const KEPT_WORDS: usize = 1 << 12;

/// A stack of frames, live or captured: see the module's comment.
///
/// Its trace (derived, #686) is a captured copy's: every word a slot, which
/// reports the closures and the registers and passes over the fixnums, and
/// each run's code by id. The live stack's is not this: a collection walks
/// its frames under their maps (`gc_roots::trace_frames`, #906), and
/// `ExecutionState` skips the field.
#[derive(Debug, Default, Trace)]
pub struct FrameStack {
    /// The headers and the registers, bottom first.
    words: Vec<Cell<TaggedValue>>,
    /// The frames' code, bottom first, once for each run of frames that
    /// share it, in `codes[..runs]`: what keeps a frame's unit loaded until
    /// 4e makes the `code` word a reference. The last of those is the top
    /// frame's. The entries past them are spent (the module's comment):
    /// the live stack alone has any, so a traced copy's trace, which
    /// reports each entry's code by id ([`FrameCode`], #836), reports the
    /// code of its frames and no other.
    codes: Vec<FrameCode>,
    /// How many of `codes` are the runs of the frames there are: as many
    /// as the frames whose `link` word has the run flag.
    #[trace(skip, reason = "a count of `codes`' entries")]
    runs: usize,
    /// The index in `words` of the top frame's first register, or 0 with
    /// no frame.
    #[trace(skip, reason = "a word index")]
    base: usize,
    /// The top frame's pc: the index of the next instruction it runs. Its
    /// header's copy is written when a frame is pushed above it.
    #[trace(skip, reason = "an instruction index")]
    pc: usize,
    /// The address of the top frame's code, or 0 with no frame: what the
    /// dispatch loop compares its own code with, and a push the code it is
    /// given. Never dereferenced; the last run's code is the object.
    #[trace(skip, reason = "an address, which is compared and never read through")]
    top_code: usize,
}

/// A capture's copy: the words as one run of stores, and a reference for
/// each run of code. An arrival copies back into the buffer the live stack
/// already has (`clone_from`), so that it neither frees and takes a buffer
/// nor leaves one with no room for the next push.
///
/// **Unless that buffer is far larger than the arrival needs:** more than
/// four times its words. Then the buffer goes, and the arrival takes one of
/// its own size, as every arrival did while it replaced the vectors. An
/// escape from the bottom of a deep recursion is such an arrival, and a
/// `guard` that catches `&stack-exhausted` is one: without this the stack
/// the recursion grew would stay with the process, gigabytes of it at the
/// default cap, until 4d.8 returns a stack's pages (#909).
impl Clone for FrameStack {
    fn clone(&self) -> Self {
        FrameStack {
            words: copy_words(&self.words),
            codes: self.live_runs().to_vec(),
            runs: self.runs,
            base: self.base,
            pc: self.pc,
            top_code: self.top_code,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        if self.words.capacity() / 4 > source.words.len().max(KEPT_WORDS) {
            self.words = copy_words(&source.words);
        } else {
            self.words.clear();
            extend_words(&mut self.words, &source.words);
        }
        // The spent runs go with the frames the arrival replaces.
        self.codes.truncate(source.runs);
        let kept = self.codes.len();
        self.codes.clone_from_slice(&source.codes[..kept]);
        self.codes
            .extend_from_slice(&source.codes[kept..source.runs]);
        self.runs = source.runs;
        self.base = source.base;
        self.pc = source.pc;
        self.top_code = source.top_code;
    }
}

impl FrameStack {
    // ── Reading ─────────────────────────────────────────────────────────

    /// Every word, headers and registers, bottom first.
    #[inline(always)]
    pub(crate) fn words(&self) -> &[Cell<TaggedValue>] {
        &self.words
    }

    /// How many frames the stack holds, by a walk of them: the stack
    /// counts its runs of code and not its frames. What the census and the
    /// step tracer report; nothing else in the machine reads it, and a
    /// build with no census never asks ([`Self::census_shape`]).
    pub(crate) fn frame_count(&self) -> usize {
        self.frames().count()
    }

    /// The frames and the registers the stack holds, for the census's
    /// hooks, and `(0, 0)` in a build without the census: the count is a
    /// walk of the frames, which a capture or a collection of a deep stack
    /// must not pay for a number nothing reads (measured 2026-11-24: on a
    /// stack 1,000,000 frames deep, eleven collections' walks took three
    /// times the program's own cycles).
    #[inline(always)]
    pub(crate) fn census_shape(&self) -> (usize, usize) {
        if patina_core::census::GC_CENSUS {
            (self.frame_count(), self.register_count())
        } else {
            (0, 0)
        }
    }

    /// The code of the runs of the frames there are, bottom first: `codes`
    /// without its spent entries.
    #[inline(always)]
    fn live_runs(&self) -> &[FrameCode] {
        &self.codes[..self.runs]
    }

    /// Let go of the spent runs, and with them the code no frame of this
    /// stack runs: before anything asks whether a frame holds a code object
    /// (the module's comment).
    pub(crate) fn drop_spent(&mut self) {
        self.codes.truncate(self.runs);
    }

    #[inline(always)]
    pub(crate) fn is_empty(&self) -> bool {
        self.base == 0
    }

    /// The registers the stack holds: its words less its headers.
    pub(crate) fn register_count(&self) -> usize {
        self.words.len() - self.frame_count() * HEADER_WORDS
    }

    /// What the stack holds in bytes: the stack's top, as an offset from
    /// its base.
    #[inline(always)]
    pub(crate) fn bytes(&self) -> usize {
        self.words.len() * WORD_BYTES
    }

    /// The bytes its buffers hold at capacity, which a snapshot's handle is
    /// charged (#606).
    pub(crate) fn buffer_bytes(&self) -> usize {
        self.words.capacity() * WORD_BYTES
            + self.codes.capacity() * std::mem::size_of::<FrameCode>()
    }

    /// The index of the top frame's first register, or 0 with no frame.
    #[inline(always)]
    pub(crate) fn base(&self) -> usize {
        self.base
    }

    /// The offset in bytes at which the top frame's header starts, or 0
    /// with no frame: the stack's top before that frame was pushed.
    #[inline(always)]
    pub(crate) fn top_frame_bytes(&self) -> usize {
        self.base.saturating_sub(HEADER_WORDS) * WORD_BYTES
    }

    /// The top frame's next pc.
    #[cfg(test)]
    pub(crate) fn pc(&self) -> usize {
        self.pc
    }

    /// The header word `field` words below the first register at `base`.
    #[inline(always)]
    fn header(&self, base: usize, field: usize) -> &Cell<TaggedValue> {
        &self.words[base - field]
    }

    /// The top frame's closure word.
    #[cfg(test)]
    pub(crate) fn top_closure(&self) -> Option<TaggedValue> {
        (self.base != 0).then(|| self.header(self.base, CLOSURE).get())
    }

    /// The closure word of the frame whose first register is word `base`:
    /// the top frame's for the dispatch loop, which has its base in hand,
    /// under one range check.
    #[inline(always)]
    pub(crate) fn closure_at(&self, base: usize) -> TaggedValue {
        self.words[base - CLOSURE].get()
    }

    /// The top frame's code.
    #[inline(always)]
    pub(crate) fn top_code(&self) -> Option<&Rc<CodeObject>> {
        // With no run the index wraps, and there is no such entry.
        self.codes
            .get(self.runs.wrapping_sub(1))
            .map(|code| &code.0)
    }

    /// The top frame's window size.
    #[inline(always)]
    fn top_num_regs(&self) -> u16 {
        link_num_regs(self.header(self.base, LINK).get())
    }

    /// Where the top frame's window ends, or 0 with no frame.
    #[inline(always)]
    pub(crate) fn top_end(&self) -> usize {
        if self.base == 0 {
            0
        } else {
            self.base + self.top_num_regs() as usize
        }
    }

    /// The first register of the frame below the top one, if there are
    /// two.
    #[inline(always)]
    pub(crate) fn caller_base(&self) -> Option<usize> {
        if self.base == 0 {
            return None;
        }
        // The bottom frame's distance is its own base.
        let caller = self.base - link_distance(self.header(self.base, LINK).get());
        (caller != 0).then_some(caller)
    }

    /// The top frame, as a view.
    pub(crate) fn top(&self) -> Option<CallFrame<'_>> {
        self.frames().next()
    }

    /// The frames, top first, each as a view of its header.
    pub(crate) fn frames(&self) -> Frames<'_> {
        Frames {
            stack: self,
            base: self.base,
            run: self.runs,
        }
    }

    // ── The dispatch loop's ─────────────────────────────────────────────

    /// One instruction's prologue: make `cur_code` the top frame's code if
    /// it is not, answering whether it changed, and take the pc, advanced
    /// past the instruction. Panics with no frame.
    #[inline(always)]
    pub(crate) fn dispatch(&mut self, cur_code: &mut Rc<CodeObject>) -> (usize, bool) {
        let switched = code_address(cur_code) != self.top_code;
        if switched {
            *cur_code = Rc::clone(self.top_code().expect("empty frame stack"));
        }
        let pc = self.pc;
        self.pc = pc + 1;
        (pc, switched)
    }

    #[inline(always)]
    pub(crate) fn set_pc(&mut self, pc: usize) {
        self.pc = pc;
    }

    // ── Pushing and popping ─────────────────────────────────────────────

    /// Write the top frame's pc into its header, where the frame about to
    /// be pushed above it, or a copy cut below it, leaves it to be read.
    #[inline(always)]
    fn spill_pc(&self) {
        if let Some(meta) = self.words.get(self.base.wrapping_sub(META)) {
            meta.set(meta_with_pc(meta.get(), self.pc));
        }
    }

    /// Take the top frame's pc and its code's address from its header:
    /// after a pop or a cut. With no frame both are 0.
    #[inline(always)]
    fn load_top(&mut self) {
        let base = self.base;
        // One range check for the header's words; with no frame the range
        // starts below zero, and there is no such slice.
        match self.words.get(base.wrapping_sub(HEADER_WORDS)..base) {
            Some(header) => {
                self.pc = meta_pc(header[HEADER_WORDS - META].get());
                self.top_code = word_address(header[HEADER_WORDS - CODE].get());
            }
            None => {
                self.pc = 0;
                self.top_code = 0;
            }
        }
    }

    /// Start a run of `code`, whose address is `address`, above the runs
    /// there are: the top frame's from here. The spent entry there is the
    /// run's again if it holds this code, and no reference is counted;
    /// otherwise the run takes a reference of its own, and the spent
    /// entry's goes.
    #[inline(always)]
    fn start_run(&mut self, code: &Rc<CodeObject>, address: usize) {
        match self.codes.get_mut(self.runs) {
            Some(spent) => {
                if code_address(spent) != address {
                    *spent = FrameCode(Rc::clone(code));
                }
            }
            None => self.grow_runs(code),
        }
        self.runs += 1;
        self.top_code = address;
    }

    /// [`Self::start_run`] where no entry is spent: the vector grows, as
    /// it does once for each depth of runs a program reaches.
    #[cold]
    #[inline(never)]
    fn grow_runs(&mut self, code: &Rc<CodeObject>) {
        self.codes.push(FrameCode(Rc::clone(code)));
    }

    /// Give the top frame's place in the side vector to `code`, whose
    /// address is `address` and is not the top frame's code's: a tail
    /// call's or a stub's next round. `starts_run` is the frame's flag as
    /// it stood. A frame that shared its caller's run starts one of its
    /// own, and the caller of this sets its flag. A frame that was the
    /// whole of its run keeps the run's place, and the code it ran goes to
    /// the spent entry above, where a tail call back to it finds it: two
    /// procedures that call each other in tail position change places
    /// there, and no reference is counted.
    #[inline(always)]
    fn set_top_code(&mut self, code: &Rc<CodeObject>, address: usize, starts_run: usize) {
        if starts_run == 0 {
            return self.start_run(code, address);
        }
        self.top_code = address;
        // The top run and the spent entry above it, under one range check;
        // with no spent entry there is no such pair.
        match self.codes.get_mut(self.runs.wrapping_sub(1)..=self.runs) {
            Some([top, spent]) => {
                if code_address(spent) != address {
                    *spent = FrameCode(Rc::clone(code));
                }
                std::mem::swap(top, spent);
            }
            _ => self.retire_top_code(code),
        }
    }

    /// [`Self::set_top_code`] where no entry is spent: the vector grows by
    /// the code the frame ran. Panics with no frame.
    #[cold]
    #[inline(never)]
    fn retire_top_code(&mut self, code: &Rc<CodeObject>) {
        let ran = std::mem::replace(&mut self.codes[self.runs - 1], FrameCode(Rc::clone(code)));
        self.codes.push(ran);
    }

    /// Push a frame of `num_regs` registers, each holding `fill`, at pc 0.
    /// Answers the index of its first register.
    #[inline(always)]
    pub(crate) fn push(
        &mut self,
        code: &Rc<CodeObject>,
        closure: TaggedValue,
        return_reg: Reg,
        num_regs: u16,
        fill: TaggedValue,
    ) -> usize {
        self.spill_pc();
        let base = self.words.len() + HEADER_WORDS;
        // The header's five words, then the window, written straight into
        // the buffer's spare room: the buffer is asked for room once, and
        // no word is written twice. `Vec`'s own `extend` and `resize` each
        // ask again, and cost a call a dozen instructions more (measured
        // 2026-11-24, on `fib`).
        let num_regs = num_regs as usize;
        let len = self.words.len();
        let total = HEADER_WORDS + num_regs;
        self.words.reserve(total);
        // The frame joins the run below it if it runs the same code, as
        // every frame of a recursion does, and starts one otherwise.
        let address = code_address(code);
        let starts_run = address != self.top_code;
        let header = [
            TaggedValue::fixnum(0),
            link_word(base - self.base, num_regs as u16, starts_run),
            code_word(code),
            meta_word(0, return_reg),
            closure,
        ];
        // SAFETY: `reserve` left room for `total` words past `len`, so every
        // write below is inside the allocation and past the initialized
        // words. All `total` are written before `set_len` takes them in,
        // and a `Cell<TaggedValue>` has no destructor to run on what the
        // spare room held.
        unsafe {
            let at = self.words.as_mut_ptr().add(len);
            for (i, word) in header.into_iter().enumerate() {
                at.add(i).write(Cell::new(word));
            }
            for i in 0..num_regs {
                at.add(HEADER_WORDS + i).write(Cell::new(fill));
            }
            self.words.set_len(len + total);
        }
        if starts_run {
            self.start_run(code, address);
        }
        self.base = base;
        self.pc = 0;
        base
    }

    /// Pop the top frame with its window, and answer its `return_reg`. If
    /// it was the first frame of its run the run's entry is spent, and
    /// still holds its code (the module's comment), so the pop drops
    /// nothing, calls nothing and does not read the side vector. Panics
    /// with no frame.
    #[inline(always)]
    pub(crate) fn pop(&mut self) -> Reg {
        let base = self.base;
        // One range check for the header's words; with no frame the range
        // starts below zero, and the slice refuses it.
        let header = &self.words[base.wrapping_sub(HEADER_WORDS)..base];
        let link = header[HEADER_WORDS - LINK].get();
        let return_reg = meta_return_reg(header[HEADER_WORDS - META].get());
        self.words.truncate(base - HEADER_WORDS);
        self.base = base - link_distance(link);
        self.runs -= link_starts_run(link);
        // The caller's pc, and its code's address whichever run it is in:
        // both from its header, so the pop has one path.
        self.load_top();
        return_reg
    }

    /// Give the top frame to `code` at pc 0, running `closure`: a tail
    /// call's replacement, in the window as it stands, grown to the code's
    /// registers if it has fewer, the new ones holding `fill`. A window
    /// never shrinks. The code is not the frame's own: a tail call to that
    /// is [`Self::restart_top`].
    #[inline(always)]
    pub(crate) fn replace_top(
        &mut self,
        code: &Rc<CodeObject>,
        closure: TaggedValue,
        fill: TaggedValue,
    ) {
        let base = self.base;
        let address = code_address(code);
        debug_assert_ne!(
            address, self.top_code,
            "a tail call to the frame's own code"
        );
        // One range check for the header's words; with no frame the range
        // starts below zero, and the slice refuses it.
        let header = &self.words[base.wrapping_sub(HEADER_WORDS)..base];
        let link = header[HEADER_WORDS - LINK].get();
        let (had, starts_run) = (link_num_regs(link), link_starts_run(link));
        let num_regs = code.num_regs.max(had);
        // The frame starts a run from here, whichever it was in.
        header[HEADER_WORDS - LINK].set(link_word(link_distance(link), num_regs, true));
        header[HEADER_WORDS - CODE].set(code_word(code));
        header[HEADER_WORDS - CLOSURE].set(closure);
        if num_regs > had {
            self.words.resize(base + num_regs as usize, Cell::new(fill));
        }
        self.set_top_code(code, address, starts_run);
        self.pc = 0;
    }

    /// Start the top frame again at pc 0 in its own code, running `closure`.
    #[inline(always)]
    pub(crate) fn restart_top(&mut self, closure: TaggedValue) {
        self.header(self.base, CLOSURE).set(closure);
        self.pc = 0;
    }

    /// Start the top frame at pc 0 in `code`, its closure and its window
    /// as they stand: a stub's next round, which may be in the code it is
    /// in.
    pub(crate) fn recode_top(&mut self, code: &Rc<CodeObject>) {
        self.pc = 0;
        let address = code_address(code);
        if address == self.top_code {
            return;
        }
        let base = self.base;
        let link = self.header(base, LINK);
        let starts_run = link_starts_run(link.get());
        link.set(link_word(
            link_distance(link.get()),
            link_num_regs(link.get()),
            true,
        ));
        self.header(base, CODE).set(code_word(code));
        self.set_top_code(code, address, starts_run);
    }

    /// Set the register of its caller that the top frame returns into.
    pub(crate) fn set_top_return_reg(&mut self, return_reg: Reg) {
        let meta = self.header(self.base, META);
        meta.set(meta_word(meta_pc(meta.get()), return_reg));
    }

    // ── Cutting, copying and appending ──────────────────────────────────

    /// The index of the first register of the topmost frame whose header
    /// starts below word `cut`, or 0 if none does, and how many runs of
    /// code start at or above it. A walk down from the top: what a cut or
    /// a copy at `cut` is about to drop or take, frame by frame.
    fn frames_from(&self, cut: usize) -> (usize, usize) {
        let mut base = self.base;
        let mut runs = 0;
        while base != 0 && base - HEADER_WORDS >= cut {
            let link = self.header(base, LINK).get();
            runs += link_starts_run(link);
            base -= link_distance(link);
        }
        (base, runs)
    }

    /// Assert, in a debug build, that word `cut` is where a frame's window
    /// ends, given the frame below it: a depth names a place between two
    /// frames, never the middle of one.
    fn debug_assert_boundary(&self, cut: usize, below: usize) {
        debug_assert_eq!(
            cut,
            if below == 0 {
                0
            } else {
                below + link_num_regs(self.header(below, LINK).get()) as usize
            },
            "a depth that falls inside a frame"
        );
    }

    /// Cut the stack back to `bytes`, dropping every frame above.
    pub(crate) fn truncate(&mut self, bytes: usize) {
        let cut = bytes / WORD_BYTES;
        if cut >= self.words.len() {
            return;
        }
        let (base, runs) = self.frames_from(cut);
        self.debug_assert_boundary(cut, base);
        self.words.truncate(cut);
        // The runs that started above the cut are spent.
        self.runs -= runs;
        self.base = base;
        self.load_top();
    }

    /// A copy of the frames below `bytes`: the stack as a cut to there
    /// would leave it.
    pub(crate) fn below(&self, bytes: usize) -> FrameStack {
        let cut = (bytes / WORD_BYTES).min(self.words.len());
        self.spill_pc();
        let (base, runs) = self.frames_from(cut);
        self.debug_assert_boundary(cut, base);
        let codes = self.codes[..self.runs - runs].to_vec();
        let mut copy = FrameStack {
            words: copy_words(&self.words[..cut]),
            runs: codes.len(),
            codes,
            base,
            pc: 0,
            top_code: 0,
        };
        copy.load_top();
        copy
    }

    /// A copy of the frames at and above `bytes`, as a stack of its own: a
    /// delimited capture's. Its outermost frame becomes the copy's bottom
    /// one: its distance is written as a bottom frame's, so that the copy
    /// is walked like any stack, and it starts a run in the copy whether or
    /// not it shared one with the frame below it here. [`Self::append`]
    /// writes the distance to the caller it is given.
    pub(crate) fn above(&self, bytes: usize) -> FrameStack {
        let cut = (bytes / WORD_BYTES).min(self.words.len());
        let (below, mut runs) = self.frames_from(cut);
        self.debug_assert_boundary(cut, below);
        if cut == self.words.len() {
            return FrameStack::default();
        }
        let words = copy_words(&self.words[cut..]);
        let link = &words[HEADER_WORDS - LINK];
        // A run that started below the cut is the copy's first too.
        runs += 1 - link_starts_run(link.get());
        link.set(link_word(HEADER_WORDS, link_num_regs(link.get()), true));
        let codes = self.codes[self.runs - runs..self.runs].to_vec();
        FrameStack {
            words,
            runs,
            codes,
            base: self.base - cut,
            pc: self.pc,
            top_code: self.top_code,
        }
    }

    /// Put `captured`'s frames on top of this stack: a delimited
    /// continuation's re-entry. One word is written in the copy, the
    /// outermost frame's distance to the frame it now sits above. Does
    /// nothing for an empty capture.
    pub(crate) fn append(&mut self, captured: &FrameStack) {
        if captured.is_empty() {
            return;
        }
        self.spill_pc();
        let at = self.words.len();
        extend_words(&mut self.words, &captured.words);
        self.drop_spent();
        self.codes.extend_from_slice(captured.live_runs());
        self.runs = self.codes.len();
        // The outermost frame starts a run here as it did in the capture:
        // a stack's bottom frame always does.
        let outermost = at + HEADER_WORDS;
        let link = self.header(outermost, LINK);
        link.set(link_word(
            outermost - self.base,
            link_num_regs(link.get()),
            true,
        ));
        self.base = at + captured.base;
        self.pc = captured.pc;
        self.top_code = captured.top_code;
    }

    /// Set the register of its caller that the outermost of the frames
    /// [`Self::append`] put at byte offset `bytes` returns into.
    pub(crate) fn set_return_reg_at(&mut self, bytes: usize, return_reg: Reg) {
        let base = bytes / WORD_BYTES + HEADER_WORDS;
        if base == self.base {
            return self.set_top_return_reg(return_reg);
        }
        let meta = self.header(base, META);
        meta.set(meta_word(meta_pc(meta.get()), return_reg));
    }

    /// Drop every frame.
    pub(crate) fn clear(&mut self) {
        self.words.clear();
        self.codes.clear();
        self.runs = 0;
        self.base = 0;
        self.pc = 0;
        self.top_code = 0;
    }

    /// Assert that the frames tile the buffer: each window ends where the
    /// header above it starts, the top one ends the buffer, the walk down
    /// from the top meets the base after as many frames as the runs of code
    /// count, and each `code` word is the address of its frame's
    /// (#714, `PRD/GC_PRD.md` §11.1, invariant 1). O(depth).
    pub(crate) fn assert_frames_tile(&self) {
        let mut end = self.words.len();
        let mut runs = 0;
        for frame in self.frames() {
            assert_eq!(
                frame.register_base + frame.num_regs as usize,
                end,
                "the window of the frame whose registers start at word {} does not end \
                 where the frame above it starts: a word lies outside every frame (#714)",
                frame.register_base
            );
            assert_eq!(
                self.header(frame.register_base, CODE).get(),
                code_word(frame.code),
                "a frame's code word is not its code object's address"
            );
            end = frame.register_base - HEADER_WORDS;
            runs += link_starts_run(self.header(frame.register_base, LINK).get());
            if end == 0 {
                assert_eq!(
                    link_starts_run(self.header(frame.register_base, LINK).get()),
                    1,
                    "the bottom frame does not start a run of code"
                );
            }
        }
        assert_eq!(end, 0, "the frames do not reach the stack's base");
        assert_eq!(
            runs, self.runs,
            "the frames that start a run of code are not as many as the runs"
        );
        assert_eq!(
            self.top_code,
            self.live_runs().last().map_or(0, |code| code_address(code)),
            "the top frame's code is not the last run's"
        );
    }

    /// How many words the buffer has room for.
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.words.capacity()
    }
}

#[cfg(test)]
impl FrameStack {
    /// A stack of one frame of `code`, running `closure` at pc 0, whose
    /// window holds `registers`: for the tests that build a snapshot by
    /// hand.
    pub(crate) fn holding(
        code: &Rc<CodeObject>,
        closure: TaggedValue,
        registers: &[TaggedValue],
    ) -> FrameStack {
        let mut stack = FrameStack::default();
        let base = stack.push(code, closure, 0, registers.len() as u16, TaggedValue::NULL);
        for (slot, &value) in stack.words[base..].iter().zip(registers) {
            slot.set(value);
        }
        stack
    }
}

/// The frames of a [`FrameStack`], top first.
pub(crate) struct Frames<'a> {
    stack: &'a FrameStack,
    /// The first register of the frame to answer next, or 0 after the
    /// bottom one.
    base: usize,
    /// The run of code the next frame is in, as its place in `codes` plus
    /// one.
    run: usize,
}

impl<'a> Iterator for Frames<'a> {
    type Item = CallFrame<'a>;

    #[inline(always)]
    fn next(&mut self) -> Option<CallFrame<'a>> {
        let base = self.base;
        if base == 0 {
            return None;
        }
        let stack = self.stack;
        let top = base == stack.base;
        // One range check for the header's five words.
        let header = &stack.words[base - HEADER_WORDS..base];
        let link = header[HEADER_WORDS - LINK].get();
        let meta = header[HEADER_WORDS - META].get();
        // The frame's run, which the frame below shares unless this frame
        // starts it.
        let run = self.run - 1;
        self.run -= link_starts_run(link);
        // The bottom frame's distance is its own base, so this ends at 0.
        self.base = base - link_distance(link);
        Some(CallFrame {
            // The top frame's pc is the stack's own (the module's comment).
            pc: if top { stack.pc } else { meta_pc(meta) },
            register_base: base,
            num_regs: link_num_regs(link),
            closure: header[HEADER_WORDS - CLOSURE].get(),
            return_reg: meta_return_reg(meta),
            code: &stack.codes[run].0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::code_object::{Arity, CodeObjectId};

    fn code(id: u64, num_regs: u16) -> Rc<CodeObject> {
        Rc::new(CodeObject {
            id: CodeObjectId(id),
            name: None,
            instructions: vec![],
            constants: vec![],
            num_regs,
            arity: Arity::Fixed(0),
            source_map: vec![],
            register_roots: None,
            links: patina_core::LinkTable::empty(),
            live_closures: Cell::new(0),
        })
    }

    const FILL: TaggedValue = TaggedValue::NULL;

    /// A stack of three frames of 2, 0 and 3 registers, the middle one at
    /// pc 7 and the bottom one at pc 4.
    fn three() -> FrameStack {
        let mut stack = FrameStack::default();
        stack.push(&code(1, 2), TaggedValue::FALSE, 0, 2, FILL);
        stack.set_pc(4);
        stack.push(&code(2, 0), TaggedValue::TRUE, 1, 0, FILL);
        stack.set_pc(7);
        stack.push(&code(3, 3), TaggedValue::FALSE, 9, 3, FILL);
        stack
    }

    fn shape(stack: &FrameStack) -> Vec<(u64, usize, usize, u16, Reg)> {
        stack
            .frames()
            .map(|f| (f.code.id.0, f.pc, f.register_base, f.num_regs, f.return_reg))
            .collect()
    }

    #[test]
    fn a_header_word_keeps_its_fields() {
        for starts_run in [false, true] {
            let link = link_word(65_540, u16::MAX, starts_run);
            assert_eq!(
                (
                    link_distance(link),
                    link_num_regs(link),
                    link_starts_run(link)
                ),
                (65_540, u16::MAX, starts_run as usize)
            );
            assert!(link.is_fixnum());
        }
        let meta = meta_word(MAX_INSTRUCTIONS - 1, u16::MAX);
        assert_eq!(
            (meta_pc(meta), meta_return_reg(meta)),
            (MAX_INSTRUCTIONS - 1, u16::MAX)
        );
        assert!(meta.is_fixnum());
        // The pc is the low half of the word, shifted past the tag (A3).
        assert_eq!(meta_word(5, 0).raw(), 5 << 3);
        assert_eq!(meta_word(0, 1).raw(), 1 << 35);
        assert_eq!(link_word(1, 1, false).raw(), (8 << 3) | (1 << 35));
        // The run flag is the last of the flags' bits (§11.1: 51–62).
        assert_eq!(link_word(0, 0, true).raw(), 1 << 62);
        let c = code(1, 0);
        assert!(code_word(&c).is_fixnum());
        assert_eq!(code_word(&c).raw() as usize, Rc::as_ptr(&c) as usize);
    }

    #[test]
    fn frames_are_walked_from_the_top_with_the_top_frames_pc() {
        let mut stack = three();
        stack.set_pc(2);
        assert_eq!(
            shape(&stack),
            [(3, 2, 17, 3, 9), (2, 7, 12, 0, 1), (1, 4, 5, 2, 0)]
        );
        assert_eq!(stack.frame_count(), 3);
        assert_eq!(stack.register_count(), 5);
        assert_eq!(stack.bytes(), 20 * 8);
        assert_eq!(stack.top_frame_bytes(), 12 * 8);
        assert_eq!(stack.caller_base(), Some(12));
        stack.assert_frames_tile();
    }

    #[test]
    fn a_pop_answers_the_return_register_and_takes_up_the_callers_pc() {
        let mut stack = three();
        assert_eq!(stack.pop(), 9);
        assert_eq!((stack.base(), stack.pc(), stack.bytes()), (12, 7, 12 * 8));
        assert_eq!(stack.pop(), 1);
        assert_eq!((stack.base(), stack.pc(), stack.bytes()), (5, 4, 7 * 8));
        assert_eq!(stack.caller_base(), None);
        assert_eq!(stack.pop(), 0);
        assert_eq!((stack.base(), stack.pc(), stack.bytes()), (0, 0, 0));
        assert!(stack.is_empty() && stack.top().is_none());
        stack.assert_frames_tile();
    }

    /// A return inside a recursion: the caller runs the callee's code, so
    /// the pop takes up its pc and nothing else. Without it the caller
    /// resumes where the callee stopped, and a recursion never ends.
    #[test]
    fn a_pop_inside_a_run_takes_up_the_callers_pc() {
        let f = code(1, 1);
        let mut stack = FrameStack::default();
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL);
        stack.set_pc(6);
        stack.push(&f, TaggedValue::FALSE, 3, 1, FILL);
        stack.set_pc(4);
        let mut cur = Rc::clone(&f);
        assert_eq!((stack.pop(), stack.pc()), (3, 6));
        assert_eq!(stack.dispatch(&mut cur), (6, false));
        // The run is the caller's still: this test's reference, the
        // dispatch's and the run's.
        stack.drop_spent();
        assert_eq!(Rc::strong_count(&f), 3);
        stack.assert_frames_tile();
    }

    #[test]
    fn the_dispatch_prologue_follows_the_top_frames_code() {
        let mut stack = three();
        let mut cur = code(9, 0);
        let (pc, switched) = stack.dispatch(&mut cur);
        assert_eq!((pc, switched, cur.id.0), (0, true, 3));
        let (pc, switched) = stack.dispatch(&mut cur);
        assert_eq!((pc, switched), (1, false));
        stack.pop();
        let (pc, switched) = stack.dispatch(&mut cur);
        assert_eq!((pc, switched, cur.id.0), (7, true, 2));
    }

    #[test]
    fn a_cut_drops_the_frames_above_it() {
        let mut stack = three();
        stack.truncate(12 * 8);
        assert_eq!(shape(&stack), [(2, 7, 12, 0, 1), (1, 4, 5, 2, 0)]);
        stack.assert_frames_tile();
        // A cut at or past the top drops nothing.
        stack.truncate(12 * 8);
        stack.truncate(usize::MAX);
        assert_eq!(stack.frame_count(), 2);
        stack.truncate(0);
        assert!(stack.is_empty());
        assert_eq!((stack.base(), stack.pc()), (0, 0));
    }

    #[test]
    fn the_copies_below_and_above_a_depth_are_stacks_of_their_own() {
        let mut stack = three();
        stack.set_pc(2);
        let below = stack.below(7 * 8);
        assert_eq!(shape(&below), [(1, 4, 5, 2, 0)]);
        below.assert_frames_tile();
        let none = stack.below(0);
        assert!(none.is_empty() && none.words().is_empty());

        let above = stack.above(7 * 8);
        // The same frames, their registers counted from the copy's base.
        assert_eq!(shape(&above), [(3, 2, 10, 3, 9), (2, 7, 5, 0, 1)]);
        assert_eq!(above.bytes(), 13 * 8);
        let empty = stack.above(stack.bytes());
        assert!(empty.is_empty() && empty.base() == 0);
    }

    #[test]
    fn appended_frames_are_walked_through_to_the_frame_they_sit_above() {
        let stack = three();
        let captured = stack.above(7 * 8);
        // Onto a stack whose top frame is another size.
        let mut live = FrameStack::default();
        live.push(&code(7, 6), TaggedValue::FALSE, 0, 6, FILL);
        live.set_pc(3);
        let at = live.bytes();
        live.append(&captured);
        live.set_return_reg_at(at, 5);
        assert_eq!(
            shape(&live),
            [(3, 0, 21, 3, 9), (2, 7, 16, 0, 5), (7, 3, 5, 6, 0)]
        );
        live.assert_frames_tile();
        // And onto an empty one, where the outermost frame is the bottom.
        let mut bare = FrameStack::default();
        bare.append(&captured);
        assert_eq!(shape(&bare), [(3, 0, 10, 3, 9), (2, 7, 5, 0, 1)]);
        bare.assert_frames_tile();
        assert_eq!(bare.pop(), 9);
        assert_eq!(bare.pop(), 1);
        assert!(bare.is_empty());
    }

    fn ids(stack: &FrameStack) -> Vec<u64> {
        stack.frames().map(|f| f.code.id.0).collect()
    }

    /// A run of code holds its object once, however many frames run it: a
    /// recursion's frames join one run, a tail call out of the middle of a
    /// run takes its frame into a run of its own, and the copies below and
    /// above a depth split a run where the depth falls.
    #[test]
    fn frames_that_share_code_share_one_reference() {
        let (f, g) = (code(1, 1), code(2, 1));
        let counts = || (Rc::strong_count(&f), Rc::strong_count(&g));
        let mut stack = FrameStack::default();
        for _ in 0..3 {
            stack.push(&f, TaggedValue::FALSE, 0, 1, FILL);
        }
        stack.push(&g, TaggedValue::FALSE, 0, 1, FILL);
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL);
        // Three references: the two runs of `f`, and this test's.
        assert_eq!(counts(), (3, 2));
        assert_eq!(stack.frame_count(), 5);
        assert_eq!(ids(&stack), [1, 2, 1, 1, 1]);
        stack.assert_frames_tile();

        // The copies split the lower run of `f` after its second frame, and
        // the copy above holds that run's code for the frame it took.
        let (below, above) = (stack.below(12 * 8), stack.above(12 * 8));
        assert_eq!((below.frame_count(), above.frame_count()), (2, 3));
        assert_eq!(ids(&above), [1, 2, 1]);
        assert_eq!(counts(), (6, 3));
        below.assert_frames_tile();
        above.assert_frames_tile();
        drop((below, above));
        assert_eq!(counts(), (3, 2));

        // The two runs above go, and a frame inside the run below them.
        stack.pop();
        stack.pop();
        stack.pop();
        stack.drop_spent();
        assert_eq!(counts(), (2, 1));
        // A tail call out of a run of two takes one frame with it.
        stack.replace_top(&g, TaggedValue::TRUE, FILL);
        assert_eq!(ids(&stack), [2, 1]);
        assert_eq!(counts(), (2, 2));
        stack.assert_frames_tile();
        stack.pop();
        assert_eq!(ids(&stack), [1]);
        stack.assert_frames_tile();
        stack.truncate(0);
        stack.drop_spent();
        assert_eq!(counts(), (1, 1));

        // A clone into a stack that held other frames leaves none of them.
        stack.push(&g, TaggedValue::FALSE, 0, 4, FILL);
        let mut other = three();
        other.clone_from(&stack);
        assert_eq!(shape(&other), shape(&stack));
        other.assert_frames_tile();
    }

    /// A run that ends keeps its place and its code, and the next run
    /// started there takes both back if it is of the same code: a callee
    /// called again counts no reference. A run of other code takes the
    /// place, and the spent run's reference goes. Nothing a copy is made of
    /// is spent, and the store's question drops what is.
    #[test]
    fn a_run_that_ends_is_spent_and_taken_back_by_the_same_code() {
        let (f, g, h) = (code(1, 1), code(2, 1), code(3, 1));
        let counts = || {
            (
                Rc::strong_count(&f),
                Rc::strong_count(&g),
                Rc::strong_count(&h),
            )
        };
        let mut stack = FrameStack::default();
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL);
        stack.push(&g, TaggedValue::FALSE, 0, 1, FILL);
        assert_eq!(counts(), (2, 2, 1));
        // `g` returns: its run is spent, and holds it still.
        stack.pop();
        assert_eq!(ids(&stack), [1]);
        assert_eq!(counts(), (2, 2, 1));
        stack.assert_frames_tile();
        // A copy takes the frames' code and no other.
        let (copy, below) = (stack.clone(), stack.below(48));
        assert_eq!((below.frame_count(), counts()), (1, (4, 2, 1)));
        drop((copy, below));
        // `g` again: the same run, and no reference counted.
        stack.push(&g, TaggedValue::FALSE, 0, 1, FILL);
        assert_eq!(ids(&stack), [2, 1]);
        assert_eq!(counts(), (2, 2, 1));
        stack.assert_frames_tile();
        // `h` in its place: `g`'s reference goes for `h`'s.
        stack.pop();
        stack.push(&h, TaggedValue::FALSE, 0, 1, FILL);
        assert_eq!(ids(&stack), [3, 1]);
        assert_eq!(counts(), (2, 1, 2));
        stack.assert_frames_tile();
        // Spent again, and dropped where the store asks.
        stack.pop();
        assert_eq!(counts(), (2, 1, 2));
        stack.drop_spent();
        assert_eq!(counts(), (2, 1, 1));
        // An arrival's copy replaces the spent runs with the frames'.
        stack.push(&h, TaggedValue::FALSE, 0, 1, FILL);
        stack.pop();
        let mut arrival = FrameStack::default();
        arrival.push(&g, TaggedValue::FALSE, 0, 1, FILL);
        stack.clone_from(&arrival);
        assert_eq!(ids(&stack), [2]);
        assert_eq!(counts(), (1, 3, 1));
        stack.assert_frames_tile();
        // A cut leaves the runs above it spent, and a clear drops them.
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL);
        stack.truncate(48);
        assert_eq!((ids(&stack), counts()), (vec![2], (2, 3, 1)));
        stack.assert_frames_tile();
        stack.clear();
        assert_eq!(counts(), (1, 2, 1));
    }

    /// Two procedures that call each other in tail position change places
    /// between the top run and the spent one above it, and count no
    /// reference after the first round; a third takes the spent place.
    #[test]
    fn tail_calls_between_two_procedures_count_no_reference() {
        let (f, g, h, k) = (code(1, 1), code(2, 1), code(3, 1), code(4, 1));
        let counts = || {
            (
                Rc::strong_count(&g),
                Rc::strong_count(&h),
                Rc::strong_count(&k),
            )
        };
        let mut stack = FrameStack::default();
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL);
        stack.push(&g, TaggedValue::FALSE, 7, 1, FILL);
        // `g` to `h`: `h` takes a reference, and `g` is spent above it.
        stack.replace_top(&h, TaggedValue::TRUE, FILL);
        assert_eq!((ids(&stack), counts()), (vec![3, 1], (2, 2, 1)));
        stack.assert_frames_tile();
        // And back, and forth: the two change places.
        for _ in 0..3 {
            stack.replace_top(&g, TaggedValue::TRUE, FILL);
            assert_eq!((ids(&stack), counts()), (vec![2, 1], (2, 2, 1)));
            stack.assert_frames_tile();
            stack.replace_top(&h, TaggedValue::TRUE, FILL);
            assert_eq!((ids(&stack), counts()), (vec![3, 1], (2, 2, 1)));
        }
        // A third procedure: the spent `g` goes for it.
        stack.replace_top(&k, TaggedValue::TRUE, FILL);
        assert_eq!((ids(&stack), counts()), (vec![4, 1], (1, 2, 2)));
        stack.assert_frames_tile();
        // The frame returns to its caller's register all the same.
        assert_eq!(stack.pop(), 7);
        assert_eq!(ids(&stack), [1]);
        stack.assert_frames_tile();
        stack.drop_spent();
        assert_eq!(counts(), (1, 1, 1));
        // A tail call to the caller's own code is a run of its own beside
        // the caller's, and pops as one.
        stack.push(&g, TaggedValue::FALSE, 7, 1, FILL);
        stack.replace_top(&f, TaggedValue::TRUE, FILL);
        assert_eq!(ids(&stack), [1, 1]);
        assert_eq!(Rc::strong_count(&f), 3);
        stack.assert_frames_tile();
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL);
        assert_eq!(Rc::strong_count(&f), 3);
        stack.pop();
        stack.pop();
        assert_eq!(ids(&stack), [1]);
        stack.assert_frames_tile();
    }

    /// A delimited copy whose outermost frame shared a run with the frame
    /// below the cut starts that run again in the copy, and appended onto a
    /// frame of the same code it is a run of its own.
    #[test]
    fn a_copy_cut_inside_a_run_starts_the_run_again() {
        let (f, g) = (code(1, 1), code(2, 1));
        let mut stack = FrameStack::default();
        for _ in 0..3 {
            stack.push(&f, TaggedValue::FALSE, 0, 1, FILL);
        }
        stack.push(&g, TaggedValue::FALSE, 0, 1, FILL);
        // The top two frames: one of `f`, cut out of its run, and `g`.
        let captured = stack.above(12 * 8);
        assert_eq!(ids(&captured), [2, 1]);
        captured.assert_frames_tile();
        assert_eq!((Rc::strong_count(&f), Rc::strong_count(&g)), (3, 3));
        // Onto the run it was cut from.
        stack.truncate(12 * 8);
        assert_eq!(ids(&stack), [1, 1]);
        stack.assert_frames_tile();
        stack.append(&captured);
        assert_eq!(ids(&stack), [2, 1, 1, 1]);
        stack.assert_frames_tile();
        assert_eq!((Rc::strong_count(&f), Rc::strong_count(&g)), (4, 3));
        for left in [3, 2, 1, 0] {
            stack.pop();
            assert_eq!(stack.frame_count(), left);
            stack.assert_frames_tile();
        }
        stack.drop_spent();
        assert_eq!((Rc::strong_count(&f), Rc::strong_count(&g)), (2, 2));
    }

    /// An arrival keeps the live stack's buffer, so that the next push has
    /// room, unless the buffer is more than four times what it needs: then
    /// it takes one of its own size, and a stack a deep recursion grew goes
    /// back to the allocator at the escape out of it.
    #[test]
    fn an_arrival_keeps_a_buffer_near_its_size_and_lets_a_far_larger_one_go() {
        let f = code(1, 59);
        let frame_words = HEADER_WORDS + 59;
        let stack_of = |frames: usize| {
            let mut stack = FrameStack::default();
            for _ in 0..frames {
                stack.push(&f, TaggedValue::FALSE, 0, 59, FILL);
            }
            stack
        };
        // A snapshot of 100 frames, 6,400 words.
        let snapshot = stack_of(100);
        // A live stack of 200 frames, whose buffer has room for 16,384
        // words: under four times the snapshot's, so it stays.
        let mut near = stack_of(200);
        let kept = near.capacity();
        near.clone_from(&snapshot);
        assert_eq!(near.capacity(), kept);
        assert_eq!(shape(&near), shape(&snapshot));
        // One of 1,000 frames, 64,000 words: over four times it, so the
        // buffer goes.
        let mut deep = stack_of(1000);
        assert!(deep.capacity() >= 1000 * frame_words);
        deep.clone_from(&snapshot);
        assert_eq!(deep.capacity(), 100 * frame_words);
        assert_eq!(shape(&deep), shape(&snapshot));
        deep.assert_frames_tile();
        // A small buffer stays whatever the arrival needs.
        let mut small = stack_of(40);
        let kept = small.capacity();
        small.clone_from(&stack_of(1));
        assert_eq!(small.capacity(), kept);
    }

    #[test]
    fn a_tail_call_reuses_the_window_and_grows_it() {
        let mut stack = three();
        stack.set_pc(5);
        // Code of fewer registers: the window stays as it is.
        stack.replace_top(&code(4, 2), TaggedValue::TRUE, FILL);
        assert_eq!(stack.bytes(), 20 * 8, "a window never shrinks");
        assert_eq!(shape(&stack)[0], (4, 0, 17, 3, 9));
        // Code of more: it grows, and the new registers hold the fill.
        stack.replace_top(&code(5, 6), TaggedValue::TRUE, TaggedValue::fixnum(7));
        assert_eq!(shape(&stack)[0], (5, 0, 17, 6, 9));
        assert_eq!(
            stack.words()[20..]
                .iter()
                .map(Cell::get)
                .collect::<Vec<_>>(),
            [TaggedValue::fixnum(7); 3]
        );
        assert_eq!(stack.top_closure(), Some(TaggedValue::TRUE));
        assert_eq!(stack.closure_at(stack.base()), TaggedValue::TRUE);
        stack.assert_frames_tile();
        stack.set_pc(3);
        stack.restart_top(TaggedValue::FALSE);
        assert_eq!(
            (stack.pc(), stack.top_closure()),
            (0, Some(TaggedValue::FALSE))
        );
        // A stub's next round, in other code and then in its own.
        let next = code(6, 6);
        stack.recode_top(&next);
        stack.set_top_return_reg(2);
        assert_eq!(shape(&stack)[0], (6, 0, 17, 6, 2));
        stack.set_pc(4);
        stack.recode_top(&next);
        assert_eq!(shape(&stack)[0], (6, 0, 17, 6, 2));
        assert_eq!(Rc::strong_count(&next), 2);
        stack.assert_frames_tile();
    }
}
