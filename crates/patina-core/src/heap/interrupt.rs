//! The `InterruptHandle`: how a signal handler, a timer or another thread
//! posts an event to a heap's carrier (#701; `PRD/GC_PRD.md` §12, decision
//! 9).
//!
//! A signal handler cannot borrow an interpreter, and there is no
//! process-wide GC singleton to find one by, so each heap hands out a
//! handle: an `Arc` of an `InterruptCell` that points at its carrier's
//! [`Mutator`]. The handle is `Send + Sync + Clone`, and
//! [`InterruptHandle::post`] is async-signal-safe.
//!
//! **Teardown** ([`Carrier`](super::mutator::Carrier)'s drop) stores null
//! into the cell, then waits until no post is in flight, and only then is the
//! `Mutator` freed. A post counts itself in before it loads the pointer and
//! out after its last write, all four operations `SeqCst`, so a poster is
//! either waited for or reads null and does nothing. A handle may outlive
//! its heap: its posts then answer `false`.
//!
//! Under M:1 the cell points at the one carrier for the heap's life. When
//! carriers share a mutator token (§18.7), it will follow the holder.

use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use super::mutator::Mutator;

/// A heap's handle for posting events from anywhere: a signal handler, a
/// timer, a profiler, a host's other thread. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct InterruptHandle(Arc<InterruptCell>);

// C11 (`PRD/GC_PRD.md` §18.6): the handle crosses threads, which is what it
// is for.
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<InterruptHandle>();
};

impl InterruptHandle {
    pub(super) fn new(cell: &Arc<InterruptCell>) -> Self {
        Self(Arc::clone(cell))
    }

    /// Post `bits` ([`event`](super::mutator::event)) to the heap's carrier,
    /// which services them at its next poll: whether the carrier was there
    /// to post to.
    ///
    /// Async-signal-safe: lock-free atomics only, no allocation and no panic
    /// path, so a signal handler may call it. A poll services
    /// [`SIGNAL`](super::mutator::event::SIGNAL) by raising `&interrupt`
    /// (`Mutator::take_signal`).
    pub fn post(&self, bits: u32) -> bool {
        self.0.post(bits).is_some()
    }

    /// Forward SIGINT to this handle while the guard lives: each one posts
    /// [`SIGNAL`](super::mutator::event::SIGNAL), where it would have ended
    /// the process or reached the handler it replaces. The guard restores
    /// that handler when it drops. One forwarding at a time in a process:
    /// a second fails while the first lives.
    ///
    /// The handler is installed with `SA_RESTART`, so a read that SIGINT
    /// interrupts resumes, and the interrupt is raised at the poll after it.
    /// A second SIGINT while the first is still posted ends the process, by
    /// SIGINT's default action: the program has reached no poll since, being
    /// blocked in a read or in a long primitive, and is not taking
    /// interrupts. A running program takes a post at its next call, long
    /// before a second key press.
    #[cfg(unix)]
    pub fn forward_sigint(&self) -> std::io::Result<SigintForwarding> {
        sigint::forward(&self.0)
    }
}

/// What every handle on a heap shares: where its carrier's `Mutator` is,
/// and how many posts are in flight.
#[derive(Debug)]
pub(super) struct InterruptCell {
    /// The carrier's `Mutator`, or null once its teardown began. Posters
    /// reach its atomic words alone.
    mutator: AtomicPtr<Mutator>,
    /// The posts in flight, each between its count in and its count out.
    posting: AtomicUsize,
}

impl InterruptCell {
    /// A cell that points at no carrier yet.
    pub(super) fn new() -> Self {
        Self {
            mutator: AtomicPtr::new(ptr::null_mut()),
            posting: AtomicUsize::new(0),
        }
    }

    /// Point posters at `mutator`, which stays at its address until
    /// [`detach`](Self::detach).
    pub(super) fn attach(&self, mutator: &Mutator) {
        self.mutator
            .store(ptr::from_ref(mutator).cast_mut(), Ordering::SeqCst);
    }

