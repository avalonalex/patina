//! The memory budget (#907, `PRD/GC_PRD.md` §17.3): what the process can
//! find out about the memory it may use, and the main stack's default cap
//! fitted to it.
//!
//! Three readings, each `None` where there is nothing to read or the read
//! fails, so that detection never fails startup:
//!
//! - **physical memory**: `hw.memsize` on macOS, `_SC_PHYS_PAGES` pages of
//!   `_SC_PAGESIZE` on Linux;
//! - **the cgroup limit** (Linux): the memory limit of the process's own
//!   cgroup, v2's `memory.max` or v1's `memory.limit_in_bytes`, and of each
//!   cgroup above it, which binds the process as its own does; the smallest
//!   of them;
//! - **the soft `RLIMIT_AS`**, the address space the process may map.
//!
//! **B**, the budget every default derives from, is the smaller of the
//! first two: never the host's memory alone where a container's limit is
//! lower. [`MemoryLimits::default_stack_cap`] is the main stack's default
//! from it, min(8 GiB, 25%·B), and under `RLIMIT_AS` a quarter of that
//! too, since the stack is one of several reservations the limit has to
//! hold (the heap's is fitted at stage 5e).
//!
//! **Nothing reads these yet.** The stack's cap, `&stack-exhausted` and the
//! knobs that override the default are #907's second half, which waits on
//! the owner's rulings; this is the detection alone, with its fitting
//! logic tested under injected values, and no lane's output changes.
//!
//! macOS has no cgroups and refuses to set `RLIMIT_AS` (`ulimit -v`
//! answers "Invalid argument" on macOS 27.2), so the test that reads a real
//! limit runs on the Linux CI leg, in a child process; the cgroup files are
//! found through a procfs directory the detection is handed, which a test
//! points at a tree of its own on either leg.

use std::path::{Path, PathBuf};

/// The main stack's default cap however much memory there is.
pub const STACK_CAP_CEILING: u64 = 8 << 30;

/// The least the main stack's default cap is, however little memory or
/// address space there is: a default below it would refuse programs that
/// run today in a few megabytes of stack, and startup never fails for it.
pub const STACK_CAP_FLOOR: u64 = 2 << 20;

/// The main stack's share of B, and of `RLIMIT_AS`: one part in four.
const STACK_SHARE: u64 = 4;

/// A cgroup limit file's value from which it means "no limit": cgroup v1
/// writes its largest page count in bytes there, `LONG_MAX` rounded down to
/// a page (`0x7FFF_FFFF_FFFF_F000` with 4 KiB pages), where v2 writes
/// `max`.
const CGROUP_NO_LIMIT: u64 = 1 << 62;

/// What the process can find out about the memory it may use. Each is in
/// bytes, and `None` where it is unlimited, absent or unreadable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryLimits {
    /// The machine's physical memory.
    pub physical: Option<u64>,
    /// The memory limit of the process's cgroup: the smallest set on it or
    /// on a cgroup above it.
    pub cgroup: Option<u64>,
    /// The soft `RLIMIT_AS`.
    pub address_space: Option<u64>,
}

impl MemoryLimits {
    /// Read the three from the system. Never fails: a reading that cannot
    /// be made is `None`. Read at each call, and nothing is kept.
    pub fn detect() -> Self {
        Self::read(cfg!(target_os = "linux").then(|| Path::new("/proc")))
    }

    /// As [`Self::detect`], with the cgroup files found through the procfs
    /// mounted at `proc`: `proc/self/cgroup` names the process's cgroup and
    /// `proc/self/mountinfo` where its hierarchy is mounted. `None` reads
    /// no cgroup, as on a system without them. The tests' way in.
    fn read(proc: Option<&Path>) -> Self {
        Self {
            physical: physical_memory(),
            cgroup: proc.and_then(cgroup_limit),
            address_space: address_space_limit(),
        }
    }

    /// B: the smaller of physical memory and the cgroup limit, or the one
    /// that is known. `None` where neither is.
    pub fn budget(&self) -> Option<u64> {
        match (self.physical, self.cgroup) {
            (Some(physical), Some(cgroup)) => Some(physical.min(cgroup)),
            (physical, cgroup) => physical.or(cgroup),
        }
    }

