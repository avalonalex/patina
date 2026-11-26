//! The VM's register stack (#908, GC stage 4d.7b and 4d.7c): frames and
//! their registers interleaved in one buffer of tagged words, which for the
//! machine's own stack is a reservation of address space that never moves.
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
//! copy has one, and a store has one live stack, so one drop reaches every
//! spent entry there is; stage 9's green threads each have a stack, and
//! the store's question drops each one's then. (Measured 2026-11-24
//! against a vector pushed and popped with every run, each run counting
//! its frames: the bookkeeping cost a call and its return about 20
//! instructions within one procedure and 30 between two.)
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
//!
//! **Two buffers.** [`FrameStack`] is written once, over what holds its
//! words ([`Buffer`]). A captured copy holds them in a vector of its own
//! size, and is what the plain name means. The machine's stack
//! ([`LiveStack`]) holds them in a reservation the size of its cap
//! ([`Reserved`], 7c): a frame's address holds from its push to its pop,
//! the stack's top is an address the frame-entry check compares with the
//! poll word, and every write that would extend the stack is refused at
//! the reservation's end ([`NoRoom`]) before a word is written. A capture
//! copies out of the reservation, and an arrival or a delimited re-entry
//! copies back into it, at its base or at its top.

use super::code_object::CodeObject;
use super::{CallFrame, FrameCode, Reg};
use patina_core::heap::reservation::Reservation;
use patina_core::tagged_value::TaggedValue;
use patina_core::{Carrier, Trace};
use std::cell::Cell;
use std::ops::Deref;
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

/// A write the stack's buffer has no room for: the live stack is at its
/// reservation's end. Nothing was written. The machine answers it as it
/// does a stack at its cap (`VmState::stack_exhausted`, #907).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NoRoom;

/// What holds a stack's words: the words themselves, as a slice, and the
/// few ways [`FrameStack`] changes how many there are. A captured copy's
/// is a vector of its own size. The live stack's is a reservation of
/// address space ([`Reserved`]), which never moves and does not grow.
pub trait Buffer: Deref<Target = [Cell<TaggedValue>]> {
    /// Whether `more` words fit past the ones held. A buffer that grows
    /// makes the room and answers `true`. A reservation answers `false` at
    /// its end, and then nothing may be written.
    fn make_room(&mut self, more: usize) -> bool;

    /// The buffer's first word. Good for writing the words
    /// [`Self::make_room`] last allowed, which start at its length.
    fn as_mut_ptr(&mut self) -> *mut Cell<TaggedValue>;

    /// Take the words below `len` as the buffer's.
    ///
    /// # Safety
    ///
    /// Each of them is written, and `len` is within the room
    /// [`Self::make_room`] last allowed.
    unsafe fn set_len(&mut self, len: usize);

    /// Drop the words from `len` up, if there are any.
    fn truncate(&mut self, len: usize);

    /// How many words the buffer holds without growing: all a reservation
    /// ever holds.
    fn capacity(&self) -> usize;
}

impl Buffer for Vec<Cell<TaggedValue>> {
    #[inline(always)]
    fn make_room(&mut self, more: usize) -> bool {
        self.reserve(more);
        true
    }

    #[inline(always)]
    fn as_mut_ptr(&mut self) -> *mut Cell<TaggedValue> {
        Vec::as_mut_ptr(self)
    }

    #[inline(always)]
    unsafe fn set_len(&mut self, len: usize) {
        // SAFETY: the caller's.
        unsafe { Vec::set_len(self, len) }
    }

    #[inline(always)]
    fn truncate(&mut self, len: usize) {
        Vec::truncate(self, len);
    }

    #[inline(always)]
    fn capacity(&self) -> usize {
        Vec::capacity(self)
    }
}

/// The live stack's buffer: a reservation of address space the size of the
/// stack's cap (#908's 7c, `PRD/GC_PRD.md` §11.1), with the words written
/// so far at its start.
///
/// **It never moves.** A frame's address holds from its push to its pop,
/// which is what JIT code, and a frame pointer in a register, need of it.
/// The reservation takes physical pages as they are first written, so a
/// stack costs what it has reached and not its cap.
///
/// **It does not grow.** Every write past the words held asks
/// [`Buffer::make_room`] first, and is refused at the reservation's end:
/// there is no guard page, and no write lands outside the mapping. The
/// reservation is the cap rounded up to a page, so the machine's own
/// checks against the cap (`VmState::check_stack_bytes`, and the poll at a
/// frame's entry) refuse first wherever they are asked, at the byte the cap
/// names. This check is what holds where they are not asked, or are wrong.
///
/// **It gives pages back** (#909, GC stage 4d.8). The stack may write up
/// to its *granted* end without asking, and a write past that takes a
/// path out of line, which grants more in steps of [`GRANT_WORDS`] up to
/// the reservation's end. So the granted end is a bound on how deep the
/// stack has been since it was last reset, kept at no cost to a push: the
/// compare a push makes is against it where it was against the
/// reservation's end. A collection is an observation
/// ([`Reserved::observe`]): the pages above what was granted since the
/// observation before go back to the system, and the grant is reset to
/// the stack's top. A recursion's pages therefore go at the second
/// collection after it returns, and a program that recurses as deep
/// between every two collections keeps them. The first [`KEPT_WORDS`]
/// never go.
pub(crate) struct Reserved {
    reservation: Reservation,
    /// How many words are written: every word below it is initialized.
    len: usize,
    /// The granted end, in words: what [`Buffer::make_room`] allows
    /// without asking further. At least `len`, at most the capacity, and
    /// a multiple of [`GRANT_WORDS`] or the capacity itself.
    granted: usize,
    /// The committed end, in words: the most `granted` has been since
    /// pages were last given back, and where they were given back from.
    /// No word at or above it has been written since. `(gc-stats)`'s
    /// `committed-stack-bytes` is this in bytes.
    committed: usize,
    /// Where the committed end is published for `(gc-stats)`: the heap's
    /// carrier, or nothing for a stack no machine owns, as a test's.
    account: Option<Rc<Carrier>>,
}