    /// [`InterruptHandle::post`]: count in, load the pointer, post if it is
    /// not null, count out. The events already posted from elsewhere and not
    /// yet taken, or `None` with no carrier to post to.
    fn post(&self, bits: u32) -> Option<u32> {
        self.posting.fetch_add(1, Ordering::SeqCst);
        let mutator = self.mutator.load(Ordering::SeqCst);
        let before = (!mutator.is_null()).then(|| {
            // SAFETY: the pointer is non-null only from `attach`, once the
            // carrier is at the address it keeps, until `detach`, in its
            // drop, before its memory is freed. This post counted itself in
            // before it loaded the pointer, and `detach` stores null before
            // it waits for the count to read 0; in the one order of the four
            // `SeqCst` operations, a load that saw the pointer came before
            // the store of null, so `detach` waits for this post's count
            // out, below, and the `Mutator` is live until then.
            unsafe { Mutator::post_remote_at(mutator, bits) }
        });
        self.posting.fetch_sub(1, Ordering::SeqCst);
        before
    }

    /// The teardown: stop posters reaching the `Mutator`, and wait out any
    /// post already reaching it. The caller frees it after.
    ///
    /// A poster interrupted between its count in and its count out is
    /// another thread's, or a signal handler that has already returned to
    /// this one, so the wait ends.
    pub(super) fn detach(&self) {
        self.mutator.store(ptr::null_mut(), Ordering::SeqCst);
        while self.posting.load(Ordering::SeqCst) != 0 {
            std::thread::yield_now();
        }
    }
}

/// SIGINT's forwarding to an interrupt cell (#701): a `sigaction` handler
/// that posts `SIGNAL` to the cell a [`SigintForwarding`] names.
#[cfg(unix)]
mod sigint {
    use std::ptr;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

    use super::InterruptCell;
    use crate::heap::mutator::event;

    /// The cell the handler posts to, or null while nothing forwards.
    static TARGET: AtomicPtr<InterruptCell> = AtomicPtr::new(ptr::null_mut());

    /// The handlers between their count in and their count out.
    static IN_HANDLER: AtomicUsize = AtomicUsize::new(0);

    /// The handler: the cell's own protocol, one level out. It counts
    /// itself in before it loads the target and out after its post, so the
    /// guard's drop, which stores null and waits for the count to read 0,
    /// lets go of the cell only once no handler can reach it.
    ///
    /// A SIGINT that finds the last one still posted ends the process
    /// ([`InterruptHandle::forward_sigint`](super::InterruptHandle::forward_sigint)).
    extern "C" fn on_sigint(_signal: libc::c_int) {
        IN_HANDLER.fetch_add(1, Ordering::SeqCst);
        let cell = TARGET.load(Ordering::SeqCst);
        let before = if cell.is_null() {
            None
        } else {
            // SAFETY: non-null only while a `SigintForwarding` holds the
            // cell's `Arc`, and its drop waits for this handler's count out
            // before releasing it.
            unsafe { (*cell).post(event::SIGNAL) }
        };
        IN_HANDLER.fetch_sub(1, Ordering::SeqCst);
        if before.is_some_and(|before| before & event::SIGNAL != 0) {
            // SAFETY: `signal` and `raise` are async-signal-safe. SIGINT is
            // blocked while its handler runs, so the default action ends the
            // process as this handler returns.
            unsafe {
                libc::signal(libc::SIGINT, libc::SIG_DFL);
                libc::raise(libc::SIGINT);
            }
        }
    }

    /// SIGINT forwarded to a heap's interrupt handle until this drops,
    /// from [`InterruptHandle::forward_sigint`](super::InterruptHandle::forward_sigint).
    #[must_use = "SIGINT is forwarded only while the guard lives"]
    pub struct SigintForwarding {
        /// The disposition the handler replaced, restored on drop.
        previous: libc::sigaction,
        /// Keeps the cell the handler posts to alive.
        _cell: Arc<InterruptCell>,
    }

