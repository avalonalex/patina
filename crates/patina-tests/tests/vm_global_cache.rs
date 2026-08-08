//! Semantic tests for the per-site global inline cache (Track P P4).
//!
//! `LoadGlobal`/`StoreGlobal` cache `(env_id, slot)` per call site after the
//! first execution. These tests pin the invariant that makes that sound:
//! redefinition and `set!` overwrite the binding's slot in place, so a hit
//! can never observe a stale value. They run against `VmBackend` explicitly —
//! the cache only exists in the VM.

mod common;
use common::eval_program_vm as eval;

#[test]
fn set_bang_updates_cached_load_site() {
    // f's LoadGlobal site caches x's slot on the first call; set! writes the
    // same slot, so the second call must see the new value.
    assert_eq!(eval("(define x 1) (define (f) x) (f) (set! x 2) (f)"), "2");
}

#[test]
fn redefine_updates_cached_load_site() {
    // Top-level redefinition of the same name overwrites the slot in place;
    // the cached site must see the new binding.
    assert_eq!(
        eval("(define x 1) (define (f) x) (f) (define x 42) (f)"),
        "42"
    );
}

#[test]
fn cached_store_site_and_load_site_agree() {
    // bump's StoreGlobal site and f's LoadGlobal site both cache x's slot.
    assert_eq!(
        eval(
            "(define x 1) \
             (define (bump) (set! x (+ x 1))) \
             (define (f) x) \
             (bump) (bump) (f)"
        ),
        "3"
    );
}

#[test]
fn defines_after_cache_fill_do_not_disturb_slots() {
    // New defines append fresh slots; existing cached slots must be
    // unaffected by the map growing/rehashing around them.
    assert_eq!(
        eval(
            "(define a 10) (define (f) a) (f) \
             (define b 1) (define c 2) (define d 3) (define e 4) \
             (define g 5) (define h 6) (define i 7) (define j 8) \
             (+ (f) b c d e g h i j)"
        ),
        "46"
    );
}

#[test]
fn forward_reference_resolves_after_define() {
    // f is compiled (and its site cache created) before x exists; the first
    // call fills the cache from the then-current binding table.
    assert_eq!(eval("(define (f) x) (define x 7) (f)"), "7");
}

#[test]
fn deep_self_recursion_through_cached_global() {
    // The self-call LoadGlobal is the hot case the cache targets (tak-shaped
    // code). Make sure a long cached-hit run computes correctly.
    assert_eq!(
        eval(
            "(define (count n acc) (if (= n 0) acc (count (- n 1) (+ acc 1)))) \
             (count 100000 0)"
        ),
        "100000"
    );
}

#[test]
fn library_and_toplevel_sites_use_their_own_environments() {
    // `map` is Scheme-defined (lib/scheme/base/higher_order.scm) and its
    // body reads the internal helper `%map-cars` from the *library's*
    // globals environment via LoadGlobal. A same-named top-level define
    // lives in a different environment; the library closure's cached site
    // must keep resolving in its own environment — before and after the
    // top-level name exists.
    assert_eq!(
        eval(
            "(define r1 (map (lambda (x) (* x x)) '(1 2 3))) \
             (define %map-cars 'not-a-procedure) \
             (list r1 (map (lambda (x) (+ x 1)) '(1 2 3)) %map-cars)"
        ),
        "((1 4 9) (2 3 4) not-a-procedure)"
    );
}