/// The step the granted end moves in: 256 KiB of words. Small against a
/// recursion deep enough to matter, so the committed bytes read close to
/// what was written, and large against a frame, so the path out of line
/// is taken once in thousands of calls into new depth: a 10 M-frame
/// recursion, 1.36 GB, takes it some 5,200 times.
const GRANT_WORDS: usize = (256 << 10) / WORD_BYTES;

/// What an observation never gives back: the first MiB of words. A stack
/// that moves within it costs no call to the system at any collection,
/// and no page written again.
const KEPT_WORDS: usize = (1 << 20) / WORD_BYTES;

impl Reserved {
    /// A reservation for a stack of at most `cap` bytes, or of as much of
    /// that as the system gives, down to `floor` bytes
    /// (`Reservation::new`). [`Self::capacity`](Buffer::capacity) says
    /// what there is.
    ///
    /// # Panics
    ///
    /// Where the system refuses even the floor: there is then no address
    /// space for a stack at all.
    fn new(cap: usize, floor: usize) -> Self {
        let reservation = Reservation::new(cap, floor).unwrap_or_else(|error| {
            panic!("patina: no address space for a register stack of {floor} bytes: {error}")
        });
        Reserved {
            reservation,
            len: 0,
            granted: 0,
            committed: 0,
            account: None,
        }
    }

    /// Grant the stack room for `more` words past the ones held, where the
    /// reservation has it: the granted end moves up to the next step past
    /// them, or to the reservation's end. Answers `false`, with nothing
    /// changed, where the reservation has no such room.
    #[cold]
    #[inline(never)]
    fn grant(&mut self, more: usize) -> bool {
        let capacity = self.capacity();
        // `len` is at most the capacity, so the difference does not wrap.
        if capacity - self.len < more {
            return false;
        }
        // At most the capacity, which is a count of words in an address
        // space, so the rounding does not overflow.
        self.granted = (self.len + more)
            .next_multiple_of(GRANT_WORDS)
            .min(capacity);
        if self.granted > self.committed {
            self.committed = self.granted;
            self.publish();
        }
        true
    }

    /// An observation (#909): give back the pages above what the stack
    /// was granted since the observation before, and above the first
    /// [`KEPT_WORDS`], and start the next period at the stack's top.
    ///
    /// What was granted bounds how deep the stack has been, so the pages
    /// given back are ones it did not reach in a whole period between two
    /// observations: the hysteresis that keeps a program which recurses
    /// as deep in every period from paying for its pages each time. They
    /// are above the top, so they hold nothing: a pushed frame writes its
    /// every word.
    fn observe(&mut self) {
        let capacity = self.capacity();
        let keep = self.granted.max(KEPT_WORDS).min(capacity);
        if self.committed > keep {
            // The reservation is whole pages and `keep` a multiple of the
            // step or the capacity, so the range is whole pages wherever a
            // page divides 256 KiB. An error is the system's refusal, with
            // each page as it was or zero: the end stays where it was, and
            // the next observation asks again.
            let range = keep * WORD_BYTES..self.committed * WORD_BYTES;
            if self.reservation.decommit(range).is_ok() {
                self.committed = keep;
                self.publish();
            }
        }
        self.granted = self.len.next_multiple_of(GRANT_WORDS).min(capacity);
    }

    /// Publish the committed end where `(gc-stats)` reads it.
    fn publish(&self) {
        if let Some(account) = &self.account {
            account.set_stack_committed(self.committed * WORD_BYTES);
        }
    }

    /// The first word's address, which is the reservation's and never
    /// changes.
    #[inline(always)]
    fn first(&self) -> *mut Cell<TaggedValue> {
        // A reservation is page-aligned, so the cast keeps a word's
        // alignment.
        self.reservation.base().as_ptr().cast()
    }

    /// The address past the last word written: the stack's top, which the
    /// frame-entry check compares with the poll word (§12).
    #[inline(always)]
    fn top_address(&self) -> usize {
        self.first() as usize + self.len * WORD_BYTES
    }
}

impl Deref for Reserved {
    type Target = [Cell<TaggedValue>];

    #[inline(always)]
    fn deref(&self) -> &[Cell<TaggedValue>] {
        // SAFETY: the reservation is this buffer's own, readable and
        // writable for its life and aligned to a page. The first `len`
        // words were written through `as_mut_ptr` before `set_len` took
        // them in, and `len` words are inside it (`make_room`). A word is
        // a `Cell`, so a shared slice allows the writes the stack makes
        // through it.
        unsafe { std::slice::from_raw_parts(self.first(), self.len) }
    }
}

impl Buffer for Reserved {
    #[inline(always)]
    fn make_room(&mut self, more: usize) -> bool {
        // Against the granted end, which is at least `len`, so the
        // difference does not wrap: the one compare a push makes, and the
        // grant past it is out of line (`Reserved::grant`).
        self.granted - self.len >= more || self.grant(more)
    }

    #[inline(always)]
    fn as_mut_ptr(&mut self) -> *mut Cell<TaggedValue> {
        self.first()
    }

    #[inline(always)]
    unsafe fn set_len(&mut self, len: usize) {
        debug_assert!(len <= self.granted, "a length past what was granted");
        self.len = len;
    }

