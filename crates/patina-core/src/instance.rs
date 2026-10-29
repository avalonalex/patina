//! `Instance`: what belongs to one interpreter but not to its heap
//! (`PRD/GC_PRD.md` §11.3, #703).
//!
//! The command line, the feature set, the file system and the library
//! availability query are per-interpreter metadata, not GC data. They rode
//! on `Heap` because the heap was the one per-instance thing the desugarer's
//! `cond-expand`, the library parser's and the `features` primitive all
//! held. A [`Cx`](crate::Cx) carries an `Instance` instead, so code that
//! takes a `Cx` reaches it without the heap.
//!
//! The current input, output and error ports are the interpreter's too
//! (#618). They were thread-local statics, so every interpreter on a thread
//! shared them, and one that redirected its output redirected the others'.
//! They are Rust ports, not heap values, so they are not GC data either, and
//! here they leave `Heap`'s layout alone, which a field of its own can move
//! in cycles (#685). Since #608 the three are parameter objects in the heap
//! (`Heap::standard_port_parameters`), and these slots cache the port each
//! holds, which a port-taking procedure called without its port reads with
//! no borrow of the heap; the store funnel refreshes a slot whenever its
//! parameter is assigned.
//!
//! **Until #707 the heap holds the interpreter's `Instance`** by `Rc`
//! ([`Heap::instance`](crate::Heap::instance)), and is its one owner of
//! record: code with no `Cx` yet reads it through the heap's accessors, and
//! a boundary that holds the heap makes a `Cx` with it (#704). #707 moves it
//! to the hosts. Shared, it is set through `&self`: the backends name
//! themselves and their file system at construction, the hosts set the
//! command line, the runtime installs its library query, and the store
//! funnel keeps the current ports.

use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::features::{FeatureRegistry, LibraryAvailability};
use crate::port::Port;
use crate::vfs::{FileSystem, NativeFs};

/// One of an interpreter's three current ports (#618): what
/// `current-input-port`, `current-output-port` and `current-error-port`
/// hold, and what a port-taking procedure called without its port uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrentPort {
    Input,
    Output,
    Error,
}

/// One interpreter's metadata. See the [module docs](self).
pub struct Instance {
    /// Scheme's `(command-line)`: the program name, then its arguments.
    command_line: RefCell<Vec<String>>,
    /// The feature identifiers `cond-expand` treats as true, which R7RS
    /// §4.2.1 requires `(features)` to return.
    features: RefCell<FeatureRegistry>,
    /// Whether the feature set has been read ([`Instance::features_and_close`]):
    /// once it has, a feature added would disagree with what `cond-expand`
    /// already resolved, so [`Instance::add_feature`] refuses one in debug
    /// builds.
    features_read: Cell<bool>,
    /// The file system `open-input-file`, `load` and `include` see.
    fs: RefCell<Arc<dyn FileSystem>>,
    /// The runtime's non-loading query for `cond-expand`'s `(library …)`.
    library_availability: RefCell<Option<Rc<dyn LibraryAvailability>>>,
    /// The current input, output and error ports, indexed by [`CurrentPort`]
    /// (#618): the port each standard port parameter holds, cached (#608).
    /// Only the store funnel writes them while the interpreter runs
    /// (`write_parameter`), so each is its parameter's port; teardown puts
    /// the standard ones back. The error port's slot has no reader, since no
    /// procedure falls back to the error port; it is kept with the others.
    current_ports: RefCell<[Rc<Port>; 3]>,
    /// The standard ports the slots start at, made with the instance, each
    /// the interpreter's own object: what [`Instance::reset_current_ports`]
    /// puts back. Kept, so that a reset makes no port: making a standard
    /// input port reads standard input's thread-locals
    /// (`crate::port::Port::stdin`), which a teardown during the thread's
    /// exit would find destroyed.
    standard_ports: [Rc<Port>; 3],
}

impl Instance {
    /// An instance over `fs`, with the command line `("patina")`, never the
    /// host process's flags, the default features, no library query, and
    /// standard ports of its own as its current ports.
    pub fn new(fs: Arc<dyn FileSystem>) -> Self {
        let standard_ports = [Port::stdin(), Port::stdout(), Port::stderr()];
        Self {
            command_line: RefCell::new(vec!["patina".to_owned()]),
            features: RefCell::new(FeatureRegistry::new()),
            features_read: Cell::new(false),
            fs: RefCell::new(fs),
            library_availability: RefCell::new(None),
            current_ports: RefCell::new(standard_ports.clone()),
            standard_ports,
        }
    }

    /// The current port `which`, cloned, so the caller holds no borrow of
    /// the instance while it reads or writes through it. Inlined: a
    /// primitive calls it from another crate on every port-less `display`
    /// and `read-char`.
    #[inline]
    pub fn current_port(&self, which: CurrentPort) -> Rc<Port> {
        Rc::clone(&self.current_ports.borrow()[which as usize])
    }