    impl std::fmt::Debug for SigintForwarding {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SigintForwarding").finish_non_exhaustive()
        }
    }

    pub(super) fn forward(cell: &Arc<InterruptCell>) -> std::io::Result<SigintForwarding> {
        let target = Arc::as_ptr(cell).cast_mut();
        if TARGET
            .compare_exchange(ptr::null_mut(), target, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(std::io::Error::other("SIGINT is already forwarded"));
        }
        // SAFETY: `sigaction` is plain data, for which all zeroes is valid;
        // the calls below fill in what is read.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = on_sigint as extern "C" fn(libc::c_int) as libc::sighandler_t;
        action.sa_flags = libc::SA_RESTART;
        // SAFETY: `previous` is plain data, written by `sigaction`.
        let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: both pointers are to live `sigaction`s, and the handler
        // is async-signal-safe: atomics only.
        let installed = unsafe {
            libc::sigemptyset(&raw mut action.sa_mask);
            libc::sigaction(libc::SIGINT, &raw const action, &raw mut previous)
        };
        if installed != 0 {
            let error = std::io::Error::last_os_error();
            TARGET.store(ptr::null_mut(), Ordering::SeqCst);
            return Err(error);
        }
        Ok(SigintForwarding {
            previous,
            _cell: Arc::clone(cell),
        })
    }

    impl Drop for SigintForwarding {
        fn drop(&mut self) {
            // SAFETY: restores the disposition `forward` read.
            unsafe { libc::sigaction(libc::SIGINT, &raw const self.previous, ptr::null_mut()) };
            TARGET.store(ptr::null_mut(), Ordering::SeqCst);
            // A handler running on this thread has returned before this
            // runs, so only another thread's can be waited for.
            while IN_HANDLER.load(Ordering::SeqCst) != 0 {
                std::thread::yield_now();
            }
        }
    }
}

