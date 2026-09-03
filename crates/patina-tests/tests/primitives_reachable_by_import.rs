//! Every registered primitive must be reachable by importing the library it
//! says it belongs to.
//!
//! This became load-bearing when the VM stopped binding the whole primitive
//! registry into globals by short name. Before that, a primitive that no
//! library exported was still callable — the blanket install handed it out
//! regardless — so an under-exporting library builder cost nothing and showed
//! up nowhere. Now such a primitive is simply unreachable: registered, never
//! exported, dead. Nothing else would notice.
//!
//! The check is reachability from *some* shipped library, not from the one the
//! primitive's own `library` field names. Those two differ in practice and the
//! difference is not a defect: the field feeds `qualified_name`, used for
//! registry lookup and error text, and several primitives are labelled with a
//! library adjacent to the one R7RS actually exports them from — `char->integer`
//! says `scheme.char` while R7RS puts it in `(scheme base)`. Asserting the label
//! would be asserting a naming convention; asserting reachability is asserting
//! the thing that would actually break.
//!
//! # Both directions
//!
//! That test runs implementation → export. The **other** direction, export →
//! implementation, is [`every_exported_primitive_can_be_dispatched`], and it
//! is here because having only the first one is how issue #169 stayed
//! invisible: `(scheme base)` exported two names that nothing implemented on
//! this backend, and a user calling one got
//! `Undefined variable: patina.internal.control/…` — an internal path, for a
//! name the library does define. Exported-but-undispatchable was not
//! something anything looked for.

mod common;
use common::{repo_root, shipped_libraries};
use patina_interpreter::{Backend, TreeWalkInterpreter};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn every_registered_primitive_is_reachable_by_some_import() {
    let mut registry = patina_primitives::PrimitiveRegistry::new();
    patina_primitives::register_all(&mut registry);

    let mut registered: BTreeMap<&str, &str> = BTreeMap::new(); // name -> declared library
    let mut candidates: BTreeSet<Vec<String>> = BTreeSet::new();
    for prim in registry.primitives() {
        registered.insert(prim.name, prim.library);
        candidates.insert(prim.library.split('.').map(str::to_string).collect());
    }
    assert!(
        registered.len() > 100,
        "expected the whole registry, got {} — did register_all change?",
        registered.len()
    );

    // Every library Patina ships is a candidate, not only the labelled ones.
    for sld in shipped_libraries(&repo_root().join("lib")) {
        candidates.insert(sld);
    }

    let interp = TreeWalkInterpreter::new_tree_walker();
    let evaluator = interp.backend().evaluator();

    let mut exported_anywhere: BTreeSet<String> = BTreeSet::new();
    for library in &candidates {
        if let Ok(lib) = evaluator.load_library(library) {
            exported_anywhere.extend(lib.exports_iter_tagged().map(|(n, _)| n.clone()));
        }
    }

    let orphans: Vec<String> = registered
        .iter()
        .filter(|(name, _)| !exported_anywhere.contains(**name))
        .map(|(name, library)| format!("{name} (registered under {library})"))
        .collect();
    assert!(
        orphans.is_empty(),
        "registered primitives that no import can reach — export them from a \
         library builder, or drop the registration:\n  {}",
        orphans.join("\n  ")
    );
}

/// Every primitive a shipped library exports can be dispatched on this
/// backend — or is one of the control primitives handled before dispatch.
///
/// The other direction of the test above, and the one issue #169 needed. A
/// `Procedure::Primitive` bound by `define_primitive` carries a
/// `qualified_name`; applying it resolves that through
/// `PrimitiveRegistry::resolve_index`, which falls back to the short name.
/// This asserts the same resolution the runtime performs, so it cannot pass
/// while a call fails.
///
/// **Resolution, not application.** Calling all 1395 of them to see which
/// error would run `exit`, `delete-file` and `read` among others.
///
/// The exceptions are real and few: control primitives are recognised **by
/// name** and handled before any registry lookup — the VM in
/// `vm_control_primitive`, the tree-walker in the CPS transform, which emits
/// `CpsExprKind::CallCC` and the wind forms. They are registered nowhere, and
/// pinning the list is the point: a *new* exported control primitive that
/// neither backend was taught fails here, which is the shape of #169.
#[test]
fn every_exported_primitive_can_be_dispatched() {
    /// Names no registry entry backs, because a backend claims them by name
    /// before dispatch. Being on this list means "not undispatchable", not
    /// "works everywhere":
    ///
    /// - `apply` and `dynamic-wind` work in head *and* value position on both
    ///   backends (`control_flow_matrix.rs`'s `Extent::Value` rows are the
    ///   value-position ones for `dynamic-wind`).
    /// - `call/cc` and `call-with-current-continuation` work in head position
    ///   only: on the tree-walker a `(define f call/cc)` then `(f …)` still
    ///   reaches a registry miss, which is the same defect as issue #169 in a
    ///   different place. Already pinned — `backend_divergence.rs`'s
    ///   `callcc_bound_with_define`, tracked as Track Q §1.2 — so it is
    ///   excluded here rather than counted twice.
    ///
    /// A name added to this list therefore owes a pin somewhere for whatever
    /// it does *not* do.
    const HANDLED_BEFORE_DISPATCH: &[&str] = &[
        "apply",
        "call-with-current-continuation",
        "call/cc",
        "dynamic-wind",
    ];

    let interp = TreeWalkInterpreter::new_tree_walker();
    let heap = interp.backend().global_env().heap().clone();
    let evaluator = interp.backend().evaluator();
    let registry = evaluator.primitive_registry();

    let mut undispatchable: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut checked = 0usize;
    for library in shipped_libraries(&repo_root().join("lib")) {
        let Ok(lib) = evaluator.load_library(&library) else {
            continue;
        };
        for (name, tv) in lib.exports_iter_tagged() {
            let qualified = {
                let heap = heap.borrow();
                match heap.get_procedure(tv).as_deref() {
                    Some(patina_runtime::Procedure::Primitive { qualified_name, .. }) => {
                        Some(qualified_name.to_string())
                    }
                    _ => None,
                }
            };
            let Some(qualified) = qualified else { continue };
            checked += 1;
            if registry.resolve_index(&qualified).is_none()
                && !HANDLED_BEFORE_DISPATCH.contains(&name.as_str())
            {
                undispatchable
                    .entry(format!("{name} ({qualified})"))
                    .or_default()
                    .insert(library.join(" "));
            }
        }
    }

    assert!(
        checked > 1000,
        "expected every library's exports, got {checked} — did the loader change?"
    );
    assert!(
        undispatchable.is_empty(),
        "{} exported primitive(s) cannot be dispatched on the tree-walker.\n\n{}\n\n\
         Either implement it, register a deliberate not-implemented error for it \
         (see `primitives::unsupported`), or add it to HANDLED_BEFORE_DISPATCH if a \
         backend claims the name before dispatch — and check the *other* backend \
         claims it too.",
        undispatchable.len(),
        undispatchable
            .iter()
            .map(|(prim, libs)| format!("  {prim}\n    exported by: {:?}", libs))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