    /// Install `port` as the current port `which`: the store funnel's
    /// refresh, when the standard port parameter `which` is assigned (#608).
    /// Crate-private, so the slot cannot drift from its parameter. The
    /// slot's hold on the port it replaces is never that port's last: the
    /// port's object holds it too, through the heap's `PortTable`, until a
    /// collection finds the object dead and its sweep lets go (#785). The
    /// funnel calls this under its `&mut Heap`.
    pub(crate) fn set_current_port(&self, which: CurrentPort, port: Rc<Port>) {
        let replaced =
            std::mem::replace(&mut self.current_ports.borrow_mut()[which as usize], port);
        drop(replaced);
    }

    /// Put the standard ports back as the current ports: what
    /// [`Heap::teardown`](crate::Heap::teardown) does, so that a file port
    /// left current is dropped, and so written out, with the heap's other
    /// ports (#618).
    pub(crate) fn reset_current_ports(&self) {
        let replaced = self.current_ports.replace(self.standard_ports.clone());
        drop(replaced);
    }

    /// Scheme's command line.
    pub fn command_line(&self) -> Ref<'_, [String]> {
        Ref::map(self.command_line.borrow(), Vec::as_slice)
    }

    /// Configure `(command-line)`, keeping its program-name element separate
    /// so even a program with no arguments has a nonempty list.
    pub fn set_command_line(&self, program_name: String, arguments: Vec<String>) {
        *self.command_line.borrow_mut() = std::iter::once(program_name).chain(arguments).collect();
    }

    /// The feature set, for a reader that does not resolve against it.
    pub fn features(&self) -> Ref<'_, FeatureRegistry> {
        self.features.borrow()
    }

    /// The feature set, closed to further additions: what `cond-expand`
    /// resolves against.
    pub fn features_and_close(&self) -> Ref<'_, FeatureRegistry> {
        self.features_read.set(true);
        self.features.borrow()
    }

    /// Advertise one more feature, at construction: after the set has been
    /// read, a debug build refuses it, since a `cond-expand` resolved against
    /// the old set would disagree with `(features)`.
    pub fn add_feature(&self, name: &str) {
        debug_assert!(
            !self.features_read.get(),
            "add_feature({name:?}) after the feature set was already read: a \
             `cond-expand` resolved against the old set is already baked into a \
             CoreExpr, so `(features)` would now disagree with it. Advertise \
             features at backend construction, before anything is expanded."
        );
        self.features.borrow_mut().add_feature(name);
    }

    /// The file system.
    pub fn fs(&self) -> Arc<dyn FileSystem> {
        Arc::clone(&self.fs.borrow())
    }

    /// Set the file system, at construction.
    pub fn set_fs(&self, fs: Arc<dyn FileSystem>) {
        *self.fs.borrow_mut() = fs;
    }

    /// Install the runtime's library query, before anything is expanded.
    pub fn set_library_availability(&self, availability: Rc<dyn LibraryAvailability>) {
        *self.library_availability.borrow_mut() = Some(availability);
    }

    /// The runtime's library query, cloned so the caller holds no borrow of
    /// the instance while it asks.
    pub fn library_availability(&self) -> Option<Rc<dyn LibraryAvailability>> {
        self.library_availability.borrow().clone()
    }
}

/// An instance over the native file system.
impl Default for Instance {
    fn default() -> Self {
        Self::new(Arc::new(NativeFs))
    }
}

impl std::fmt::Debug for Instance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Instance")
            .field("command_line", &self.command_line.borrow())
            .field("features", &self.features.borrow())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_keeps_its_program_name() {
        let instance = Instance::default();
        instance.set_command_line("prog".into(), vec![]);
        assert_eq!(&*instance.command_line(), ["prog".to_string()]);
    }

    /// Each instance has its own three ports, and one that installs a port
    /// installs it in its own slot only (#618).
    #[test]
    fn each_instance_keeps_its_own_current_ports() {
        let a = Instance::default();
        let b = Instance::default();
        let standard = [CurrentPort::Input, CurrentPort::Output, CurrentPort::Error];
        for which in standard {
            assert!(!Rc::ptr_eq(&a.current_port(which), &b.current_port(which)));
        }
        let b_output = b.current_port(CurrentPort::Output);
        let sink = Port::new_output_string();
        a.set_current_port(CurrentPort::Output, Rc::clone(&sink));
        assert!(Rc::ptr_eq(&a.current_port(CurrentPort::Output), &sink));
        assert!(Rc::ptr_eq(&b.current_port(CurrentPort::Output), &b_output));
    }

    /// A reset puts back the instance's own standard ports and lets go of
    /// the ones it replaces.
    #[test]
    fn a_reset_puts_the_standard_ports_back() {
        let instance = Instance::default();
        let output = instance.current_port(CurrentPort::Output);
        let sink = Port::new_output_string();
        instance.set_current_port(CurrentPort::Output, Rc::clone(&sink));
        assert_eq!(Rc::strong_count(&sink), 2);
        instance.reset_current_ports();
        assert_eq!(Rc::strong_count(&sink), 1);
        assert!(Rc::ptr_eq(
            &instance.current_port(CurrentPort::Output),
            &output
        ));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "after the feature set was already read")]
    fn a_feature_after_the_set_was_read_is_refused() {
        let instance = Instance::default();
        let _ = instance.features_and_close();
        instance.add_feature("late");
    }
}