    #[inline(always)]
    fn truncate(&mut self, len: usize) {
        // A word has no destructor: dropping the words is forgetting them.
        self.len = self.len.min(len);
    }

    #[inline(always)]
    fn capacity(&self) -> usize {
        self.reservation.size() / WORD_BYTES
    }
}

impl std::fmt::Debug for Reserved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reserved")
            .field("len", &self.len)
            .field("granted", &self.granted)
            .field("committed", &self.committed)
            .field("capacity", &self.capacity())
            .finish()
    }
}

/// Put a copy of `words` on the end of `buffer`, as one run of stores, or
/// refuse where they do not fit. `<[T]>::to_vec` and `extend_from_slice`
/// clone a slice of cells an element at a time under a guard, which costs a
/// capture several instructions a word.
#[inline(always)]
fn extend_words<B: Buffer>(buffer: &mut B, words: &[Cell<TaggedValue>]) -> Result<(), NoRoom> {
    if !buffer.make_room(words.len()) {
        return Err(NoRoom);
    }
    let len = buffer.len();
    // SAFETY: `make_room` allowed `words.len()` words past `len`, and each
    // is written before `set_len` takes it in. `words` is not the buffer's
    // spare room, which no slice reaches.
    unsafe {
        let at = buffer.as_mut_ptr().add(len);
        for (i, word) in words.iter().enumerate() {
            at.add(i).write(Cell::new(word.get()));
        }
        buffer.set_len(len + words.len());
    }
    Ok(())
}

/// A copy of `words`, in a buffer of its size.
fn copy_words(words: &[Cell<TaggedValue>]) -> Vec<Cell<TaggedValue>> {
    let mut copy = Vec::with_capacity(words.len());
    copy.extend(words.iter().map(|word| Cell::new(word.get())));
    copy
}

/// A stack of frames, live or captured: see the module's comment. A
/// captured copy's words are in a vector of its own, which is what the
/// plain name means. The live stack's are in a reservation
/// ([`LiveStack`]).
///
/// Its trace (derived, #686) is a captured copy's: every word a slot, which
/// reports the closures and the registers and passes over the fixnums, and
/// each run's code by id. The live stack's is not this: a collection walks
/// its frames under their maps (`gc_roots::trace_frames`, #906), and
/// `ExecutionState` skips the field.
#[derive(Debug, Default, Trace)]
pub struct FrameStack<B = Vec<Cell<TaggedValue>>> {
    /// The headers and the registers, bottom first.
    words: B,
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

/// The machine's own stack: its words in a reservation the size of its
/// cap, which never moves (#908's 7c).
pub(crate) type LiveStack = FrameStack<Reserved>;

impl LiveStack {
    /// An empty stack of at most `cap` bytes, in a reservation of that
    /// size, or of as much of it as the system gives, down to `floor`
    /// bytes. [`Self::capacity_bytes`] says what there is.
    pub(crate) fn reserved(cap: usize, floor: usize) -> Self {
        FrameStack {
            words: Reserved::new(cap, floor),
            codes: Vec::new(),
            runs: 0,
            base: 0,
            pc: 0,
            top_code: 0,
        }
    }

    /// [`Self::reserved`], with the committed bytes published on
    /// `account` for `(gc-stats)`'s `committed-stack-bytes` (#909): the
    /// machine's own stack, which hands over its heap's carrier.
    pub(crate) fn reserved_on(cap: usize, floor: usize, account: Rc<Carrier>) -> Self {
        let mut stack = Self::reserved(cap, floor);
        stack.words.account = Some(account);
        stack.words.publish();
        stack
    }

    /// An observation, which a collection is (#909): the stack's pages
    /// above what it has reached since the observation before go back to
    /// the system, the first MiB excepted ([`Reserved::observe`]).
    pub(crate) fn observe(&mut self) {
        self.words.observe();
    }

    /// The bytes of the reservation that are committed: what the stack
    /// has been granted room to write and has not given back.
    #[cfg(test)]
    pub(crate) fn committed_bytes(&self) -> usize {
        self.words.committed * WORD_BYTES
    }

    /// The bytes the reservation holds: the most the stack can.
    pub(crate) fn capacity_bytes(&self) -> usize {
        self.words.capacity() * WORD_BYTES
    }

    /// The address of the stack's first word, which never changes: what a
    /// limit in bytes is counted from to make the poll word's address.
    #[inline(always)]
    pub(crate) fn base_address(&self) -> usize {
        self.words.first() as usize
    }

    /// The address of the stack's top: what the frame-entry check compares
    /// with the poll word (`PRD/GC_PRD.md` §12).
    #[inline(always)]
    pub(crate) fn top_address(&self) -> usize {
        self.words.top_address()
    }
}

/// A copy of a captured copy.
impl Clone for FrameStack {
    fn clone(&self) -> Self {
        self.copy()
    }
}

impl<B: Buffer> FrameStack<B> {
    // ── Copying whole ───────────────────────────────────────────────────

    /// A capture's copy: the words as one run of stores, in a buffer of
    /// their size, and a reference for each run of code.
    pub(crate) fn copy(&self) -> FrameStack {
        FrameStack {
            words: copy_words(&self.words),
            codes: self.live_runs().to_vec(),
            runs: self.runs,
            base: self.base,
            pc: self.pc,
            top_code: self.top_code,
        }
    }

