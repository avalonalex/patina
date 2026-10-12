//! A thread's identity (#688, addition C15).

/// Which thread something belongs to: an SRFI 18 thread's id in its heap's
/// thread table (`PRD/GC_PRD.md` §6, §18.1). Under M:1 that is a green
/// thread (decision 7), never an OS thread, so this is not
/// `std::thread::ThreadId`.
///
/// Stage 9 builds the thread table and mints the ids. Until then the only
/// thread is the primordial one, [`ThreadId::PRIMORDIAL`], which every
/// library load names (`LibraryRegistry::begin_loading`); grep for it to find
/// what stage 9 hands the running thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadId(pub u32);

impl ThreadId {
    /// The thread a heap starts with, which runs the program: the only one
    /// until stage 9.
    pub const PRIMORDIAL: ThreadId = ThreadId(0);
}
