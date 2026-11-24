//! A reservation of address space (#908, `PRD/GC_PRD.md` §11.1 and §17.3,
//! K13): one range of memory that never moves, takes physical pages as they
//! are written, and gives them back a range at a time.
//!
//! The register stack is to live in one, so that a frame's address holds
//! for the frame's life (#908's 7c), and the heap's own reservation follows
//! at stage 5, when this moves to `patina-gc`. **Nothing uses it yet**:
//! this is the mapping alone, with its tests, and no lane's output changes.
//!
//! - **Reserving** makes one private anonymous mapping, read-write, with
//!   `MAP_NORESERVE` on Linux, where the kernel's heuristic would refuse a
//!   mapping larger than memory and swap that asked to be accounted.
//!   Nothing is committed: a page becomes the process's when it is first
//!   written, and reads as zero until then. Where the system refuses the
//!   size asked for (a soft `RLIMIT_AS`, strict overcommit, an address
//!   space already full), the reservation is made at half of it, and half
//!   again, down to a floor, rather than fail: [`Reservation::size`] says
//!   what there is. By halves, since what a smaller reservation leaves is
//!   for the allocations that follow it (the heap's arenas are `malloc`'d
//!   until stage 5e).
//! - **Decommitting** a range gives back the whole pages inside it, which
//!   read as zero the next time they are touched. On Linux it is
//!   `madvise(MADV_DONTNEED)`, which splits no mapping. Elsewhere on unix
//!   it is a fresh mapping placed over the range, `mmap(MAP_FIXED)`, the
//!   one call that returns the pages on macOS (measured below).
//! - **No guard page.** Whoever writes into a reservation checks its end
//!   first (`PRD/GC_PRD.md` §11.1's frame-entry check).
//! - **Targets other than unix**, which CI does not build, get one zeroed
//!   allocation of the size, and a decommit there zeroes the pages and
//!   gives nothing back. The tests run that backing on unix too.
//!
//! **What returns pages on macOS.** 256 MiB written in an 8 GiB mapping,
//! then one call over the 256 MiB, on macOS 27.2 (an M4 Pro; 2026-11-24).
//! The footprint is `proc_pid_rusage`'s `ri_phys_footprint`, which
//! `(gc-stats)` reports as `resident-bytes`:
//!
//! | call | footprint | resident size | pages `mincore` finds | contents |
//! |---|---|---|---|---|
//! | none | 257.3 MiB | 257.7 MiB | all 16,384 | kept |
//! | `madvise(MADV_DONTNEED)` | 257.3 MiB | 257.7 MiB | all | kept |
//! | `madvise(MADV_FREE)` | 257.3 MiB | 257.7 MiB | all | kept |
//! | `madvise(MADV_FREE_REUSABLE)` | 1.3 MiB | 257.7 MiB | all | kept |
//! | `mmap(MAP_FIXED)` over the range | 1.1 MiB | 1.7 MiB | none | zero |
//!
//! `MADV_FREE_REUSABLE` takes the pages out of the footprint and leaves
//! them out when they are written again (1.3 MiB after the 256 MiB was
//! rewritten), until a `MADV_FREE_REUSE` says they are back in use, which
//! every push into them would owe the kernel. The fresh mapping took 1.7 ms
//! for the 256 MiB, and writing the range again 12 ms, 47 µs a MiB, against
//! 0.1 ms for pages still there. An 8 GiB mapping is 64 map entries there
//! from the start, and 75 after a thousand such calls at as many places.

use std::io;
use std::ops::Range;
use std::ptr::NonNull;

/// A range of address space that is this value's until it is dropped: the
/// same address throughout, readable and writable from end to end, zero
/// where it has not been written since it was reserved or decommitted.
///
/// It hands out no reference: a user reads and writes through
/// [`Self::base`], in unsafe code that keeps to [`Self::size`].
#[derive(Debug)]
pub struct Reservation {
    base: NonNull<u8>,
    size: usize,
    backing: Backing,
}

/// Where a reservation's memory came from, which says how it is given back.
#[derive(Debug)]
enum Backing {
    /// One anonymous mapping.
    #[cfg(unix)]
    Mapped,
    /// One zeroed allocation, of the layout it was made with.
    #[cfg(any(not(unix), test))]
    Allocated(std::alloc::Layout),
}