    /// Make this stack a copy of `source`, in the buffer it has: a
    /// continuation's arrival. Refused, with the stack as it was, where
    /// the buffer cannot hold `source`'s words.
    pub(crate) fn copy_from(&mut self, source: &FrameStack) -> Result<(), NoRoom> {
        // Room for what the source holds beyond what is here, asked before
        // anything goes, so that a refusal leaves the stack whole.
        let more = source.words.len().saturating_sub(self.words.len());
        if !self.words.make_room(more) {
            return Err(NoRoom);
        }
        self.words.truncate(0);
        extend_words(&mut self.words, &source.words)?;
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
        Ok(())
    }

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
            words: &self.words,
            codes: &self.codes,
            top: self.base,
            pc: self.pc,
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
    ) -> Result<usize, NoRoom> {
        // The header's five words, then the window, written straight into
        // the buffer's spare room: the buffer is asked for room once, and
        // no word is written twice. `Vec`'s own `extend` and `resize` each
        // ask again, and cost a call a dozen instructions more (measured
        // 2026-11-24, on `fib`). The live stack's buffer refuses at its
        // reservation's end, before anything is written.
        let num_regs = num_regs as usize;
        let len = self.words.len();
        let total = HEADER_WORDS + num_regs;
        if !self.words.make_room(total) {
            return Err(NoRoom);
        }
        self.spill_pc();
        let base = len + HEADER_WORDS;
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
        // SAFETY: `make_room` allowed `total` words past `len`, so every
        // write below is inside the buffer and past the initialized
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
        Ok(base)
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
    /// is [`Self::restart_top`]. Refused, with the frame as it was, where
    /// the buffer has no room for a window that must grow.
    #[inline(always)]
    pub(crate) fn replace_top(
        &mut self,
        code: &Rc<CodeObject>,
        closure: TaggedValue,
        fill: TaggedValue,
    ) -> Result<(), NoRoom> {
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
        if code.num_regs > had {
            // Rare: a window never shrinks, so a frame that tail calls
            // among the same few procedures grows at most once for each.
            return self.replace_top_grown(code, closure, fill);
        }
        // The frame starts a run from here, whichever it was in.
        header[HEADER_WORDS - LINK].set(link_word(link_distance(link), had, true));
        header[HEADER_WORDS - CODE].set(code_word(code));
        header[HEADER_WORDS - CLOSURE].set(closure);
        self.set_top_code(code, address, starts_run);
        self.pc = 0;
        Ok(())
    }

    /// [`Self::replace_top`] for code with more registers than the window
    /// has: the window grows first, so that a refusal leaves the frame as
    /// it was, and the header is found again, since a buffer that grows
    /// may have moved it.
    #[cold]
    #[inline(never)]
    fn replace_top_grown(
        &mut self,
        code: &Rc<CodeObject>,
        closure: TaggedValue,
        fill: TaggedValue,
    ) -> Result<(), NoRoom> {
        let base = self.base;
        if !self.grow_top_window(base + code.num_regs as usize, fill) {
            return Err(NoRoom);
        }
        let header = &self.words[base - HEADER_WORDS..base];
        let link = header[HEADER_WORDS - LINK].get();
        header[HEADER_WORDS - LINK].set(link_word(link_distance(link), code.num_regs, true));
        header[HEADER_WORDS - CODE].set(code_word(code));
        header[HEADER_WORDS - CLOSURE].set(closure);
        self.set_top_code(code, code_address(code), link_starts_run(link));
        self.pc = 0;
        Ok(())
    }

    /// Grow the top frame's window, which ends the buffer, to end at word
    /// `end`, the new registers holding `fill`. Answers `false`, with
    /// nothing written, where the buffer has no room.
    fn grow_top_window(&mut self, end: usize, fill: TaggedValue) -> bool {
        let len = self.words.len();
        if !self.words.make_room(end - len) {
            return false;
        }
        // SAFETY: `make_room` allowed the words from `len` to `end`, and
        // each is written before `set_len` takes it in.
        unsafe {
            let at = self.words.as_mut_ptr();
            for i in len..end {
                at.add(i).write(Cell::new(fill));
            }
            self.words.set_len(end);
        }
        true
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
    /// nothing for an empty capture. Refused, with the stack as it was,
    /// where the buffer has no room for the capture's words.
    pub(crate) fn append(&mut self, captured: &FrameStack) -> Result<(), NoRoom> {
        if captured.is_empty() {
            return Ok(());
        }
        let at = self.words.len();
        extend_words(&mut self.words, &captured.words)?;
        // The frame the capture lands above keeps its pc in its header
        // from here: written after the copy, which a refusal leaves
        // undone, and before `base` moves.
        self.spill_pc();
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
        Ok(())
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
        self.words.truncate(0);
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
        let mut stack = <FrameStack>::default();
        let base = stack
            .push(code, closure, 0, registers.len() as u16, TaggedValue::NULL)
            .expect("a vector grows");
        for (slot, &value) in stack.words[base..].iter().zip(registers) {
            slot.set(value);
        }
        stack
    }
}

/// The frames of a [`FrameStack`], top first.
pub(crate) struct Frames<'a> {
    /// The stack's words, and the code of its runs with any spent entries
    /// after them.
    words: &'a [Cell<TaggedValue>],
    codes: &'a [FrameCode],
    /// The top frame's first register, and its pc, which the stack keeps
    /// beside its words.
    top: usize,
    pc: usize,
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
        let top = base == self.top;
        // One range check for the header's five words.
        let header = &self.words[base - HEADER_WORDS..base];
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
            pc: if top { self.pc } else { meta_pc(meta) },
            register_base: base,
            num_regs: link_num_regs(link),
            closure: header[HEADER_WORDS - CLOSURE].get(),
            return_reg: meta_return_reg(meta),
            code: &self.codes[run].0,
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
        let mut stack = <FrameStack>::default();
        stack
            .push(&code(1, 2), TaggedValue::FALSE, 0, 2, FILL)
            .unwrap();
        stack.set_pc(4);
        stack
            .push(&code(2, 0), TaggedValue::TRUE, 1, 0, FILL)
            .unwrap();
        stack.set_pc(7);
        stack
            .push(&code(3, 3), TaggedValue::FALSE, 9, 3, FILL)
            .unwrap();
        stack
    }

