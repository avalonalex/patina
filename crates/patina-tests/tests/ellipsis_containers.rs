//! Ellipsis handling across every container shape a syntax-rules form can take.
//!
//! R7RS allows `p ...` inside proper lists, dotted lists and vectors, in both
//! patterns and templates. Patina implemented it only for proper lists: five
//! separate code paths each walked their sub-forms with a hand-rolled loop that
//! never looked for an ellipsis.
//!
//! The failures came in two flavours, and the second is why these tests assert
//! on *output* rather than merely that a macro compiles:
//!
//!   * Loud — the compiler rejected the form outright:
//!     "Pattern variable x at level 1 used at level 0", or
//!     "Ellipsis in template contains no pattern variables".
//!   * Silent — the expander built the wrong structure. `(x ... . t)` produced
//!     `((x1 x2) . t)` instead of `(x1 x2 . t)`, and `#(x ...)` would have
//!     produced `#((x1 x2))` instead of `#(x1 x2)`.
//!
//! Every container kind now routes through one shared ellipsis-aware helper per
//! phase, so a sixth container shape cannot pick up the same bug by omission.
//!
//! Found by importing `(chibi optional)`, whose `let*-to-let` helper uses the
//! dotted-tail template shape; it in turn blocks `(chibi diff)` and
//! `(chibi test)`.

mod common;
use common::eval_program as eval;

#[test]
fn test_simple_ellipsis_with_dotted_tail() {
    assert_eq!(
        eval("(define-syntax f (syntax-rules () ((f (x ...) t) '(x ... . t)))) (f (1 2 3) 9)"),
        "(1 2 3 . 9)"
    );
}

#[test]
fn test_dotted_subtemplate_under_ellipsis_with_dotted_tail() {
    assert_eq!(
        eval(
            "(define-syntax g (syntax-rules () ((g ((a b) ...) t) '((a . b) ... . t)))) \
             (g ((1 2) (3 4)) end)"
        ),
        "((1 . 2) (3 . 4) . end)"
    );
}

#[test]
fn test_fixed_prefix_before_ellipsis_and_dotted_tail() {
    assert_eq!(
        eval(
            "(define-syntax w (syntax-rules () ((w a (x ...) t) '(a x ... . t)))) \
             (w head (1 2) tail)"
        ),
        "(head 1 2 . tail)"
    );
}

#[test]
fn test_zero_repetitions_collapses_to_tail() {
    // With no repetitions, `(x ... . t)` is just `t` — not `(() . t)`.
    assert_eq!(
        eval("(define-syntax z (syntax-rules () ((z (x ...) t) '(x ... . t)))) (z () tail)"),
        "tail"
    );
}

#[test]
fn test_proper_list_ellipsis_still_works() {
    // Guards against the fix breaking the non-dotted path it now shares.
    assert_eq!(
        eval("(define-syntax p (syntax-rules () ((p (x ...)) '(x ...)))) (p (1 2 3))"),
        "(1 2 3)"
    );
}

/// The shape from `(chibi optional)` that exposed the bug.
#[test]
fn test_chibi_optional_let_star_to_let_shape() {
    let src = r#"
        (define-syntax let*-to-let
          (syntax-rules ()
            ((let*-to-let letstar ls (vars ...) ((v . d) . rest) . body)
             (let*-to-let letstar ls (vars ... (v tmp . d)) rest . body))
            ((let*-to-let letstar ls ((var tmp . d) ...) rest . body)
             (letstar ls ((tmp . d) ... . rest)
               (let ((var tmp) ...) . body)))))
        'compiled
    "#;
    assert_eq!(eval(src), "compiled");
}

// ─── Vectors: templates ──────────────────────────────────────────────────────

#[test]
fn test_vector_template_with_ellipsis() {
    // Was rejected outright at compile time.
    assert_eq!(
        eval("(define-syntax v (syntax-rules () ((v (x ...)) '#(x ...)))) (v (1 2 3))"),
        "#(1 2 3)"
    );
}

#[test]
fn test_vector_template_splices_rather_than_nests() {
    // The silent failure mode: must be #(1 2 3), never #((1 2 3)).
    let out = eval("(define-syntax v (syntax-rules () ((v (x ...)) '#(x ...)))) (v (1 2 3))");
    assert!(
        !out.contains("#(("),
        "ellipsis was nested instead of spliced: {out}"
    );
}

#[test]
fn test_vector_template_zero_repetitions() {
    assert_eq!(
        eval("(define-syntax v (syntax-rules () ((v (x ...)) '#(x ...)))) (v ())"),
        "#()"
    );
}

// ─── Vectors: patterns ───────────────────────────────────────────────────────

#[test]
fn test_vector_pattern_with_ellipsis() {
    assert_eq!(
        eval("(define-syntax vp (syntax-rules () ((vp #(x ...)) '(x ...)))) (vp #(1 2 3))"),
        "(1 2 3)"
    );
}

#[test]
fn test_vector_pattern_with_fixed_elements_around_ellipsis() {
    // Exercises num_following: the trailing `z` must not be swallowed.
    assert_eq!(
        eval(
            "(define-syntax vt (syntax-rules () ((vt #(a x ... z)) '(a (x ...) z)))) \
             (vt #(1 2 3 4))"
        ),
        "(1 (2 3) 4)"
    );
}

#[test]
fn test_fixed_length_vector_pattern_still_requires_exact_length() {
    // The no-ellipsis path keeps its up-front length check.
    assert_eq!(
        eval("(define-syntax vf (syntax-rules () ((vf #(a b)) '(a b)))) (vf #(1 2))"),
        "(1 2)"
    );
}

// ─── Rule-level dotted pattern ───────────────────────────────────────────────

#[test]
fn test_rule_level_dotted_pattern_with_ellipsis() {
    // `(kw x ... . r)` is a valid R7RS rule pattern; the rule-pattern entry
    // point bypassed the ellipsis-aware compiler its proper-list sibling used.
    assert_eq!(
        eval("(define-syntax q (syntax-rules () ((q x ... . r) '((x ...) r)))) (q 1 2 . 3)"),
        "((1 2) 3)"
    );
}