impl Reservation {
    /// Reserve `wanted` bytes, rounded up to a whole number of pages, or as
    /// much of that as the system gives: on a refusal half the size is
    /// tried, and half of that, and at last `floor`, which is also what is
    /// reserved where less than it is wanted. An error is the system's
    /// refusal of the floor.
    pub fn new(wanted: usize, floor: usize) -> io::Result<Self> {
        #[cfg(unix)]
        {
            Self::mapped(wanted, floor)
        }
        #[cfg(not(unix))]
        {
            Self::allocated(wanted, floor)
        }
    }

    /// [`Self::new`] as one anonymous mapping.
    #[cfg(unix)]
    fn mapped(wanted: usize, floor: usize) -> io::Result<Self> {
        let (base, size) = reserve_with(wanted, floor, page_size(), map)?;
        Ok(Self {
            base,
            size,
            backing: Backing::Mapped,
        })
    }

    /// [`Self::new`] as one zeroed allocation: the backing of a target with
    /// no mapping calls this module knows.
    #[cfg(any(not(unix), test))]
    fn allocated(wanted: usize, floor: usize) -> io::Result<Self> {
        use std::alloc::Layout;

        let page = page_size();
        let layout = |size| Layout::from_size_align(size, page).map_err(io::Error::other);
        let (base, size) = reserve_with(wanted, floor, page, |size| {
            let layout = layout(size)?;
            // SAFETY: the layout's size is a positive number of pages.
            let memory = unsafe { std::alloc::alloc_zeroed(layout) };
            NonNull::new(memory).ok_or_else(|| io::ErrorKind::OutOfMemory.into())
        })?;
        Ok(Self {
            base,
            size,
            backing: Backing::Allocated(layout(size)?),
        })
    }

    /// The first byte: page-aligned, and the same for the reservation's
    /// life.
    pub fn base(&self) -> NonNull<u8> {
        self.base
    }