    fn shape<B: Buffer>(stack: &FrameStack<B>) -> Vec<(u64, usize, usize, u16, Reg)> {
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

    /// The census's hooks are handed the stack's shape, which is a walk of
    /// the frames, and a build without the census must not make it: a
    /// collection or a capture of a deep stack would pay for a number
    /// nothing reads.
    #[test]
    fn the_census_shape_is_walked_only_in_a_census_build() {
        let stack = three();
        let walked = (stack.frame_count(), stack.register_count());
        assert_eq!(walked, (3, 5));
        let expected = if patina_core::census::GC_CENSUS {
            walked
        } else {
            (0, 0)
        };
        assert_eq!(stack.census_shape(), expected);
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
        let mut stack = <FrameStack>::default();
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL).unwrap();
        stack.set_pc(6);
        stack.push(&f, TaggedValue::FALSE, 3, 1, FILL).unwrap();
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
        let mut live = <FrameStack>::default();
        live.push(&code(7, 6), TaggedValue::FALSE, 0, 6, FILL)
            .unwrap();
        live.set_pc(3);
        let at = live.bytes();
        live.append(&captured).unwrap();
        live.set_return_reg_at(at, 5);
        assert_eq!(
            shape(&live),
            [(3, 0, 21, 3, 9), (2, 7, 16, 0, 5), (7, 3, 5, 6, 0)]
        );
        live.assert_frames_tile();
        // And onto an empty one, where the outermost frame is the bottom.
        let mut bare = <FrameStack>::default();
        bare.append(&captured).unwrap();
        assert_eq!(shape(&bare), [(3, 0, 10, 3, 9), (2, 7, 5, 0, 1)]);
        bare.assert_frames_tile();
        assert_eq!(bare.pop(), 9);
        assert_eq!(bare.pop(), 1);
        assert!(bare.is_empty());
    }

    fn ids<B: Buffer>(stack: &FrameStack<B>) -> Vec<u64> {
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
        let mut stack = <FrameStack>::default();
        for _ in 0..3 {
            stack.push(&f, TaggedValue::FALSE, 0, 1, FILL).unwrap();
        }
        stack.push(&g, TaggedValue::FALSE, 0, 1, FILL).unwrap();
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL).unwrap();
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
        stack.replace_top(&g, TaggedValue::TRUE, FILL).unwrap();
        assert_eq!(ids(&stack), [2, 1]);
        assert_eq!(counts(), (2, 2));
        stack.assert_frames_tile();
        stack.pop();
        assert_eq!(ids(&stack), [1]);
        stack.assert_frames_tile();
        stack.truncate(0);
        stack.drop_spent();
        assert_eq!(counts(), (1, 1));

