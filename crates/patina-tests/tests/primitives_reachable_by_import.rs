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

/// Every primitive a shipped library exports can be dispatched — on **both**
/// backends.
///
/// The other direction of the test above, and the one issue #169 needed. A
/// `Procedure::Primitive` bound by `define_primitive` carries a
/// `qualified_name`; applying it resolves that through
/// `PrimitiveRegistry::resolve_index`, which falls back to the short name.
/// This runs the same resolution, so a green tick means **the call will not
/// report a lookup miss** — not that the call is correct. That fallback can
/// resolve a qualified name whose own library never registered it, landing on
/// whichever primitive claimed the short name first; a mis-dispatch of that
/// kind is invisible here.
///
/// **Resolution, not application.** Calling all 1395 of them to see which
/// error would run `exit`, `delete-file` and `read` among others.
///
/// Both backends, because #169 is by definition a per-backend gap: the VM
/// builds its registry from `patina_primitives::register_all` alone
/// (`patina-vm/src/runtime/vm_state.rs`), the tree-walker adds to it. A name
/// one backend handles and the other does not is exactly the shape being
/// guarded against, and a single-backend check lets half of it ship — this
/// test found the VM half of #169 the moment it started looking at both.
#[test]
fn every_exported_primitive_can_be_dispatched() {
    /// Names each backend claims **by name, before any registry lookup**, so
    /// no registry entry backs them there. Qualified, so an exclusion names
    /// the binding it excuses rather than every export sharing a short name.
    ///
    /// Per backend, because the sets genuinely differ — and the difference is
    /// what this test is for. The VM matches all six in `vm_control_primitive`;
    /// the tree-walker claims four and *registers* the other two with a
    /// deliberate not-implemented error, which is issue #169.
    ///
    /// Being on a list means "not undispatchable", not "works everywhere":
    ///
    /// - `apply` and `dynamic-wind` are claimed at **apply** time, by a
    ///   short-name match on `Procedure::Primitive` (the VM's
    ///   `vm_control_primitive`, the tree-walker's `cps_eval/application.rs`),
    ///   so they work in head *and* value position on both backends.
    /// - `call/cc` and `call-with-current-continuation` are claimed
    ///   **syntactically** on the tree-walker, in `cps_transform.rs`'s
    ///   `is_callcc_reference`, so `(define f call/cc)` then `(f …)` still
    ///   reaches a registry miss there. That is #169's defect in another
    ///   place, already pinned as `backend_divergence.rs`'s
    ///   `callcc_bound_with_define` under Track Q §1.2, so it is excluded here
    ///   rather than counted twice.
    ///
    /// A name added to either list owes a pin somewhere for whatever it does
    /// *not* do.
    const CLAIMED_BEFORE_DISPATCH: &[(&str, &[&str])] = &[
        (
            "tree-walker",
            &[
                "patina.internal.control/apply",
                "patina.internal.control/call-with-current-continuation",
                "patina.internal.control/call/cc",
                "patina.internal.control/dynamic-wind",
            ],
        ),
        (
            "vm",
            &[
                "patina.internal.control/abort-current-continuation",
                "patina.internal.control/apply",
                "patina.internal.control/call-with-continuation-prompt",
                "patina.internal.control/call-with-current-continuation",
                "patina.internal.control/call/cc",
                "patina.internal.control/dynamic-wind",
            ],
        ),
    ];

    let interp = TreeWalkInterpreter::new_tree_walker();
    let heap = interp.backend().global_env().heap().clone();
    let evaluator = interp.backend().evaluator();

    // The VM's registry is `register_all` and nothing else — see `VmState::new`.
    let mut vm_registry = patina_primitives::PrimitiveRegistry::new();
    patina_primitives::register_all(&mut vm_registry);
    let backends: [(&str, &patina_primitives::PrimitiveRegistry); 2] = [
        ("tree-walker", evaluator.primitive_registry()),
        ("vm", &vm_registry),
    ];

    let mut undispatchable: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut failed_to_load: Vec<String> = Vec::new();
    let mut checked = 0usize;
    let mut libraries = 0usize;
    for library in shipped_libraries(&repo_root().join("lib")) {
        // Not skipped: a library that stops loading contributes no exports, so
        // silently continuing would let this pass *because* something broke.
        let lib = match evaluator.load_library(&library) {
            Ok(lib) => lib,
            Err(e) => {
                failed_to_load.push(format!("({}) — {e}", library.join(" ")));
                continue;
            }
        };
        libraries += 1;
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
            for (backend, registry) in backends {
                let claimed = CLAIMED_BEFORE_DISPATCH
                    .iter()
                    .find(|(b, _)| *b == backend)
                    .expect("every backend under test lists what it claims")
                    .1;
                if registry.resolve_index(&qualified).is_none()
                    && !claimed.contains(&qualified.as_str())
                {
                    undispatchable
                        .entry(format!("{name} ({qualified}) [{backend}]"))
                        .or_default()
                        .insert(library.join(" "));
                }
            }
        }
    }

    assert!(
        failed_to_load.is_empty(),
        "shipped librar{} failed to load, so their exports went unchecked:\n  {}",
        if failed_to_load.len() == 1 {
            "y"
        } else {
            "ies"
        },
        failed_to_load.join("\n  ")
    );
    assert!(
        libraries > 20 && checked > 1000,
        "expected every library's exports, got {checked} from {libraries} libraries \
         — did the loader change?"
    );
    assert!(
        undispatchable.is_empty(),
        "{} exported primitive(s) cannot be dispatched.\n\n{}\n\n\
         Either implement it, register a deliberate not-implemented error for it \
         (see `crates/patina-tree-walker/src/eval/primitives/unsupported.rs`), or \
         add it to that backend's CLAIMED_BEFORE_DISPATCH list if the backend \
         claims the name before dispatch — and check what the *other* backend \
         does with it.",
        undispatchable.len(),
        undispatchable
            .iter()
            .map(|(prim, libs)| format!("  {prim}\n    exported by: {libs:?}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
