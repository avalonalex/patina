//! Barrier-cost sweep on the M4 Pro: what makes `dmb ishst` / `dmb ish` / `stlr`
//! expensive. Every loop is hand-written asm so the compiler cannot move anything.
use std::arch::asm;
use std::time::Instant;

unsafe extern "C" {
    fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
}

macro_rules! sweep_fn {
    ($name:ident, $body:literal) => {
        #[inline(never)]
        fn $name(base: *mut u64, hot: *mut u64, n: u64, mask: u64) {
            unsafe {
                asm!(
                    "// {hot}",
                    "mov {i}, #0",
                    "2:",
                    "and {off}, {i}, {mask}",
                    "add {p}, {base}, {off}",
                    $body,
                    "add {i}, {i}, #16",
                    "subs {n}, {n}, #1",
                    "b.ne 2b",
                    base = in(reg) base,
                    hot = in(reg) hot,
                    mask = in(reg) mask,
                    n = inout(reg) n => _,
                    i = out(reg) _,
                    off = out(reg) _,
                    p = out(reg) _,
                    options(nostack)
                );
            }
        }
    };
}
sweep_fn!(s_stp, "stp {i}, {i}, [{p}]");
sweep_fn!(s_stp_ishst, "stp {i}, {i}, [{p}]\n dmb ishst");
sweep_fn!(s_stp_ish, "stp {i}, {i}, [{p}]\n dmb ish");
sweep_fn!(s_str2_ishst, "str {i}, [{p}]\n str {i}, [{p}, #8]\n dmb ishst");
sweep_fn!(s_str_stlr, "str {i}, [{p}]\n add {p}, {p}, #8\n stlr {i}, [{p}]");
sweep_fn!(s_stp_ishst_hot, "stp {i}, {i}, [{p}]\n dmb ishst\n str {i}, [{hot}]");
sweep_fn!(s_stp_hot_stlr, "stp {i}, {i}, [{p}]\n stlr {i}, [{hot}]");
sweep_fn!(s_stp_delay8, "stp {i}, {i}, [{p}]\n add {off}, {off}, #1\n add {off}, {off}, #1\n add {off}, {off}, #1\n add {off}, {off}, #1\n add {off}, {off}, #1\n add {off}, {off}, #1\n add {off}, {off}, #1\n add {off}, {off}, #1\n str {off}, [{hot}]");
sweep_fn!(s_stp_delay8_ishst, "stp {i}, {i}, [{p}]\n add {off}, {off}, #1\n add {off}, {off}, #1\n add {off}, {off}, #1\n add {off}, {off}, #1\n add {off}, {off}, #1\n add {off}, {off}, #1\n add {off}, {off}, #1\n add {off}, {off}, #1\n dmb ishst\n str {off}, [{hot}]");
sweep_fn!(s_bumpmem, "ldr {off}, [{hot}, #64]\n add {off}, {off}, #16\n str {off}, [{hot}, #64]\n and {off}, {off}, {mask}\n add {p}, {base}, {off}\n stp {i}, {i}, [{p}]\n orr {p}, {p}, #4\n str {p}, [{hot}]");
sweep_fn!(s_bumpmem_ishst, "ldr {off}, [{hot}, #64]\n add {off}, {off}, #16\n str {off}, [{hot}, #64]\n and {off}, {off}, {mask}\n add {p}, {base}, {off}\n stp {i}, {i}, [{p}]\n orr {p}, {p}, #4\n dmb ishst\n str {p}, [{hot}]");
sweep_fn!(s_stp_ldr_hot_ishst, "stp {i}, {i}, [{p}]\n ldr {off}, [{hot}]\n dmb ishst\n str {off}, [{hot}, #8]");

fn main() {
    unsafe { pthread_set_qos_class_self_np(0x21, 0) };
    let mut buf = vec![1u64; 64 * 1024 * 1024 / 8];
    let mut hot = vec![0u64; 16];
    let base = buf.as_mut_ptr();
    let hotp = hot.as_mut_ptr();
    let windows: [u64; 9] = [16, 128, 1024, 8 << 10, 64 << 10, 512 << 10, 4 << 20, 16 << 20, 64 << 20];
    let fns: [(&str, fn(*mut u64, *mut u64, u64, u64)); 12] = [
        ("stp", s_stp),
        ("stp;dmb ishst", s_stp_ishst),
        ("stp;dmb ish", s_stp_ish),
        ("str;str;dmb ishst", s_str2_ishst),
        ("str;stlr", s_str_stlr),
        ("stp;dmb ishst;str hot", s_stp_ishst_hot),
        ("stp;stlr hot", s_stp_hot_stlr),
        ("stp;ldr hot;dmb ishst;str hot", s_stp_ldr_hot_ishst),
        ("stp;8 adds;str hot", s_stp_delay8),
        ("stp;8 adds;dmb ishst;str hot", s_stp_delay8_ishst),
        ("bump ptr in memory;stp;str hot", s_bumpmem),
        ("bump ptr in memory;stp;dmb ishst;str hot", s_bumpmem_ishst),
    ];
    print!("ns/iter (median of 9)\twindow:");
    for w in windows {
        if w < 1024 { print!("\t{w}B") } else if w < (1 << 20) { print!("\t{}KiB", w >> 10) } else { print!("\t{}MiB", w >> 20) }
    }
    println!();
    for (name, f) in fns {
        print!("{name}\t");
        for w in windows {
            let n = 2_000_000u64;
            f(base, hotp, n, w - 1); // warm
            let mut v = Vec::new();
            for _ in 0..9 {
                let t = Instant::now();
                f(base, hotp, n, w - 1);
                v.push(t.elapsed().as_nanos() as f64 / n as f64);
            }
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            print!("\t{:.2}", v[4]);
        }
        println!();
    }
    std::hint::black_box(&buf);
    std::hint::black_box(&hot);
}
