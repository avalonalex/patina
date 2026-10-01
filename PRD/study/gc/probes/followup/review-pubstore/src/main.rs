//! JIT-like list-building loops with `ap` in a register, streaming through a window.
//! Question: does "publish at the store, not at allocation" avoid the M4's fence tail
//! when the publishing store is a set-cdr! of the previous fresh pair (tail-building)?
use std::arch::asm;
use std::time::Instant;

unsafe extern "C" {
    fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
}

macro_rules! loop_fn {
    ($name:ident, $body:literal) => {
        #[inline(never)]
        fn $name(base: *mut u64, old: *mut u64, n: u64, mask: u64) {
            unsafe {
                asm!(
                    "mov {i}, #0",
                    "mov {prev}, {old}",          // first holder: an old pair
                    "2:",
                    "and {off}, {i}, {mask}",
                    "add {p}, {base}, {off}",     // bump: p = ap
                    $body,
                    "add {i}, {i}, #16",
                    "subs {n}, {n}, #1",
                    "b.ne 2b",
                    base = in(reg) base,
                    old = in(reg) old,
                    mask = in(reg) mask,
                    n = inout(reg) n => _,
                    i = out(reg) _,
                    off = out(reg) _,
                    p = out(reg) _,
                    prev = out(reg) _,
                    t = out(reg) _,
                    options(nostack)
                );
            }
        }
    };
}
// A: cons onto a list (initializing stores only; the new pair points at prev). No publication store.
loop_fn!(a_cons, "// {t}\n stp {i}, {prev}, [{p}]\n mov {prev}, {p}");
// A+: same, DESIGN §12 placement: one dmb ishst per allocation group.
loop_fn!(a_cons_allocfence, "// {t}\n stp {i}, {prev}, [{p}]\n dmb ishst\n mov {prev}, {p}");
// B: tail-build: init new pair (car, nil), then set-cdr! of the previous pair with the new pair (plain str).
loop_fn!(b_tail_str, "// {t}\n stp {i}, xzr, [{p}]\n str {p}, [{prev}, #8]\n mov {prev}, {p}");
// C: tail-build with the ANSWER's rule 4: release store of the heap value.
loop_fn!(c_tail_stlr, "stp {i}, xzr, [{p}]\n add {t}, {prev}, #8\n stlr {p}, [{t}]\n mov {prev}, {p}");
// D: tail-build with Chez's placement: dmb ishst before the heap-valued store.
loop_fn!(d_tail_ishst, "// {t}\n stp {i}, xzr, [{p}]\n dmb ishst\n str {p}, [{prev}, #8]\n mov {prev}, {p}");
// E: vecset-like: fresh pair stored into a fixed old slot (the Chez vecset/setcar shape), release store.
loop_fn!(e_old_stlr, "stp {i}, xzr, [{p}]\n add {t}, {old}, #8\n stlr {p}, [{t}]");
loop_fn!(e_old_str, "// {t} {prev}\n stp {i}, xzr, [{p}]\n str {p}, [{old}, #8]");


// F: fresh pair into the fixed old slot, Chez placement (dmb ishst; str).
loop_fn!(f_old_ishst, "// {t} {prev}\n stp {i}, xzr, [{p}]\n dmb ishst\n str {p}, [{old}, #8]");
// G: fresh pair into old slots cycling over a 10 K-element old vector (80 KB): str / stlr / dmb ishst;str.
loop_fn!(g_vec_str, "// {prev}\n stp {i}, xzr, [{p}]\n and {t}, {i}, #0x1fff0\n add {t}, {t}, {old}\n str {p}, [{t}]");
loop_fn!(g_vec_stlr, "// {prev}\n stp {i}, xzr, [{p}]\n and {t}, {i}, #0x1fff0\n add {t}, {t}, {old}\n stlr {p}, [{t}]");
loop_fn!(g_vec_ishst, "// {prev}\n stp {i}, xzr, [{p}]\n and {t}, {i}, #0x1fff0\n add {t}, {t}, {old}\n dmb ishst\n str {p}, [{t}]");

fn main() {
    unsafe { pthread_set_qos_class_self_np(0x21, 0) };
    let mut buf = vec![1u64; 64 * 1024 * 1024 / 8];
    let mut old = vec![0u64; 0x20000/8 + 16];
    let base = buf.as_mut_ptr();
    let oldp = old.as_mut_ptr();
    let windows: [u64; 6] = [8 << 10, 32 << 10, 512 << 10, 4 << 20, 16 << 20, 64 << 20];
    let fns: [(&str, fn(*mut u64, *mut u64, u64, u64)); 11] = [
        ("A cons (init only)", a_cons),
        ("A cons + dmb ishst per alloc (DESIGN 12)", a_cons_allocfence),
        ("B tail set-cdr! str", b_tail_str),
        ("C tail set-cdr! stlr (rule 4)", c_tail_stlr),
        ("D tail set-cdr! dmb ishst;str (Chez)", d_tail_ishst),
        ("E fresh into old slot str", e_old_str),
        ("E fresh into old slot stlr", e_old_stlr),
        ("F fresh into old slot dmb ishst;str (Chez)", f_old_ishst),
        ("G fresh into old 128KB vector str", g_vec_str),
        ("G fresh into old 128KB vector stlr (rule 4)", g_vec_stlr),
        ("G fresh into old 128KB vector dmb ishst;str (Chez)", g_vec_ishst),
    ];
    print!("ns/iter (median of 9)");
    for w in windows {
        if w < (1 << 20) { print!("\t{}KiB", w >> 10) } else { print!("\t{}MiB", w >> 20) }
    }
    println!();
    for (name, f) in fns {
        print!("{name}");
        for w in windows {
            let n = 2_000_000u64;
            f(base, oldp, n, w - 1);
            let mut v = Vec::new();
            for _ in 0..9 {
                let t = Instant::now();
                f(base, oldp, n, w - 1);
                v.push(t.elapsed().as_nanos() as f64 / n as f64);
            }
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            print!("\t{:.2}", v[4]);
        }
        println!();
    }
    std::hint::black_box(&buf);
    std::hint::black_box(&old);
}
