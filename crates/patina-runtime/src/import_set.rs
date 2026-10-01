//! Import name policy shared by every evaluation context (#592).
//!
//! Only strings are transformed here. Loading a library can run Scheme and
//! remains the caller's job, as do binding installation and VM invalidation.

use crate::LibraryError;
use crate::library_loader::ImportSet;
use std::collections::{HashMap, HashSet};

type Binding = (String, String); // importing name, original library export

impl ImportSet {
    /// The library at the center of this set, before any name modifiers.
    pub fn library_name(&self) -> &[String] {
        let mut current = self;
        loop {
            match current {
                Self::Library(name) => return name,
                Self::Only { import_set, .. }
                | Self::Except { import_set, .. }
                | Self::Prefix { import_set, .. }
                | Self::Rename { import_set, .. } => current = import_set,
            }
        }
    }

    /// Resolve importing names to original export names, from the inside out.
    /// Install each pair with `Library::import_into` (or the VM's guarded
    /// equivalent), never by copying its current value. Syntax exports follow
    /// exactly the same rules as variables.
    ///
    /// Consume in order and stop on error. A final `only` yields the names
    /// preceding a missing name, preserving the existing partial-install
    /// behavior; a failure inside another modifier yields just the error.
    /// Loading/initialization is not rolled back when validation fails.
    pub fn resolve_bindings<'a>(
        &self,
        exports: impl IntoIterator<Item = &'a str>,
    ) -> impl Iterator<Item = Result<Binding, LibraryError>> {
        let mut modifiers = Vec::new();
        let mut current = self;
        while !matches!(current, Self::Library(_)) {
            modifiers.push(current);
            current = match current {
                Self::Only { import_set, .. }
                | Self::Except { import_set, .. }
                | Self::Prefix { import_set, .. }
                | Self::Rename { import_set, .. } => import_set,
                Self::Library(_) => unreachable!(),
            };
        }
        let mut bindings: Vec<Result<Binding, LibraryError>> = exports
            .into_iter()
            .map(|name| Ok((name.to_owned(), name.to_owned())))
            .collect();
        for modifier in modifiers.into_iter().rev() {
            // Inner sets were previously resolved into staging environments:
            // an inner failure must not install anything in the destination.
            let input = match bindings.into_iter().collect::<Result<Vec<_>, _>>() {
                Ok(input) => input,
                Err(error) => return vec![Err(error)].into_iter(),
            };
            bindings = modifier.select(input);
        }
        bindings.into_iter()
    }

    fn select(&self, bindings: Vec<Binding>) -> Vec<Result<Binding, LibraryError>> {
        let missing = |name: &str, action: &str| {
            LibraryError::load(None, format!("Identifier '{name}' not found {action}"))
        };
        match self {
            Self::Only { identifiers, .. } => {
                let by_name: HashMap<_, _> = bindings.into_iter().collect();
                let mut selected = Vec::with_capacity(identifiers.len());
                for name in identifiers {
                    let Some(export) = by_name.get(name) else {
                        selected.push(Err(missing(name, "in import set")));
                        break;
                    };
                    selected.push(Ok((name.clone(), export.clone())));
                }
                selected
            }
            Self::Except { identifiers, .. } => {
                // #489: unknown names exclude nothing; repeated names are fine.
                let excluded: HashSet<_> = identifiers.iter().collect();
                bindings
                    .into_iter()
                    .filter(|(name, _)| !excluded.contains(name))
                    .map(Ok)
                    .collect()
            }
            Self::Prefix { prefix, .. } => bindings
                .into_iter()
                .map(|(name, export)| Ok((format!("{prefix}{name}"), export)))
                .collect(),
            Self::Rename { renames, .. } => {
                let names: HashSet<_> = bindings.iter().map(|(name, _)| name).collect();
                for (old, _) in renames {
                    if !names.contains(old) {
                        return vec![Err(missing(old, "for rename"))];
                    }
                }
                let renames: HashMap<_, _> = renames.iter().cloned().collect();
                // Simultaneous renaming: a swap cannot overwrite its own input.
                // Exports not named by a rename still come through unchanged.
                bindings
                    .into_iter()
                    .map(|(name, export)| Ok((renames.get(&name).cloned().unwrap_or(name), export)))
                    .collect()
            }
            Self::Library(_) => unreachable!("only modifiers are stacked"),
        }
    }
}