    /// The reservation's size in bytes, a whole number of pages: what was
    /// asked for, or less where the system refused that.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Give back the whole pages inside `range`, which is in bytes from
    /// the base: what they held is gone, and they read as zero. A page the
    /// range covers only in part keeps its bytes, so a range with no whole
    /// page in it does nothing. It takes the reservation exclusively:
    /// nothing may be reading a page while it goes.
    ///
    /// An error is the system's, and leaves every page as it was.
    ///
    /// # Panics
    ///
    /// If the range ends past the reservation or before it starts.
    pub fn decommit(&mut self, range: Range<usize>) -> io::Result<()> {
        assert!(
            range.start <= range.end && range.end <= self.size,
            "decommit of {range:?} in a reservation of {} bytes",
            self.size
        );
        let page = page_size();
        let start = range.start.next_multiple_of(page);
        let end = range.end & !(page - 1);
        if start >= end {
            return Ok(());
        }
        // SAFETY: `start < end <= self.size`, so the address is inside the
        // reservation.
        let first = unsafe { self.base.add(start) };
        match self.backing {
            #[cfg(unix)]
            Backing::Mapped => release(first, end - start),
            #[cfg(any(not(unix), test))]
            Backing::Allocated(_) => {
                // SAFETY: the `end - start` bytes at `first` are inside the
                // allocation, and `&mut self` says nothing else reads them.
                unsafe { first.write_bytes(0, end - start) };
                Ok(())
            }
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        match self.backing {
            #[cfg(unix)]
            Backing::Mapped => {
                // SAFETY: the range is the one `map` answered, with whatever
                // `release` has mapped inside it since, and nothing reads
                // it after its owner is dropped.
                let status = unsafe { libc::munmap(self.base.as_ptr().cast(), self.size) };
                debug_assert_eq!(status, 0, "munmap: {}", io::Error::last_os_error());
            }
            #[cfg(any(not(unix), test))]
            Backing::Allocated(layout) => {
                // SAFETY: the memory was allocated with this layout, by
                // this allocator, and is freed once.
                unsafe { std::alloc::dealloc(self.base.as_ptr(), layout) }
            }
        }
    }
}

/// The system's page size in bytes: every reservation is a whole number of
/// them, and a decommit works in them.
pub fn page_size() -> usize {
    #[cfg(unix)]
    {
        // SAFETY: `sysconf` reads a constant of the system.
        let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        usize::try_from(size)
            .ok()
            .filter(|size| size.is_power_of_two())
            .expect("the system's page size")
    }
    #[cfg(not(unix))]
    {
        4096
    }
}

/// The sizes a reservation is tried at, in order: `wanted`, then half of
/// the size before while that is above `floor`, then `floor`. Each is a
/// positive whole number of pages of `page` bytes, a power of two, and at
/// most the largest an allocation may be, `isize::MAX` rounded down to a
/// page; `wanted` and `floor` are rounded up to one, a half down.
fn sizes(wanted: usize, floor: usize, page: usize) -> impl Iterator<Item = usize> {
    let largest = isize::MAX.unsigned_abs() & !(page - 1);
    let pages = |bytes: usize| {
        bytes
            .max(1)
            .checked_next_multiple_of(page)
            .map_or(largest, |bytes| bytes.min(largest))
    };
    let floor = pages(floor);
    std::iter::successors(Some(pages(wanted).max(floor)), move |&size| {
        (size > floor).then(|| ((size / 2) & !(page - 1)).max(floor))
    })
}

/// Reserve through `map`, which answers the address of a size or the
/// system's refusal of it, at the first of [`sizes`] it does not refuse.
/// The address and the size it was given at, or the refusal of the floor.
fn reserve_with(
    wanted: usize,
    floor: usize,
    page: usize,
    mut map: impl FnMut(usize) -> io::Result<NonNull<u8>>,
) -> io::Result<(NonNull<u8>, usize)> {
    let mut refusal = None;
    for size in sizes(wanted, floor, page) {
        match map(size) {
            Ok(base) => return Ok((base, size)),
            Err(error) => refusal = Some(error),
        }
    }
    Err(refusal.expect("at least the floor is tried"))
}

/// What a reservation's pages allow.
#[cfg(unix)]
const PROTECTION: libc::c_int = libc::PROT_READ | libc::PROT_WRITE;

/// How a reservation is mapped: private and anonymous, and on Linux not
/// accounted until it is written (K13).
#[cfg(any(target_os = "linux", target_os = "android"))]
const MAPPING: libc::c_int = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE;
#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
const MAPPING: libc::c_int = libc::MAP_PRIVATE | libc::MAP_ANON;

/// Map `size` bytes, a positive number of pages, wherever the kernel puts
/// them.
#[cfg(unix)]
fn map(size: usize) -> io::Result<NonNull<u8>> {
    // SAFETY: an anonymous mapping at an address of the kernel's choosing
    // touches no memory the process has.
    let memory = unsafe { libc::mmap(std::ptr::null_mut(), size, PROTECTION, MAPPING, -1, 0) };
    if memory == libc::MAP_FAILED {
        return Err(io::Error::last_os_error());
    }
    NonNull::new(memory.cast()).ok_or_else(|| io::Error::other("a mapping at address zero"))
}

/// Give back the `len` bytes of whole pages at `first`, inside a mapping
/// [`map`] made: they read as zero afterwards. `MADV_DONTNEED` drops a
/// private anonymous mapping's pages at once and leaves the mapping whole.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn release(first: NonNull<u8>, len: usize) -> io::Result<()> {
    // SAFETY: the pages are the caller's own mapping's, which it holds
    // exclusively; the call changes their contents and nothing else.
    match unsafe { libc::madvise(first.as_ptr().cast(), len, libc::MADV_DONTNEED) } {
        0 => Ok(()),
        _ => Err(io::Error::last_os_error()),
    }
}

