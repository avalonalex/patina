//! The procedures some part of Patina still recognises **by spelling**.
//!
//! R7RS makes every one of these an ordinary procedure, and each has a real
//! binding. But one place looks at the *name* a call was written with,
//! instead of asking what the name is bound to:
//!
//! | name | who looks | why |
//! |---|---|---|
//! | [`CALL_WITH_VALUES`], [`DYNAMIC_WIND`] | the VM's code generator, which emits an instruction sequence instead of a call | the sequence avoids `run_thunk`, which is what keeps a continuation captured inside the producer or the body re-enterable (#442) |
//!
//! It is a hygiene hole of its own — a program's `(define (dynamic-wind a b
//! c) …)` is ignored on the VM — and has its own issue. The names are listed
//! here because of what they forbid: **renaming a reference to one of these
//! changes what the program does.** The desugarer gives a library macro's
//! reference to an imported procedure a name of its own, so that a later
//! definition of the spelling cannot capture it (#438,
//! `Desugarer::early_bound`), and it must leave these alone. Measured
//! 2026-09-19 with them renamed: the VM lost one corpus package, five Larceny
//! assertions and seven suite rows, through `call-with-values` and
//! `dynamic-wind` falling off the instruction path.
//!
//! Two recognisers have left this list by learning to ask about the binding,
//! and early binding protects their names now:
//!
//! - **`apply`** (#443). The desugarer lowered `(apply f args)` to
//!   `CoreExpr::Apply` by the spelling of the head; it lowers the call where
//!   the head is bound to the `apply` primitive, and a renamed `apply` is only
//!   an unlowered call.
//! - **`call/cc` and `call-with-current-continuation`** (#441). The
//!   tree-walker's CPS transform made a `CallCC` node of any call so spelled,
//!   and that node was the backend's only implementation: the value did not
//!   work as a procedure, and a variable of that spelling was taken for it.
//!   The evaluator claims the primitive by its value when it is applied, as
//!   it does every other control procedure.
//!
//! The recognisers name these constants rather than string literals, so the
//! list cannot drift from them. **A new recogniser adds its name here**; one
//! that learns to ask about the binding instead takes its name out, and early
//! binding starts protecting that name the same day.

/// `call-with-values`.
pub const CALL_WITH_VALUES: &str = "call-with-values";
/// `dynamic-wind`.
pub const DYNAMIC_WIND: &str = "dynamic-wind";

/// Whether a recogniser decides what a call means from this spelling — from
/// the name on the **reference** a call's operator becomes, the `Var`, so
/// that giving the reference another name changes what the program does.
/// What early binding (#438) asks.
///
/// Asked by the desugarer, which does not know which backend it is feeding,
/// so the tree-walker leaves these unprotected too although it claims both
/// by value; that stands until #442 retires the VM's recogniser.
pub fn is_recognized(name: &str) -> bool {
    matches!(name, CALL_WITH_VALUES | DYNAMIC_WIND)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_and_nothing_else() {
        for name in ["call-with-values", "dynamic-wind"] {
            assert!(is_recognized(name), "{name}");
        }
        // Control procedures that are dispatched on the *value* at apply
        // time, on both backends, and so survive a rename — `apply` among
        // them since #443, and both spellings of `call/cc` since #441.
        for name in [
            "apply",
            "call/cc",
            "call-with-current-continuation",
            "values",
            "with-exception-handler",
            "raise",
            "force",
            "car",
        ] {
            assert!(!is_recognized(name), "{name}");
        }
    }
}
