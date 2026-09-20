//! The procedures some part of Patina still recognises **by spelling**.
//!
//! R7RS makes every one of these an ordinary procedure, and each has a real
//! binding. But three places look at the *name* a call was written with,
//! before or instead of asking what the name is bound to:
//!
//! | name | who looks | why |
//! |---|---|---|
//! | [`APPLY`] | the desugarer, which lowers `(apply f args)` to `CoreExpr::Apply` | an optimisation (#443) |
//! | [`CALL_CC`], [`CALL_WITH_CURRENT_CONTINUATION`] | the tree-walker's CPS transform, which makes the call a `CallCC` node | on that backend it is the *only* implementation: the value does not work as a procedure (#441; Track Q §1.2; pinned in `tests/scheme/control/callability.scm`) |
//! | [`CALL_WITH_VALUES`], [`DYNAMIC_WIND`] | the VM's code generator, which emits an instruction sequence instead of a call | the sequence avoids `run_thunk`, which is what keeps a continuation captured inside the producer or the body re-enterable (#442) |
//!
//! Each is a hygiene hole of its own — a program's `(define (apply a b) …)`
//! is ignored — and has its own issue. They are listed *together* here
//! because of what they jointly forbid: **renaming a reference to one of
//! these changes what the program does.** The desugarer gives a library
//! macro's reference to an imported procedure a name of its own, so that a
//! later definition of the spelling cannot capture it (#438,
//! `Desugarer::early_bound`), and it must leave these alone. Measured
//! 2026-09-19 with them renamed: the tree-walker lost every `call/cc` a
//! library macro makes — 1092 errors in chibi's suite, 20 of `control/
//! guard.scm`'s 22 rows — and the VM one corpus package, five Larceny
//! assertions and seven suite rows, through `call-with-values` and
//! `dynamic-wind` falling off the instruction path.
//!
//! (`apply` is the mild one: its recogniser looks at the head of the *form*,
//! before any reference is emitted, so a renamed `apply` in value position
//! would still work. It is listed with the rest so that the rule stays one
//! rule.)
//!
//! The recognisers name these constants rather than string literals, so the
//! list cannot drift from them. **A new recogniser adds its name here**; one
//! that learns to ask about the binding instead takes its name out, and early
//! binding starts protecting that name the same day.

/// `apply`.
pub const APPLY: &str = "apply";
/// `call/cc`.
pub const CALL_CC: &str = "call/cc";
/// `call-with-current-continuation`.
pub const CALL_WITH_CURRENT_CONTINUATION: &str = "call-with-current-continuation";
/// `call-with-values`.
pub const CALL_WITH_VALUES: &str = "call-with-values";
/// `dynamic-wind`.
pub const DYNAMIC_WIND: &str = "dynamic-wind";

/// Whether some part of Patina decides what a call means from this spelling.
pub fn is_recognized(name: &str) -> bool {
    matches!(
        name,
        APPLY | CALL_CC | CALL_WITH_CURRENT_CONTINUATION | CALL_WITH_VALUES | DYNAMIC_WIND
    )
}

/// Whether `name` is either spelling of `call/cc`.
pub fn is_call_cc(name: &str) -> bool {
    matches!(name, CALL_CC | CALL_WITH_CURRENT_CONTINUATION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_five_and_nothing_else() {
        for name in [
            "apply",
            "call/cc",
            "call-with-current-continuation",
            "call-with-values",
            "dynamic-wind",
        ] {
            assert!(is_recognized(name), "{name}");
        }
        // Control procedures that are dispatched on the *value* at apply
        // time, on both backends, and so survive a rename.
        for name in ["values", "with-exception-handler", "raise", "force", "car"] {
            assert!(!is_recognized(name), "{name}");
        }
        assert!(is_call_cc("call/cc") && is_call_cc("call-with-current-continuation"));
        assert!(!is_call_cc("call-with-values"));
    }
}