    /// The main stack's default cap: min(8 GiB, 25%·B), and at most a
    /// quarter of `RLIMIT_AS` where one is set, never below
    /// [`STACK_CAP_FLOOR`]. Unknown memory leaves the 8 GiB, and a limit too
    /// small for the floor gives the floor.
    pub fn default_stack_cap(&self) -> u64 {
        let mut cap = STACK_CAP_CEILING;
        if let Some(budget) = self.budget() {
            cap = cap.min(budget / STACK_SHARE);
        }
        if let Some(address_space) = self.address_space {
            cap = cap.min(address_space / STACK_SHARE);
        }
        cap.max(STACK_CAP_FLOOR)
    }
}

/// The machine's physical memory: `hw.memsize`.
#[cfg(target_os = "macos")]
fn physical_memory() -> Option<u64> {
    let mut bytes: u64 = 0;
    let mut len = size_of::<u64>();
    // SAFETY: the name is a NUL-terminated string; `sysctlbyname` writes at
    // most `len` bytes, the size of `bytes`, which lives for the call, and
    // the count it wrote into `len`; nothing is set.
    let status = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut bytes).cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (status == 0 && len == size_of::<u64>() && bytes != 0).then_some(bytes)
}

/// The machine's physical memory: its page count times its page size.
#[cfg(target_os = "linux")]
fn physical_memory() -> Option<u64> {
    // SAFETY: `sysconf` reads a configuration value.
    let (pages, page) = unsafe {
        (
            libc::sysconf(libc::_SC_PHYS_PAGES),
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    let pages = u64::try_from(pages).ok().filter(|&pages| pages != 0)?;
    let page = u64::try_from(page).ok().filter(|&page| page != 0)?;
    pages.checked_mul(page)
}

/// No physical memory to read.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn physical_memory() -> Option<u64> {
    None
}

/// The soft `RLIMIT_AS`, or `None` where it is unlimited or cannot be read.
#[cfg(unix)]
fn address_space_limit() -> Option<u64> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `getrlimit` writes the one `rlimit` it is given, which lives
    // for the call, and reads nothing else.
    let status = unsafe { libc::getrlimit(libc::RLIMIT_AS, &mut limit) };
    if status != 0 || limit.rlim_cur == libc::RLIM_INFINITY {
        return None;
    }
    bytes(limit.rlim_cur)
}

/// No address-space limit to read.
#[cfg(not(unix))]
fn address_space_limit() -> Option<u64> {
    None
}

/// A count the system gave in a type of its own (`rlim_t`, which is not
/// `u64` on every Unix), as bytes.
#[cfg(unix)]
fn bytes<T: TryInto<u64>>(count: T) -> Option<u64> {
    count.try_into().ok()
}