/// Give back the `len` bytes of whole pages at `first`, inside a mapping
/// [`map`] made: they read as zero afterwards. A fresh mapping of the same
/// kind is placed over them, which frees the pages the old one held (the
/// module's text has what the `madvise` calls do instead on macOS).
///
/// The kernel takes the old mapping out before it puts the new one in, so
/// a failure may leave a hole in the reservation, which a later mapping of
/// anyone's could fill and the reservation's user would then write into.
/// Nothing is known to make the call fail on a range already mapped this
/// way; if it does, the process stops here, as it does where memory cannot
/// be allocated.
#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
fn release(first: NonNull<u8>, len: usize) -> io::Result<()> {
    let at = first.as_ptr().cast();
    // SAFETY: the range is whole pages of the caller's own mapping, which
    // it holds exclusively, so replacing them takes nothing from anyone;
    // `MAP_FIXED` maps at `at` or fails.
    let memory = unsafe { libc::mmap(at, len, PROTECTION, MAPPING | libc::MAP_FIXED, -1, 0) };
    if memory == at {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    eprintln!("patina: {len} bytes of a reservation could not be mapped again at {at:p}: {error}");
    std::process::abort()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIB: usize = 1 << 10;
    const MIB: usize = 1 << 20;
    const GIB: usize = 1 << 30;

    /// The main stack's bounds (`heap::budget`), as a reservation's.
    const CEILING: usize = 8 * GIB;
    const FLOOR: usize = 2 * MIB;

    fn tried(wanted: usize, floor: usize, page: usize) -> Vec<usize> {
        sizes(wanted, floor, page).collect()
    }

    #[test]
    fn the_sizes_tried_fall_by_halves_to_the_floor() {
        let page = 16 * KIB;
        assert_eq!(
            tried(CEILING, FLOOR, page),
            [
                8 * GIB,
                4 * GIB,
                2 * GIB,
                GIB,
                512 * MIB,
                256 * MIB,
                128 * MIB,
                64 * MIB,
                32 * MIB,
                16 * MIB,
                8 * MIB,
                4 * MIB,
                2 * MIB
            ]
        );
        // A size that is no power of two: its halves, and the floor last
        // where a half falls below it.
        assert_eq!(
            tried(6 * GIB, GIB, page),
            [6 * GIB, 3 * GIB, 3 * GIB / 2, GIB]
        );
        // The floor alone, where no more is wanted, or less.
        assert_eq!(tried(FLOOR, FLOOR, page), [FLOOR]);
        assert_eq!(tried(MIB, FLOOR, page), [FLOOR]);
        assert_eq!(tried(0, FLOOR, page), [FLOOR]);
        // At least a page, where neither asks for one.
        assert_eq!(tried(0, 0, page), [page]);
        assert_eq!(tried(1, 0, page), [page]);
        // The size and the floor are rounded up to a page, and a half down.
        assert_eq!(tried(page + 1, 1, page), [2 * page, page]);
        assert_eq!(tried(5 * page, page + 1, page), [5 * page, 2 * page]);
        assert_eq!(tried(7 * page, 0, page), [7 * page, 3 * page, page]);
        assert_eq!(tried(3 * 4096, 4096, 4096), [3 * 4096, 4096]);
    }

    #[test]
    fn no_size_tried_is_larger_than_an_allocation_may_be() {
        let page = 16 * KIB;
        let largest = isize::MAX.unsigned_abs() & !(page - 1);
        for wanted in [usize::MAX, usize::MAX - 1, largest + 1, largest] {
            let sizes = tried(wanted, FLOOR, page);
            assert_eq!(sizes[0], largest, "{wanted:#x}");
            assert_eq!(sizes.last(), Some(&FLOOR), "{wanted:#x}");
        }
        assert_eq!(tried(usize::MAX, usize::MAX, page), [largest]);
        for wanted in [0, 1, page - 1, page, 3 * page + 5, CEILING, usize::MAX] {
            for floor in [0, 1, page, FLOOR, GIB + 1, usize::MAX] {
                for page in [4 * KIB, 16 * KIB, 64 * KIB] {
                    let sizes = tried(wanted, floor, page);
                    let largest = isize::MAX.unsigned_abs() & !(page - 1);
                    let least = floor.max(1).min(largest);
                    assert!(sizes.len() <= usize::BITS as usize + 1);
                    for pair in sizes.windows(2) {
                        assert!(pair[0] > pair[1], "{wanted:#x} {floor:#x}: {sizes:x?}");
                    }
                    for size in &sizes {
                        assert!(
                            *size >= least && *size <= largest && size.is_multiple_of(page),
                            "{wanted:#x} {floor:#x} {page:#x}: {size:#x}"
                        );
                    }
                    assert!(sizes[0] >= wanted.min(largest));
                    assert!(*sizes.last().unwrap() < least.saturating_add(page));
                }
            }
        }
    }

    /// A mapper that refuses every size above `limit`, as an address-space
    /// limit does, and notes each size it was asked for.
    fn under(
        limit: usize,
        asked: &mut Vec<usize>,
    ) -> impl FnMut(usize) -> io::Result<NonNull<u8>> + '_ {
        move |size| {
            asked.push(size);
            if size > limit {
                Err(io::Error::other(format!("{size} refused")))
            } else {
                Ok(NonNull::dangling())
            }
        }
    }

    #[test]
    fn a_refused_size_is_tried_again_at_half() {
        let page = 16 * KIB;
        for (limit, got, attempts) in [
            (usize::MAX, 8 * GIB, 1),
            (8 * GIB, 8 * GIB, 1),
            (8 * GIB - 1, 4 * GIB, 2),
            (4 * GIB, 4 * GIB, 2),
            (3 * GIB, 2 * GIB, 3),
            (GIB + page, GIB, 4),
            (100 * MIB, 64 * MIB, 8),
            (FLOOR, FLOOR, 13),
        ] {
            let mut asked = Vec::new();
            let (_, size) =
                reserve_with(CEILING, FLOOR, page, under(limit, &mut asked)).expect("a size fits");
            assert_eq!(size, got, "under {limit:#x}");
            assert_eq!(asked.len(), attempts, "under {limit:#x}: {asked:x?}");
            assert_eq!(asked, tried(CEILING, FLOOR, page)[..attempts]);
        }
    }

    #[test]
    fn a_refusal_of_the_floor_is_the_error() {
        let page = 16 * KIB;
        let mut asked = Vec::new();
        let error = reserve_with(CEILING, FLOOR, page, under(FLOOR - 1, &mut asked))
            .expect_err("nothing fits");
        assert_eq!(asked, tried(CEILING, FLOOR, page), "every size was tried");
        assert_eq!(
            error.to_string(),
            format!("{FLOOR} refused"),
            "the last refusal"
        );
    }

    fn read(reservation: &Reservation, at: usize) -> u8 {
        assert!(at < reservation.size());
        // SAFETY: inside the reservation, which is readable throughout.
        unsafe { reservation.base().add(at).read() }
    }

    fn write(reservation: &mut Reservation, at: usize, byte: u8) {
        assert!(at < reservation.size());
        // SAFETY: inside the reservation, which is writable throughout and
        // held exclusively.
        unsafe { reservation.base().add(at).write(byte) }
    }

    /// How many of the pages of `range`, whole pages from the base, are in
    /// memory.
    #[cfg(unix)]
    fn resident(reservation: &Reservation, range: Range<usize>) -> usize {
        let page = page_size();
        assert!(range.start.is_multiple_of(page) && range.end.is_multiple_of(page));
        let mut pages = vec![0u8; range.len() / page];
        // SAFETY: the range is whole pages of the reservation, and the
        // vector has a byte for each.
        let status = unsafe {
            libc::mincore(
                reservation.base().add(range.start).as_ptr().cast(),
                range.len(),
                pages.as_mut_ptr().cast(),
            )
        };
        assert_eq!(status, 0, "mincore: {}", io::Error::last_os_error());
        pages.iter().filter(|page| *page & 1 != 0).count()
    }

    /// Write every byte of sixteen pages, decommit a range that starts and
    /// ends inside a page, and read all sixteen back: the whole pages
    /// inside the range are zero, and every byte outside them is as
    /// written, the rest of the two pages the range cuts among them.
    fn decommit_inside(reservation: &mut Reservation) {
        let page = page_size();
        let byte = |at: usize| u8::try_from(at % 251).unwrap() + 1;
        for at in 0..16 * page {
            write(reservation, at, byte(at));
        }
        let range = 3 * page + 100..11 * page + page / 2;
        reservation.decommit(range.clone()).unwrap();
        for at in 0..16 * page {
            let expected = if (4 * page..11 * page).contains(&at) {
                0
            } else {
                byte(at)
            };
            assert_eq!(read(reservation, at), expected, "byte {at} of {range:?}");
        }
        // A decommitted page is written again like any other.
        write(reservation, 5 * page, 7);
        assert_eq!(read(reservation, 5 * page), 7);
        assert_eq!(read(reservation, 5 * page + 1), 0);
    }

    #[test]
    #[cfg(unix)]
    fn a_reservation_larger_than_memory_is_made_whole() {
        use crate::heap::budget::MemoryLimits;

        let page = page_size();
        let limits = MemoryLimits::detect();
        let physical = limits
            .physical
            .and_then(|bytes| usize::try_from(bytes).ok());
        let wanted = CEILING.max(physical.map_or(0, |bytes| bytes.saturating_mul(4))) + 1;
        let mut reservation = Reservation::new(wanted, FLOOR).unwrap();
        // Strict overcommit accounts for every mapping in full, whatever it
        // asks, and an address-space limit holds whatever is mapped.
        let accounted = std::fs::read_to_string("/proc/sys/vm/overcommit_memory")
            .is_ok_and(|mode| mode.trim() == "2");
        if limits.address_space.is_none() && !accounted {
            assert_eq!(
                reservation.size(),
                wanted.next_multiple_of(page),
                "{limits:?}"
            );
        }
        assert!(reservation.size() >= FLOOR);
        assert_eq!(reservation.size() % page, 0);
        assert_eq!(reservation.base().as_ptr().addr() % page, 0);

        // It is all there, zero, and nothing of it is in memory until it
        // is written.
        let last = reservation.size() - 1;
        assert_eq!(resident(&reservation, 0..16 * page), 0);
        for at in [0, 1, page, reservation.size() / 2, last] {
            assert_eq!(read(&reservation, at), 0, "byte {at}");
            write(&mut reservation, at, 0xA5);
            assert_eq!(read(&reservation, at), 0xA5, "byte {at}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn decommitted_pages_are_zero_and_out_of_memory() {
        let page = page_size();
        let mut reservation = Reservation::new(CEILING, FLOOR).unwrap();
        decommit_inside(&mut reservation);

        // The pages go back to the system, not only to zero: written, they
        // are in memory, and decommitted, none of them is.
        let range = 32 * page..96 * page;
        for at in range.clone().step_by(page) {
            write(&mut reservation, at, 1);
        }
        assert!(resident(&reservation, range.clone()) > 0);
        reservation.decommit(range.clone()).unwrap();
        assert_eq!(resident(&reservation, range.clone()), 0);
        for at in range.step_by(page) {
            assert_eq!(read(&reservation, at), 0);
        }

        // To the end of the reservation, and all of it.
        let size = reservation.size();
        write(&mut reservation, size - 1, 9);
        reservation.decommit(size - page..size).unwrap();
        assert_eq!(read(&reservation, size - 1), 0);
        write(&mut reservation, 0, 9);
        reservation.decommit(0..size).unwrap();
        assert_eq!(read(&reservation, 0), 0);
    }

    #[test]
    fn a_range_with_no_whole_page_gives_nothing_back() {
        let page = page_size();
        let mut reservation = Reservation::new(FLOOR, FLOOR).unwrap();
        for at in 0..3 * page {
            write(&mut reservation, at, 0x5A);
        }
        for range in [
            0..0,
            0..page - 1,
            1..page,
            1..2 * page - 1,
            page + 1..2 * page + (page - 1),
            3 * page..3 * page,
        ] {
            reservation.decommit(range.clone()).unwrap();
            for at in 0..3 * page {
                assert_eq!(read(&reservation, at), 0x5A, "byte {at} after {range:?}");
            }
        }
        // One page exactly, and its neighbours' bytes stay.
        reservation.decommit(page..2 * page).unwrap();
        assert_eq!(read(&reservation, page - 1), 0x5A);
        assert_eq!(read(&reservation, page), 0);
        assert_eq!(read(&reservation, 2 * page - 1), 0);
        assert_eq!(read(&reservation, 2 * page), 0x5A);
    }

    #[test]
    #[should_panic(expected = "decommit of")]
    fn a_range_past_the_end_is_refused() {
        let mut reservation = Reservation::new(FLOOR, FLOOR).unwrap();
        let size = reservation.size();
        let _ = reservation.decommit(0..size + 1);
    }

    #[test]
    #[should_panic(expected = "decommit of")]
    fn a_range_that_ends_before_it_starts_is_refused() {
        let mut reservation = Reservation::new(FLOOR, FLOOR).unwrap();
        let page = page_size();
        let _ = reservation.decommit(2 * page..page);
    }

    #[test]
    fn the_allocated_backing_keeps_the_same_contract() {
        let page = page_size();
        let mut reservation = Reservation::allocated(FLOOR + 1, FLOOR).unwrap();
        assert!(matches!(reservation.backing, Backing::Allocated(_)));
        assert_eq!(reservation.size(), (FLOOR + 1).next_multiple_of(page));
        assert_eq!(reservation.base().as_ptr().addr() % page, 0);
        let last = reservation.size() - 1;
        for at in [0, page, last] {
            assert_eq!(read(&reservation, at), 0, "byte {at}");
        }
        decommit_inside(&mut reservation);
        write(&mut reservation, last, 9);
        reservation.decommit(0..last + 1).unwrap();
        assert_eq!(read(&reservation, last), 0);
    }

    /// A reservation that was not unmapped would keep its address space:
    /// 65,536 of 8 GiB are 512 TiB, more than a process has (128 TiB on
    /// x86-64 Linux, and on arm64 macOS, where the 16,324th left mapped
    /// got 2 GiB), and each is as large as the first.
    #[test]
    #[cfg(unix)]
    fn a_dropped_reservation_gives_its_address_space_back() {
        let first = Reservation::new(CEILING, FLOOR).unwrap().size();
        for round in 0..65_536 {
            let mut reservation = Reservation::new(CEILING, FLOOR).unwrap();
            assert_eq!(reservation.size(), first, "round {round}");
            if round % 4096 == 0 {
                // Decommitted pages, mapped anew on macOS, go with it.
                write(&mut reservation, 0, 1);
                reservation.decommit(0..first / 2).unwrap();
            }
        }
    }

    /// The environment variable through which
    /// `a_reservation_fits_under_a_real_address_space_limit` tells its child
    /// where to write the size it got. Test code reads it, never the module.
    const CHILD_REPORT: &str = "PATINA_RESERVATION_TEST_REPORT";

    /// The child of `a_reservation_fits_under_a_real_address_space_limit`:
    /// run by name in a process of its own, under the limit its parent set,
    /// it reserves the main stack's ceiling, uses both ends of what it got,
    /// and writes the size where the parent asked. Run with the rest
    /// (`--include-ignored`) it has nothing to do.
    #[test]
    #[ignore = "the child process of a_reservation_fits_under_a_real_address_space_limit"]
    fn reserve_under_the_parent_s_limit() {
        let Some(report) = std::env::var_os(CHILD_REPORT) else {
            return;
        };
        let mut reservation = Reservation::new(CEILING, FLOOR).unwrap();
        let size = reservation.size();
        for at in [0, size - 1] {
            assert_eq!(read(&reservation, at), 0);
            write(&mut reservation, at, 3);
        }
        reservation.decommit(0..size).unwrap();
        assert_eq!(read(&reservation, size - 1), 0);
        std::fs::write(report, format!("{size}\n")).unwrap();
    }

    /// On Linux, a reservation in a process that really is under an
    /// address-space limit (`PRD/GC_PRD.md` §17.3, K13): this test's own
    /// binary, run again as a child under `ulimit -v` of 4 GiB. The 8 GiB
    /// it asks for is refused, and so is 4 GiB, since the process has
    /// mapped something already; it gets 2 GiB, and startup would go on.
    ///
    /// Linux only: macOS refuses to set `RLIMIT_AS`.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_reservation_fits_under_a_real_address_space_limit() {
        let report = std::env::temp_dir().join(format!(
            "patina-reservation-test-{}-report",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&report);
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(r#"ulimit -S -v 4194304 && exec "$0" "$@""#)
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "heap::reservation::tests::reserve_under_the_parent_s_limit",
                "--ignored",
                "--test-threads=1",
            ])
            .env(CHILD_REPORT, &report)
            .status()
            .unwrap();
        assert!(status.success(), "the child, {status}");
        let text = std::fs::read_to_string(&report).unwrap();
        let _ = std::fs::remove_file(&report);
        assert_eq!(text.trim().parse(), Ok(2 * GIB), "{text:?}");
    }
}
