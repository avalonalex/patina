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
//! collector, and is what a frame's view hands out. It holds a run for
//! each stretch of consecutive frames that run the same code ([`CodeRun`]):
//! the object once, and how many frames. A recursion a million frames deep
//! is one run, so a frame costs its 40 bytes and its registers and nothing
//! more, and a capture of it copies one reference, where a reference a
//! frame cost a count up at every capture and a count down at every drop
//! (measured 2026-11-24: 14% of `samedepth1000`'s instructions). 4e
//! replaces the address with a reference to a code descriptor, and the
//! vector goes.
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

/// A `link` word: the distance to the caller in words, and the window's
/// registers. The flags are zero until stage 7 has one to set.
#[inline(always)]
fn link_word(distance: usize, num_regs: u16) -> TaggedValue {
    TaggedValue::fixnum(((distance * WORD_BYTES) as u64 | (num_regs as u64) << HIGH_SHIFT) as i64)
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

/// A stretch of consecutive frames that run the same code: the code, once,
/// and how many frames (the module's comment). Its trace reports the
/// code's id ([`FrameCode`], #836).
#[derive(Debug, Clone, Trace)]
struct CodeRun {
    code: FrameCode,
    #[trace(skip, reason = "a count of frames")]
    frames: usize,
}

/// A copy of `words`, as one run of stores. `<[T]>::to_vec` clones a slice
/// of cells an element at a time under a guard, which costs a capture
/// several instructions a word.
fn copy_words(words: &[Cell<TaggedValue>]) -> Vec<Cell<TaggedValue>> {
    let mut copy = Vec::with_capacity(words.len());
    copy.extend(words.iter().map(|word| Cell::new(word.get())));
    copy
}

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
    /// The frames' code, bottom first, a run for each stretch of frames
    /// that share one: what keeps a frame's unit loaded until 4e makes the
    /// `code` word a reference. The last run's code is the top frame's.
    codes: Vec<CodeRun>,
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
/// each run of code. An arrival copies back into the buffers the live stack
/// already has (`clone_from`), so that it neither frees and takes a buffer
/// nor leaves one with no room for the next push.
impl Clone for FrameStack {
    fn clone(&self) -> Self {
        FrameStack {
            words: copy_words(&self.words),
            codes: self.codes.clone(),
            base: self.base,
            pc: self.pc,
            top_code: self.top_code,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.words.clear();
        self.words
            .extend(source.words.iter().map(|word| Cell::new(word.get())));
        self.codes.clone_from(&source.codes);
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

    /// How many frames the stack holds: the runs' counts, summed. What
    /// the census and the step tracer report; nothing on a call's path
    /// reads it.
    pub(crate) fn frame_count(&self) -> usize {
        self.codes.iter().map(|run| run.frames).sum()
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
        self.words.capacity() * WORD_BYTES + self.codes.capacity() * std::mem::size_of::<CodeRun>()
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
    #[inline(always)]
    pub(crate) fn top_closure(&self) -> Option<TaggedValue> {
        (self.base != 0).then(|| self.header(self.base, CLOSURE).get())
    }

    /// The top frame's code.
    #[inline(always)]
    pub(crate) fn top_code(&self) -> Option<&Rc<CodeObject>> {
        self.codes.last().map(|run| &run.code.0)
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
            run: self.codes.len(),
            left: 0,
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

    /// Take the top frame's pc and code from its header and the side
    /// vector: after a pop or a cut, or where frames were appended.
    #[inline(always)]
    fn load_top(&mut self) {
        match self.codes.last() {
            Some(run) => {
                self.pc = meta_pc(self.header(self.base, META).get());
                self.top_code = code_address(&run.code);
            }
            None => {
                self.pc = 0;
                self.top_code = 0;
            }
        }
    }

    /// Give the top frame's place in the side vector to `code`: the last
    /// run's own, if the frame was the whole of it, and a run of its own
    /// otherwise.
    #[inline(always)]
    fn set_top_code(&mut self, code: Rc<CodeObject>) {
        self.top_code = code_address(&code);
        let run = self.codes.last_mut().expect("the top frame's code");
        if run.frames == 1 {
            run.code = FrameCode(code);
        } else {
            run.frames -= 1;
            self.codes.push(CodeRun {
                code: FrameCode(code),
                frames: 1,
            });
        }
    }

    /// Take the top `frames` frames out of the side vector.
    fn drop_codes(&mut self, mut frames: usize) {
        while frames > 0 {
            let run = self.codes.last_mut().expect("a frame has its code");
            let taken = frames.min(run.frames);
            run.frames -= taken;
            frames -= taken;
            if run.frames == 0 {
                self.codes.pop();
            }
        }
    }

    /// The side vector of the top `frames` frames, as a copy.
    fn top_codes(&self, mut frames: usize) -> Vec<CodeRun> {
        let mut runs = Vec::new();
        for run in self.codes.iter().rev() {
            if frames == 0 {
                break;
            }
            let taken = frames.min(run.frames);
            runs.push(CodeRun {
                code: run.code.clone(),
                frames: taken,
            });
            frames -= taken;
        }
        runs.reverse();
        runs
    }

    /// Push a frame of `num_regs` registers, each holding `fill`, at pc 0.
    /// Answers the index of its first register.
    #[inline(always)]
    pub(crate) fn push(
        &mut self,
        code: Rc<CodeObject>,
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
        let header = [
            TaggedValue::fixnum(0),
            link_word(base - self.base, num_regs as u16),
            code_word(&code),
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
        // The frame joins the run below it if it runs the same code, as
        // every frame of a recursion does: the run already holds the code,
        // and the reference handed in goes.
        let address = code_address(&code);
        if address == self.top_code {
            self.codes.last_mut().expect("the top frame's code").frames += 1;
        } else {
            self.top_code = address;
            self.codes.push(CodeRun {
                code: FrameCode(code),
                frames: 1,
            });
        }
        self.base = base;
        self.pc = 0;
        base
    }

    /// Pop the top frame with its window, and answer its `return_reg`, and
    /// its code if it was the last frame of its run. The code is handed
    /// back and not dropped here, so that the pop calls nothing: the last
    /// frame of a code object frees it where the caller lets it go. Panics
    /// with no frame.
    #[inline(always)]
    pub(crate) fn pop(&mut self) -> (Reg, Option<FrameCode>) {
        let base = self.base;
        // One range check for the header's words; with no frame the range
        // starts below zero, and the slice refuses it.
        let header = &self.words[base.wrapping_sub(HEADER_WORDS)..base];
        let link = header[HEADER_WORDS - LINK].get();
        let return_reg = meta_return_reg(header[HEADER_WORDS - META].get());
        self.words.truncate(base - HEADER_WORDS);
        self.base = base - link_distance(link);
        let run = self.codes.last_mut().expect("a frame has its code");
        run.frames -= 1;
        if run.frames != 0 {
            // The caller runs the same code: only its pc is taken up.
            self.pc = meta_pc(self.header(self.base, META).get());
            return (return_reg, None);
        }
        let ended = self.codes.pop().map(|run| run.code);
        self.load_top();
        (return_reg, ended)
    }

    /// Grow the top window to `num_regs` registers if it has fewer, the new
    /// ones holding `fill`. A window never shrinks.
    #[inline(always)]
    pub(crate) fn grow_top(&mut self, num_regs: u16, fill: TaggedValue) {
        let base = self.base;
        let link = self.header(base, LINK).get();
        if num_regs > link_num_regs(link) {
            self.words.resize(base + num_regs as usize, Cell::new(fill));
            self.header(base, LINK)
                .set(link_word(link_distance(link), num_regs));
        }
    }

    /// Give the top frame to `code` at pc 0, running `closure`: a tail
    /// call's replacement, in the window as it stands.
    #[inline(always)]
    pub(crate) fn replace_top(&mut self, code: Rc<CodeObject>, closure: TaggedValue) {
        let base = self.base;
        self.header(base, CODE).set(code_word(&code));
        self.header(base, CLOSURE).set(closure);
        self.set_top_code(code);
        self.pc = 0;
    }

    /// Start the top frame again at pc 0 in its own code, running `closure`.
    #[inline(always)]
    pub(crate) fn restart_top(&mut self, closure: TaggedValue) {
        self.header(self.base, CLOSURE).set(closure);
        self.pc = 0;
    }

    /// Start the top frame at pc 0 in `code`, its closure as it stands: a
    /// stub's next round.
    pub(crate) fn recode_top(&mut self, code: Rc<CodeObject>) {
        self.header(self.base, CODE).set(code_word(&code));
        self.set_top_code(code);
        self.pc = 0;
    }

    /// Set the register of its caller that the top frame returns into.
    pub(crate) fn set_top_return_reg(&mut self, return_reg: Reg) {
        let meta = self.header(self.base, META);
        meta.set(meta_word(meta_pc(meta.get()), return_reg));
    }

    // ── Cutting, copying and appending ──────────────────────────────────

    /// The index of the first register of the topmost frame whose header
    /// starts below word `cut`, or 0 if none does, and how many frames lie
    /// at or above it. A walk down from the top: what a cut or a copy at
    /// `cut` is about to drop or take, frame by frame.
    fn frames_from(&self, cut: usize) -> (usize, usize) {
        let mut base = self.base;
        let mut above = 0;
        while base != 0 && base - HEADER_WORDS >= cut {
            base -= link_distance(self.header(base, LINK).get());
            above += 1;
        }
        (base, above)
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
        let (base, above) = self.frames_from(cut);
        self.debug_assert_boundary(cut, base);
        self.words.truncate(cut);
        self.drop_codes(above);
        self.base = base;
        self.load_top();
    }

    /// A copy of the frames below `bytes`: the stack as a cut to there
    /// would leave it.
    pub(crate) fn below(&self, bytes: usize) -> FrameStack {
        let cut = (bytes / WORD_BYTES).min(self.words.len());
        self.spill_pc();
        let (base, above) = self.frames_from(cut);
        self.debug_assert_boundary(cut, base);
        let mut copy = FrameStack {
            words: copy_words(&self.words[..cut]),
            codes: self.codes.clone(),
            base,
            pc: 0,
            top_code: 0,
        };
        copy.drop_codes(above);
        copy.load_top();
        copy
    }

    /// A copy of the frames at and above `bytes`, as a stack of its own: a
    /// delimited capture's. Its outermost frame becomes the copy's bottom
    /// one, and its distance is written as a bottom frame's, so that the
    /// copy is walked like any stack; [`Self::append`] writes the distance
    /// to the caller it is given.
    pub(crate) fn above(&self, bytes: usize) -> FrameStack {
        let cut = (bytes / WORD_BYTES).min(self.words.len());
        let (below, above) = self.frames_from(cut);
        self.debug_assert_boundary(cut, below);
        if above == 0 {
            return FrameStack::default();
        }
        let copy = FrameStack {
            words: copy_words(&self.words[cut..]),
            codes: self.top_codes(above),
            base: self.base - cut,
            pc: self.pc,
            top_code: self.top_code,
        };
        let link = copy.header(HEADER_WORDS, LINK);
        link.set(link_word(HEADER_WORDS, link_num_regs(link.get())));
        copy
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
        self.words.extend_from_slice(&captured.words);
        self.codes.extend(captured.codes.iter().cloned());
        let outermost = at + HEADER_WORDS;
        let link = self.header(outermost, LINK);
        link.set(link_word(outermost - self.base, link_num_regs(link.get())));
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
        let mut frames = 0;
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
            frames += 1;
        }
        assert_eq!(end, 0, "the frames do not reach the stack's base");
        assert_eq!(
            frames,
            self.frame_count(),
            "the runs of code do not count the frames"
        );
        assert!(
            self.codes.iter().all(|run| run.frames != 0),
            "an empty run of code"
        );
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
        let base = stack.push(
            Rc::clone(code),
            closure,
            0,
            registers.len() as u16,
            TaggedValue::NULL,
        );
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
    /// one, and how many of that run's frames are still to answer.
    run: usize,
    left: usize,
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
        while self.left == 0 {
            self.run -= 1;
            self.left = stack.codes[self.run].frames;
        }
        self.left -= 1;
        // One range check for the header's five words.
        let header = &stack.words[base - HEADER_WORDS..base];
        let link = header[HEADER_WORDS - LINK].get();
        let meta = header[HEADER_WORDS - META].get();
        // The bottom frame's distance is its own base, so this ends at 0.
        self.base = base - link_distance(link);
        Some(CallFrame {
            // The top frame's pc is the stack's own (the module's comment).
            pc: if top { stack.pc } else { meta_pc(meta) },
            register_base: base,
            num_regs: link_num_regs(link),
            closure: header[HEADER_WORDS - CLOSURE].get(),
            return_reg: meta_return_reg(meta),
            code: &stack.codes[self.run].code.0,
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
        stack.push(code(1, 2), TaggedValue::FALSE, 0, 2, FILL);
        stack.set_pc(4);
        stack.push(code(2, 0), TaggedValue::TRUE, 1, 0, FILL);
        stack.set_pc(7);
        stack.push(code(3, 3), TaggedValue::FALSE, 9, 3, FILL);
        stack
    }

    fn shape(stack: &FrameStack) -> Vec<(u64, usize, usize, u16, Reg)> {
        stack
            .frames()
            .map(|f| (f.code.id.0, f.pc, f.register_base, f.num_regs, f.return_reg))
            .collect()
    }

    #[test]
    fn a_header_word_keeps_both_its_fields() {
        let link = link_word(65_540, u16::MAX);
        assert_eq!(
            (link_distance(link), link_num_regs(link)),
            (65_540, u16::MAX)
        );
        assert!(link.is_fixnum());
        let meta = meta_word(MAX_INSTRUCTIONS - 1, u16::MAX);
        assert_eq!(
            (meta_pc(meta), meta_return_reg(meta)),
            (MAX_INSTRUCTIONS - 1, u16::MAX)
        );
        assert!(meta.is_fixnum());
        // The pc is the low half of the word, shifted past the tag (A3).
        assert_eq!(meta_word(5, 0).raw(), 5 << 3);
        assert_eq!(meta_word(0, 1).raw(), 1 << 35);
        assert_eq!(link_word(1, 1).raw(), (8 << 3) | (1 << 35));
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
        assert_eq!(stack.pop().0, 9);
        assert_eq!((stack.base(), stack.pc(), stack.bytes()), (12, 7, 12 * 8));
        assert_eq!(stack.pop().0, 1);
        assert_eq!((stack.base(), stack.pc(), stack.bytes()), (5, 4, 7 * 8));
        assert_eq!(stack.caller_base(), None);
        assert_eq!(stack.pop().0, 0);
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
        stack.push(Rc::clone(&f), TaggedValue::FALSE, 0, 1, FILL);
        stack.set_pc(6);
        stack.push(Rc::clone(&f), TaggedValue::FALSE, 3, 1, FILL);
        stack.set_pc(4);
        let mut cur = Rc::clone(&f);
        let (return_reg, ended) = stack.pop();
        assert_eq!((return_reg, ended.is_none(), stack.pc()), (3, true, 6));
        assert_eq!(stack.dispatch(&mut cur), (6, false));
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
        live.push(code(7, 6), TaggedValue::FALSE, 0, 6, FILL);
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
        assert_eq!(bare.pop().0, 9);
        assert_eq!(bare.pop().0, 1);
        assert!(bare.is_empty());
    }

    /// A run of code holds its object once, however many frames run it: a
    /// recursion's frames join one run, a tail call out of the middle of a
    /// run takes its frame into a run of its own, and the copies below and
    /// above a depth split a run where the depth falls.
    #[test]
    fn frames_that_share_code_share_one_reference() {
        let (f, g) = (code(1, 1), code(2, 1));
        let mut stack = FrameStack::default();
        for _ in 0..3 {
            stack.push(Rc::clone(&f), TaggedValue::FALSE, 0, 1, FILL);
        }
        stack.push(Rc::clone(&g), TaggedValue::FALSE, 0, 1, FILL);
        stack.push(Rc::clone(&f), TaggedValue::FALSE, 0, 1, FILL);
        // Three references: the two runs of `f`, and this test's.
        assert_eq!((Rc::strong_count(&f), Rc::strong_count(&g)), (3, 2));
        assert_eq!(stack.frame_count(), 5);
        assert_eq!(
            stack.frames().map(|f| f.code.id.0).collect::<Vec<_>>(),
            [1, 2, 1, 1, 1]
        );
        stack.assert_frames_tile();

        // The copies split the lower run of `f` after its second frame.
        let (below, above) = (stack.below(12 * 8), stack.above(12 * 8));
        assert_eq!((below.frame_count(), above.frame_count()), (2, 3));
        assert_eq!(
            above.frames().map(|f| f.code.id.0).collect::<Vec<_>>(),
            [1, 2, 1]
        );
        below.assert_frames_tile();
        above.assert_frames_tile();
        drop((below, above));
        assert_eq!((Rc::strong_count(&f), Rc::strong_count(&g)), (3, 2));

        // A pop inside a run hands no code back; the run's last frame does.
        assert!(
            stack.pop().1.is_some(),
            "the upper run of `f` was one frame"
        );
        assert!(stack.pop().1.is_some(), "and so was `g`'s");
        assert!(stack.pop().1.is_none());
        // A tail call out of a run of two takes one frame with it.
        stack.replace_top(Rc::clone(&g), TaggedValue::TRUE);
        assert_eq!(
            stack.frames().map(|f| f.code.id.0).collect::<Vec<_>>(),
            [2, 1]
        );
        stack.assert_frames_tile();
        stack.truncate(0);
        assert_eq!((Rc::strong_count(&f), Rc::strong_count(&g)), (1, 1));

        // A clone into a stack that held other frames leaves none of them.
        stack.push(Rc::clone(&g), TaggedValue::FALSE, 0, 4, FILL);
        let mut other = three();
        other.clone_from(&stack);
        assert_eq!(shape(&other), shape(&stack));
        other.assert_frames_tile();
    }

    #[test]
    fn a_tail_call_reuses_the_window_and_grows_it() {
        let mut stack = three();
        stack.set_pc(5);
        stack.grow_top(2, FILL);
        assert_eq!(stack.bytes(), 20 * 8, "a window never shrinks");
        stack.grow_top(6, FILL);
        stack.replace_top(code(4, 6), TaggedValue::TRUE);
        assert_eq!(shape(&stack)[0], (4, 0, 17, 6, 9));
        assert_eq!(stack.top_closure(), Some(TaggedValue::TRUE));
        stack.assert_frames_tile();
        stack.set_pc(3);
        stack.restart_top(TaggedValue::FALSE);
        assert_eq!(
            (stack.pc(), stack.top_closure()),
            (0, Some(TaggedValue::FALSE))
        );
        stack.recode_top(code(5, 6));
        stack.set_top_return_reg(2);
        assert_eq!(shape(&stack)[0], (5, 0, 17, 6, 2));
    }
}