/// The memory limit of the process's cgroup, through the procfs at `proc`:
/// the smallest limit set on the cgroup `proc/self/cgroup` names or on a
/// cgroup above it, up to where `proc/self/mountinfo` says its hierarchy is
/// mounted. `None` where there is no cgroup, no limit, or a file that
/// cannot be read or does not parse.
///
/// A process is in one cgroup of the v2 hierarchy (`0::/path`) and one of
/// each v1 hierarchy (`7:memory:/path`). Where a v1 hierarchy has the
/// memory controller, the limits are its `memory.limit_in_bytes`, as on a
/// hybrid system; otherwise they are the v2 hierarchy's `memory.max`.
fn cgroup_limit(proc: &Path) -> Option<u64> {
    let membership = std::fs::read_to_string(proc.join("self/cgroup")).ok()?;
    let mountinfo = std::fs::read_to_string(proc.join("self/mountinfo")).ok()?;
    let (mut v1, mut v2) = (None, None);
    for line in membership.lines() {
        let mut fields = line.splitn(3, ':');
        let (Some(hierarchy), Some(controllers), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if controllers.split(',').any(|name| name == "memory") {
            v1 = v1.or(Some(path));
        } else if hierarchy == "0" && controllers.is_empty() {
            v2 = v2.or(Some(path));
        }
    }
    let (path, file, mount) = match (v1, v2) {
        (Some(path), _) => (
            path,
            "memory.limit_in_bytes",
            cgroup_mount(&mountinfo, |fstype, options| {
                fstype == "cgroup" && options.split(',').any(|name| name == "memory")
            })?,
        ),
        (None, Some(path)) => (
            path,
            "memory.max",
            cgroup_mount(&mountinfo, |fstype, _| fstype == "cgroup2")?,
        ),
        (None, None) => return None,
    };

    // The cgroup's directory: the mount point, then the cgroup's path below
    // the mount's root. A path that is not below the root, or that climbs
    // out of it (`/..`, a cgroup outside the process's cgroup namespace),
    // has no directory under this mount but the mount point itself.
    let below = if mount.root == "/" {
        path
    } else {
        match path.strip_prefix(mount.root.as_str()) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => rest,
            _ => "",
        }
    };
    let mut dir = mount.point.clone();
    if below.split('/').all(|name| name != "..") {
        dir.extend(
            below
                .split('/')
                .filter(|name| !name.is_empty() && *name != "."),
        );
    }

    // The cgroup's own limit and each ancestor's, up to the mount point.
    // `dir` is the mount point with names pushed, so the walk ends there.
    let mut limit: Option<u64> = None;
    loop {
        if let Some(bytes) = cgroup_limit_file(&dir.join(file)) {
            limit = Some(limit.map_or(bytes, |least| least.min(bytes)));
        }
        if dir == mount.point || !dir.pop() {
            return limit;
        }
    }
}

/// One limit file: a decimal byte count, or `max` (v2) or a count no
/// machine has (v1) for no limit.
fn cgroup_limit_file(file: &Path) -> Option<u64> {
    let text = std::fs::read_to_string(file).ok()?;
    let bytes: u64 = text.trim().parse().ok()?;
    (bytes < CGROUP_NO_LIMIT).then_some(bytes)
}

/// Where a cgroup hierarchy is mounted, and the cgroup its mount shows as
/// its root.
struct CgroupMount {
    root: String,
    point: PathBuf,
}

/// The first mount in `mountinfo` whose filesystem type and super options
/// `wanted` accepts. A line is `id parent major:minor root point options
/// [optional…] - fstype source super-options` (proc(5)).
fn cgroup_mount(mountinfo: &str, wanted: impl Fn(&str, &str) -> bool) -> Option<CgroupMount> {
    mountinfo.lines().find_map(|line| {
        let (mount, filesystem) = line.split_once(" - ")?;
        let mut filesystem = filesystem.split(' ');
        let (fstype, _source, options) =
            (filesystem.next()?, filesystem.next()?, filesystem.next()?);
        if !wanted(fstype, options) {
            return None;
        }
        let mut mount = mount.split(' ').skip(3);
        let (root, point) = (mount.next()?, mount.next()?);
        Some(CgroupMount {
            root: unescape(root)?,
            point: PathBuf::from(unescape(point)?),
        })
    })
}

