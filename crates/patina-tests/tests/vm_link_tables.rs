//! Semantic tests for the VM's linked global reads and writes (#836), moved
//! from `vm_global_cache.rs`, whose per-site cache (Track P P4) the link
//! tables replaced.
//!
//! A unit links each global it names at load, in the namespace it was
//! compiled in, and `LoadGlobal` and `StoreGlobal` read and write through the
//! record its table holds. These tests pin what makes that sound: a binding's
//! record never moves, so redefinition and `set!` write the location it names
//! (the cell its record names, #834) and a read never observes a stale value;
//! a name defined after the code that reads it was linked is read once it is
//! defined; and a library's code reads and writes its own namespace's
//! bindings, not the program's, by the linked paths and by name alike. They
//! run against `VmBackend` explicitly: the link tables exist only in the VM.

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
    // placeholder for x, and f reads x once it is defined. The answer
    // cannot show that the define adopted the placeholder: a define that
    // made a record of its own would be read by name, by the cold read, and
    // answer the same. `gc_vm.rs`' `a_bound_placeholder_is_never_dropped`
    // holds the adoption.
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

#[test]
fn a_store_by_name_marks_the_primitive_it_replaces() {
    // `car-alias` is an alias in the global namespace, as a macro expansion
    // installs one, to the namespace's own `car`: the table of the unit that
    // assigns it links the heap's by-name record, so the store goes by name
    // (`store_global_cold`), through the alias into `car`'s binding. It must
    // mark the primitive it replaces before it writes, as a linked store
    // does, or `f`'s compiled `car` site keeps calling the primitive (#836).
    use patina_runtime::Backend;

    let interp = common::vm_interpreter();
    let env = interp.backend().global_env();
    env.define_alias("car-alias", env.clone(), "car".into());
    let answer = interp
        .eval_program_owned(
            "(define (f x) (car x)) \
             (define before (f '(1 2))) \
             (set! car-alias (lambda (x) 'replaced)) \
             (list before (f '(1 2)) (car '(3)))",
        )
        .unwrap();
    assert_eq!(interp.display_tagged(&answer), "(1 replaced replaced)");
}

#[test]
fn a_store_by_name_writes_the_namespace_its_unit_was_linked_in() {
    // A store the table resolves no cell for goes by name
    // (`store_global_cold`) in the namespace the unit was linked in, which
    // is not the program's for a library's unit or for one compiled in an
    // environment specifier's namespace (#836).
    //
    // `(probe b)`'s `step` expands `(probe a)`'s macro, whose template
    // assigns `counter`, a binding private to `(probe a)`: the expansion
    // installs an alias for it in `(probe b)`'s namespace, so `step`'s unit
    // links the by-name record and its store goes by name, through that
    // alias. The program's namespace has no such alias, and a store by name
    // there would find nothing to assign.
    assert_eq!(
        eval(
            "(define-library (probe a)
               (export bump! get)
               (import (scheme base))
               (begin
                 (define counter 0)
                 (define-syntax bump!
                   (syntax-rules () ((_) (set! counter (+ counter 1)))))
                 (define (get) counter)))
             (define-library (probe b)
               (export step)
               (import (scheme base) (probe a))
               (begin (define (step) (bump!))))
             (import (scheme base) (probe a) (probe b))
             (define r (guard (e (#t 'error)) (step) (step) 'assigned))
             (list r (get))"
        ),
        "(assigned 2)"
    );
    // `zzz-836` is bound in the program's namespace alone. The specifier's
    // namespace does not bind it, so the `set!` compiled there links a
    // placeholder and stores by name in that namespace: an error, and the
    // program's binding is left as it was.
    assert_eq!(
        eval(
            "(import (scheme base) (scheme eval))
             (define zzz-836 1)
             (define r
               (guard (e (#t 'unbound))
                 (eval '(set! zzz-836 2) (environment '(scheme base)))
                 'assigned))
             (list r zzz-836)"
        ),
        "(unbound 1)"
    );
}

#[test]
fn a_host_form_s_closures_read_the_host_namespace() {
    // A form evaluated in a host namespace is linked in that namespace, and
    // so are the closures it makes, one made inside a closure too (#779):
    // called after a collection, each reads the host's `who`, not the
    // program's. A closure holds no environment (#836): the namespace its
    // code reads is its unit's link table's, linked where the form was
    // compiled, and a closure made inside a closure is of a member of the
    // same unit, which shares the table. Read through the closures' calls,
    // so in a release build too.
    let mut interp = common::vm_interpreter();
    interp
        .eval_program_owned("(import (scheme base) (patina debug)) (define who 'global)")
        .unwrap();
    let env = interp.new_environment();
    interp.eval_str_in(&env, "(define who 'host)").unwrap();
    let procedures: Vec<_> = ["(lambda () who)", "((lambda () (lambda () who)))"]
        .into_iter()
        .map(|form| (form, interp.eval_str_in(&env, form).unwrap()))
        .collect();
    interp.eval_str_owned("(gc)").unwrap();
    for (form, procedure) in &procedures {
        let who = interp.call(procedure, &[]).unwrap();
        assert_eq!(interp.display_tagged(&who), "host", "{form}");
    }
    let who = interp.eval_str_owned("who").unwrap();
    assert_eq!(interp.display_tagged(&who), "global");
}
