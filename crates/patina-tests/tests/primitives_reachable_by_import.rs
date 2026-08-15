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

mod common;
use common::{files_under, repo_root};
use patina_interpreter::TreeWalkInterpreter;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Library names for every `.sld` under `root`, e.g. `lib/srfi/130.sld` ->
/// `["srfi", "130"]`.
fn shipped_libraries(root: &Path) -> Vec<Vec<String>> {
    files_under(root)
        .into_iter()
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("sld"))
        .map(|path| {
            path.strip_prefix(root)
                .expect("under lib/")
                .with_extension("")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .collect()
}

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