        // An arrival into a stack that held other frames leaves none of them.
        stack.push(&g, TaggedValue::FALSE, 0, 4, FILL).unwrap();
        let mut other = three();
        other.copy_from(&stack).unwrap();
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
        let mut stack = <FrameStack>::default();
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL).unwrap();
        stack.push(&g, TaggedValue::FALSE, 0, 1, FILL).unwrap();
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
        stack.push(&g, TaggedValue::FALSE, 0, 1, FILL).unwrap();
        assert_eq!(ids(&stack), [2, 1]);
        assert_eq!(counts(), (2, 2, 1));
        stack.assert_frames_tile();
        // `h` in its place: `g`'s reference goes for `h`'s.
        stack.pop();
        stack.push(&h, TaggedValue::FALSE, 0, 1, FILL).unwrap();
        assert_eq!(ids(&stack), [3, 1]);
        assert_eq!(counts(), (2, 1, 2));
        stack.assert_frames_tile();
        // Spent again, and dropped where the store asks.
        stack.pop();
        assert_eq!(counts(), (2, 1, 2));
        stack.drop_spent();
        assert_eq!(counts(), (2, 1, 1));
        // An arrival's copy replaces the spent runs with the frames'.
        stack.push(&h, TaggedValue::FALSE, 0, 1, FILL).unwrap();
        stack.pop();
        let mut arrival = <FrameStack>::default();
        arrival.push(&g, TaggedValue::FALSE, 0, 1, FILL).unwrap();
        stack.copy_from(&arrival).unwrap();
        assert_eq!(ids(&stack), [2]);
        assert_eq!(counts(), (1, 3, 1));
        stack.assert_frames_tile();
        // A cut leaves the runs above it spent, and a clear drops them.
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL).unwrap();
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
        let mut stack = <FrameStack>::default();
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL).unwrap();
        stack.push(&g, TaggedValue::FALSE, 7, 1, FILL).unwrap();
        // `g` to `h`: `h` takes a reference, and `g` is spent above it.
        stack.replace_top(&h, TaggedValue::TRUE, FILL).unwrap();
        assert_eq!((ids(&stack), counts()), (vec![3, 1], (2, 2, 1)));
        stack.assert_frames_tile();
        // And back, and forth: the two change places.
        for _ in 0..3 {
            stack.replace_top(&g, TaggedValue::TRUE, FILL).unwrap();
            assert_eq!((ids(&stack), counts()), (vec![2, 1], (2, 2, 1)));
            stack.assert_frames_tile();
            stack.replace_top(&h, TaggedValue::TRUE, FILL).unwrap();
            assert_eq!((ids(&stack), counts()), (vec![3, 1], (2, 2, 1)));
        }
        // A third procedure: the spent `g` goes for it.
        stack.replace_top(&k, TaggedValue::TRUE, FILL).unwrap();
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
        stack.push(&g, TaggedValue::FALSE, 7, 1, FILL).unwrap();
        stack.replace_top(&f, TaggedValue::TRUE, FILL).unwrap();
        assert_eq!(ids(&stack), [1, 1]);
        assert_eq!(Rc::strong_count(&f), 3);
        stack.assert_frames_tile();
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL).unwrap();
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
        let mut stack = <FrameStack>::default();
        for _ in 0..3 {
            stack.push(&f, TaggedValue::FALSE, 0, 1, FILL).unwrap();
        }
        stack.push(&g, TaggedValue::FALSE, 0, 1, FILL).unwrap();
        // The top two frames: one of `f`, cut out of its run, and `g`.
        let captured = stack.above(12 * 8);
        assert_eq!(ids(&captured), [2, 1]);
        captured.assert_frames_tile();
        assert_eq!((Rc::strong_count(&f), Rc::strong_count(&g)), (3, 3));
        // Onto the run it was cut from.
        stack.truncate(12 * 8);
        assert_eq!(ids(&stack), [1, 1]);
        stack.assert_frames_tile();
        stack.append(&captured).unwrap();
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

    /// A live stack in the smallest reservation there is, one page.
    fn one_page() -> LiveStack {
        LiveStack::reserved(1, 1)
    }

    /// The live stack's words are in a reservation (#908's 7c): they are
    /// where it starts however many there are, and a frame that would end
    /// past it is refused with nothing written. A frame that ends at it
    /// is not.
    #[test]
    fn a_live_stack_never_moves_and_refuses_a_frame_past_its_end() {
        // 65 words a frame, which divides no page: some words are left.
        let f = code(1, 60);
        let frame = HEADER_WORDS + 60;
        let mut stack = one_page();
        let room = stack.capacity_bytes() / WORD_BYTES;
        let first = stack.base_address();
        assert_eq!(stack.top_address(), first);
        for i in 0..room / frame {
            let base = stack.push(&f, TaggedValue::FALSE, 0, 60, FILL);
            assert_eq!(base, Ok(i * frame + HEADER_WORDS));
        }
        let held = room / frame * frame;
        assert_eq!(stack.words().as_ptr() as usize, first);
        assert_eq!(stack.base_address(), first);
        assert_eq!(stack.top_address(), first + held * WORD_BYTES);

        // The page has room for less than another frame.
        stack.set_pc(7);
        let before = (shape(&stack), stack.bytes(), Rc::strong_count(&f));
        assert_eq!(stack.push(&f, TaggedValue::FALSE, 0, 60, FILL), Err(NoRoom));
        let g = code(2, 60);
        assert_eq!(stack.push(&g, TaggedValue::FALSE, 0, 60, FILL), Err(NoRoom));
        assert_eq!(Rc::strong_count(&g), 1, "no run was started for it");
        assert_eq!((shape(&stack), stack.bytes(), Rc::strong_count(&f)), before);
        stack.assert_frames_tile();

        // A frame of exactly the words left ends at the reservation's end,
        // and after it not even a header fits.
        let left = room - held;
        assert!(
            left >= HEADER_WORDS,
            "the test's frame leaves room for a header"
        );
        let last = code(3, (left - HEADER_WORDS) as u16);
        let base = stack.push(&last, TaggedValue::FALSE, 0, last.num_regs, FILL);
        assert_eq!(base, Ok(held + HEADER_WORDS));
        assert_eq!(stack.bytes(), stack.capacity_bytes());
        assert_eq!(
            stack.push(&code(4, 0), TaggedValue::FALSE, 0, 0, FILL),
            Err(NoRoom)
        );
        stack.assert_frames_tile();
        // A pop gives the room back.
        stack.pop();
        assert_eq!(stack.pc(), 7);
        assert!(
            stack
                .push(&code(4, 0), TaggedValue::FALSE, 0, 0, FILL)
                .is_ok()
        );
    }

    /// A tail call whose window must grow past the reservation is refused
    /// with the frame as it was: its code, its closure, its window, and no
    /// reference taken for the code it was to run.
    #[test]
    fn a_tail_call_that_cannot_grow_its_window_is_refused_with_the_frame_as_it_was() {
        let mut stack = one_page();
        let room = stack.capacity_bytes() / WORD_BYTES;
        let small = code(1, 1);
        stack.push(&small, TaggedValue::TRUE, 3, 1, FILL).unwrap();
        stack.set_pc(5);
        let before = (shape(&stack), stack.bytes(), stack.top_closure());

        let wide = code(2, (room - HEADER_WORDS + 1) as u16);
        assert_eq!(
            stack.replace_top(&wide, TaggedValue::FALSE, FILL),
            Err(NoRoom)
        );
        assert_eq!((shape(&stack), stack.bytes(), stack.top_closure()), before);
        assert_eq!(Rc::strong_count(&wide), 1);
        assert_eq!(stack.top_code().map(|code| code.id.0), Some(1));
        stack.assert_frames_tile();

        // One whose window ends at the reservation's end grows into it.
        let fits = code(3, (room - HEADER_WORDS) as u16);
        stack.replace_top(&fits, TaggedValue::FALSE, FILL).unwrap();
        assert_eq!(stack.bytes(), stack.capacity_bytes());
        assert_eq!(ids(&stack), [3]);
        stack.assert_frames_tile();
    }

    /// A delimited re-entry and a full arrival that the reservation cannot
    /// hold are refused with the stack as it was, and one it can hold
    /// lands at the reservation's base.
    #[test]
    fn an_append_or_an_arrival_past_the_reservation_is_refused_with_the_stack_as_it_was() {
        let (f, g) = (code(1, 1), code(2, 59));
        let mut stack = one_page();
        let room = stack.capacity_bytes() / WORD_BYTES;
        let first = stack.base_address();
        stack.push(&f, TaggedValue::FALSE, 0, 1, FILL).unwrap();
        stack.set_pc(3);
        let frames_of_g = |frames: usize| {
            let mut copy = <FrameStack>::default();
            for _ in 0..frames {
                copy.push(&g, TaggedValue::FALSE, 0, 59, FILL).unwrap();
            }
            copy
        };
        // More frames than the page holds, and as many as it holds with
        // the frame that is there.
        let big = frames_of_g(room / (HEADER_WORDS + 59) + 1);
        let full = frames_of_g(room / (HEADER_WORDS + 59));
        let before = (shape(&stack), stack.bytes(), Rc::strong_count(&g));

        assert_eq!(stack.append(&big), Err(NoRoom));
        assert_eq!(stack.append(&full), Err(NoRoom));
        assert_eq!(stack.copy_from(&big), Err(NoRoom));
        assert_eq!((shape(&stack), stack.bytes(), Rc::strong_count(&g)), before);
        assert_eq!(stack.pc(), 3);
        stack.assert_frames_tile();

        // What fits arrives where the stack starts, in place of the frame
        // that was there, and can be appended to.
        stack.copy_from(&full).unwrap();
        assert_eq!(shape(&stack), shape(&full));
        assert_eq!(
            (stack.base_address(), stack.words().as_ptr() as usize),
            (first, first)
        );
        stack.assert_frames_tile();
        stack.copy_from(&frames_of_g(1)).unwrap();
        stack.append(&frames_of_g(2)).unwrap();
        assert_eq!(ids(&stack), [2, 2, 2]);
        stack.assert_frames_tile();
    }

    #[test]
    fn a_tail_call_reuses_the_window_and_grows_it() {
        let mut stack = three();
        stack.set_pc(5);
        // Code of fewer registers: the window stays as it is.
        stack
            .replace_top(&code(4, 2), TaggedValue::TRUE, FILL)
            .unwrap();
        assert_eq!(stack.bytes(), 20 * 8, "a window never shrinks");
        assert_eq!(shape(&stack)[0], (4, 0, 17, 3, 9));
        // Code of more: it grows, and the new registers hold the fill.
        stack
            .replace_top(&code(5, 6), TaggedValue::TRUE, TaggedValue::fixnum(7))
            .unwrap();
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

    // ── The reservation's pages (4d.8, #909) ────────────────────────────

    /// A frame of the pages' tests: 1,000 registers, 8,040 bytes.
    const WIDE: u16 = 1000;

    /// Push frames of [`WIDE`] registers, each holding `#t`, until the
    /// stack holds at least `bytes`.
    fn fill_to(stack: &mut LiveStack, f: &Rc<CodeObject>, bytes: usize) {
        while stack.bytes() < bytes {
            stack
                .push(f, TaggedValue::FALSE, 0, WIDE, TaggedValue::TRUE)
                .unwrap();
        }
    }

    /// Pop frames until the stack holds at most `bytes`.
    fn drain_to(stack: &mut LiveStack, bytes: usize) {
        while stack.bytes() > bytes {
            stack.pop();
        }
    }

    /// The raw word of a register near `bytes` into the reservation, in
    /// whichever frame of [`WIDE`] registers [`fill_to`] put there, read
    /// where it lies whether or not the stack still holds it: `#t`'s word
    /// once a frame was pushed over it, and zero on a page that was never
    /// written or has been given back.
    fn word_near(stack: &LiveStack, bytes: usize) -> u64 {
        let frame = HEADER_WORDS + WIDE as usize;
        let index = bytes / WORD_BYTES / frame * frame + HEADER_WORDS + 7;
        assert!(index < stack.capacity_bytes() / WORD_BYTES);
        // SAFETY: the index is inside the reservation, which is readable
        // from end to end and zero where it is unwritten; a word is plain
        // data.
        unsafe { (*stack.words.first().add(index)).get().raw() }
    }

    const MIB: usize = 1 << 20;
    const STEP: usize = GRANT_WORDS * WORD_BYTES;

    /// The stack is granted room in steps, and what it was granted is what
    /// it holds committed: the figure `(gc-stats)` reads, published on the
    /// carrier where it moves.
    #[test]
    fn the_stack_is_granted_room_in_steps_and_publishes_what_it_holds() {
        let carrier = Carrier::new();
        let mut stack = LiveStack::reserved_on(8 * MIB, MIB, Rc::clone(&carrier));
        assert_eq!((stack.committed_bytes(), carrier.stack_committed()), (0, 0));
        let f = code(1, WIDE);
        fill_to(&mut stack, &f, 1);
        assert_eq!(stack.committed_bytes(), STEP, "the first frame: one step");
        fill_to(&mut stack, &f, STEP - 8040);
        assert_eq!(stack.committed_bytes(), STEP, "within it: no more");
        fill_to(&mut stack, &f, STEP + 1);
        assert_eq!(stack.committed_bytes(), 2 * STEP, "past it: the next");
        assert_eq!(carrier.stack_committed(), 2 * STEP);
        // An append and an arrival are granted room as a push is.
        let captured = stack.copy();
        stack.append(&captured).unwrap();
        let held = stack.bytes().next_multiple_of(STEP);
        assert_eq!(
            (stack.committed_bytes(), carrier.stack_committed()),
            (held, held)
        );
        stack.assert_frames_tile();
        // The last step ends at the reservation's end, and nothing is
        // granted past it.
        let nearly = stack.capacity_bytes() - 8040;
        fill_to(&mut stack, &f, nearly);
        assert_eq!(stack.committed_bytes(), stack.capacity_bytes());
        assert_eq!(
            stack.push(&f, TaggedValue::FALSE, 0, WIDE, TaggedValue::TRUE),
            Err(NoRoom)
        );
        assert_eq!(carrier.stack_committed(), stack.capacity_bytes());
    }

    /// An observation gives back the pages above what the stack reached
    /// since the observation before, so a recursion's pages go at the
    /// second observation after it returns, and what is under the mark
    /// stays, the frames still on the stack among it.
    #[test]
    fn an_observation_gives_back_what_the_stack_did_not_reach_since_the_one_before() {
        assert_ne!(TaggedValue::TRUE.raw(), 0);
        let carrier = Carrier::new();
        let mut stack = LiveStack::reserved_on(8 * MIB, MIB, Rc::clone(&carrier));
        let f = code(1, WIDE);
        fill_to(&mut stack, &f, 4 * MIB);
        let deep = stack.bytes().next_multiple_of(STEP);
        assert_eq!(stack.committed_bytes(), deep);
        // Back to a little over 1.5 MiB, which the stack keeps holding.
        drain_to(&mut stack, 3 * MIB / 2 + 100_000);
        let kept = stack.bytes().next_multiple_of(STEP);
        let frames = shape(&stack);
        assert_ne!(word_near(&stack, 3 * MIB), 0, "written by the descent");

        // The first observation after the return: the period it ends went
        // to 4 MiB, so nothing goes.
        stack.observe();
        assert_eq!(stack.committed_bytes(), deep);
        assert_ne!(word_near(&stack, 3 * MIB), 0);

        // The second: the stack has not been above its top since.
        stack.observe();
        assert_eq!(
            (stack.committed_bytes(), carrier.stack_committed()),
            (kept, kept)
        );
        assert_eq!(word_near(&stack, 3 * MIB), 0, "the page went back");
        assert_eq!(word_near(&stack, kept + STEP), 0, "from the mark up");
        assert_ne!(word_near(&stack, MIB), 0, "and not from under the top");
        assert_eq!(shape(&stack), frames);
        stack.assert_frames_tile();
        for frame in stack.frames() {
            let base = frame.register_base;
            assert!(
                stack.words()[base..base + WIDE as usize]
                    .iter()
                    .all(|word| word.get() == TaggedValue::TRUE)
            );
        }

        // A third changes nothing, and the stack grows over the pages
        // that went as over any it never wrote.
        stack.observe();
        assert_eq!(stack.committed_bytes(), kept);
        fill_to(&mut stack, &f, 4 * MIB);
        assert_eq!(stack.committed_bytes(), deep);
        assert_ne!(word_near(&stack, 3 * MIB), 0);
        stack.assert_frames_tile();
    }

    /// The hysteresis: a stack that goes as deep between every two
    /// observations keeps its pages, with nothing given back and written
    /// again; and one that goes less deep each time gives back only what
    /// lies above the deepest it went in the period.
    #[test]
    fn a_stack_that_goes_as_deep_in_every_period_keeps_its_pages() {
        let mut stack = LiveStack::reserved(8 * MIB, MIB);
        let f = code(1, WIDE);
        fill_to(&mut stack, &f, 4 * MIB);
        let deep = stack.committed_bytes();
        for _ in 0..10 {
            drain_to(&mut stack, 0);
            stack.observe();
            assert_eq!(stack.committed_bytes(), deep);
            assert_ne!(word_near(&stack, 3 * MIB), 0, "no page went back");
            fill_to(&mut stack, &f, 4 * MIB);
        }
        // A period that reaches only 3 MiB gives back what is above that.
        drain_to(&mut stack, 0);
        stack.observe();
        fill_to(&mut stack, &f, 3 * MIB);
        let reached = stack.bytes().next_multiple_of(STEP);
        drain_to(&mut stack, 0);
        stack.observe();
        assert_eq!(stack.committed_bytes(), reached);
        assert_ne!(word_near(&stack, 3 * MIB - STEP), 0);
        assert_eq!(word_near(&stack, 4 * MIB - STEP), 0);
    }

    /// The first MiB is never given back: a stack that moves within it
    /// makes no call to the system at any observation.
    #[test]
    fn the_first_mebibyte_is_never_given_back() {
        let mut stack = LiveStack::reserved(8 * MIB, MIB);
        let f = code(1, WIDE);
        fill_to(&mut stack, &f, 3 * STEP);
        let held = stack.committed_bytes();
        assert!(held <= MIB);
        drain_to(&mut stack, 0);
        for _ in 0..3 {
            stack.observe();
            assert_eq!(stack.committed_bytes(), held);
            assert_ne!(word_near(&stack, 2 * STEP), 0);
        }
        // Past it, what goes back is what lies above the first MiB.
        fill_to(&mut stack, &f, 2 * MIB);
        drain_to(&mut stack, 0);
        stack.observe();
        stack.observe();
        assert_eq!(stack.committed_bytes(), MIB);
        assert_ne!(word_near(&stack, MIB - STEP), 0);
        assert_eq!(word_near(&stack, MIB + STEP), 0);
    }
}
