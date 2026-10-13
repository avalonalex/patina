//! `Instance`: what belongs to one interpreter but not to its heap
//! (`PRD/GC_PRD.md` §11.3, #703).
//!
//! The command line, the feature set, the file system and the library
//! availability query are per-interpreter metadata, not GC data. They rode
//! on `Heap` because the heap was the one per-instance thing the desugarer's
//! `cond-expand`, the library parser's and the `features` primitive all
//! held (`Heap::features`). A [`Cx`](crate::Cx) carries an `Instance`
//! instead, so code that takes a `Cx` reaches it without the heap.
//!
//! Until the crates outside core move onto `Cx` (#704, #706, #707), the
//! heap keeps its copies and nothing reads both.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::features::{FeatureRegistry, LibraryAvailability};
use crate::vfs::{FileSystem, NativeFs};

/// One interpreter's metadata. See the [module docs](self).
pub struct Instance {
    /// Scheme's `(command-line)`: the program name, then its arguments.
    command_line: Vec<String>,
    /// The feature identifiers `cond-expand` treats as true, which R7RS
    /// §4.2.1 requires `(features)` to return.
    features: FeatureRegistry,
    /// Whether the feature set has been read ([`Instance::features_and_close`]):
    /// once it has, a feature added would disagree with what `cond-expand`
    /// already resolved, so [`Instance::add_feature`] refuses one in debug
    /// builds.
    features_read: Cell<bool>,
    /// The file system `open-input-file`, `load` and `include` see.
    fs: Arc<dyn FileSystem>,
    /// The runtime's non-loading query for `cond-expand`'s `(library …)`.
    library_availability: Option<Rc<dyn LibraryAvailability>>,
}

impl Instance {
    /// An instance over `fs`, with an empty command line, the default
    /// features and no library query.
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        Self {
            command_line: Vec::new(),
            features: FeatureRegistry::new(),
            features_read: Cell::new(false),
            fs,
            library_availability: None,
        }
    }

    /// Scheme's command line.
    pub fn command_line(&self) -> &[String] {
        &self.command_line
    }

    /// Configure `(command-line)`, keeping its program-name element separate
    /// so even a program with no arguments has a nonempty list.
    pub fn set_command_line(&mut self, program_name: String, arguments: Vec<String>) {
        self.command_line = std::iter::once(program_name).chain(arguments).collect();
    }

    /// The feature set, for a reader that does not resolve against it.
    pub fn features(&self) -> &FeatureRegistry {
        &self.features
    }

    /// The feature set, closed to further additions: what `cond-expand`
    /// resolves against.
    pub fn features_and_close(&self) -> &FeatureRegistry {
        self.features_read.set(true);
        &self.features
    }

    /// Advertise one more feature, at construction: after the set has been
    /// read, a debug build refuses it.
    pub fn add_feature(&mut self, name: &str) {
        debug_assert!(
            !self.features_read.get(),
            "add_feature({name:?}) after the feature set was already read: a \
             `cond-expand` resolved against the old set would disagree with \
             `(features)`. Advertise features at construction."
        );
        self.features.add_feature(name);
    }

    /// The file system.
    pub fn fs(&self) -> &Arc<dyn FileSystem> {
        &self.fs
    }

    /// Install the runtime's library query, before anything is expanded.
    pub fn set_library_availability(&mut self, availability: Rc<dyn LibraryAvailability>) {
        self.library_availability = Some(availability);
    }

    /// The runtime's library query, cloned so the caller holds no borrow of
    /// the instance while it asks.
    pub fn library_availability(&self) -> Option<Rc<dyn LibraryAvailability>> {
        self.library_availability.clone()
    }
}

/// An instance over the native file system.
impl Default for Instance {
    fn default() -> Self {
        Self::new(Arc::new(NativeFs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_keeps_its_program_name() {
        let mut instance = Instance::default();
        instance.set_command_line("prog".into(), vec![]);
        assert_eq!(instance.command_line(), ["prog".to_string()]);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "after the feature set was already read")]
    fn a_feature_after_the_set_was_read_is_refused() {
        let mut instance = Instance::default();
        let _ = instance.features_and_close();
        instance.add_feature("late");
    }
}
