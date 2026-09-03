//! Exports this backend does not implement.
//!
//! `(scheme base)` exports `call-with-continuation-prompt` and
//! `abort-current-continuation`, and only the VM implements them. Neither
//! backend *registers* them: control primitives are recognised by name and
//! handled before any registry lookup — the VM in `vm_control_primitive`, the
//! tree-walker in the CPS transform, which emits `CpsExprKind::CallCC` and the
//! wind forms. These two names are in neither list, so on the tree-walker they
//! fell all the way through to a registry miss and surfaced as
//!
//! ```text
//! Error: Undefined variable: patina.internal.control/call-with-continuation-prompt
//! ```
//!
//! which names an internal library path and says "undefined" of something the
//! library does define. Registering them here replaces that with a deliberate
//! message: what is missing, which backend has it, and where it is tracked.
//!
//! **The VM is unaffected.** It builds its registry from
//! `patina_primitives::register_all` alone (`vm_state.rs`) and intercepts both
//! names before a lookup would happen, so these entries exist only on this
//! backend — which is the point of registering them here rather than in the
//! backend-agnostic crate.
//!
//! Each of these is deleted by the change that implements it. See issue #169
//! for what that involves; the twelve `UNSUPPORTED` rows in
//! `control_flow_matrix.rs` are its acceptance criteria.

use patina_core::TaggedValue;
use patina_primitives::{PrimitiveFn, PrimitiveRegistry};
use patina_runtime::{Arity, EvalError, ExceptionKind, SharedHeap};

/// Register this backend's not-implemented exports.
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.control",
        "call-with-continuation-prompt",
        Arity::Min(1),
        "Not implemented on the tree-walker backend (issue #169).",
        |_heap, _args| Err(unimplemented_here("call-with-continuation-prompt")),
    ));
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.control",
        "abort-current-continuation",
        Arity::Min(1),
        "Not implemented on the tree-walker backend (issue #169).",
        |_heap, _args| Err(unimplemented_here("abort-current-continuation")),
    ));
}

/// A raised Scheme error rather than an internal one.
///
/// `SchemeException` so that it reads as the procedure failing — and so a
/// `guard` can catch it, which is the behaviour a program is entitled to from
/// anything `(scheme base)` exports. `InternalError` would read as an
/// interpreter bug and `UndefinedVariable` is what this replaces.
fn unimplemented_here(name: &str) -> EvalError {
    EvalError::SchemeException {
        kind: ExceptionKind::Error,
        message: format!(
            "{name}: delimited continuations are not implemented on the tree-walker \
             backend; the bytecode VM implements them, so running without \
             --tree-walker works. Tracked in issue #169"
        ),
        irritants_display: String::new(),
    }
}

/// Never called: registration hands the two closures above straight to the
/// registry. Present so the module's imports are all used in every build
/// configuration.
#[allow(dead_code)]
fn _unused(_: &SharedHeap, _: TaggedValue) {}
