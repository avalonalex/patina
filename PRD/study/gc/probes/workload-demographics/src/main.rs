//! Probe: can one process reserve many large PROT_NONE regions on this machine?
use std::time::Instant;
unsafe extern "C" {
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
    fn mprotect(addr: *mut u8, len: usize, prot: i32) -> i32;
    fn getpagesize() -> i32;
    fn madvise(addr: *mut u8, len: usize, advice: i32) -> i32;
}
const PROT_NONE: i32 = 0;
const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const MAP_PRIVATE: i32 = 0x0002;
const MAP_ANON: i32 = 0x1000;
const MAP_NORESERVE: i32 = 0x0040;
const MADV_FREE: i32 = 5;
const GIB: usize = 1 << 30;

fn rss_kb() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=,vsz=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    println!("    ps rss,vsz (KiB): {}", s.trim());
    s.split_whitespace().next().unwrap_or("0").parse().unwrap_or(0)
}

fn try_reserve(n: usize, size: usize, flags: i32, label: &str) -> Vec<*mut u8> {
    let t = Instant::now();
    let mut v = Vec::new();
    for i in 0..n {
        let p = unsafe { mmap(std::ptr::null_mut(), size, PROT_NONE, flags, -1, 0) };
        if p as isize == -1 {
            println!("  {label}: FAILED at region {i} of {n} ({} GiB each), errno {}", size / GIB,
                std::io::Error::last_os_error());
            break;
        }
        v.push(p);
    }
    println!("  {label}: reserved {} x {} GiB = {} GiB in {:?}", v.len(), size / GIB, v.len() * size / GIB, t.elapsed());
    v
}

fn main() {
    let page = unsafe { getpagesize() } as usize;
    println!("page size: {page}");
    rss_kb();
    // 1. 64 x 16 GiB PROT_NONE, MAP_PRIVATE|MAP_ANON
    let regions = try_reserve(64, 16 * GIB, MAP_PRIVATE | MAP_ANON, "64x16GiB PRIVATE|ANON");
    rss_kb();
    // 2. Commit and touch 64 MiB in each of 4 regions, then release with MADV_FREE + PROT_NONE
    let t = Instant::now();
    for &p in regions.iter().take(4) {
        let len = 64 << 20;
        assert_eq!(unsafe { mprotect(p, len, PROT_READ | PROT_WRITE) }, 0);
        for off in (0..len).step_by(page) {
            unsafe { *p.add(off) = 1 };
        }
    }
    println!("  committed+touched 4 x 64 MiB in {:?}", t.elapsed());
    rss_kb();
    for &p in regions.iter().take(4) {
        let len = 64 << 20;
        unsafe {
            madvise(p, len, MADV_FREE);
            mprotect(p, len, PROT_NONE);
        }
    }
    println!("  after MADV_FREE + PROT_NONE:");
    rss_kb();
    // touch at the far end of a region (address arithmetic across 16 GiB)
    let p = regions[63];
    let far = 16 * GIB - page;
    assert_eq!(unsafe { mprotect(p.add(far), page, PROT_READ | PROT_WRITE) }, 0);
    unsafe { *p.add(far) = 7 };
    println!("  touched last page of region 63 at {:p}", unsafe { p.add(far) });
    // alignment of returned regions
    let aligned_4g = regions.iter().filter(|&&p| (p as usize) % (4 * GIB) == 0).count();
    println!("  regions 4 GiB-aligned: {aligned_4g}/{}; first {:p} last {:p}", regions.len(), regions[0], regions[63]);
    for &p in &regions {
        unsafe { munmap(p, 16 * GIB) };
    }
    // 3. How far does it go? Keep reserving 16 GiB regions until failure (cap 4096 = 64 TiB).
    let more = try_reserve(4096, 16 * GIB, MAP_PRIVATE | MAP_ANON, "until-failure 16GiB");
    rss_kb();
    for &p in &more {
        unsafe { munmap(p, 16 * GIB) };
    }
    // 4. With MAP_NORESERVE (no-op on Darwin?)
    let nr = try_reserve(64, 16 * GIB, MAP_PRIVATE | MAP_ANON | MAP_NORESERVE, "64x16GiB +NORESERVE");
    for &p in &nr {
        unsafe { munmap(p, 16 * GIB) };
    }
    // 5. One single huge reservation (1 TiB) and then 64 TiB
    for tib in [1usize, 8, 64] {
        let r = try_reserve(1, tib * 1024 * GIB, MAP_PRIVATE | MAP_ANON, &format!("1x{tib}TiB"));
        for &p in &r {
            unsafe { munmap(p, tib * 1024 * GIB) };
        }
    }
    // 6. Writable (not PROT_NONE) 64 x 16 GiB lazily-committed reservations
    let t = Instant::now();
    let mut w = Vec::new();
    for _ in 0..64 {
        let p = unsafe { mmap(std::ptr::null_mut(), 16 * GIB, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0) };
        if p as isize == -1 { println!("  RW reservation failed: {}", std::io::Error::last_os_error()); break; }
        w.push(p);
    }
    println!("  64x16GiB PROT_READ|WRITE: {} ok in {:?}", w.len(), t.elapsed());
    rss_kb();
    for &p in &w { unsafe { munmap(p, 16 * GIB) }; }
}
