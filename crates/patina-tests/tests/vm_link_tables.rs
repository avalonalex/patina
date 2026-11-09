//! Semantic tests for the VM's linked global reads and writes (#836), moved
//! from `vm_global_cache.rs`, whose per-site cache (Track P P4) the link
//! tables replaced.
//!
//! A unit links each global it names at load, in the namespace it was
//! compiled in, and `LoadGlobal` and `StoreGlobal` read and write through the
//! record its table holds. These tests pin what makes that sound: a binding's
//! record never moves, so redefinition and `set!` write the location it names
//! (the cell its record names, #834) and a read never observes a stale value;
//! a name defined after the code that reads it was linked is read through the
//! placeholder the define adopts; and a library's code reads its own
//! namespace's bindings, not the program's. They run against `VmBackend`
//! explicitly: the link tables exist only in the VM.

mod common;
use common::eval_program_vm as eval;

#[test]
fn set_bang_updates_cached_load_site() {
    // f's LoadGlobal reads x's record, linked at f's load; set! writes the
    // cell it names, so the second call must see the new value.
    assert_eq!(eval("(define x 1) (define (f) x) (f) (set! x 2) (f)"), "2");
}

#[test]
fn redefine_updates_cached_load_site() {
    // Top-level redefinition of the same name stores into the cell its
    // record names; the linked site must see the new binding.
    assert_eq!(
        eval("(define x 1) (define (f) x) (f) (define x 42) (f)"),
        "42"
    );
}

#[test]
fn cached_store_site_and_load_site_agree() {
    // bump's StoreGlobal and f's LoadGlobal both link x's one record.
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
    // New defines append fresh slots; an existing linked record must be
    // unaffected by the map growing/rehashing around it.
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
    // f is compiled and linked before x exists, so its table links a
    // placeholder for x, which x's define adopts, keeping its word.
    assert_eq!(eval("(define (f) x) (define x 7) (f)"), "7");
}

#[test]
fn deep_self_recursion_through_cached_global() {
    // The self-call LoadGlobal is the hot case the linked read targets
    // (tak-shaped code). Make sure a long run of linked reads computes
    // correctly.
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
    // lives in a different environment; the library unit's table, linked in
    // the library's namespace at its load, must keep reading there — before
    // and after the top-level name exists.
    assert_eq!(
        eval(
            "(define r1 (map (lambda (x) (* x x)) '(1 2 3))) \
             (define %map-cars 'not-a-procedure) \
             (list r1 (map (lambda (x) (+ x 1)) '(1 2 3)) %map-cars)"
        ),
        "((1 4 9) (2 3 4) not-a-procedure)"
    );
}
