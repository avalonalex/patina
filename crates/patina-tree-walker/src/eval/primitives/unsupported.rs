//! Exports this backend does not implement.
//!
//! `(scheme base)` exports `call-with-continuation-prompt` and
//! `abort-current-continuation`, and only the VM implements them. Neither
//! backend *registers* them, because control primitives are claimed by name
//! before a registry lookup would happen — but the two backends claim them in
//! different places, and that is worth knowing before adding a third:
//!
//! - the **VM** matches qualified names in `vm_control_primitive`
//!   (`crates/patina-vm/src/runtime/vm_state.rs`), covering `call/cc`,
//!   `dynamic-wind`, `apply`, `values`, `call-with-values`, the two names
//!   here, and the exception primitives;
//! - the **tree-walker** claims `call/cc` and `call-with-current-continuation`
//!   *syntactically*, in the CPS transform (`patina-ir/src/cps_transform.rs`,
//!   `is_callcc_reference`), and claims `dynamic-wind`, `apply`, `raise`,
//!   `error`, `force` and `call-with-values` at **apply** time, by a
//!   short-name match on `Procedure::Primitive` in
//!   `cps_eval/application.rs`.
//!
//! That split is why `(define f dynamic-wind)` works on this backend and
//! `(define f call/cc)` does not: an apply-time match sees the primitive
//! whatever name reached it, a syntactic one only sees the call it was
//! written in. (`call/cc` in value position is tracked separately, Track Q
//! §1.2.)
//!
//! These two names are in **neither** list, so a call fell all the way through
//! to a registry miss and surfaced as
//!
//! ```text
//! Error: Undefined variable: patina.internal.control/call-with-continuation-prompt
//! ```
//!
//! which names an internal library path and says "undefined" of something the
//! library does define. Registering them here replaces that with a deliberate
//! message: what is missing, which backend has it, and where it is tracked.
//! It is an ordinary raised Scheme error, the same kind `(error …)` produces —
//! `guard` catches it, as it also caught the lookup failure this replaces.
//!
//! **The VM is unaffected.** It builds its registry from
//! `patina_primitives::register_all` alone (`vm_state.rs`) and matches both
//! names before a lookup, so these entries exist only on this backend — which
//! is why they are registered here rather than in the backend-agnostic crate.
//!
//! Both are deleted by the change that implements them. See issue #169; the
//! twelve `UNSUPPORTED` rows in `control_flow_matrix.rs` are its acceptance
//! criteria.

use patina_primitives::{PrimitiveFn, PrimitiveRegistry};
use patina_runtime::{Arity, EvalError, ExceptionKind};

use patina_core::TaggedValue;
use patina_runtime::SharedHeap;

/// Register this backend's not-implemented exports.
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    for (name, handler) in [
        (
            "call-with-continuation-prompt",
            call_with_continuation_prompt as fn(&SharedHeap, &[TaggedValue]) -> _,
        ),
        (
            "abort-current-continuation",
            abort_current_continuation as _,
        ),
    ] {
        registry.register(PrimitiveFn::new_heap(
            "patina.internal.control",
            name,
            // `Min(0)`, not this procedure's real arity: `apply_by_index`
            // checks arity *before* calling the handler, so a declared minimum
            // would answer `(call-with-continuation-prompt)` with a complaint
            // about argument count — an arity error for a procedure that does
            // not exist here, which is the same class of misleading message
            // this module exists to remove. Accepting every arity lets the
            // real answer win.
            Arity::Min(0),
            "Not implemented on the tree-walker backend (issue #169).",
            handler,
        ));
    }
}

fn call_with_continuation_prompt(
    _heap: &SharedHeap,
    _args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    Err(not_implemented("call-with-continuation-prompt"))
}

fn abort_current_continuation(
    _heap: &SharedHeap,
    _args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    Err(not_implemented("abort-current-continuation"))
}

/// An ordinary raised Scheme error, the kind `(error …)` produces.
fn not_implemented(name: &str) -> EvalError {
    EvalError::SchemeException {
        kind: ExceptionKind::Error,
        message: format!(
            "{name}: delimited continuations are not implemented on the \
             tree-walker backend. The bytecode VM backend implements them"
        ),
        irritants_display: String::new(),
    }
}