#[cfg(unix)]
pub use sigint::SigintForwarding;

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    use super::super::mutator::{Carrier, Mutator, event};
    use super::InterruptHandle;

    #[test]
    fn a_post_reaches_the_carrier() {
        let carrier = Carrier::new();
        let handle = carrier.interrupt_handle();
        assert!(!carrier.poll_due());
        assert!(handle.post(event::SIGNAL));
        assert!(carrier.poll_due());
        assert!(!carrier.gc_posted(), "a remote post is not the owner's");
    }

    #[test]
    fn a_post_after_teardown_does_nothing() {
        let carrier = Carrier::new();
        let handle = carrier.interrupt_handle();
        let clone = handle.clone();
        drop(carrier);
        assert!(!handle.post(event::SIGNAL));
        assert!(!clone.post(event::SIGNAL));
    }

    /// The wait itself, deterministically: a post that loaded the pointer
    /// before the teardown began holds the teardown until it has written
    /// and counted itself out.
    #[test]
    fn teardown_waits_out_a_post_in_flight() {
        let carrier = Carrier::new();
        let handle = carrier.interrupt_handle();
        let wrote = Arc::new(AtomicBool::new(false));
        let (loaded, ready) = mpsc::channel();
        let poster = {
            let handle = handle.clone();
            let wrote = Arc::clone(&wrote);
            std::thread::spawn(move || {
                // `post`'s steps, with the teardown let in between the load
                // and the write.
                let cell = &*handle.0;
                cell.posting.fetch_add(1, Ordering::SeqCst);
                let mutator = cell.mutator.load(Ordering::SeqCst);
                assert!(!mutator.is_null());
                loaded.send(()).unwrap();
                while !cell.mutator.load(Ordering::SeqCst).is_null() {
                    std::hint::spin_loop();
                }
                std::thread::sleep(Duration::from_millis(20));
                // SAFETY: the teardown has begun, and waits for this post.
                unsafe { Mutator::post_remote_at(mutator, event::SIGNAL) };
                wrote.store(true, Ordering::SeqCst);
                cell.posting.fetch_sub(1, Ordering::SeqCst);
            })
        };
        ready.recv().unwrap();
        drop::<Rc<Carrier>>(carrier);
        assert!(
            wrote.load(Ordering::SeqCst),
            "the teardown returned before the post in flight ended"
        );
        poster.join().unwrap();
        assert!(!handle.post(event::SIGNAL));
    }

    /// SIGINT posts a signal while it is forwarded, and the handler it
    /// replaced is back once the guard drops. `raise` runs the handler on
    /// this thread before it returns.
    #[cfg(unix)]
    #[test]
    fn sigint_posts_a_signal_while_it_is_forwarded() {
        fn installed() -> libc::sighandler_t {
            // SAFETY: plain data, written by `sigaction`, which only reads
            // the disposition here.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                libc::sigaction(libc::SIGINT, std::ptr::null(), &raw mut action);
                action.sa_sigaction
            }
        }
        let carrier = Carrier::new();
        let before = installed();
        let forwarding = carrier.interrupt_handle().forward_sigint().unwrap();
        assert!(
            carrier.interrupt_handle().forward_sigint().is_err(),
            "one forwarding at a time"
        );
        assert_ne!(installed(), before);
        // SAFETY: SIGINT's handler is the forwarding's, which only posts.
        unsafe { libc::raise(libc::SIGINT) };
        assert!(carrier.take_signal());
        drop(forwarding);
        assert_eq!(installed(), before);
    }

    /// A block the size and alignment of a carrier's `Rc` allocation, so the
    /// allocator hands a freed carrier's memory to one: a post that wrote
    /// the freed `Mutator` would change its bytes.
    #[repr(C, align(128))]
    struct Probe([u64; 48]);

    const PATTERN: u64 = 0xA5A5_5A5A_A5A5_5A5A;

    /// Teardown under posting (#701's acceptance): a thread posts in a tight
    /// loop to every handle it has been sent, while this one makes carriers
    /// and drops them. A post that begins after a drop has returned answers
    /// `false`, and none writes the memory the drop freed, which the probes
    /// allocated in its place would show. Run in the debug build and in
    /// release with `gc-check`.
    #[test]
    fn teardown_is_safe_under_posting() {
        // A carrier's `Rc` allocation: the counts, padded to the carrier's
        // alignment, then the carrier.
        assert_eq!(
            std::mem::size_of::<Probe>(),
            128 + std::mem::size_of::<Carrier>()
        );
        const ROUNDS: usize = 2_000;
        let (send, receive) = mpsc::channel::<(InterruptHandle, Arc<AtomicBool>)>();
        let done = Arc::new(AtomicBool::new(false));
        let poster = {
            let done = Arc::clone(&done);
            std::thread::spawn(move || {
                let mut handles = Vec::new();
                let mut late = 0usize;
                while !done.load(Ordering::SeqCst) {
                    while let Ok(next) = receive.try_recv() {
                        handles.push(next);
                    }
                    for (handle, dropped) in &handles {
                        let after_drop = dropped.load(Ordering::SeqCst);
                        let posted = handle.post(event::SIGNAL);
                        assert!(
                            !(after_drop && posted),
                            "a post begun after teardown reached the carrier"
                        );
                        late += usize::from(after_drop);
                    }
                    // Keep the recent handles, so a post meets each drop.
                    if handles.len() > 8 {
                        handles.drain(..handles.len() - 8);
                    }
                }
                late
            })
        };
        for _ in 0..ROUNDS {
            let carrier = Carrier::new();
            let dropped = Arc::new(AtomicBool::new(false));
            send.send((carrier.interrupt_handle(), Arc::clone(&dropped)))
                .unwrap();
            // Let the poster reach it before the drop, now and then.
            for _ in 0..64 {
                std::hint::spin_loop();
            }
            drop::<Rc<Carrier>>(carrier);
            dropped.store(true, Ordering::SeqCst);
            let probes: Vec<Box<Probe>> = (0..4).map(|_| Box::new(Probe([PATTERN; 48]))).collect();
            for _ in 0..256 {
                std::hint::spin_loop();
            }
            for probe in &probes {
                assert!(
                    probe.0.iter().all(|&word| word == PATTERN),
                    "a post wrote a freed carrier's memory"
                );
            }
        }
        done.store(true, Ordering::SeqCst);
        let late = poster.join().unwrap();
        assert!(late > 0, "the poster never posted after a teardown");
    }
}