/// A `mountinfo` path field with its escapes undone: the kernel writes a
/// space, a tab, a newline and a backslash as `\040`, `\011`, `\012` and
/// `\134`, a backslash and three octal digits. `None` where the result is
/// not UTF-8.
fn unescape(field: &str) -> Option<String> {
    let escaped = field.as_bytes();
    let mut plain = Vec::with_capacity(escaped.len());
    let mut at = 0;
    while at < escaped.len() {
        let octal = (escaped[at] == b'\\')
            .then(|| escaped.get(at + 1..at + 4))
            .flatten()
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u8::from_str_radix(digits, 8).ok());
        match octal {
            Some(byte) => {
                plain.push(byte);
                at += 4;
            }
            None => {
                plain.push(escaped[at]);
                at += 1;
            }
        }
    }
    String::from_utf8(plain).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIB: u64 = 1 << 10;
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;

    fn limits(
        physical: Option<u64>,
        cgroup: Option<u64>,
        address_space: Option<u64>,
    ) -> MemoryLimits {
        MemoryLimits {
            physical,
            cgroup,
            address_space,
        }
    }

    /// B is the smaller of physical memory and the cgroup limit, and the
    /// one that is known where only one is.
    #[test]
    fn the_budget_is_the_smaller_of_memory_and_the_cgroup_limit() {
        assert_eq!(limits(None, None, None).budget(), None);
        assert_eq!(limits(Some(24 * GIB), None, None).budget(), Some(24 * GIB));
        assert_eq!(limits(None, Some(GIB), None).budget(), Some(GIB));
        assert_eq!(
            limits(Some(24 * GIB), Some(GIB), None).budget(),
            Some(GIB),
            "a container's limit below the host's memory"
        );
        assert_eq!(
            limits(Some(4 * GIB), Some(64 * GIB), None).budget(),
            Some(4 * GIB),
            "a limit above the machine's memory"
        );
        // The address-space limit is not memory, and B does not read it.
        assert_eq!(
            limits(Some(4 * GIB), None, Some(GIB)).budget(),
            Some(4 * GIB)
        );
    }

    /// The default cap, row by row: min(8 GiB, 25%·B), a quarter of
    /// `RLIMIT_AS` where one is set, and the floor.
    #[test]
    fn the_default_stack_cap_fits_the_budget_and_the_address_space() {
        let rows: &[(&str, MemoryLimits, u64)] = &[
            ("nothing known", limits(None, None, None), 8 * GIB),
            (
                "24 GiB of memory",
                limits(Some(24 * GIB), None, None),
                6 * GIB,
            ),
            (
                "32 GiB: the ceiling exactly",
                limits(Some(32 * GIB), None, None),
                8 * GIB,
            ),
            (
                "64 GiB: the ceiling",
                limits(Some(64 * GIB), None, None),
                8 * GIB,
            ),
            (
                "the 7 GB macOS runner, which still holds 10 M frames (1.37 GB)",
                limits(Some(7_000_000_000), None, None),
                1_750_000_000,
            ),
            (
                "a 512 MiB container on a 64 GiB host",
                limits(Some(64 * GIB), Some(512 * MIB), None),
                128 * MIB,
            ),
            (
                "a cgroup limit above the machine's memory",
                limits(Some(4 * GIB), Some(64 * GIB), None),
                GIB,
            ),
            (
                "only a cgroup limit",
                limits(None, Some(2 * GIB), None),
                512 * MIB,
            ),
            (
                "ulimit -v 4 GiB on a 64 GiB host",
                limits(Some(64 * GIB), None, Some(4 * GIB)),
                GIB,
            ),
            (
                "an address-space limit above what B allows",
                limits(Some(4 * GIB), None, Some(64 * GIB)),
                GIB,
            ),
            (
                "only an address-space limit",
                limits(None, None, Some(16 * GIB)),
                4 * GIB,
            ),
            (
                "the three together: the address space is the least",
                limits(Some(64 * GIB), Some(8 * GIB), Some(GIB)),
                256 * MIB,
            ),
            (
                "an address-space limit too small for the floor",
                limits(Some(64 * GIB), None, Some(4 * MIB)),
                STACK_CAP_FLOOR,
            ),
            (
                "the floor's own quarter",
                limits(Some(8 * MIB), None, None),
                STACK_CAP_FLOOR,
            ),
            (
                "a cgroup limit of 0",
                limits(Some(64 * GIB), Some(0), None),
                STACK_CAP_FLOOR,
            ),
            (
                "an address-space limit of 0",
                limits(None, None, Some(0)),
                STACK_CAP_FLOOR,
            ),
            (
                "no overflow at the top",
                limits(Some(u64::MAX), Some(u64::MAX), Some(u64::MAX)),
                8 * GIB,
            ),
        ];
        for (what, limits, cap) in rows {
            assert_eq!(limits.default_stack_cap(), *cap, "{what}: {limits:?}");
        }
    }

    /// Over a sweep of sizes on both sides of every bound: the cap is never
    /// below the floor nor above the ceiling, never above a quarter of B or
    /// of the address space unless that quarter is below the floor, and it
    /// is the ceiling exactly where no reading binds.
    #[test]
    fn the_default_stack_cap_stays_within_its_bounds() {
        let mut sizes = vec![None, Some(0), Some(1), Some(u64::MAX)];
        for shift in [10, 20, 21, 22, 23, 30, 32, 33, 35, 36, 40, 62, 63] {
            let size = 1u64 << shift;
            sizes.extend([
                Some(size - 1),
                Some(size),
                Some(size + 1),
                Some(size / 3 * 5),
            ]);
        }
        for &physical in &sizes {
            for &cgroup in &sizes {
                for &address_space in &sizes {
                    let limits = limits(physical, cgroup, address_space);
                    let cap = limits.default_stack_cap();
                    assert!(
                        (STACK_CAP_FLOOR..=STACK_CAP_CEILING).contains(&cap),
                        "{limits:?}: {cap}"
                    );
                    let quarters = [limits.budget(), address_space].map(|size| size.map(|n| n / 4));
                    let binding = quarters.into_iter().flatten().min();
                    let expected = binding
                        .map_or(STACK_CAP_CEILING, |least| least.min(STACK_CAP_CEILING))
                        .max(STACK_CAP_FLOOR);
                    assert_eq!(cap, expected, "{limits:?}");
                }
            }
        }
    }

    /// What this machine answers: its physical memory is known on the two
    /// systems the detection reads it on, and is at least the 64 MiB no
    /// machine that builds this has less than; the default cap is within
    /// its bounds and no more than a quarter of that memory.
    #[test]
    fn detection_reads_this_machine() {
        let limits = MemoryLimits::detect();
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            let physical = limits.physical.expect("physical memory");
            assert!(physical >= 64 * MIB, "{limits:?}");
            assert!(limits.default_stack_cap() <= (physical / 4).max(STACK_CAP_FLOOR));
        }
        if !cfg!(target_os = "linux") {
            assert_eq!(limits.cgroup, None, "cgroups are Linux's");
        }
        let cap = limits.default_stack_cap();
        assert!(
            (STACK_CAP_FLOOR..=STACK_CAP_CEILING).contains(&cap),
            "{limits:?}"
        );
    }

    /// A procfs and a cgroup hierarchy of a test's own, under the system's
    /// temporary directory: `proc/self/cgroup`, `proc/self/mountinfo`, and
    /// the limit files the test writes under the mount points it names.
    /// Removed when dropped.
    struct Tree {
        dir: PathBuf,
    }

    impl Tree {
        /// `membership` is `proc/self/cgroup`'s text, and `mountinfo`
        /// `proc/self/mountinfo`'s, with each `{}` replaced by this tree's
        /// directory, so that a mount point lies inside it.
        fn new(name: &str, membership: &str, mountinfo: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("patina-budget-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let tree = Self { dir };
            // As the kernel writes a path's space.
            let here = (tree.dir.to_str())
                .expect("a UTF-8 temporary directory")
                .replace(' ', "\\040");
            tree.write("proc/self/cgroup", membership);
            tree.write("proc/self/mountinfo", &mountinfo.replace("{}", &here));
            tree
        }

        fn write(&self, file: &str, text: &str) {
            let file = self.dir.join(file);
            std::fs::create_dir_all(file.parent().expect("a file in a directory")).unwrap();
            std::fs::write(file, text).unwrap();
        }

        fn proc(&self) -> PathBuf {
            self.dir.join("proc")
        }

        fn limit(&self) -> Option<u64> {
            cgroup_limit(&self.proc())
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A v2 hierarchy mounted at the tree's `cg`, among other mounts.
    const V2_MOUNTS: &str = "\
        22 28 0:21 / /sys rw,nosuid,nodev,noexec,relatime shared:7 - sysfs sysfs rw\n\
        23 28 0:22 / /proc rw,nosuid,nodev,noexec,relatime shared:13 - proc proc rw\n\
        31 22 0:26 / {}/cg rw,nosuid,nodev,noexec,relatime shared:9 - cgroup2 cgroup2 rw,nsdelegate,memory_recursiveprot\n";

    /// Cgroup v2: the process's own `memory.max`; `max` and a missing file
    /// are no limit; and an ancestor's limit binds as the cgroup's own
    /// does, the smallest of them answering.
    #[test]
    fn a_v2_cgroup_s_limit_is_the_least_of_its_own_and_its_ancestors() {
        let membership = "0::/user.slice/user-1000.slice/session-3.scope\n";
        let tree = Tree::new("v2", membership, V2_MOUNTS);
        assert_eq!(tree.limit(), None, "no limit file at all");

        let own = "cg/user.slice/user-1000.slice/session-3.scope/memory.max";
        tree.write(own, "max\n");
        assert_eq!(tree.limit(), None, "max is no limit");

        tree.write(own, "536870912\n");
        assert_eq!(tree.limit(), Some(512 * MIB));

        tree.write("cg/user.slice/memory.max", "268435456\n");
        assert_eq!(tree.limit(), Some(256 * MIB), "a lower limit two levels up");

        tree.write("cg/user.slice/user-1000.slice/memory.max", "max\n");
        tree.write(own, "max\n");
        assert_eq!(tree.limit(), Some(256 * MIB), "an ancestor's alone");

        tree.write(own, "1048576\n");
        assert_eq!(tree.limit(), Some(MIB), "the cgroup's own, the least");

        // A limit file at the mount point is read too: a container's root
        // cgroup is its mount point, and carries the container's limit.
        tree.write("cg/memory.max", "4096\n");
        assert_eq!(tree.limit(), Some(4 * KIB));

        // A sibling's limit is nobody's.
        tree.write("cg/system.slice/memory.max", "1\n");
        assert_eq!(tree.limit(), Some(4 * KIB));
    }

    /// A container's view: its cgroup is `/`, and the limit file is at the
    /// mount point. And a mount whose root is the container's cgroup on the
    /// host, as without a cgroup namespace: the path below that root.
    #[test]
    fn a_v2_cgroup_is_found_below_its_mount_s_root() {
        let tree = Tree::new("v2-root", "0::/\n", V2_MOUNTS);
        tree.write("cg/memory.max", "2147483648\n");
        assert_eq!(tree.limit(), Some(2 * GIB));

        let mounts = "31 22 0:26 /docker/abc {}/cg rw,relatime - cgroup2 cgroup2 rw\n";
        let tree = Tree::new("v2-below", "0::/docker/abc/worker\n", mounts);
        tree.write("cg/worker/memory.max", "1073741824\n");
        tree.write("cg/memory.max", "3221225472\n");
        assert_eq!(tree.limit(), Some(GIB));

        // The mount's root itself.
        let tree = Tree::new("v2-at", "0::/docker/abc\n", mounts);
        tree.write("cg/memory.max", "3221225472\n");
        assert_eq!(tree.limit(), Some(3 * GIB));

        // A cgroup that is not below the mount's root, and one that climbs
        // out of its namespace: the mount point's limit, and no file read
        // outside the mount.
        for (name, membership) in [
            ("v2-beside", "0::/docker/abcdef\n"),
            ("v2-elsewhere", "0::/kubepods/pod1\n"),
            ("v2-climbs", "0::/../../other\n"),
        ] {
            let tree = Tree::new(name, membership, mounts);
            tree.write("cg/memory.max", "3221225472\n");
            tree.write("other/memory.max", "1\n");
            tree.write("cg/other/memory.max", "1\n");
            tree.write("cg/kubepods/pod1/memory.max", "1\n");
            tree.write("cg/docker/abcdef/memory.max", "1\n");
            tree.write("cg/def/memory.max", "1\n");
            assert_eq!(tree.limit(), Some(3 * GIB), "{membership}");
        }
        let tree = Tree::new("v2-climbs-root", "0::/../other\n", V2_MOUNTS);
        tree.write("cg/memory.max", "3221225472\n");
        tree.write("other/memory.max", "1\n");
        tree.write("cg/other/memory.max", "1\n");
        assert_eq!(tree.limit(), Some(3 * GIB), "below a root of /");
    }

    /// Cgroup v1, and a hybrid system: the memory controller's hierarchy,
    /// found by the controller's name in its list and in its mount's super
    /// options; its largest count is no limit; and where a v1 hierarchy has
    /// the memory controller, the v2 hierarchy's files are not the limit.
    #[test]
    fn a_v1_memory_cgroup_s_limit_is_read_from_its_own_hierarchy() {
        let membership = "\
            12:pids:/user.slice\n\
            7:cpu,cpuacct:/elsewhere\n\
            6:name=memoryless:/wrong\n\
            5:memory:/docker/abc\n\
            1:name=systemd:/user.slice/session.scope\n\
            0::/user.slice/session.scope\n";
        let mounts = "\
            30 22 0:25 / {}/unified rw,nosuid - cgroup2 cgroup2 rw,nsdelegate\n\
            33 29 0:28 / {}/cpu rw,nosuid - cgroup cgroup rw,cpu,cpuacct\n\
            35 29 0:30 / {}/mem rw,nosuid - cgroup cgroup rw,memory\n";
        let tree = Tree::new("v1", membership, mounts);
        tree.write("unified/user.slice/session.scope/memory.max", "1\n");
        tree.write("cpu/elsewhere/memory.limit_in_bytes", "1\n");
        assert_eq!(tree.limit(), None);

        let own = "mem/docker/abc/memory.limit_in_bytes";
        tree.write(own, "9223372036854771712\n");
        assert_eq!(tree.limit(), None, "v1's largest count is no limit");

        tree.write(own, "805306368\n");
        assert_eq!(tree.limit(), Some(768 * MIB));

        tree.write("mem/docker/memory.limit_in_bytes", "104857600\n");
        assert_eq!(tree.limit(), Some(100 * MIB), "an ancestor's lower limit");

        // A v1 membership with no memory controller mounted: no limit, and
        // the v2 hierarchy is not consulted in its place.
        let tree = Tree::new("v1-unmounted", membership, V2_MOUNTS);
        tree.write("cg/user.slice/session.scope/memory.max", "1\n");
        assert_eq!(tree.limit(), None);
    }

    /// Whatever the files hold, detection answers, and `None` where they
    /// name no cgroup hierarchy: a missing procfs, files that are not what
    /// procfs writes, a limit that is not a number.
    #[test]
    fn a_cgroup_that_cannot_be_read_is_no_limit() {
        let missing = std::env::temp_dir().join("patina-budget-no-such-procfs");
        assert_eq!(cgroup_limit(&missing), None);
        assert_eq!(MemoryLimits::read(Some(&missing)).cgroup, None);

        for (name, membership, mounts) in [
            ("empty", "", ""),
            ("no-mounts", "0::/a\n", ""),
            ("no-membership", "", V2_MOUNTS),
            (
                "garbage",
                "not a cgroup line\n::\n0\n",
                "nor a mount line\n - \n1 2 3 - cgroup2\n",
            ),
            (
                "short-mount",
                "0::/a\n",
                "31 22 0:26 / - cgroup2 cgroup2 rw\n",
            ),
            (
                "other-fs",
                "0::/a\n",
                "31 22 0:26 / {}/cg rw - tmpfs tmpfs rw\n",
            ),
        ] {
            let tree = Tree::new(name, membership, mounts);
            tree.write("cg/a/memory.max", "1024\n");
            assert_eq!(tree.limit(), None, "{name}");
        }

        let tree = Tree::new("not-a-number", "0::/a\n", V2_MOUNTS);
        for text in [
            "",
            "\n",
            "lots\n",
            "-1\n",
            "1e9\n",
            "99999999999999999999999\n",
        ] {
            tree.write("cg/a/memory.max", text);
            assert_eq!(tree.limit(), None, "{text:?}");
        }
        tree.write("cg/a/memory.max", "  4096  \n");
        assert_eq!(tree.limit(), Some(4 * KIB), "surrounding blanks");
    }

    /// A mount point with a space in it, which `mountinfo` writes `\040`.
    #[test]
    fn a_mount_point_s_escapes_are_undone() {
        assert_eq!(
            unescape("/sys/fs/cgroup").as_deref(),
            Some("/sys/fs/cgroup")
        );
        assert_eq!(
            unescape(r"/a\040b\011c\012d\134e").as_deref(),
            Some("/a b\tc\nd\\e")
        );
        assert_eq!(
            unescape(r"\04").as_deref(),
            Some(r"\04"),
            "a short escape is itself"
        );
        assert_eq!(
            unescape(r"a\x40\999").as_deref(),
            Some(r"a\x40\999"),
            "not octal"
        );
        assert_eq!(unescape(r"\377"), None, "not UTF-8");

        let mounts = "31 22 0:26 / {}/my\\040cgroups rw - cgroup2 cgroup2 rw\n";
        let tree = Tree::new("escaped", "0::/a\n", mounts);
        tree.write("my cgroups/a/memory.max", "8192\n");
        assert_eq!(tree.limit(), Some(8 * KIB));
    }

    /// The environment variables through which
    /// `detection_reads_a_real_address_space_limit_and_a_cgroup_file`
    /// tells its child which procfs to read and where to write what it
    /// read. Test code reads them, never the detection.
    const CHILD_PROC: &str = "PATINA_BUDGET_TEST_PROC";
    const CHILD_REPORT: &str = "PATINA_BUDGET_TEST_REPORT";

    /// The child of
    /// `detection_reads_a_real_address_space_limit_and_a_cgroup_file`: run by
    /// name in a process of its own, under the limit its parent set, it
    /// reads the limits and writes them where the parent asked. Run with the
    /// rest (`--include-ignored`) it has nothing to do.
    #[test]
    #[ignore = "the child process of detection_reads_a_real_address_space_limit_and_a_cgroup_file"]
    fn report_the_limits_to_the_parent() {
        let (Some(proc), Some(report)) =
            (std::env::var_os(CHILD_PROC), std::env::var_os(CHILD_REPORT))
        else {
            return;
        };
        let limits = MemoryLimits::read(Some(Path::new(&proc)));
        let field = |size: Option<u64>| size.map_or("-".to_string(), |n| n.to_string());
        let text = format!(
            "{} {} {} {}\n",
            field(limits.physical),
            field(limits.cgroup),
            field(limits.address_space),
            limits.default_stack_cap()
        );
        std::fs::write(report, text).unwrap();
    }

    /// On Linux, the detection in a process that really is under the limits
    /// (`PRD/GC_PRD.md` §17.3): this test's own binary, run again as a
    /// child under `ulimit -v` of 4 GiB, with its cgroup files injected.
    /// The child reads its soft `RLIMIT_AS` as the limit set, and its
    /// default stack cap fits under a quarter of it; with a 512 MiB cgroup
    /// limit injected as well, the cap is a quarter of that.
    ///
    /// Linux only: macOS refuses to set `RLIMIT_AS`.
    #[test]
    #[cfg(target_os = "linux")]
    fn detection_reads_a_real_address_space_limit_and_a_cgroup_file() {
        let run = |name: &str, max: &str| -> (Option<u64>, Option<u64>, Option<u64>, u64) {
            let tree = Tree::new(name, "0::/job\n", V2_MOUNTS);
            tree.write("cg/job/memory.max", max);
            let report = tree.dir.join("report");
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(r#"ulimit -S -v 4194304 && exec "$0" "$@""#)
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "heap::budget::tests::report_the_limits_to_the_parent",
                    "--ignored",
                    "--test-threads=1",
                ])
                .env(CHILD_PROC, tree.proc())
                .env(CHILD_REPORT, &report)
                .status()
                .unwrap();
            assert!(status.success(), "{name}: the child, {status}");
            let text = std::fs::read_to_string(&report).unwrap();
            let fields: Vec<Option<u64>> = text
                .split_whitespace()
                .map(|field| field.parse().ok())
                .collect();
            assert_eq!(fields.len(), 4, "{name}: {text:?}");
            (fields[0], fields[1], fields[2], fields[3].expect("the cap"))
        };

        let (physical, cgroup, address_space, cap) = run("child-max", "max\n");
        let physical = physical.expect("physical memory");
        assert_eq!(cgroup, None);
        assert_eq!(address_space, Some(4 * GIB), "ulimit -v counts KiB");
        assert_eq!(cap, GIB.min(physical / 4).max(STACK_CAP_FLOOR));
        assert!(cap <= GIB, "under a quarter of the address space");

        let (physical, cgroup, address_space, cap) = run("child-512m", "536870912\n");
        assert_eq!(cgroup, Some(512 * MIB));
        assert_eq!(address_space, Some(4 * GIB));
        let budget = physical.expect("physical memory").min(512 * MIB);
        assert_eq!(cap, (budget / 4).max(STACK_CAP_FLOOR));
    }
}
