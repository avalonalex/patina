use crate::heap::SharedHeap;
use crate::scope::ScopeSet;
use crate::scope_resolve::AmbiguousReference;
use crate::tagged_value::TaggedValue;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A binding with its associated scope set
/// Used for scope-based hygiene lookup
#[derive(Debug, Clone)]
pub struct ScopedBinding {
    pub scopes: ScopeSet,
    /// Internal storage uses TaggedValue for efficiency
    pub(crate) tagged_value: TaggedValue,
    /// Whether a name-only lookup may fall back to this binding: false for a
    /// macro-introduced parameter, true for a macro-introduced definition.
    /// `define_scoped_definition` is where that difference is decided and
    /// explained.
    pub(crate) visible_by_name: bool,
}

/// The bindings of one name in one environment: one per scope set it is bound
/// at, which is one in all but the rare shadowing case. Inline, so binding a
/// macro-introduced parameter does not allocate a `Vec` per call.
type ScopedBindingList = SmallVec<[ScopedBinding; 1]>;

/// The scoped bindings of one environment, plus the one fact a lookup needs
/// before it is worth searching them.
///
/// The flag exists because the map is almost never empty and almost never
/// relevant: every lambda application binds each parameter here as well as by
/// name (`define_with_scopes`), so a `get` walking a parent chain would hash
/// the name a second time on every frame only to find that none of those
/// parameters is a *definition*. Only `define_scoped_definition` sets the
/// flag, so a frame holding parameters alone answers in one bool load.
///
/// Derefs to the map, so the accessors that only read it are unchanged.
///
/// Whether the table holds a name-visible binding is *not* recorded here but
/// on the environment, as `has_visible_scoped`: that question is asked of
/// every frame a name lookup walks past, and answering it from a field beside
/// the map would cost a `RefCell` borrow per frame to learn there is nothing
/// there. One copy of the fact, in one place, with `insert_scoped` its only
/// writer — two would be a way for a lookup to miss a binding a scoped
/// resolution still finds, which is the shape of triage families 36 and 38.
#[derive(Debug, Default)]
struct ScopedTable {
    map: FxHashMap<Rc<str>, ScopedBindingList>,
}

impl std::ops::Deref for ScopedTable {
    type Target = FxHashMap<Rc<str>, ScopedBindingList>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl std::ops::DerefMut for ScopedTable {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.map
    }
}

/// Where a macro-expansion alias points: the environment holding the real
/// binding, the name it has there, and — for a definition reachable only
/// under its scopes — the scope set that is its identity.
///
/// `env` is `None` for the environment holding the alias. Not an
/// optimisation: an `Rc<Environment>` pointing at the environment that owns
/// the table is a cycle refcounting can never break, so a self-alias would
/// pin that environment — and its heap — for the process. It also saves an
/// `Rc` clone and drop on every alias hit.
///
/// `scopes` is what lets an alias name a *binding* rather than a spelling
/// (#408). A definition a macro introduced is stored by the tree-walker under
/// the scopes it was introduced at, and the bare name reaches at most one of
/// a spelling's definitions — the latest, or none where a plain definition
/// has the name. A generated macro used outside its library has to reach the
/// one its own generator introduced, so its alias carries that definition's
/// scopes and is read and written with them. The VM renames such a
/// definition to a global of its own, so there the alias is a plain one to
/// the renamed name and this stays `None`.
///
/// Every walk reads, writes and locates a target through the three methods
/// below, so that what an alias means is stated once.
#[derive(Debug, Clone)]
struct AliasTarget {
    env: Option<Rc<Environment>>,
    name: Rc<str>,
    scopes: Option<ScopeSet>,
}

impl AliasTarget {
    /// The environment the target is in, given the one `holder` holding the
    /// alias.
    fn env<'a>(&'a self, holder: &'a Environment) -> &'a Environment {
        self.env.as_deref().unwrap_or(holder)
    }

    // The scopes are a definition's *identity*, which resolution chose when
    // the alias was made, so all three match them exactly in the target's own
    // table and cannot disagree on the cell. Resolving them again as a
    // *reference* (`get_with_scopes`) would answer the same while the
    // definition is there, at a candidate list per read — and would fall back
    // by name if it ever were not, reading and writing a plain binding of the
    // spelling where a dangling plain alias answers nothing.

    fn get(&self, holder: &Environment) -> Option<TaggedValue> {
        let env = self.env(holder);
        match &self.scopes {
            None => env.get(&self.name),
            Some(scopes) => env.scoped_definition_value(&self.name, scopes),
        }
    }

    /// `Err` carries the name, as [`Environment::set`]'s does.
    fn set(&self, holder: &Environment, value: TaggedValue) -> Result<(), String> {
        let env = self.env(holder);
        match &self.scopes {
            None => env.set(&self.name, value),
            Some(scopes) => env.set_scoped_definition(&self.name, scopes, value),
        }
    }

    fn location(&self, holder: &Environment) -> Option<BindingLocation> {
        let env = self.env(holder);
        match &self.scopes {
            None => env.binding_location(&self.name),
            Some(scopes) => env.scoped_definition_location(&self.name, scopes),
        }
    }
}

/// Alias name -> the binding it forwards to.
type AliasBindings = FxHashMap<Rc<str>, AliasTarget>;

/// Why a scoped assignment did not happen.
///
/// Two outcomes, kept apart because they call for different diagnostics and
/// because collapsing them is how an ambiguous `set!` came to be reported as
/// an undefined variable. `Undefined` means no binding of the name is
/// reachable; `Ambiguous` means two are and set-of-scopes resolution does not
/// determine which, so nothing was written.
#[derive(Debug)]
pub enum ScopedSetError {
    /// No binding of this name was reachable. Carries the name.
    Undefined(String),
    /// The reference names two bindings, neither containing the other. No cell
    /// was changed.
    Ambiguous(Box<AmbiguousReference>),
}

impl std::fmt::Display for ScopedSetError {
    /// Both arms render a sentence. `Undefined` used to render the bare name,
    /// which reads as a diagnostic only to a caller that already knows what
    /// the string is — and the caller that matches the variant, rather than
    /// formatting it, does not need it at all.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScopedSetError::Undefined(name) => write!(f, "undefined variable: {name}"),
            ScopedSetError::Ambiguous(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ScopedSetError {
    /// The ambiguity is a real error in its own right and is reported
    /// verbatim by [`Display`], so it is also the source. `Undefined` carries
    /// a name rather than an error and has none.
    ///
    /// [`Display`]: std::fmt::Display
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ScopedSetError::Undefined(_) => None,
            ScopedSetError::Ambiguous(e) => Some(e.as_ref()),
        }
    }
}

/// Per spelling, the macro-introduced top-level definitions of that name:
/// the scope set each was introduced at, and the global it was renamed to.
///
/// A map on the inside as well, keyed by the scope set that *is* the
/// binding's identity. A list scanned for that key made recording `n`
/// expansions of one spelling quadratic.
type IntroducedGlobals = FxHashMap<Rc<str>, FxHashMap<ScopeSet, Rc<str>>>;

/// Simple (non-scoped) binding storage: an append-only list of slots, each
/// holding its own name, with a hash index built only for large environments.
///
/// Slots are append-only: a binding's slot never moves or disappears once
/// created, and redefining a name overwrites its slot in place. The VM's
/// per-site global caches rest on this invariant (see `GlobalCacheEntry`
/// in patina-vm), so every mutation must go through these methods.
///
/// Nearly every environment the tree-walker builds is a frame of one or two
/// bindings — a CPS `LetVal` temporary, a lambda's parameters — and it builds
/// several per procedure call. A `HashMap` charges such a frame a table
/// allocation for its first binding and a hash for every lookup, which is
/// most of what the frame costs. So a small frame lives inline and is scanned
/// linearly, and the index is built only when one grows past `LINEAR_MAX`:
/// in practice the global environment and the per-library environments, the
/// ones where a linear scan would actually be the wrong shape.
#[derive(Debug, Default)]
struct Bindings {
    /// The bindings in slot order: a name's slot *is* its index here. One
    /// vector rather than parallel name and value vectors, so there is no
    /// length invariant for `read_slot` to index past and no second
    /// allocation when a frame outgrows its inline capacity.
    slots: SmallVec<[(Rc<str>, TaggedValue); 3]>,
    /// name → slot, present only above `LINEAR_MAX` entries.
    index: Option<Box<FxHashMap<Rc<str>, u32>>>,
}

/// Frames up to this many bindings are searched linearly, without a hash index.
const LINEAR_MAX: usize = 8;

/// Does a stored binding name match the name being looked up?
///
/// The address check is the point. The CPS transform gives a `let`-bound
/// temporary and every reference to it the *same* `Rc<str>`, and the
/// evaluator binds and looks up through that symbol, so on the hot path the
/// two `&str`s are the same slice and an address comparison settles it
/// without touching the bytes. Names that reach here from different symbols —
/// a global, say — fall through to the byte comparison and are unaffected.
#[inline]
fn name_matches(candidate: &str, name: &str) -> bool {
    (std::ptr::eq(candidate.as_ptr(), name.as_ptr()) && candidate.len() == name.len())
        || candidate == name
}

impl Bindings {
    fn slot_of(&self, name: &str) -> Option<u32> {
        match &self.index {
            Some(index) => index.get(name).copied(),
            None => self
                .slots
                .iter()
                .position(|(n, _)| name_matches(n, name))
                .map(|i| i as u32),
        }
    }

    /// What the slot holds, which for an imported binding is
    /// `TaggedValue::FORWARDED` and not a value. Only `Environment`'s
    /// resolving reads call this; there is deliberately no by-name `get`
    /// here, so nothing can read around them.
    fn read_slot(&self, slot: u32) -> TaggedValue {
        self.slots[slot as usize].1
    }

    fn write_slot(&mut self, slot: u32, value: TaggedValue) {
        self.slots[slot as usize].1 = value;
    }

    /// Define semantics: overwrite the existing slot or append a new one.
    ///
    /// Returns the slot, and whether what it overwrote was an imported
    /// binding's marker — in which case the caller has an [`Owner`] to drop.
    /// Asked of the old value rather than of a side table, so the frames the
    /// tree-walker builds by the million pay one compare and nothing else.
    fn insert(&mut self, name: Rc<str>, value: TaggedValue) -> (u32, bool) {
        // A fresh frame is the common case on the tree-walker's hot path —
        // one is built per `let`-bound temporary and per call — and it has
        // nothing to search.
        if !self.slots.is_empty()
            && let Some(slot) = self.slot_of(&name)
        {
            let old = std::mem::replace(&mut self.slots[slot as usize].1, value);
            return (slot, old == TaggedValue::FORWARDED);
        }
        let slot = self.slots.len() as u32;
        match &mut self.index {
            Some(index) => {
                index.insert(Rc::clone(&name), slot);
            }
            // Cross into indexed form *after* pushing, so the new name is in
            // the table the index is built from. `>` rather than `>=`: the
            // index appears on the binding that takes the frame past
            // `LINEAR_MAX`, and `linear_and_indexed_frames_agree_on_slots`
            // pins that the two forms answer alike either side of it.
            None if slot as usize + 1 > LINEAR_MAX => {
                self.slots.push((name, value));
                let mut index = FxHashMap::default();
                index.reserve(self.slots.len());
                for (i, (n, _)) in self.slots.iter().enumerate() {
                    index.insert(Rc::clone(n), i as u32);
                }
                self.index = Some(Box::new(index));
                return (slot, false);
            }
            None => {}
        }
        self.slots.push((name, value));
        (slot, false)
    }

    /// Every bound name, in slot order.
    fn names(&self) -> impl Iterator<Item = &Rc<str>> {
        self.slots.iter().map(|(n, _)| n)
    }

    /// Every bound value, in slot order.
    fn values(&self) -> impl Iterator<Item = TaggedValue> + '_ {
        self.slots.iter().map(|&(_, v)| v)
    }
}

/// Where an imported binding lives: the environment that owns the location,
/// and its slot there.
///
/// R7RS §5.2 has an import name a *binding*, so an importer and the library
/// share one location: what the library assigns later, the importer sees.
/// Installing the value the variable held at import time instead was #406 —
/// every importer read a stale copy, silently, and no suite caught it because
/// none assigns a variable another library imported.
///
/// **Every import shares, a primitive registered from Rust included.** A
/// first version copied those, reasoning that nothing can assign them and
/// that one program's `(set! car …)` should stay out of every other library.
/// Both halves were wrong. A Scheme library can re-export a primitive and
/// assign it later, and its importers then held exactly the stale copy this
/// type exists to remove — before or after the import deciding which. And
/// for a procedure they do not inline, chibi 0.12 and Gauche 0.9.15 agree
/// that the location is one: a program's `(set! list-copy …)` is seen by a
/// library that imported `list-copy`, and a library's by the program
/// (measured 2026-09-19; both rows are in `stdlib/library-bindings.scm`).
/// They differ from each other only where each inlines at compile time —
/// `car` is an opcode in chibi, `length` is inlined by Gauche — and Patina's
/// VM deoptimizes a `CallPrimitive` site when its primitive is rebound
/// precisely so that inlining is never observable. R7RS calls assigning an
/// imported binding "an error", so all of this is latitude; one location is
/// the answer that does not depend on what happened first.
///
/// It is also free. Measured the same day, interleaved with `main`: copying
/// primitives against sharing them made no difference on either backend,
/// including on a loop of nothing but primitive calls.
type Owner = (Rc<Environment>, u32);

/// Which location a name reaches by its by-name view — [`Environment::get`]'s
/// answer as a place rather than as what the place holds.
///
/// Two environments mean one binding by a name exactly when these are equal,
/// which a comparison of values cannot say: two variables that are both `0`
/// are two bindings. Comparable across environments because an import is the
/// exporting library's location ([`Owner`]): a name imported twice, by any
/// routes and under any import sets, is one `Slot`.
///
/// The exception is the one [`Environment::share_binding`] names: an export
/// that is not a plain definition — one a macro introduced under its scopes,
/// or one reached through an alias — is still installed as a copy, so each
/// importer has a `Slot` of its own for it and the library's is a third. The
/// relinker then aliases such a mention to the library's, which is the
/// binding the template meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingLocation {
    /// A plain binding: the id of the environment that owns the location, and
    /// its slot there.
    Slot(u64, u32),
    /// The name-only view of a definition a macro introduced: the environment
    /// holding it, its spelling there, and its place among that spelling's
    /// scoped bindings.
    Scoped(u64, Rc<str>, usize),
}

/// How a top-level definition a macro introduced is held, which is what an
/// alias to it has to say — see [`Environment::introduced_definition`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntroducedDefinition {
    /// The VM's: renamed to this global, a plain binding of the root.
    Renamed(Rc<str>),
    /// The tree-walker's: filed under this scope set, its identity.
    Scoped(ScopeSet),
}

/// The tables that only a global or a library environment ever fills, kept
/// out of line so that the environments which never do — and the tree-walker
/// builds one per call and per `let`-bound temporary — do not carry them.
/// `Environment::rare` has the measurement.
#[derive(Debug, Default)]
struct RareTables {
    /// The identities of the macro-introduced top-level definitions compiled
    /// so far: for each spelling, the scope sets it was introduced at and the
    /// global each was renamed to.
    ///
    /// Read at *compile* time and never on a lookup path, which is what makes
    /// a plain table acceptable here where `Environment`'s side tables carry a
    /// `Cell<bool>` guard. The VM's `alpha_rename` runs once per top-level
    /// form and its frames come from that form alone, so without this a
    /// reference in a later form has no candidate to resolve against,
    /// degrades to its bare spelling, and is answered by whatever the name
    /// means at run time — a user's global of that spelling, or the bare-name
    /// alias. That is Larceny triage family 40, and in the assignment
    /// direction it silently wrote a macro's private state onto a user's
    /// variable.
    ///
    /// Only a parentless global environment ever holds entries: they are
    /// installed beside the aliases in the VM's `compile_pipeline`, which
    /// asserts that. Lookups therefore do not walk parents.
    introduced_global_names: RefCell<IntroducedGlobals>,
    /// Per slot, the [`Owner`] of the binding when that is not this
    /// environment; the slot itself then holds `TaggedValue::FORWARDED`.
    /// Indexed by slot and grown on demand, so `None` — or a slot past the
    /// end — is an ordinary binding, which nearly every slot of nearly every
    /// environment is.
    ///
    /// Always the environment that *owns* the location, never another
    /// importer: `share_binding` resolves through a source that is itself
    /// forwarded, so a read is one hop however many libraries re-exported
    /// the name.
    ///
    /// Reads never ask whether this table exists: a forwarded slot announces
    /// itself by its marker, so the read path pays one compare, and so does
    /// the one writer that must drop an entry — `define` over an import.
    links: RefCell<Vec<Option<Owner>>>,
    /// For each imported location something here has been *early-bound* to,
    /// the hidden names that hold it — see [`Environment::import_alias`].
    /// Keyed by the owner's environment id and slot, then by the spelling that
    /// was asked about: one name for one location has one alias however many
    /// expansions ask. Nearly always one entry; a location imported under two
    /// names has two, because the desugarer reads a spelling back off an
    /// alias (`Desugarer::settle_early_bindings`).
    import_aliases: RefCell<FxHashMap<(u64, u32), ImportAliases>>,
    /// The distinct environments `links` points into, for the collector. An
    /// importer of `(scheme base)` holds several hundred links into a dozen
    /// environments, and a collection should look at the dozen. Only ever
    /// added to: an owner whose last link was defined over stays listed,
    /// which keeps a library's environment reachable for as long as one of
    /// its importers is — and the registry does that anyway.
    owners: RefCell<Vec<Rc<Environment>>>,
}

/// The aliases for one imported location: the spelling each was asked for
/// under, and the alias. A scan, not a table — it holds one entry, or two.
type ImportAliases = SmallVec<[(Rc<str>, Rc<str>); 1]>;

/// Mint a process-unique, never-reused environment id (0 is reserved as the
/// "empty" sentinel in the VM's global caches).
fn fresh_env_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Environment for variable bindings
///
/// Uses Rc<RefCell<>> for shared mutable state (needed for set!)
///
/// ## TaggedValue Storage
///
/// Environment internally stores values as `TaggedValue` for memory efficiency:
/// - 8 bytes per binding vs 64+ bytes with `Value`
/// - Faster cloning when closures capture environments
/// - API accepts/returns `Value` for compatibility (converted at boundaries)
///
/// ## Hygiene Support
///
/// Uses Racket-style scope sets hygiene (based on "Binding as Sets of Scopes", Flatt 2016):
///
/// - `bindings`: Simple name → value mapping for top-level bindings (built-ins, global defines)
/// - `scoped_bindings`: name → Vec<ScopedBinding> mapping for hygienic bindings
///   - Each binding has an associated scope set
///   - Lookup finds binding where `binding.scopes ⊆ reference.scopes`
///   - Most specific (largest) matching scope set wins
///
/// Lookup order for `get_with_scopes(name, scopes)`:
/// 1. Collect all bindings where `binding.scopes ⊆ scopes`
/// 2. Return the binding with the largest scope set (most specific)
/// 3. Fall back by name — to simple bindings and aliases, and to the
///    name-only view of a scoped definition **only when this resolution did
///    not just reject it**. A binding the rule refused stays refused; the
///    fallback answering it anyway was triage family 36.
///
/// ## Deliberately not `Clone`
///
/// The binding tables are owned inline, so a derived `Clone` would *fork*
/// them while copying `env_id` — and `env_id` is what patina-vm's per-site
/// global cache keys a resolved slot on (`GlobalCacheEntry` in
/// `patina-vm/src/types/code_object.rs`). Two environments sharing an id and
/// disagreeing about a name would hand that cache a hit against the wrong
/// table and return the wrong value silently. An environment is shared by its
/// `Rc` and by nothing else; `gc_identity` rests on the same rule. Do not add
/// the derive.
#[derive(Debug)]
pub struct Environment {
    /// Shared heap for TaggedValue interpretation
    heap: SharedHeap,
    /// Simple name-based bindings (for built-ins and top-level)
    /// Stores TaggedValue internally for memory efficiency
    bindings: RefCell<Bindings>,
    /// Process-unique id for this environment, minted at construction and
    /// shared by every holder of its `Rc`. Never reused, unlike an address —
    /// see `GlobalCacheEntry` in patina-vm for the cache soundness argument,
    /// and `gc_identity` for the address-based identity the GC uses to dedup
    /// *live* environments.
    env_id: u64,
    /// Scope-aware bindings (for scope sets hygiene)
    /// Each name can have multiple bindings with different scope sets
    scoped_bindings: RefCell<ScopedTable>,
    /// Bindings installed by macro expansion that point at a binding in
    /// another environment instead of holding a value.
    ///
    /// A `syntax-rules` template may reference a name bound where the macro was
    /// *defined* — typically a library-private helper — which does not exist
    /// where the macro is *used*. Expansion renames such a reference to a
    /// unique name and records the alias here, so the use site can resolve it.
    ///
    /// The indirection is deliberate: resolving through the alias on every
    /// lookup means a later `set!` on the original binding is visible, which
    /// copying the value at expansion time would silently freeze.
    alias_bindings: RefCell<AliasBindings>,
    /// Whether `alias_bindings` holds anything, and whether `scoped_bindings`
    /// holds a name-visible definition.
    ///
    /// Both questions are asked of every frame a name lookup walks past, and
    /// the answer is no for almost every frame — a lambda's parameters are
    /// scoped bindings but not *visible* ones, and aliases exist only where a
    /// macro was expanded. Kept outside the `RefCell`s they describe so that
    /// walking a chain of frames costs a load per frame rather than two
    /// borrow-flag round trips.
    ///
    /// These are the **only** copies of those two facts, and each has exactly
    /// one writer — `define_alias` and `insert_scoped`. A second copy stored
    /// beside the table it describes is how a lookup comes to miss a binding
    /// that scoped resolution still finds, which is what triage families 36
    /// and 38 were. Both latch on and never clear: nothing removes a binding
    /// from either table, so a `true` is permanent and a stale one would only
    /// cost a borrow, never an answer.
    has_aliases: Cell<bool>,
    has_visible_scoped: Cell<bool>,
    /// The tables only a global or a library environment ever fills
    /// ([`RareTables`]), absent until one of them is first written and one
    /// pointer wide either way.
    ///
    /// Out of line because of what this struct's size costs. The tree-walker
    /// builds an environment per call and per `let`-bound temporary, and none
    /// of those ever imports anything or records a macro-introduced global.
    /// Measured 2026-09-19 on four loops, best of five, interleaved with
    /// `main`: adding the link table inline took this struct from 256 bytes
    /// to 264 and that backend 3–4% slower on every program, imports or no;
    /// moving both tables here took it to 224 and 4–6% *faster* than `main`.
    /// `environment_size_is_watched` holds the line.
    rare: OnceCell<Box<RareTables>>,
    parent: Option<Rc<Environment>>,
}

impl Environment {
    /// Create a new empty environment with a new heap
    pub fn new() -> Self {
        Self::with_heap(crate::heap::new_shared_heap())
    }

    /// Create a new empty environment with a shared heap
    pub fn with_heap(heap: SharedHeap) -> Self {
        Environment {
            heap,
            bindings: RefCell::new(Bindings::default()),
            env_id: fresh_env_id(),
            scoped_bindings: RefCell::new(ScopedTable::default()),
            alias_bindings: RefCell::new(FxHashMap::default()),
            has_aliases: Cell::new(false),
            has_visible_scoped: Cell::new(false),
            rare: OnceCell::new(),
            parent: None,
        }
    }

    /// Create a new environment with a parent (shares the parent's heap)
    pub fn with_parent(parent: Rc<Environment>) -> Self {
        Environment {
            heap: parent.heap.clone(),
            bindings: RefCell::new(Bindings::default()),
            env_id: fresh_env_id(),
            scoped_bindings: RefCell::new(ScopedTable::default()),
            alias_bindings: RefCell::new(FxHashMap::default()),
            has_aliases: Cell::new(false),
            has_visible_scoped: Cell::new(false),
            rare: OnceCell::new(),
            parent: Some(parent),
        }
    }

    /// Process-unique, never-reused id for this environment's simple
    /// bindings. Always non-zero.
    #[inline(always)]
    pub fn env_id(&self) -> u64 {
        self.env_id
    }

    /// Slot index of `name` in *this* environment's simple bindings
    /// (parents are not consulted). Slot indices are stable for the life
    /// of the environment: redefinition overwrites the slot in place.
    #[inline]
    pub fn local_slot(&self, name: &str) -> Option<u32> {
        self.bindings.borrow().slot_of(name)
    }

    /// Read the value in a slot previously obtained from `local_slot`.
    ///
    /// An imported binding is read from the library that owns it, so this is
    /// the *binding's* current value, never a snapshot.
    #[inline]
    pub fn slot_value(&self, slot: u32) -> TaggedValue {
        let tv = self.bindings.borrow().read_slot(slot);
        if tv == TaggedValue::FORWARDED {
            return self.shared_value(slot);
        }
        tv
    }

    /// Overwrite the value in a slot previously obtained from `local_slot`.
    ///
    /// An imported binding is written where it lives: the library sees the
    /// assignment, as it does under chibi and Gauche. R7RS §5.2 calls
    /// assigning an imported binding "an error", so any answer conforms; this
    /// is the one that keeps a location one location.
    #[inline]
    pub fn set_slot_value(&self, slot: u32, value: TaggedValue) {
        let forwarded = {
            let mut bindings = self.bindings.borrow_mut();
            let forwarded = bindings.read_slot(slot) == TaggedValue::FORWARDED;
            if !forwarded {
                bindings.write_slot(slot, value);
            }
            forwarded
        };
        if forwarded {
            self.write_shared(slot, value);
        }
    }

    /// The owner of a slot that holds the marker.
    fn shared_target(&self, slot: u32) -> Owner {
        // `forward` writes the marker and the link together and `define`
        // drops them together, so a marker without a link is a bug here, not
        // a state a program can reach.
        self.owner_of(slot)
            .unwrap_or_else(|| unreachable!("slot {slot} is forwarded but has no owner"))
    }

    /// Kept out of line: `slot_value` is inlined into the VM's `LoadGlobal`,
    /// and most globals a program reads there are its own. Measured
    /// 2026-09-19: a read that comes through here costs the VM about 2 ns
    /// more than one that does not, and a loop calling `for-each` — imported,
    /// and defined in Scheme — runs at parity with `main`.
    ///
    /// Reads the owner under this table's borrow rather than through
    /// `shared_target`, which would clone the owner's `Rc` — a count up and
    /// down on every read of an imported global. Nothing on a read path
    /// borrows a link table mutably, so holding this one across the owner's
    /// read is safe.
    #[inline(never)]
    fn shared_value(&self, slot: u32) -> TaggedValue {
        let links = self.rare.get().map(|rare| rare.links.borrow());
        match links.as_ref().and_then(|links| links.get(slot as usize)) {
            Some(Some((owner, owner_slot))) => owner.slot_value(*owner_slot),
            // As `shared_target` says: not a state a program can reach.
            _ => unreachable!("slot {slot} is forwarded but has no owner"),
        }
    }

    #[inline(never)]
    fn write_shared(&self, slot: u32, value: TaggedValue) {
        let (owner, owner_slot) = self.shared_target(slot);
        owner.set_slot_value(owner_slot, value);
    }

    /// The owner of `slot`'s binding, when that is not this environment.
    fn owner_of(&self, slot: u32) -> Option<Owner> {
        self.rare
            .get()
            .and_then(|rare| rare.links.borrow().get(slot as usize).cloned())
            .flatten()
    }

    fn set_owner(&self, slot: u32, owner: Owner) {
        let rare = self.rare.get_or_init(Default::default);
        {
            let mut owners = rare.owners.borrow_mut();
            if !owners.iter().any(|known| Rc::ptr_eq(known, &owner.0)) {
                owners.push(Rc::clone(&owner.0));
            }
        }
        let mut links = rare.links.borrow_mut();
        let slot = slot as usize;
        if links.len() <= slot {
            links.resize(slot + 1, None);
        }
        links[slot] = Some(owner);
    }

    /// The slot is this environment's own from now on.
    fn drop_owner(&self, slot: u32) {
        if let Some(rare) = self.rare.get()
            && let Some(link) = rare.links.borrow_mut().get_mut(slot as usize)
        {
            *link = None;
        }
    }

    /// Get the shared heap
    pub fn heap(&self) -> &SharedHeap {
        &self.heap
    }

    /// Define a new binding in this environment
    ///
    /// Use this for top-level defines, built-ins, and other simple bindings.
    /// This is the primary API - accepts TaggedValue directly.
    pub fn define(&self, name: impl Into<Rc<str>>, value: TaggedValue) {
        let (slot, was_forwarded) = self.define_slot(name.into(), value);
        // A definition is a new binding of this environment's own, whatever
        // the name was before — so an importer that defines a name it
        // imported shadows it here and leaves the library's alone, which is
        // what chibi and Gauche do.
        if was_forwarded {
            self.drop_owner(slot);
        }
    }

    /// Make `name` here the same binding as `source_name` in `source` — what
    /// an import does. `false`, having done nothing, when `source` has no
    /// plain binding of that name for this to be: the caller then installs
    /// the value it has, which is what every import used to do.
    ///
    /// That fallback is reached by an export that is not a plain definition —
    /// one a macro introduced under its scopes, or one reached through a
    /// macro-expansion alias. Those stay a copy (#406 leaves them as they
    /// were).
    ///
    /// See [`Owner`] for why every import shares, primitives included.
    pub fn share_binding(
        &self,
        name: impl Into<Rc<str>>,
        source: &Rc<Environment>,
        source_name: &str,
    ) -> bool {
        self.take_binding(name.into(), source, source_name, true)
    }

    /// Bring a binding out of a *staging* environment — the scratch one an
    /// `only`, `except`, `prefix` or `rename` import set is resolved into
    /// before its names are filtered or changed.
    ///
    /// The difference from [`share_binding`] is who may own a location. A
    /// library owns what it defined; a staging environment owns nothing, it
    /// only carries bindings through. So a forwarded slot arrives as the same
    /// forward, and a plain value — the fallback copy described on
    /// `share_binding` — as that value, rather than as a forward into an
    /// environment about to be dropped.
    ///
    /// [`share_binding`]: Self::share_binding
    pub fn copy_binding(
        &self,
        name: impl Into<Rc<str>>,
        staging: &Rc<Environment>,
        staged_name: &str,
    ) -> bool {
        self.take_binding(name.into(), staging, staged_name, false)
    }

    fn take_binding(
        &self,
        name: Rc<str>,
        source: &Rc<Environment>,
        source_name: &str,
        source_owns: bool,
    ) -> bool {
        let Some(slot) = source.local_slot(source_name) else {
            return false;
        };
        let owner = match source.owner_of(slot) {
            Some(owner) => owner,
            None if source_owns => (Rc::clone(source), slot),
            None => {
                self.define(name, source.slot_value(slot));
                return true;
            }
        };
        self.forward(name, owner);
        true
    }

    /// `define` without its bookkeeping: writes the slot, and reports it and
    /// whether an imported binding was there before.
    #[inline]
    fn define_slot(&self, name: Rc<str>, value: TaggedValue) -> (u32, bool) {
        // A plain binding is reachable by name and by nothing else, so
        // `byname=true` here is a property of the table, not a decision.
        crate::scope_trace::bind(&name, &ScopeSet::new(), true);
        self.bindings.borrow_mut().insert(name, value)
    }

    fn forward(&self, name: Rc<str>, owner: Owner) {
        // An environment cannot hold an `Rc` to itself without leaking, and
        // has no need to: a name that would forward to this environment is a
        // second name for a value already here.
        if std::ptr::eq(Rc::as_ptr(&owner.0), self) {
            if self.local_slot(&name) != Some(owner.1) {
                self.define(name, self.slot_value(owner.1));
            }
            return;
        }
        let (slot, _) = self.define_slot(name, TaggedValue::FORWARDED);
        self.set_owner(slot, owner);
    }

    /// A name of this environment's own for the *location* `name` is imported
    /// from, which goes on meaning that location whatever `name` later comes
    /// to mean here — or `None` when `name` is not an import of this
    /// environment's.
    ///
    /// For a macro template's reference to a name its use site imported
    /// (#438). Such a reference is a bare global name, looked up when it runs,
    /// and `define` over an import rightly gives the *importer* a binding of
    /// its own under that name: a program that defined `count`, or `car`,
    /// after using a library macro whose template mentions it captured the
    /// template's reference. Bound to this alias instead, the reference is to
    /// the location.
    ///
    /// The alias is an ordinary forwarded slot, so it reads, writes and caches
    /// like the import it stands beside; it is minted once per name for a
    /// location (`mint` runs only then) and shared by every expansion after.
    ///
    /// Per *name*, not per location alone: a location this environment
    /// imported under two names — `car`, and `first` by a `rename` — gets an
    /// alias for each. The desugarer takes an early binding back by reading
    /// the spelling off the alias, and one alias for both spellings settled a
    /// reference written `first` by what the form said about `car`.
    ///
    /// Being an ordinary slot, it can be defined over like one, by a program
    /// that happens to spell it — `car.17` is a legal identifier, and
    /// machine-written Scheme is full of names like it. The remembered alias
    /// is therefore checked to still *be* the forward it was minted as, and
    /// replaced when it is not: what was bound to the old one is lost to that
    /// definition, but nothing expanded afterwards follows it there. For the
    /// same reason `mint` is asked again while the spelling it offers is
    /// already bound here, since forwarding a name overwrites what it held.
    /// (Only a *reference* is ever given this name — `Desugarer::early_bound`
    /// — so nothing Patina does defines over one.)
    pub fn import_alias(&self, name: &str, mut mint: impl FnMut() -> Rc<str>) -> Option<Rc<str>> {
        let owner = self.owner_of(self.local_slot(name)?)?;
        let key = (owner.0.env_id(), owner.1);
        let rare = self.rare.get()?;
        // Out of the borrow first: `owner_of` borrows a table beside it.
        let known = rare.import_aliases.borrow().get(&key).and_then(|aliases| {
            aliases
                .iter()
                .find(|(spelling, _)| &**spelling == name)
                .map(|(_, alias)| Rc::clone(alias))
        });
        if let Some(alias) = known {
            let still_forwards = self
                .local_slot(&alias)
                .and_then(|slot| self.owner_of(slot))
                .is_some_and(|(env, slot)| Rc::ptr_eq(&env, &owner.0) && slot == owner.1);
            if still_forwards {
                return Some(alias);
            }
        }
        let alias = loop {
            let candidate = mint();
            if self.local_slot(&candidate).is_none() {
                break candidate;
            }
        };
        self.forward(Rc::clone(&alias), owner);
        let mut table = rare.import_aliases.borrow_mut();
        let aliases = table.entry(key).or_default();
        match aliases.iter_mut().find(|(spelling, _)| &**spelling == name) {
            // Replacing one that was defined over.
            Some((_, stale)) => *stale = Rc::clone(&alias),
            None => aliases.push((Rc::from(name), Rc::clone(&alias))),
        }
        Some(alias)
    }

    /// The value of a binding in this environment's own table, read through
    /// to its owner when it was imported. Every by-name read of the table
    /// goes through here; `Bindings` has no `get` of its own, so none can
    /// hand out the marker.
    #[inline(always)]
    fn local_value(&self, name: &str) -> Option<TaggedValue> {
        let (slot, tv) = {
            let bindings = self.bindings.borrow();
            let slot = bindings.slot_of(name)?;
            (slot, bindings.read_slot(slot))
        };
        if tv == TaggedValue::FORWARDED {
            return Some(self.shared_value(slot));
        }
        Some(tv)
    }

    /// Define a primitive procedure in this environment.
    ///
    /// Consolidates the common pattern of creating a `Procedure::Primitive`
    /// and converting it to TaggedValue for storage.
    ///
    /// The `library` parameter (e.g., `["scheme", "base"]`) is joined with "."
    /// and combined with `name` to produce a `qualified_name` (e.g., `"scheme.base/+"`).
    /// This qualified name is computed once here at init time, avoiding repeated
    /// `format!()` allocations on every primitive call.
    pub fn define_primitive(
        &self,
        name: &'static str,
        arity: crate::procedure::Arity,
        library: Vec<String>,
    ) {
        let qualified_name: Rc<str> = Rc::from(format!("{}/{}", library.join("."), name));
        let proc = crate::procedure::Procedure::primitive(name, arity, qualified_name, None);
        let tv = self.heap.borrow_mut().alloc_procedure(proc);
        self.define(name, tv);
    }

    /// Set an existing binding (searches parent environments)
    /// This is the primary API - accepts TaggedValue directly.
    pub fn set(&self, name: &str, value: TaggedValue) -> Result<(), String> {
        if let Some(slot) = self.local_slot(name) {
            self.set_slot_value(slot, value);
            return Ok(());
        }
        // Assign through a macro-expansion alias, so a template that mutates a
        // binding private to its defining library works. Reads follow aliases
        // in `get`; writes have to as well or the two disagree.
        if let Some(target) = self.alias_target(name) {
            return target.set(self, value);
        }
        // The write side of the same fallback `get` takes, and it has to be
        // here for the reason `alias_bindings` gives for its own pair: a name
        // that reads through to a scoped definition and does not write through
        // to it leaves the two disagreeing. Reached when a macro-generated
        // macro's template assigns to a definition its expansion introduced,
        // which arrives relinked to the bare name.
        if let Some(i) = self.visible_scoped_index(name)
            && let Some(bindings) = self.scoped_bindings.borrow_mut().get_mut(name)
        {
            bindings[i].tagged_value = value;
            return Ok(());
        }
        match &self.parent {
            Some(parent) => parent.set(name, value),
            None => Err(name.to_string()),
        }
    }

    /// Get a TaggedValue from the environment (searches parent environments)
    ///
    /// This is the primary API - returns TaggedValue directly.
    /// For the simple name-based lookup for identifiers.
    pub fn get(&self, name: &str) -> Option<TaggedValue> {
        // Walked iteratively rather than by recursing into the parent: the
        // tree-walker builds a frame per `let`-bound temporary, so this chain
        // is the hottest loop in the backend.
        let mut env = self;
        loop {
            if let Some(tv) = env.local_value(name) {
                return Some(tv);
            }
            // Follow a macro-expansion alias into the environment the macro was
            // defined in. Checked after real bindings so a local definition always
            // wins, and before the parent so the alias is not shadowed by an
            // unrelated outer binding of the same (unique) name.
            if env.has_aliases.get()
                && let Some(target) = env.alias_target(name)
            {
                return target.get(env);
            }
            // A macro-introduced definition lives under its scopes; this is the
            // name-only view of it. Checked after real bindings and aliases, so a
            // binding written in source always wins.
            if env.has_visible_scoped.get()
                && let Some(i) = env.visible_scoped_index(name)
            {
                return env.scoped_bindings.borrow()[name]
                    .get(i)
                    .map(|b| b.tagged_value);
            }
            env = env.parent.as_deref()?;
        }
    }

    /// The location `get(name)` reads, or `None` where `get` finds nothing.
    ///
    /// The same walk as [`get`], step for step — a plain binding, then a
    /// macro-expansion alias, then the name-only view of a scoped definition,
    /// then the parent — because the two must agree on *which* binding the
    /// name reaches, or this would name one and `get` read another. One
    /// shared walk is Track Q's Q7.3; until then they mirror by hand.
    ///
    /// [`get`]: Self::get
    pub fn binding_location(&self, name: &str) -> Option<BindingLocation> {
        let mut env = self;
        loop {
            if let Some(slot) = env.local_slot(name) {
                return Some(match env.owner_of(slot) {
                    Some((owner, owner_slot)) => BindingLocation::Slot(owner.env_id(), owner_slot),
                    None => BindingLocation::Slot(env.env_id(), slot),
                });
            }
            if env.has_aliases.get()
                && let Some(target) = env.alias_target(name)
            {
                return target.location(env);
            }
            if let Some(i) = env.visible_scoped_index(name) {
                let table = env.scoped_bindings.borrow();
                let (spelling, _) = table.get_key_value(name)?;
                return Some(BindingLocation::Scoped(
                    env.env_id(),
                    Rc::clone(spelling),
                    i,
                ));
            }
            env = env.parent.as_deref()?;
        }
    }

    /// The location of the scoped definition of `name` whose identity is
    /// exactly `scopes`, in this environment's own table.
    fn scoped_definition_location(&self, name: &str, scopes: &ScopeSet) -> Option<BindingLocation> {
        let table = self.scoped_bindings.borrow();
        let (spelling, bindings) = table.get_key_value(name)?;
        let index = bindings.iter().position(|b| b.scopes == *scopes)?;
        Some(BindingLocation::Scoped(
            self.env_id(),
            Rc::clone(spelling),
            index,
        ))
    }

    /// What that definition holds — [`scoped_definition_location`]'s read.
    ///
    /// [`scoped_definition_location`]: Self::scoped_definition_location
    fn scoped_definition_value(&self, name: &str, scopes: &ScopeSet) -> Option<TaggedValue> {
        let table = self.scoped_bindings.borrow();
        let binding = table.get(name)?.iter().find(|b| b.scopes == *scopes)?;
        Some(binding.tagged_value)
    }

    /// Assign that definition — [`scoped_definition_location`]'s write. `Err`
    /// carries the name, as [`set`]'s does.
    ///
    /// [`scoped_definition_location`]: Self::scoped_definition_location
    /// [`set`]: Self::set
    fn set_scoped_definition(
        &self,
        name: &str,
        scopes: &ScopeSet,
        value: TaggedValue,
    ) -> Result<(), String> {
        let mut table = self.scoped_bindings.borrow_mut();
        let binding = table
            .get_mut(name)
            .and_then(|bindings| bindings.iter_mut().find(|b| b.scopes == *scopes));
        match binding {
            Some(binding) => {
                binding.tagged_value = value;
                Ok(())
            }
            None => Err(name.to_string()),
        }
    }

    /// Whether the root of this chain holds *any* top-level definition of
    /// `name` that a macro introduced — the cheap question that lets a caller
    /// skip [`introduced_definition`] for the names it cannot answer, which
    /// is nearly all of them (`list`, `if`, `+`).
    ///
    /// [`introduced_definition`]: Self::introduced_definition
    pub fn has_introduced_definition(&self, name: &str) -> bool {
        // Walked by reference: `root` clones an `Rc` per frame, and this is
        // asked for every name a generated macro mentions.
        let mut root = self;
        while let Some(parent) = root.parent.as_deref() {
            root = parent;
        }
        let renamed = root.rare.get().is_some_and(|rare| {
            rare.introduced_global_names
                .borrow()
                .get(name)
                .is_some_and(|identities| !identities.is_empty())
        });
        renamed
            || root
                .scoped_bindings
                .borrow()
                .get(name)
                .is_some_and(|bindings| !bindings.is_empty())
    }

    /// The *top-level* definition a macro introduced that `scopes` selects
    /// for `name`, as something an alias can point at — or `None` when
    /// `scopes` selects anything else: nothing, a plain binding, or a lexical
    /// one.
    ///
    /// For relinking a generated macro's mention of a definition its own
    /// generator introduced (#408). The bare name does not identify such a
    /// definition: it reaches the latest of that spelling, or a plain
    /// definition that has the name. The mention's scopes do, by the same
    /// rule every scoped reference resolves with.
    ///
    /// Only a definition held by the **root** of this chain qualifies. A
    /// scoped binding in a child frame is a lexical variable — a `let`'s, a
    /// parameter's, an internal define's — which lives in a run-time frame the
    /// use site cannot be given a name for, and which ordinary set-of-scopes
    /// resolution is already the right judge of.
    ///
    /// The two backends store such a definition differently, and the answer
    /// says which: the VM renames it to a global of its own and records the
    /// identity ([`define_introduced_global`]); the tree-walker files it under
    /// its scopes.
    ///
    /// [`define_introduced_global`]: Self::define_introduced_global
    pub fn introduced_definition(
        self: &Rc<Self>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Option<(Rc<Environment>, IntroducedDefinition)> {
        let chosen = self.scoped_binding_of(name, scopes).ok()??;
        let root = self.root();
        if let Some(renamed) = root.introduced_global(name, &chosen) {
            return Some((root, IntroducedDefinition::Renamed(renamed)));
        }
        root.scoped_definition_location(name, &chosen)
            .is_some()
            .then(|| (root, IntroducedDefinition::Scoped(chosen)))
    }

    /// The global the macro-introduced top-level definition of `name` at
    /// exactly `scopes` was renamed to, if this environment recorded one
    /// ([`define_introduced_global`]). A keyed lookup: the table is keyed by
    /// the scope set so that nothing has to scan a spelling's identities.
    ///
    /// [`define_introduced_global`]: Self::define_introduced_global
    fn introduced_global(&self, name: &str, scopes: &ScopeSet) -> Option<Rc<str>> {
        self.rare
            .get()?
            .introduced_global_names
            .borrow()
            .get(name)?
            .get(scopes)
            .cloned()
    }

    /// Resolve a macro-expansion alias installed here, if any.
    ///
    /// The emptiness check matters: this sits on every global lookup that falls
    /// through to a parent, and almost every environment has no aliases at all.
    fn alias_target(&self, name: &str) -> Option<AliasTarget> {
        let aliases = self.alias_bindings.borrow();
        if aliases.is_empty() {
            return None;
        }
        aliases.get(name).cloned()
    }

    /// Record that a macro-introduced top-level definition of `name`, at
    /// `scopes`, was renamed to the global `renamed_to`.
    ///
    /// The companion of [`define_alias`] for the same definition: the alias
    /// answers the *bare* spelling at run time, which is what
    /// definition-environment relinking asks for, and this records the
    /// *binding identity*, which is what a later form's scoped reference
    /// needs. Keeping both is deliberate. Dropping the alias would strand
    /// relinking; dropping this leaves a later form resolving by spelling,
    /// which is triage family 40.
    ///
    /// Two different expansions get different scope sets and so are different
    /// entries, which is the whole point — and which means a program that
    /// expands such a macro `n` times records `n` entries. That is
    /// proportional to something that already grows: each of those expansions
    /// also defines its own global, under the name recorded here, and those
    /// are never reclaimed either.
    ///
    /// Keyed by the scope set rather than scanned for it, which is not a
    /// micro-optimisation: a linear scan per insert made compiling `n`
    /// expansions of one spelling quadratic, measured at 12x main for
    /// n = 12000. The same key makes a repeated identical `(name, scopes)`
    /// overwrite, which happens when one expansion is compiled twice.
    ///
    /// [`define_alias`]: Self::define_alias
    pub fn define_introduced_global(&self, name: Rc<str>, scopes: ScopeSet, renamed_to: Rc<str>) {
        self.rare
            .get_or_init(Default::default)
            .introduced_global_names
            .borrow_mut()
            .entry(name)
            .or_default()
            .insert(scopes, renamed_to);
    }

    /// Call `f` with every macro-introduced top-level definition of `name`:
    /// the scope set it was introduced at, and the global it was renamed to.
    ///
    /// A callback rather than a returned collection because the caller keeps
    /// only the candidates for one reference, and building the whole list
    /// first meant cloning every entry — a scope set and an `Rc` apiece — on
    /// every scoped reference to the spelling, then discarding most of them.
    ///
    /// What remains is a scan: a subset query has no better shape, so a
    /// reference costs one visit per definition of its spelling. Measured
    /// against a build without any of this, on a file of `n` expansions of
    /// one macro introducing one spelling: at n = 500, indistinguishable
    /// (0.01 s both); at n = 12000, 0.68 s against 0.12 s. The second is not
    /// a shape real code has — it is twelve thousand expansions of a single
    /// macro — and it is recorded so the next person to see this scan knows
    /// it was measured rather than overlooked.
    ///
    /// The order entries arrive in is unspecified, and nothing may depend on
    /// it. Ties are what an order would decide, and there are none to decide:
    /// each scope set appears once, and two distinct sets that are both
    /// candidates for one reference are refused as ambiguous by
    /// [`crate::scope_resolve::resolve_index`] rather than settled by
    /// position.
    ///
    /// Parents are not walked: only a parentless global environment holds
    /// these, as [`define_introduced_global`] describes.
    ///
    /// [`define_introduced_global`]: Self::define_introduced_global
    pub fn for_each_introduced_global(&self, name: &str, mut f: impl FnMut(&ScopeSet, &Rc<str>)) {
        let Some(rare) = self.rare.get() else {
            return;
        };
        if let Some(entries) = rare.introduced_global_names.borrow().get(name) {
            for (scopes, renamed_to) in entries {
                f(scopes, renamed_to);
            }
        }
    }

    /// The [`env_id`] of the root of this environment's parent chain — for
    /// asking whether two environments belong to one program or library
    /// without cloning an `Rc` per frame, which [`root`] does.
    ///
    /// [`env_id`]: Self::env_id
    /// [`root`]: Self::root
    pub fn root_id(&self) -> u64 {
        let mut env = self;
        while let Some(parent) = env.parent.as_deref() {
            env = parent;
        }
        env.env_id
    }

    /// The root of this environment's parent chain.
    ///
    /// Macro-expansion aliases must be installed where the code will actually
    /// be resolved. A desugar-time environment may be a transient child made
    /// for a `let-syntax` or internal-define body and dropped once desugaring
    /// finishes, so aliases go to the root instead.
    pub fn root(self: &Rc<Self>) -> Rc<Environment> {
        let mut env = self.clone();
        while let Some(parent) = env.parent.clone() {
            env = parent;
        }
        env
    }

    /// Install a macro-expansion alias: `alias` resolves to `target_name` as
    /// bound in `target_env`, looked up afresh on every access.
    ///
    /// Two kinds of caller, and they rely on different things:
    ///
    /// - The desugarer installs a **generated, unique** `alias`, so it cannot
    ///   shadow anything the program wrote.
    /// - The VM's compiler installs one under a **bare** name, for a
    ///   macro-introduced global it renamed. `get` consults `bindings` first,
    ///   so a real binding of that name wins — which is what keeps a macro's
    ///   temporary from overwriting a user's global of the same spelling. It
    ///   used to be why a user's later global of that spelling stole the
    ///   macro's private definition, `(jab get 10) (define mh 99) (get)`; that
    ///   answers 10 now, as chibi and Gauche do, because the renamer resolves
    ///   such a reference to the introduced global's identity
    ///   (`define_introduced_global`) and never asks the alias. What the alias
    ///   still answers is anything reaching the name that no identity
    ///   accepts: a scoped reference from a different expansion (triage family
    ///   40), a source reference to the bare name, a library export of the bare
    ///   name, and the definition-environment relinker's by-name lookup of its
    ///   target. `PRD/macro/SYNTAX_CASE_DESIGN.md`, "Scoped Relinking, Sized",
    ///   measures each and what removing the alias takes.
    ///
    ///   The bare kind is sound only in an environment with **no parent**,
    ///   since `get` *returns* on an alias hit rather than falling through, so
    ///   one whose target is unbound would eclipse a parent's binding. The
    ///   environments that path compiles against are the parentless global
    ///   ones, asserted at the install site.
    ///
    /// Keyed by `alias`, so a second install under the same name replaces the
    /// first. For the bare-name kind that means the most recently compiled
    /// definition of a given spelling is the one relinking reaches.
    pub fn define_alias(
        &self,
        alias: impl Into<Rc<str>>,
        target_env: Rc<Environment>,
        target_name: Rc<str>,
    ) {
        self.install_alias(alias.into(), target_env, target_name, None);
    }

    /// [`define_alias`] to the definition [`introduced_definition`] found:
    /// `found`, of `name`, held by `home`. How the backend holds it decides
    /// what kind of alias that is, and deciding it here is what keeps the
    /// relinker from having to know.
    ///
    /// [`define_alias`]: Self::define_alias
    /// [`introduced_definition`]: Self::introduced_definition
    pub fn define_alias_to_introduced(
        &self,
        alias: impl Into<Rc<str>>,
        home: Rc<Environment>,
        name: Rc<str>,
        found: IntroducedDefinition,
    ) {
        match found {
            IntroducedDefinition::Renamed(renamed) => self.define_alias(alias, home, renamed),
            IntroducedDefinition::Scoped(scopes) => {
                self.define_scoped_alias(alias, home, name, scopes)
            }
        }
    }

    /// [`define_alias`] to a definition reachable only under `scopes` — see
    /// [`AliasTarget`] for why an alias can need them.
    ///
    /// [`define_alias`]: Self::define_alias
    pub fn define_scoped_alias(
        &self,
        alias: impl Into<Rc<str>>,
        target_env: Rc<Environment>,
        target_name: Rc<str>,
        scopes: ScopeSet,
    ) {
        self.install_alias(alias.into(), target_env, target_name, Some(scopes));
    }

    fn install_alias(
        &self,
        alias: Rc<str>,
        target_env: Rc<Environment>,
        name: Rc<str>,
        scopes: Option<ScopeSet>,
    ) {
        // A target that is this environment is stored as `None`: see
        // `AliasTarget`.
        let env = (target_env.env_id() != self.env_id()).then_some(target_env);
        self.has_aliases.set(true);
        self.alias_bindings
            .borrow_mut()
            .insert(alias, AliasTarget { env, name, scopes });
    }

    /// Define a binding with a scope set (for scope-based hygiene)
    ///
    /// Use this when creating bindings from binding forms (lambda, let, etc.)
    /// where you want to track the lexical scope for hygiene.
    /// This is the primary API - accepts TaggedValue directly.
    pub fn define_with_scopes(
        &self,
        name: impl Into<Rc<str>>,
        scopes: ScopeSet,
        value: TaggedValue,
    ) {
        use crate::macro_debug;

        let name = name.into();
        if macro_debug::is_enabled() {
            let desc = crate::debug_format::format_tagged(value, &self.heap.borrow());
            println!(
                "[ENV] Defining '{}' with scopes {} = {}",
                name, scopes, desc
            );
        }

        self.insert_scoped(name, scopes, value, false);
    }

    /// Store a binding under its scopes, or by name when it has none.
    ///
    /// The one place either kind of scoped binding is written, so the two
    /// public entry points differ only in `visible_by_name` and their docs.
    ///
    /// A scope set already present is *overwritten* rather than pushed
    /// beside. `set_with_scopes` finds a binding by exact scope-set match, so
    /// a second entry for the same set would be unreachable — and every entry
    /// is a GC root (`for_each_local_value`), so re-evaluating a top-level
    /// form that expands a macro would otherwise pin one dead value per
    /// evaluation for the life of the process.
    fn insert_scoped(
        &self,
        name: Rc<str>,
        scopes: ScopeSet,
        value: TaggedValue,
        visible_by_name: bool,
    ) {
        if scopes.is_empty() {
            self.define(name, value);
            return;
        }
        crate::scope_trace::bind(&name, &scopes, visible_by_name);
        if visible_by_name {
            self.has_visible_scoped.set(true);
        }
        let mut table = self.scoped_bindings.borrow_mut();
        let bindings = table.entry(name).or_default();
        match bindings.iter_mut().find(|b| b.scopes == scopes) {
            Some(existing) => {
                existing.tagged_value = value;
                existing.visible_by_name = visible_by_name;
            }
            None => bindings.push(ScopedBinding {
                scopes,
                tagged_value: value,
                visible_by_name,
            }),
        }
    }

    /// Define a binding that a name-only lookup can also reach.
    ///
    /// Source-written parameters and internal definitions need this view:
    /// they acquire body scopes while their source references can still be
    /// name-only. Introduced body definitions use `define_with_scopes`
    /// instead (#269); their generated getters and setters reach the scoped
    /// binding directly. Macro-introduced top-level definitions retain this
    /// view for now, alongside the VM's global aliases (#427).
    ///
    /// The name-only view's reach is *plain* access — [`get`], [`set`], and
    /// the relinker resolving through them — not scoped resolution's
    /// fallback. A **scoped** reference whose resolution rejected this
    /// binding stays refused (`get_scoped_fallback` / `set_scoped_fallback`):
    /// since the family 36 fix, one expansion's introduced definition is not
    /// reachable from a different expansion's introduced reference, which is
    /// what chibi answers too. The VM still reaches it through its bare-name
    /// alias — triage family 40 pins that divergence, and Track L §6's
    /// relinking-by-name entry is its root.
    ///
    /// [`get`]: Self::get
    /// [`set`]: Self::set
    ///
    /// It is still **one** binding. An earlier version stored the value under
    /// the bare name as well, and a `set!` through the scoped path then left
    /// the two disagreeing — the freeze `alias_bindings` names as the reason
    /// its own indirection exists.
    pub fn define_scoped_definition(
        &self,
        name: impl Into<Rc<str>>,
        scopes: ScopeSet,
        value: TaggedValue,
    ) {
        self.insert_scoped(name.into(), scopes, value, true);
    }

    /// Position of the most recent name-visible scoped definition of `name`
    /// in this environment, if there is one.
    ///
    /// This is the name-only view of a name-visible scoped binding. It is one
    /// cell, not a copy: an earlier version stored the value under the bare
    /// name as well, and a `set!` through the scoped path then left the two
    /// disagreeing — the freeze `alias_bindings` documents as the reason its
    /// own indirection exists.
    ///
    /// Most recent wins, which is the behaviour from when a definition carried
    /// no scopes at all and each expansion simply overwrote the last.
    ///
    /// Returned as a position rather than a value so the read and the write
    /// share one rule: `get` and `set` must agree on *which* binding the bare
    /// name means, and two predicates that merely look alike would not have to.
    fn visible_scoped_index(&self, name: &str) -> Option<usize> {
        if !self.has_visible_scoped.get() {
            return None;
        }
        let table = self.scoped_bindings.borrow();
        table.get(name)?.iter().rposition(|b| b.visible_by_name)
    }

    /// Assign through a scoped reference, resolving exactly as [`get_with_scopes`]
    /// does: every candidate on the chain, one call to the shared rule, and the
    /// binding it names.
    ///
    /// The two must agree on *which* binding a reference denotes, or a
    /// reference can read one cell and write another. Three ways they did not,
    /// all fixed by resolving here the way the read already did — this used to
    /// walk one environment at a time with an inline copy of the rule:
    ///
    /// - an **ambiguous** reference was settled by scope-set size and written,
    ///   where the read refuses it (#289);
    /// - the walk **stopped at the first frame** holding any candidate, so a
    ///   more specific binding in a parent lost to a less specific one in a
    ///   child (#290);
    /// - the fallback began **at the root**, so a plain binding in between was
    ///   unreachable and a global of the same spelling was written instead
    ///   (#291).
    ///
    /// Nothing is written unless the rule names a binding: an ambiguous
    /// reference leaves every cell as it was, which is what makes the refusal
    /// worth anything.
    ///
    /// [`get_with_scopes`]: Self::get_with_scopes
    pub fn set_with_scopes(
        &self,
        name: &str,
        scopes: &ScopeSet,
        value: TaggedValue,
    ) -> Result<(), ScopedSetError> {
        if scopes.is_empty() {
            // Empty scopes - use simple lookup
            return self.set(name, value).map_err(ScopedSetError::Undefined);
        }

        /// Every candidate binding of `name` on this chain, paired with where
        /// to write it. The read collects values; a write needs the cell, so
        /// this carries the environment and the index instead.
        ///
        /// The order is the read's, and must stay so: latest binding first
        /// within a frame, innermost frame first. That is what
        /// [`crate::scope_resolve::resolve_index`] documents, and the read and
        /// the write have to break a tie the same way or they part company on
        /// exactly the shapes nobody tests.
        fn collect<'a>(
            env: &'a Environment,
            name: &str,
            ref_scopes: &ScopeSet,
            out: &mut Vec<(ScopeSet, (&'a Environment, usize))>,
        ) {
            {
                let scoped = env.scoped_bindings.borrow();
                if let Some(bindings) = scoped.get(name) {
                    for (index, binding) in bindings.iter().enumerate().rev() {
                        if crate::scope_resolve::is_candidate(&binding.scopes, ref_scopes) {
                            out.push((binding.scopes.clone(), (env, index)));
                        }
                    }
                }
            }
            if let Some(parent) = &env.parent {
                collect(parent, name, ref_scopes, out);
            }
        }

        let mut candidates: Vec<(ScopeSet, (&Environment, usize))> = Vec::new();
        collect(self, name, scopes, &mut candidates);
        let chosen = crate::scope_resolve::resolve_index(name, scopes, &candidates);

        // One record for the whole resolution, as the read writes one. It used
        // to be one per environment, because the walk was per environment —
        // and a trace you cannot compare against the read's is a trace that
        // cannot answer the question it exists for.
        if crate::scope_trace::enabled() {
            use crate::scope_trace::{Op, Outcome};
            let (picked, outcome) = match &chosen {
                Ok(Some(i)) => (Some(candidates[*i].0.clone()), Outcome::Scoped),
                Ok(None) => (None, Outcome::ByName),
                Err(_) => (None, Outcome::Ambiguous),
            };
            crate::scope_trace::resolve(
                name,
                scopes,
                candidates.len(),
                picked.as_ref(),
                Op::Set,
                outcome,
            );
        }

        match chosen {
            Err(ambiguous) => {
                crate::scope_trace::wrote(name, scopes, "ambiguous");
                Err(ScopedSetError::Ambiguous(ambiguous))
            }
            Ok(Some(index)) => {
                let (env, position) = candidates[index].1;
                let mut scoped = env.scoped_bindings.borrow_mut();
                match scoped.get_mut(name).and_then(|bs| bs.get_mut(position)) {
                    Some(binding) => {
                        binding.tagged_value = value;
                        drop(scoped);
                        crate::scope_trace::wrote(name, scopes, "scoped");
                        Ok(())
                    }
                    // The table cannot shrink between collecting and writing —
                    // nothing removes a scoped binding — so this is
                    // unreachable rather than merely unlikely. Reported as
                    // undefined instead of panicking because a wrong answer
                    // here is a lost assignment, not a corrupt one.
                    None => Err(ScopedSetError::Undefined(name.to_string())),
                }
            }
            Ok(None) => {
                let landed = self.set_scoped_fallback(name, scopes, value);
                crate::scope_trace::wrote(
                    name,
                    scopes,
                    if landed.is_ok() {
                        "byname"
                    } else {
                        "undefined"
                    },
                );
                landed.map_err(ScopedSetError::Undefined)
            }
        }
    }

    /// The by-name fallback for a scoped assignment no scoped binding
    /// answered — [`get_scoped_fallback`]'s mirror, and deliberately the same
    /// shape: plain binding, then alias, then the name-only view of a scoped
    /// definition, then the parent.
    ///
    /// It starts where the resolution started, not at the root. Starting at
    /// the root was #291: a plain binding in an intervening frame could be
    /// read through this reference and not written, and with a global of the
    /// same spelling in scope the assignment left the frame entirely and
    /// changed the global instead.
    ///
    /// The one asymmetry with [`get`] is the same one the read has: a frame's
    /// name-only view is skipped when this resolution *rejected* the binding
    /// behind it. Reaching by spelling a binding set-of-scopes resolution just
    /// refused would override the rule with the capture scope sets exist to
    /// replace, and it must hold on both sides or a reference could clobber by
    /// spelling what it may not read.
    ///
    /// [`get_scoped_fallback`]: Self::get_scoped_fallback
    /// [`get`]: Self::get
    fn set_scoped_fallback(
        &self,
        name: &str,
        scopes: &ScopeSet,
        value: TaggedValue,
    ) -> Result<(), String> {
        if let Some(slot) = self.local_slot(name) {
            self.set_slot_value(slot, value);
            return Ok(());
        }
        if self.has_aliases.get()
            && let Some(target) = self.alias_target(name)
        {
            return target.set(self, value);
        }
        if self.has_visible_scoped.get()
            && let Some(i) = self.visible_scoped_index(name)
        {
            let table = self.scoped_bindings.borrow();
            if let Some(binding) = table.get(name).and_then(|bs| bs.get(i)) {
                // Provably dead on this path, and asserted rather than pruned
                // for the reason `get_scoped_fallback` gives for its twin: the
                // fallback runs only after resolution rejected every scoped
                // binding on this chain, so a candidate showing up here means
                // the rule changed underneath and family 36 is back.
                debug_assert!(
                    !crate::scope_resolve::is_candidate(&binding.scopes, scopes),
                    "set_scoped_fallback reached a binding of `{name}` that is a \
                     candidate for {scopes} — resolution should have written it"
                );
                if crate::scope_resolve::is_candidate(&binding.scopes, scopes) {
                    drop(table);
                    let mut table = self.scoped_bindings.borrow_mut();
                    if let Some(binding) = table.get_mut(name).and_then(|bs| bs.get_mut(i)) {
                        binding.tagged_value = value;
                        return Ok(());
                    }
                    return Err(name.to_string());
                }
            }
            // Rejected for these scopes: fall through to the parent rather
            // than clobber it by spelling.
        }
        match &self.parent {
            Some(parent) => parent.set_scoped_fallback(name, scopes, value),
            None => Err(name.to_string()),
        }
    }

    /// Get a value with scope sets (for hygienic lookup)
    ///
    /// This is the key lookup algorithm for scope-based hygiene:
    /// 1. Collect all bindings for this name where `binding.scopes ⊆ reference.scopes`
    /// 2. Return the binding with the largest scope set (most specific match)
    /// 3. Fall back by name via [`get_scoped_fallback`] — which answers from
    ///    plain bindings and aliases freely, but never from the name-only
    ///    view of a scoped binding this resolution rejected in step 1. So a
    ///    scoped reference can come back `None` for a name a plain [`get`]
    ///    would answer; that refusal is the fix for triage family 36, and it
    ///    is what chibi answers for the same shapes.
    ///
    /// The "most specific" rule ensures that inner bindings shadow outer ones
    /// when their scopes are a subset of the reference's scopes.
    ///
    /// [`get_scoped_fallback`]: Self::get_scoped_fallback
    /// [`get`]: Self::get
    ///
    /// `Err` when step 2 has no most specific match to return: two candidates
    /// neither of which contains the other. The caller reports it — a
    /// `DesugarError` in the desugarer, an `EvalError` here at runtime — since
    /// no answer would be better than a guess. See [`AmbiguousReference`].
    pub fn get_with_scopes(
        &self,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<Option<TaggedValue>, Box<AmbiguousReference>> {
        self.resolve_with_scopes(name, scopes)
            .map(|(value, _)| value)
    }

    /// [`get_with_scopes`], also saying **how** the read ended: `true` when a
    /// binding's scopes selected it, `false` when it was answered by the
    /// name — step 3 — or by nothing.
    ///
    /// For a caller that has to know whether a reference is *headed for the
    /// by-name view* without resolving it a second time: the desugarer
    /// resolves every reference once already, to refuse syntax used as a
    /// value, and early binding (#438) asks exactly this of the same
    /// reference.
    ///
    /// [`get_with_scopes`]: Self::get_with_scopes
    pub fn resolve_with_scopes(
        &self,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<(Option<TaggedValue>, bool), Box<AmbiguousReference>> {
        use crate::macro_debug;

        let debug = macro_debug::is_enabled();

        if debug {
            println!("[ENV] Looking up '{}' with scopes {}", name, scopes);
        }

        if scopes.is_empty() {
            // Empty scopes = simple lookup (top-level identifiers)
            if debug {
                println!("[ENV]   Empty scopes -> simple lookup");
            }
            let result = self.get(name);
            if debug {
                match result {
                    Some(tv) => {
                        let v = crate::debug_format::format_tagged(tv, &self.heap.borrow());
                        println!("[ENV]   Result: {}", v);
                    }
                    None => println!("[ENV]   Result: NOT FOUND"),
                }
            }
            return Ok((result, false));
        }

        // Every binding of this name in this environment and its parents,
        // in the order `resolve_scoped` wants them.
        let mut candidates: Vec<(ScopeSet, TaggedValue)> = Vec::new();
        self.collect_scoped_candidates(name, scopes, &mut candidates, debug);

        // One rule, shared with the VM's renamer: see
        // `crate::scope_resolve::resolve_scoped`. `None` means no candidate
        // was a subset, and the unmarked binding answers instead.
        // The rule hands back *which* candidate won, so the trace names the
        // binding it actually chose. Searching `candidates` for the winning
        // value instead named whichever came first when two held the same one
        // — and at `phase=desugar` every binder is the same placeholder.
        let chosen = crate::scope_resolve::resolve_index(name, scopes, &candidates);
        // The fallback runs before the trace record is written, so the record
        // can say how the whole read ended, not how it was about to continue.
        // `via=byname` used to cover both "spelling answered" and "nothing
        // answered", and the difference is exactly the new refusal: a read
        // that skips a rejected name-visible binding and finds nothing else
        // traces `via=unbound`, where the old label asserted the opposite of
        // what happened on the path being debugged.
        let result = match &chosen {
            Ok(Some(i)) => Some(candidates[*i].1),
            Ok(None) => self.get_scoped_fallback(name, scopes),
            Err(_) => None,
        };
        if crate::scope_trace::enabled() {
            use crate::scope_trace::{Op, Outcome};
            let (picked, outcome) = match &chosen {
                Ok(Some(i)) => (Some(candidates[*i].0.clone()), Outcome::Scoped),
                Ok(None) if result.is_some() => (None, Outcome::ByName),
                Ok(None) => (None, Outcome::Unbound),
                // Recorded before it propagates: an ambiguous reference is the
                // most interesting thing that can happen here and used to leave
                // no record at all, the `?` having carried it away.
                Err(_) => (None, Outcome::Ambiguous),
            };
            crate::scope_trace::resolve(
                name,
                scopes,
                candidates.len(),
                picked.as_ref(),
                Op::Get,
                outcome,
            );
        }
        let selected = chosen?.is_some();

        if debug {
            match &result {
                Some(tv) => {
                    let v = crate::debug_format::format_tagged(*tv, &self.heap.borrow());
                    println!("[ENV]   Result (scoped or fallback): {}", v);
                }
                None => println!("[ENV]   No scoped match and the fallback found nothing"),
            }
        }

        Ok((result, selected))
    }

    /// The by-name fallback for a scoped reference no scoped binding answered.
    ///
    /// Walks as [`get`] does — plain bindings, aliases, then the name-only
    /// view of scoped definitions — except that a frame's name-only view is
    /// skipped when this resolution *rejected* the binding behind it.
    /// Reaching by spelling a binding set-of-scopes resolution just refused
    /// would override the rule with the spelling-based capture scope sets
    /// exist to replace; that override was triage family 36's read half. A
    /// plain binding was never a candidate for anything, so falling back to
    /// one is the fallback doing its job rather than overriding a decision.
    ///
    /// The predicate is stated with `is_candidate` rather than as "skip every
    /// visible binding" — equivalent on this path, since the resolution that
    /// fell back here saw every scoped binding on this chain — because an
    /// *alias* jumps into another environment chain this resolution never
    /// looked at. Nothing over there was rejected, so the walk continues
    /// through plain [`get`] on that side.
    ///
    /// [`get`]: Self::get
    fn get_scoped_fallback(&self, name: &str, scopes: &ScopeSet) -> Option<TaggedValue> {
        if let Some(tv) = self.local_value(name) {
            return Some(tv);
        }
        if self.has_aliases.get()
            && let Some(target) = self.alias_target(name)
        {
            return target.get(self);
        }
        if self.has_visible_scoped.get()
            && let Some(i) = self.visible_scoped_index(name)
        {
            let table = self.scoped_bindings.borrow();
            // `.get(i)`, as `get` reads the same table — the two copies of
            // this walk must not disagree on out-of-bounds behavior. One
            // shared walk is Track Q's Q7.3; until then they mirror by hand.
            if let Some(binding) = table.get(name).and_then(|bs| bs.get(i)) {
                // Provably dead on this path today: the fallback only runs
                // after `resolve_index` returned no candidate over this same
                // chain, so every scoped binding here already failed
                // `is_candidate`. Kept as a live arm rather than pruned, with
                // the invariant asserted, so a future change to the rule (a
                // visibility filter, an ambiguity-policy change) fails a
                // debug test loudly instead of silently resurrecting a
                // rejected binding — which would be family 36 again.
                debug_assert!(
                    !crate::scope_resolve::is_candidate(&binding.scopes, scopes),
                    "get_scoped_fallback reached a binding of `{name}` that is a \
                     candidate for {scopes} — resolution should have chosen it"
                );
                if crate::scope_resolve::is_candidate(&binding.scopes, scopes) {
                    return Some(binding.tagged_value);
                }
            }
            // Rejected for these scopes: fall through to the parent rather
            // than resurrect it by name.
        }
        self.parent
            .as_ref()
            .and_then(|p| p.get_scoped_fallback(name, scopes))
    }

    /// Which scoped binding a reference denotes, named by the scope set it
    /// was bound at — `None` when no scoped binding is a candidate and a read
    /// would fall back by name.
    ///
    /// The question [`get_with_scopes`] cannot answer, because it returns a
    /// value, and at desugar time every local binder holds the same
    /// placeholder. A caller *comparing* two references has to know whether
    /// they reach one binding: R7RS §4.3.2 matches a `syntax-rules` literal
    /// that way (`matches_literal` in `patina-macros`).
    ///
    /// A scope set is enough to name the binding. Every binding form mints a
    /// fresh scope for what it binds, so two bindings of one name on a chain
    /// differ in their scopes — except a procedure's formals and its body's
    /// definitions, which share the form's scope from a parent and a child
    /// frame. Every reference that sees the parent's also sees the child's,
    /// so all of them break that tie the same way.
    ///
    /// The walk and the rule are [`get_with_scopes`]'s, so the binding named
    /// here is the one a read of the same reference reaches, and an ambiguous
    /// reference is refused here as it is there. No `scope_trace` record: this
    /// resolves a reference without reading it, and a record of a read that
    /// did not happen would make the trace disagree with the program.
    ///
    /// [`get_with_scopes`]: Self::get_with_scopes
    pub fn scoped_binding_of(
        &self,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<Option<ScopeSet>, Box<AmbiguousReference>> {
        if scopes.is_empty() {
            return Ok(None);
        }
        let mut candidates: Vec<(ScopeSet, TaggedValue)> = Vec::new();
        self.collect_scoped_candidates(name, scopes, &mut candidates, false);
        let chosen = crate::scope_resolve::resolve_index(name, scopes, &candidates)?;
        Ok(chosen.map(|index| candidates.swap_remove(index).0))
    }

    /// Whether the name-only view of `name` — what [`get`] answers, and so
    /// what an alias to `name` in this environment forwards to — is the
    /// *binding* a reference at `scopes` denotes.
    ///
    /// The definition-environment relinker's question. It aliases by name, so
    /// it may only alias a mention whose own resolution lands where the name
    /// alone does. Comparing the two views' *values* — what it did first —
    /// cannot answer that: two bindings of one spelling that happen to hold
    /// equal values read as one, and the alias is then installed onto the
    /// wrong one. A definer macro used twice in a library, each expansion
    /// introducing its own `(define count 0)` beside a generated macro that
    /// bumps it, had both generated macros bumping a single counter from
    /// outside the library; and since the values stop being equal after the
    /// first bump, whether a later use was aliased or refused depended on
    /// what the program had run so far.
    ///
    /// Walks as [`get`] does and stops at the first frame that answers the
    /// name. A plain binding or an alias there is what the reference reaches
    /// exactly when no scoped binding claimed it, since
    /// [`get_scoped_fallback`] takes the same walk. A name-visible scoped
    /// definition is, when it is the one resolution chose. The one alias that
    /// can *be* a chosen scoped binding is the VM's bare-name alias for an
    /// introduced global it renamed, recognised by the identity recorded
    /// beside it ([`define_introduced_global`]).
    ///
    /// `Err` as [`scoped_binding_of`] gives it: an ambiguous reference
    /// denotes no binding.
    ///
    /// [`get`]: Self::get
    /// [`get_scoped_fallback`]: Self::get_scoped_fallback
    /// [`define_introduced_global`]: Self::define_introduced_global
    /// [`scoped_binding_of`]: Self::scoped_binding_of
    pub fn name_reaches_binding_of(
        &self,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<bool, Box<AmbiguousReference>> {
        if scopes.is_empty() {
            // The reference *is* the name-only view.
            return Ok(true);
        }
        let chosen = self.scoped_binding_of(name, scopes)?;
        let mut env = self;
        loop {
            if env.bindings.borrow().slot_of(name).is_some() {
                return Ok(chosen.is_none());
            }
            if env.has_aliases.get()
                && let Some(target) = env.alias_target(name)
            {
                let Some(chosen) = chosen else {
                    return Ok(true);
                };
                let renamed = env.introduced_global(name, &chosen);
                return Ok(target.env.is_none()
                    && target.scopes.is_none()
                    && renamed.is_some_and(|r| r == target.name));
            }
            if let Some(i) = env.visible_scoped_index(name) {
                let table = env.scoped_bindings.borrow();
                let visible = table.get(name).and_then(|bindings| bindings.get(i));
                return Ok(match (visible, &chosen) {
                    (Some(binding), Some(chosen)) => binding.scopes == *chosen,
                    _ => false,
                });
            }
            match env.parent.as_deref() {
                Some(parent) => env = parent,
                // Unbound by name: nothing for an alias to reach.
                None => return Ok(false),
            }
        }
    }

    /// Every scoped binding of `name` on this chain that is a candidate for a
    /// reference at `ref_scopes`: latest first within a frame, innermost frame
    /// first — the order `resolve_scoped` documents — and, at the root, the
    /// macro-introduced top-level definitions the VM renamed.
    ///
    /// Those last are the same kind of binding as a scoped definition, kept in
    /// a different table. The tree-walker files a macro-introduced top-level
    /// definition in the scoped table when it runs; the VM renames it and
    /// records only its identity ([`define_introduced_global`]), so a walk of
    /// the scoped table alone saw it on one backend and not the other. A later
    /// form's desugar-time reads then disagreed: a generated macro's `cond`
    /// matched a macro-introduced `else` as its literal on the VM only, and
    /// once that was fixed, rejected the same `else` in test position as
    /// "syntax used as a value" on the VM only. They come last, as the oldest,
    /// which is also where `RenameEnv::resolve` puts them. The tree-walker
    /// never records one, so its per-read walk pays an empty-map check.
    ///
    /// Candidacy is tested with the rule's own `is_candidate`, so this is a
    /// filter and not a second copy of the rule; a binding that fails it is
    /// shown neither to the resolver nor to the check, so cloning its scope
    /// set would be waste on a path the tree-walker takes per variable read.
    ///
    /// [`define_introduced_global`]: Self::define_introduced_global
    fn collect_scoped_candidates(
        &self,
        name: &str,
        ref_scopes: &ScopeSet,
        candidates: &mut Vec<(ScopeSet, TaggedValue)>,
        debug: bool,
    ) {
        {
            let scoped = self.scoped_bindings.borrow();
            if let Some(bindings) = scoped.get(name) {
                for binding in bindings.iter().rev() {
                    let is_candidate =
                        crate::scope_resolve::is_candidate(&binding.scopes, ref_scopes);
                    if debug {
                        println!(
                            "[ENV]   Candidate {} ⊆ {} : {}",
                            binding.scopes,
                            ref_scopes,
                            if is_candidate { "YES" } else { "NO" }
                        );
                    }
                    if is_candidate {
                        candidates.push((binding.scopes.clone(), binding.tagged_value));
                    }
                }
            }
        }
        match &self.parent {
            Some(parent) => parent.collect_scoped_candidates(name, ref_scopes, candidates, debug),
            None => self.collect_introduced_globals(name, ref_scopes, candidates, debug),
        }
    }

    /// The root's share of [`collect_scoped_candidates`]: each macro-introduced
    /// top-level definition of `name` the VM renamed, paired with the value its
    /// renamed global holds. One that is recorded but not yet bound — its
    /// defining form was compiled and has not run — is not a binding yet, and
    /// is not a candidate.
    ///
    /// [`collect_scoped_candidates`]: Self::collect_scoped_candidates
    fn collect_introduced_globals(
        &self,
        name: &str,
        ref_scopes: &ScopeSet,
        candidates: &mut Vec<(ScopeSet, TaggedValue)>,
        debug: bool,
    ) {
        // The tree-walker's case, on every scoped read: nothing recorded, so
        // return before `for_each_introduced_global` hashes the name.
        if self
            .rare
            .get()
            .is_none_or(|rare| rare.introduced_global_names.borrow().is_empty())
        {
            return;
        }
        // Collected before any value is read, so no borrow of the identity
        // table is held across a read of the bindings.
        let mut found: Vec<(ScopeSet, Rc<str>)> = Vec::new();
        self.for_each_introduced_global(name, |scopes, renamed_to| {
            if crate::scope_resolve::is_candidate(scopes, ref_scopes) {
                found.push((scopes.clone(), renamed_to.clone()));
            }
        });
        for (scopes, renamed_to) in found {
            let value = self.local_value(&renamed_to);
            if debug {
                println!(
                    "[ENV]   Introduced global {} as {} ⊆ {} : {}",
                    name,
                    renamed_to,
                    ref_scopes,
                    if value.is_some() {
                        "YES"
                    } else {
                        "NOT YET BOUND"
                    }
                );
            }
            if let Some(value) = value {
                candidates.push((scopes, value));
            }
        }
    }

    /// Check if a binding exists
    #[allow(dead_code)]
    pub fn has(&self, name: &str) -> bool {
        self.bindings.borrow().slot_of(name).is_some()
            || self.parent.as_ref().is_some_and(|p| p.has(name))
    }

    /// Get all variable names defined in this environment and parent environments
    pub fn get_all_names(&self) -> Vec<String> {
        // Through `local_names`, so an `import_alias` is left out here too.
        let mut names = self.local_names();
        // Include names from scoped bindings
        for name in self.scoped_bindings.borrow().keys() {
            if !names.iter().any(|n| n.as_str() == name.as_ref()) {
                names.push(name.to_string());
            }
        }
        if let Some(parent) = &self.parent {
            names.extend(parent.get_all_names());
        }
        names.sort();
        names.dedup();
        names
    }

    /// The names bound in this environment only, in slot order. What an
    /// import set walks when it filters or renames: it moves bindings, so it
    /// has no use for the values `bindings` would read.
    ///
    /// An [`import_alias`] is a slot here too, and is left out: it is this
    /// environment's bookkeeping, not a name a program bound, and nothing that
    /// enumerates names — an import set today, completion tomorrow — should
    /// offer it.
    ///
    /// [`import_alias`]: Self::import_alias
    pub fn local_names(&self) -> Vec<String> {
        self.visible_slots()
            .into_iter()
            .map(|(_, name)| name)
            .collect()
    }

    /// Each name a program bound here with its slot, in slot order. With the
    /// slot, because leaving a name out makes position and slot two things.
    fn visible_slots(&self) -> Vec<(u32, String)> {
        // A table, not a scan per name: an importer that expands library
        // macros holds an alias for each location their templates mention.
        let hidden: FxHashMap<Rc<str>, (u64, u32)> =
            self.rare.get().map_or_else(FxHashMap::default, |rare| {
                rare.import_aliases
                    .borrow()
                    .iter()
                    .flat_map(|(location, aliases)| {
                        aliases
                            .iter()
                            .map(|(_, alias)| (Rc::clone(alias), *location))
                    })
                    .collect()
            });
        self.bindings
            .borrow()
            .names()
            .enumerate()
            .filter(|(slot, name)| {
                // Hidden only while it is still the forward it was minted as.
                // A program that defined the alias's spelling bound a name,
                // and `import_aliases` goes on listing it until the location
                // is next asked for (`import_alias` replaces it then).
                hidden.get(&***name).is_none_or(|location| {
                    self.owner_of(*slot as u32)
                        .is_none_or(|(env, owner_slot)| (env.env_id(), owner_slot) != *location)
                })
            })
            .map(|(slot, name)| (slot as u32, name.to_string()))
            .collect()
    }

    /// Get all bindings in this environment only (not including parent)
    ///
    /// Returns a vector of (name, TaggedValue) pairs for all bindings defined locally.
    /// This is useful for library imports where we need to iterate over all exports.
    pub fn bindings(&self) -> Vec<(String, TaggedValue)> {
        // Names first and values after: an imported binding's value is read
        // from its owner, and that read must not happen under this borrow.
        self.visible_slots()
            .into_iter()
            .map(|(slot, name)| (name, self.slot_value(slot)))
            .collect()
    }

    // =========================================================================
    // GC support (see heap/gc.rs and docs/GC_DESIGN.md)
    // =========================================================================

    /// The parent environment, if any. Used by the GC to walk the chain.
    pub fn parent(&self) -> Option<&Rc<Environment>> {
        self.parent.as_ref()
    }

    /// Stable identity for GC deduplication.
    ///
    /// The struct's own address. An environment is only ever shared by its
    /// `Rc`, never copied — `Environment` is deliberately not `Clone`, since
    /// its binding tables are owned inline — so one address means one
    /// environment, and a collection cannot free one while tracing it.
    pub fn gc_identity(&self) -> usize {
        self as *const Environment as usize
    }

    /// Visit every value bound locally (simple and scoped bindings, not the
    /// parent chain). GC tracing hook — allocation-free, unlike `bindings()`.
    pub fn for_each_local_value(&self, f: &mut dyn FnMut(TaggedValue)) {
        for tv in self.bindings.borrow().values() {
            f(tv);
        }
        for scoped in self.scoped_bindings.borrow().values() {
            for binding in scoped {
                f(binding.tagged_value);
            }
        }
    }

    /// Visit the environments that own the bindings this one imported.
    ///
    /// GC tracing hook, and the same shape as `for_each_alias_target` below
    /// for the same reason: an imported binding's value is in its owner's
    /// slot, so `for_each_local_value` sees only the marker here, and the edge
    /// to the owner is an `Rc<Environment>` in a side table. A loaded library
    /// is rooted by the registry as well, but an environment is not obliged
    /// to be a registered library's to be imported from, and the collector
    /// should not have to know which are.
    pub fn for_each_shared_owner(&self, f: &mut dyn FnMut(&Rc<Environment>)) {
        let Some(rare) = self.rare.get() else {
            return;
        };
        for owner in rare.owners.borrow().iter() {
            f(owner);
        }
    }

    /// Visit the environments this one's macro-expansion aliases point at.
    ///
    /// GC tracing hook. Values reachable only through an alias -- a library
    /// private referenced by an exported macro -- are live, but the alias edge
    /// is an `Rc<Environment>` in a side table rather than a `TaggedValue` in a
    /// slot, so `for_each_local_value` cannot see it.
    pub fn for_each_alias_target(&self, f: &mut dyn FnMut(&Rc<Environment>)) {
        for target in self.alias_bindings.borrow().values() {
            // A `None` target is this environment, which the caller is already
            // tracing.
            if let Some(env) = &target.env {
                f(env);
            }
        }
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_define_and_get() {
        let env = Environment::new();
        env.define("x".to_string(), TaggedValue::fixnum(42));
        assert_eq!(env.get("x"), Some(TaggedValue::fixnum(42)));
    }

    #[test]
    fn test_slot_stability_across_redefine() {
        let env = Environment::new();
        env.define("x".to_string(), TaggedValue::fixnum(1));
        let slot = env.local_slot("x").unwrap();

        // Redefinition overwrites the slot in place — same index, new value.
        env.define("x".to_string(), TaggedValue::fixnum(2));
        assert_eq!(env.local_slot("x"), Some(slot));
        assert_eq!(env.slot_value(slot), TaggedValue::fixnum(2));

        // set! through the slot API is visible to name lookup and vice versa.
        env.set_slot_value(slot, TaggedValue::fixnum(3));
        assert_eq!(env.get("x"), Some(TaggedValue::fixnum(3)));
        env.set("x", TaggedValue::fixnum(4)).unwrap();
        assert_eq!(env.slot_value(slot), TaggedValue::fixnum(4));

        // New defines append fresh slots without disturbing existing ones.
        env.define("y".to_string(), TaggedValue::fixnum(10));
        assert_ne!(env.local_slot("y"), Some(slot));
        assert_eq!(env.slot_value(slot), TaggedValue::fixnum(4));
    }

    #[test]
    fn test_local_slot_ignores_parent() {
        let parent = Rc::new(Environment::new());
        parent.define("x".to_string(), TaggedValue::fixnum(42));
        let child = Environment::with_parent(parent);
        // x resolves via the parent chain but has no local slot in the child.
        assert_eq!(child.get("x"), Some(TaggedValue::fixnum(42)));
        assert_eq!(child.local_slot("x"), None);
    }

    #[test]
    fn linear_and_indexed_frames_agree_on_slots() {
        // `Bindings` answers by linear scan until a frame passes
        // `LINEAR_MAX`, then by hash index. Only the global environment
        // crosses that line at runtime, and it crosses it during startup,
        // where a name the index misses looks like a missing primitive
        // rather than an indexing bug — so the crossover is pinned here.
        let env = Environment::new();
        let names: Vec<String> = (0..LINEAR_MAX + 2).map(|i| format!("v{i}")).collect();
        for (i, name) in names.iter().enumerate() {
            env.define(name.as_str(), TaggedValue::fixnum(i as i64));
        }
        // Every name keeps the slot its insertion order gave it, on both
        // sides of the crossover, and reads through the index agree with the
        // values.
        for (i, name) in names.iter().enumerate() {
            assert_eq!(env.local_slot(name), Some(i as u32), "slot of {name}");
            assert_eq!(env.get(name), Some(TaggedValue::fixnum(i as i64)));
        }
        // Redefining after the index exists must overwrite in place, not
        // append a second slot the index then shadows.
        env.define("v0", TaggedValue::fixnum(99));
        assert_eq!(env.local_slot("v0"), Some(0));
        assert_eq!(env.get("v0"), Some(TaggedValue::fixnum(99)));
        assert_eq!(env.bindings().len(), names.len());
    }

    #[test]
    fn test_env_ids_unique_and_nonzero() {
        let a = Rc::new(Environment::new());
        let b = Environment::new();
        assert_ne!(a.env_id(), 0);
        assert_ne!(a.env_id(), b.env_id());
        // Sharing an environment is sharing its `Rc`, so an id is stable for
        // every holder of it. `Environment` is deliberately not `Clone`: its
        // binding tables are owned inline rather than behind an `Rc`, so a
        // value copy would be a *fork*, and two environments that disagreed
        // about `x` would both claim to be the one holding it.
        let alias = Rc::clone(&a);
        assert_eq!(alias.env_id(), a.env_id());
    }

    #[test]
    fn test_parent_lookup() {
        let parent = Rc::new(Environment::new());
        parent.define("x".to_string(), TaggedValue::fixnum(42));

        let child = Environment::with_parent(parent);
        assert_eq!(child.get("x"), Some(TaggedValue::fixnum(42)));
    }

    #[test]
    fn test_bindings() {
        let env = Environment::new();
        env.define("x".to_string(), TaggedValue::fixnum(42));
        env.define("y".to_string(), TaggedValue::TRUE);
        env.define("z".to_string(), TaggedValue::fixnum(100));

        let bindings = env.bindings();
        assert_eq!(bindings.len(), 3);

        // Check that all bindings are present
        let names: Vec<String> = bindings.iter().map(|(k, _)| k.clone()).collect();
        assert!(names.contains(&"x".to_string()));
        assert!(names.contains(&"y".to_string()));
        assert!(names.contains(&"z".to_string()));

        // Verify values directly as TaggedValues
        for (name, tv) in &bindings {
            match name.as_str() {
                "x" => assert_eq!(*tv, TaggedValue::fixnum(42)),
                "y" => assert_eq!(*tv, TaggedValue::TRUE),
                "z" => assert_eq!(*tv, TaggedValue::fixnum(100)),
                _ => panic!("Unexpected binding: {}", name),
            }
        }
    }

    #[test]
    fn test_bindings_excludes_parent() {
        let parent = Rc::new(Environment::new());
        parent.define("x".to_string(), TaggedValue::fixnum(42));
        parent.define("y".to_string(), TaggedValue::fixnum(100));

        let child = Environment::with_parent(parent);
        child.define("z".to_string(), TaggedValue::TRUE);

        // bindings() should only return local bindings
        let bindings = child.bindings();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].0, "z");
        assert_eq!(bindings[0].1, TaggedValue::TRUE);
    }

    #[test]
    fn test_scoped_bindings_basic() {
        use crate::scope::ScopeId;

        let env = Environment::new();
        let s1 = ScopeId(1);
        let scopes = ScopeSet::singleton(s1);

        env.define_with_scopes("x".to_string(), scopes.clone(), TaggedValue::fixnum(42));

        // Lookup with matching scopes should find the binding
        let result = env.get_with_scopes("x", &scopes).unwrap();
        assert_eq!(result, Some(TaggedValue::fixnum(42)));

        // Lookup with superset scopes should also find it (binding.scopes ⊆ ref.scopes)
        let s2 = ScopeId(2);
        let larger_scopes = ScopeSet::from_iter([s1, s2]);
        let result = env.get_with_scopes("x", &larger_scopes).unwrap();
        assert_eq!(result, Some(TaggedValue::fixnum(42)));
    }

    #[test]
    fn test_scoped_bindings_hygiene_example() {
        use crate::scope::ScopeId;

        // Simulating:
        // (let ((x 'outer))           ; S1
        //   (let-syntax ((m ...))     ; S2
        //     (let ((x 'inner))       ; S3
        //       (m))))

        let env = Environment::new();
        let s1 = ScopeId(1);
        let s2 = ScopeId(2);
        let s3 = ScopeId(3);

        // Outer x binding: introduced at S1 (Symbol requires heap allocation)
        let outer_x_scopes = ScopeSet::singleton(s1);
        let outer_tv = env.heap().borrow_mut().intern_symbol("outer");
        env.define_with_scopes("x".to_string(), outer_x_scopes.clone(), outer_tv);

        // Inner x binding: inside S1, S2, S3
        let inner_x_scopes = ScopeSet::from_iter([s1, s2, s3]);
        let inner_tv = env.heap().borrow_mut().intern_symbol("inner");
        env.define_with_scopes("x".to_string(), inner_x_scopes.clone(), inner_tv);

        // Free var x in macro m: inside S1, S2 (captured when macro was defined)
        let macro_x_scopes = ScopeSet::from_iter([s1, s2]);

        // Lookup should find outer x, NOT inner x
        let result_tv = env.get_with_scopes("x", &macro_x_scopes).unwrap().unwrap();
        let name = env
            .heap()
            .borrow()
            .get_symbol_name(result_tv)
            .map(|s| s.to_string());
        assert_eq!(name.as_deref(), Some("outer"));
    }

    #[test]
    fn test_scoped_bindings_most_specific() {
        use crate::scope::ScopeId;

        let env = Environment::new();
        let s1 = ScopeId(1);
        let s2 = ScopeId(2);

        // Less specific binding
        env.define_with_scopes(
            "x".to_string(),
            ScopeSet::singleton(s1),
            TaggedValue::fixnum(1),
        );

        // More specific binding
        env.define_with_scopes(
            "x".to_string(),
            ScopeSet::from_iter([s1, s2]),
            TaggedValue::fixnum(2),
        );

        // Lookup with {S1, S2} should find the more specific binding
        let lookup_scopes = ScopeSet::from_iter([s1, s2]);
        assert_eq!(
            env.get_with_scopes("x", &lookup_scopes).unwrap(),
            Some(TaggedValue::fixnum(2))
        );

        // Lookup with just {S1} should find the less specific binding
        let lookup_scopes_s1 = ScopeSet::singleton(s1);
        assert_eq!(
            env.get_with_scopes("x", &lookup_scopes_s1).unwrap(),
            Some(TaggedValue::fixnum(1))
        );
    }

    #[test]
    fn test_scoped_bindings_fallback_to_unmarked() {
        use crate::scope::ScopeId;

        let env = Environment::new();

        // Define an unmarked binding (Symbol requires heap allocation)
        let tv = env.heap().borrow_mut().intern_symbol("primitive-cons");
        env.define("cons".to_string(), tv);

        // Lookup with scopes should fall back to unmarked binding
        let s1 = ScopeId(1);
        let scopes = ScopeSet::singleton(s1);
        let result_tv = env.get_with_scopes("cons", &scopes).unwrap().unwrap();
        let name = env
            .heap()
            .borrow()
            .get_symbol_name(result_tv)
            .map(|s| s.to_string());
        assert_eq!(name.as_deref(), Some("primitive-cons"));
    }

    /// Two bindings of one name, neither visible-from the other, are not
    /// resolved to either — they are reported.
    ///
    /// `{S1}` and `{S2}` are both subsets of a `{S1,S2}` reference and are
    /// unordered against each other, so no candidate is the most specific and
    /// there is nothing to return. The old rule broke the tie by scope-set
    /// size and, at equal size, by which environment was walked first: an
    /// answer that depended on insertion order rather than on the program.
    #[test]
    fn two_unordered_bindings_do_not_resolve() {
        use crate::scope::ScopeId;

        let env = Environment::new();
        let (s1, s2) = (ScopeId(1), ScopeId(2));

        env.define_with_scopes(
            "x".to_string(),
            ScopeSet::singleton(s1),
            TaggedValue::fixnum(1),
        );
        env.define_with_scopes(
            "x".to_string(),
            ScopeSet::singleton(s2),
            TaggedValue::fixnum(2),
        );

        let both = ScopeSet::from_iter([s1, s2]);
        let err = env
            .get_with_scopes("x", &both)
            .expect_err("neither binding contains the other");
        assert_eq!(err.name, "x");

        // Each is still reachable on its own: ambiguity is a property of the
        // reference, not of the bindings.
        assert_eq!(
            env.get_with_scopes("x", &ScopeSet::singleton(s1)).unwrap(),
            Some(TaggedValue::fixnum(1))
        );
        assert_eq!(
            env.get_with_scopes("x", &ScopeSet::singleton(s2)).unwrap(),
            Some(TaggedValue::fixnum(2))
        );
    }
}

#[cfg(test)]
mod scoped_write_tests {
    use super::*;

    fn scopes(ids: &[usize]) -> ScopeSet {
        let mut set = ScopeSet::new();
        for id in ids {
            set.add_scope(crate::scope::ScopeId(*id));
        }
        set
    }

    /// #289. Two eligible bindings, neither containing the other: the write
    /// must say *why* it refused, not merely that it did. The variant is the
    /// whole reason this returns `ScopedSetError` rather than a string — the
    /// tree-walker reports one as a syntax error and the other as an
    /// undefined variable — so a test that checked only `is_err` would let
    /// the two be swapped.
    #[test]
    fn an_ambiguous_write_reports_ambiguity() {
        let env = Environment::new();
        env.define_with_scopes("x", scopes(&[1]), TaggedValue::fixnum(1));
        env.define_with_scopes("x", scopes(&[2]), TaggedValue::fixnum(2));
        match env.set_with_scopes("x", &scopes(&[1, 2]), TaggedValue::fixnum(9)) {
            Err(ScopedSetError::Ambiguous(e)) => assert_eq!(e.name, "x"),
            other => panic!("expected an ambiguity, got {other:?}"),
        }
        // And nothing moved.
        assert_eq!(
            env.get_with_scopes("x", &scopes(&[1])).unwrap(),
            Some(TaggedValue::fixnum(1))
        );
        assert_eq!(
            env.get_with_scopes("x", &scopes(&[2])).unwrap(),
            Some(TaggedValue::fixnum(2))
        );
    }

    /// The other arm: a name nothing binds is undefined, not ambiguous.
    #[test]
    fn a_write_to_nothing_reports_undefined() {
        let env = Environment::new();
        match env.set_with_scopes("absent", &scopes(&[1]), TaggedValue::fixnum(9)) {
            Err(ScopedSetError::Undefined(name)) => assert_eq!(name, "absent"),
            other => panic!("expected undefined, got {other:?}"),
        }
    }

    /// Both arms render a sentence, so a caller that formats rather than
    /// matching gets a diagnostic either way.
    #[test]
    fn both_arms_render_a_message() {
        let env = Environment::new();
        let undefined = env
            .set_with_scopes("absent", &scopes(&[1]), TaggedValue::fixnum(9))
            .unwrap_err()
            .to_string();
        assert!(undefined.contains("undefined variable"), "{undefined}");
        assert!(undefined.contains("absent"), "{undefined}");

        env.define_with_scopes("x", scopes(&[1]), TaggedValue::fixnum(1));
        env.define_with_scopes("x", scopes(&[2]), TaggedValue::fixnum(2));
        let ambiguous = env
            .set_with_scopes("x", &scopes(&[1, 2]), TaggedValue::fixnum(9))
            .unwrap_err()
            .to_string();
        assert!(ambiguous.contains("ambiguous reference"), "{ambiguous}");
    }
}

#[cfg(test)]
mod introduced_global_tests {
    use super::*;

    fn scopes(ids: &[usize]) -> ScopeSet {
        let mut set = ScopeSet::new();
        for id in ids {
            set.add_scope(crate::scope::ScopeId(*id));
        }
        set
    }

    fn recorded(env: &Environment, name: &str) -> Vec<(ScopeSet, Rc<str>)> {
        let mut out = Vec::new();
        env.for_each_introduced_global(name, |scopes, renamed| {
            out.push((scopes.clone(), renamed.clone()))
        });
        out.sort_by_key(|(scopes, _)| scopes.len());
        out
    }

    #[test]
    fn a_name_with_no_introduced_definition_yields_nothing() {
        let env = Environment::new();
        env.define_introduced_global("x".into(), scopes(&[1]), "x #1".into());
        assert!(recorded(&env, "y").is_empty());
    }

    /// The property the fix rests on: two expansions of one macro introduce
    /// the same spelling and must stay two bindings. Collapsing them is what
    /// the bare-name alias does, and why it cannot answer a scoped reference.
    #[test]
    fn two_expansions_of_one_spelling_are_two_entries() {
        let env = Environment::new();
        env.define_introduced_global("count".into(), scopes(&[1]), "count #1".into());
        env.define_introduced_global("count".into(), scopes(&[2]), "count #2".into());
        let entries = recorded(&env, "count");
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .any(|(s, n)| *s == scopes(&[1]) && &**n == "count #1")
        );
        assert!(
            entries
                .iter()
                .any(|(s, n)| *s == scopes(&[2]) && &**n == "count #2")
        );
    }

    /// Recording the same identity twice overwrites, so re-compiling one
    /// expansion does not leave two names for one binding.
    #[test]
    fn the_same_identity_recorded_twice_overwrites() {
        let env = Environment::new();
        env.define_introduced_global("count".into(), scopes(&[1, 2]), "stale".into());
        env.define_introduced_global("count".into(), scopes(&[1, 2]), "fresh".into());
        assert_eq!(
            recorded(&env, "count"),
            vec![(scopes(&[1, 2]), Rc::from("fresh"))]
        );
    }

    /// A child environment does not see them, which is what lets the lookup
    /// skip walking parents: only the parentless global environment records
    /// any, and the VM asserts that where it installs them.
    #[test]
    fn entries_are_local_to_the_environment_that_recorded_them() {
        let parent = Rc::new(Environment::new());
        parent.define_introduced_global("x".into(), scopes(&[1]), "x #1".into());
        let child = Environment::with_parent(Rc::clone(&parent));
        assert!(recorded(&child, "x").is_empty());
        assert_eq!(recorded(&parent, "x").len(), 1);
    }
}

/// `name_reaches_binding_of` answers by *binding*. Every case here holds the
/// same value in both bindings on purpose: comparing values is what the
/// relinker did first, and it called these one binding.
#[cfg(test)]
mod name_view_tests {
    use super::*;

    fn scopes(ids: &[usize]) -> ScopeSet {
        let mut set = ScopeSet::new();
        for id in ids {
            set.add_scope(crate::scope::ScopeId(*id));
        }
        set
    }

    /// One value for every binding, so nothing here can be told apart by it.
    fn zero() -> TaggedValue {
        TaggedValue::fixnum(0)
    }

    fn reaches(env: &Environment, name: &str, ids: &[usize]) -> bool {
        env.name_reaches_binding_of(name, &scopes(ids))
            .expect("not ambiguous")
    }

    /// The tree-walker's shape: a definer macro run twice files two
    /// name-visible definitions, and the name means the most recent.
    #[test]
    fn the_name_reaches_only_the_latest_of_two_introduced_definitions() {
        let env = Environment::new();
        env.define_scoped_definition("count", scopes(&[1]), zero());
        env.define_scoped_definition("count", scopes(&[2]), zero());
        assert!(!reaches(&env, "count", &[1]));
        assert!(reaches(&env, "count", &[2]));
    }

    /// A plain definition beside an introduced one takes the name, so the
    /// name reaches what an unscoped mention means and not what a mention
    /// carrying the introducing expansion's scope means.
    #[test]
    fn a_plain_sibling_takes_the_name_from_an_introduced_definition() {
        let env = Environment::new();
        env.define("x", zero());
        env.define_scoped_definition("x", scopes(&[1]), zero());
        assert!(!reaches(&env, "x", &[1]));
        assert!(reaches(&env, "x", &[]));
        // Scopes no binding claims fall back to the plain one, as a read does.
        assert!(reaches(&env, "x", &[7]));
    }

    /// The VM's shape: each introduced global is renamed and its identity
    /// recorded, and the bare name is an alias to the most recent.
    #[test]
    fn a_bare_alias_is_the_introduced_global_it_was_installed_for() {
        let env = Rc::new(Environment::new());
        for (id, renamed) in [(1, "count #1"), (2, "count #2")] {
            env.define(renamed, zero());
            env.define_introduced_global("count".into(), scopes(&[id]), renamed.into());
            env.define_alias("count", Rc::clone(&env), renamed.into());
        }
        assert!(!reaches(&env, "count", &[1]));
        assert!(reaches(&env, "count", &[2]));
    }

    /// A child frame that binds nothing of the name defers to its parent.
    #[test]
    fn the_walk_continues_into_the_parent() {
        let parent = Rc::new(Environment::new());
        parent.define("x", zero());
        parent.define_scoped_definition("x", scopes(&[1]), zero());
        let child = Environment::with_parent(parent);
        assert!(!reaches(&child, "x", &[1]));
        assert!(reaches(&child, "x", &[2]));
    }
}

#[cfg(test)]
#[path = "hygiene_properties.rs"]
mod hygiene_properties;

/// What an import installs (#406): the library's location, not a copy of what
/// it held. `tests/scheme/stdlib/library-bindings.scm` says the same from
/// Scheme, with Gauche arbitrating; these pin the parts only Rust can see —
/// which slots forward, which copy, and what retires a link.
#[cfg(test)]
mod shared_binding_tests {
    use super::*;

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    /// A library environment holding `count`, and an importer on its heap.
    fn library_and_importer() -> (Rc<Environment>, Rc<Environment>) {
        let library = Rc::new(Environment::new());
        library.define("count", n(0));
        let importer = Rc::new(Environment::with_heap(library.heap().clone()));
        (library, importer)
    }

    #[test]
    fn a_shared_binding_reads_what_its_owner_holds_now() {
        let (library, importer) = library_and_importer();
        assert!(importer.share_binding("count", &library, "count"));
        library.set("count", n(2)).unwrap();

        assert_eq!(importer.get("count"), Some(n(2)));
        // Every read, not only `get`: the VM reads by slot, a scoped reference
        // falls back by name, and an import set enumerates.
        let slot = importer.local_slot("count").unwrap();
        assert_eq!(importer.slot_value(slot), n(2));
        assert_eq!(
            importer.get_with_scopes("count", &ScopeSet::new()).unwrap(),
            Some(n(2))
        );
        assert_eq!(importer.bindings(), vec![("count".to_string(), n(2))]);
    }

    #[test]
    fn a_write_to_a_shared_binding_lands_in_its_owner() {
        let (library, importer) = library_and_importer();
        importer.share_binding("count", &library, "count");

        importer.set("count", n(10)).unwrap();
        assert_eq!(library.get("count"), Some(n(10)));

        let slot = importer.local_slot("count").unwrap();
        importer.set_slot_value(slot, n(11));
        assert_eq!(library.get("count"), Some(n(11)));
        assert_eq!(importer.get("count"), Some(n(11)));
    }

    #[test]
    fn a_definition_over_a_shared_binding_is_the_importers_own() {
        let (library, importer) = library_and_importer();
        importer.share_binding("count", &library, "count");
        let slot = importer.local_slot("count").unwrap();

        importer.define("count", n(99));
        assert_eq!(importer.get("count"), Some(n(99)));
        assert_eq!(library.get("count"), Some(n(0)));
        // In place, which is what the VM's per-site global caches rest on.
        assert_eq!(importer.local_slot("count"), Some(slot));

        // And it stays the importer's: a later write goes no further.
        importer.set("count", n(100)).unwrap();
        assert_eq!(library.get("count"), Some(n(0)));
    }

    #[test]
    fn sharing_over_a_definition_replaces_it_in_place() {
        let (library, importer) = library_and_importer();
        importer.define("count", n(7));
        let slot = importer.local_slot("count").unwrap();

        importer.share_binding("count", &library, "count");
        assert_eq!(importer.local_slot("count"), Some(slot));
        assert_eq!(importer.slot_value(slot), n(0));
    }

    #[test]
    fn a_re_export_forwards_to_the_owner_and_not_to_the_library_between() {
        let (library, relay) = library_and_importer();
        relay.share_binding("relayed", &library, "count");
        let importer = Rc::new(Environment::with_heap(library.heap().clone()));
        importer.share_binding("again", &relay, "relayed");

        library.set("count", n(3)).unwrap();
        assert_eq!(importer.get("again"), Some(n(3)));

        // One hop: the relay can go back to being its own binding without the
        // importer noticing.
        relay.define("relayed", n(-1));
        library.set("count", n(4)).unwrap();
        assert_eq!(importer.get("again"), Some(n(4)));

        let mut owners = Vec::new();
        importer.for_each_shared_owner(&mut |env| owners.push(Rc::as_ptr(env)));
        assert_eq!(owners, vec![Rc::as_ptr(&library)]);
    }

    #[test]
    fn a_staging_environment_carries_a_binding_without_owning_it() {
        let (library, staging) = library_and_importer();
        staging.share_binding("count", &library, "count");
        // The fallback an import takes for an export that is not a plain
        // binding: the staging environment is given the value outright.
        staging.define("loose", n(5));

        let importer = Rc::new(Environment::with_heap(library.heap().clone()));
        assert!(importer.copy_binding("c:count", &staging, "count"));
        assert!(importer.copy_binding("c:loose", &staging, "loose"));
        assert!(!importer.copy_binding("c:absent", &staging, "absent"));
        assert_eq!(importer.get("c:absent"), None);

        library.set("count", n(2)).unwrap();
        assert_eq!(importer.get("c:count"), Some(n(2)));

        // A value, not a forward into an environment about to be dropped.
        staging.set("loose", n(6)).unwrap();
        assert_eq!(importer.get("c:loose"), Some(n(5)));
        let mut owners = Vec::new();
        importer.for_each_shared_owner(&mut |env| owners.push(Rc::as_ptr(env)));
        assert_eq!(owners, vec![Rc::as_ptr(&library)]);
    }

    #[test]
    fn a_re_exported_binding_is_one_location_whoever_assigns_and_whenever() {
        // `primitives` stands for a library registered from Rust, `base` for
        // the Scheme library that re-exports it. A first version copied such
        // a binding down the chain, and an importer then saw a library's
        // assignment only if the import happened to come after it.
        let (primitives, base) = library_and_importer();
        base.share_binding("count", &primitives, "count");
        let early = Rc::new(Environment::with_heap(primitives.heap().clone()));
        early.share_binding("count", &base, "count");

        base.set("count", n(50)).unwrap();
        let late = Rc::new(Environment::with_heap(primitives.heap().clone()));
        late.share_binding("count", &base, "count");

        for env in [&primitives, &base, &early, &late] {
            assert_eq!(env.get("count"), Some(n(50)));
        }
        early.set("count", n(51)).unwrap();
        for env in [&primitives, &base, &early, &late] {
            assert_eq!(env.get("count"), Some(n(51)));
        }
    }

    #[test]
    fn the_collector_is_shown_each_owner_once() {
        let (library, importer) = library_and_importer();
        library.define("other", n(1));
        let second = Rc::new(Environment::with_heap(library.heap().clone()));
        second.define("third", n(2));
        importer.share_binding("count", &library, "count");
        importer.share_binding("third", &second, "third");
        importer.share_binding("other", &library, "other");

        let mut owners = Vec::new();
        importer.for_each_shared_owner(&mut |env| owners.push(Rc::as_ptr(env)));
        assert_eq!(owners, vec![Rc::as_ptr(&library), Rc::as_ptr(&second)]);
    }

    #[test]
    fn a_name_the_source_does_not_bind_plainly_is_left_to_the_caller() {
        let (library, importer) = library_and_importer();
        assert!(!importer.share_binding("missing", &library, "missing"));
        assert_eq!(importer.get("missing"), None);

        // A parent's binding is not the source's own location to share.
        let child = Rc::new(Environment::with_parent(Rc::clone(&library)));
        assert!(!importer.share_binding("count", &child, "count"));
    }

    #[test]
    fn an_environment_never_forwards_to_itself() {
        let (library, _) = library_and_importer();
        assert!(library.share_binding("count", &library, "count"));
        assert!(library.share_binding("also", &library, "count"));
        assert_eq!(library.get("count"), Some(n(0)));
        assert_eq!(library.get("also"), Some(n(0)));

        let mut owners = 0;
        library.for_each_shared_owner(&mut |_| owners += 1);
        assert_eq!(owners, 0, "a self-reference would leak the environment");
    }
}

/// `binding_location` answers "is this one binding?" across environments,
/// which a comparison of values cannot (#407).
#[cfg(test)]
mod binding_location_tests {
    use super::*;

    fn zero() -> TaggedValue {
        TaggedValue::fixnum(0)
    }

    #[test]
    fn equal_values_in_two_environments_are_two_locations() {
        let library = Rc::new(Environment::new());
        library.define("X", zero());
        let program = Rc::new(Environment::with_heap(library.heap().clone()));
        program.define("X", zero());

        assert_eq!(library.get("X"), program.get("X"));
        assert_ne!(library.binding_location("X"), program.binding_location("X"));
        assert_eq!(program.binding_location("missing"), None);
    }

    #[test]
    fn an_import_is_its_librarys_location_by_any_route_and_any_name() {
        let library = Rc::new(Environment::new());
        library.define("X", zero());
        let relay = Rc::new(Environment::with_heap(library.heap().clone()));
        relay.share_binding("relayed", &library, "X");
        let program = Rc::new(Environment::with_heap(library.heap().clone()));
        program.share_binding("direct", &library, "X");
        program.share_binding("through-relay", &relay, "relayed");

        let home = library.binding_location("X");
        assert!(home.is_some());
        assert_eq!(program.binding_location("direct"), home);
        assert_eq!(program.binding_location("through-relay"), home);
        assert_eq!(relay.binding_location("relayed"), home);

        // Defined over, it is the program's own again.
        program.define("direct", zero());
        assert_ne!(program.binding_location("direct"), home);
    }

    #[test]
    fn the_walk_is_gets_walk() {
        let root = Rc::new(Environment::new());
        root.define("X", zero());
        let child = Rc::new(Environment::with_parent(Rc::clone(&root)));
        // Through the parent.
        assert_eq!(child.binding_location("X"), root.binding_location("X"));

        // Through a macro-expansion alias, into another environment.
        let library = Rc::new(Environment::with_heap(root.heap().clone()));
        library.define("helper", zero());
        root.define_alias("helper.1", Rc::clone(&library), "helper".into());
        assert_eq!(
            child.binding_location("helper.1"),
            library.binding_location("helper")
        );

        // Through an alias whose target is the environment holding it, which
        // is stored without one: the walk restarts there, as `get`'s does.
        root.define("renamed.7", zero());
        root.define_alias("bare", Rc::clone(&root), "renamed.7".into());
        assert_eq!(child.get("bare"), Some(zero()));
        assert_eq!(
            child.binding_location("bare"),
            root.binding_location("renamed.7")
        );
        // A dangling alias is nothing, to both.
        root.define_alias("dangling", Rc::clone(&root), "nowhere".into());
        assert_eq!(child.get("dangling"), None);
        assert_eq!(child.binding_location("dangling"), None);

        // A plain binding wins over a parent's, as it does for `get`.
        child.define("X", zero());
        assert_ne!(child.binding_location("X"), root.binding_location("X"));
    }

    #[test]
    fn a_macro_introduced_definition_has_a_location_of_its_own() {
        let env = Rc::new(Environment::new());
        let scopes = ScopeSet::from_iter([crate::scope::ScopeId(1)]);
        env.define_scoped_definition("X", scopes, zero());
        let other = Rc::new(Environment::with_heap(env.heap().clone()));
        other.define("X", zero());

        // `get` reaches it by name where nothing plain has the name, and then
        // it is that definition, not whatever else holds an equal value.
        assert_eq!(env.get("X"), Some(zero()));
        let here = env.binding_location("X");
        assert!(matches!(here, Some(BindingLocation::Scoped(..))));
        assert_ne!(here, other.binding_location("X"));

        // A plain binding of the name takes the by-name view, as it does for
        // `get`.
        env.define("X", zero());
        assert!(matches!(
            env.binding_location("X"),
            Some(BindingLocation::Slot(..))
        ));
    }
}

/// An alias can name a *binding* a macro introduced, not only a spelling
/// (#408). `hygiene_matrix.rs`'s `introducing` rows say it from Scheme; these
/// pin how each backend's storage is found and what is refused.
#[cfg(test)]
mod introduced_definition_tests {
    use super::*;

    fn scopes(ids: &[usize]) -> ScopeSet {
        let mut set = ScopeSet::new();
        for id in ids {
            set.add_scope(crate::scope::ScopeId(*id));
        }
        set
    }

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    /// A library as the tree-walker leaves it after a definer ran twice beside
    /// a plain definition of the same spelling.
    fn tree_walker_library() -> Rc<Environment> {
        let library = Rc::new(Environment::new());
        library.define("count", n(100));
        library.define_scoped_definition("count", scopes(&[1]), n(10));
        library.define_scoped_definition("count", scopes(&[2]), n(20));
        library
    }

    #[test]
    fn a_mentions_scopes_select_its_own_generators_definition() {
        let library = tree_walker_library();
        // A use of the generated macro adds its own expansion scope.
        for (mention, run) in [(scopes(&[1, 7]), 1), (scopes(&[2, 8]), 2)] {
            let (home, found) = library.introduced_definition("count", &mention).unwrap();
            assert!(Rc::ptr_eq(&home, &library));
            assert_eq!(found, IntroducedDefinition::Scoped(scopes(&[run])));
        }
        // No scopes select the plain one, which is not an introduced
        // definition; nor is a spelling the library does not have.
        assert!(
            library
                .introduced_definition("count", &scopes(&[9]))
                .is_none()
        );
        assert!(
            library
                .introduced_definition("other", &scopes(&[1]))
                .is_none()
        );
        assert!(library.has_introduced_definition("count"));
        assert!(!library.has_introduced_definition("other"));
    }

    #[test]
    fn a_scoped_alias_reads_writes_and_locates_that_definition() {
        let library = tree_walker_library();
        let program = Rc::new(Environment::with_heap(library.heap().clone()));
        program.define("count", n(-1));
        program.define_scoped_alias("count.1", Rc::clone(&library), "count".into(), scopes(&[1]));
        program.define_scoped_alias("count.2", Rc::clone(&library), "count".into(), scopes(&[2]));

        assert_eq!(program.get("count.1"), Some(n(10)));
        assert_eq!(program.get("count.2"), Some(n(20)));

        program.set("count.1", n(11)).unwrap();
        assert_eq!(program.get("count.1"), Some(n(11)));
        // Nothing else of the spelling moved: not the second run's, not the
        // library's plain one, not the program's.
        assert_eq!(program.get("count.2"), Some(n(20)));
        assert_eq!(library.get("count"), Some(n(100)));
        assert_eq!(program.get("count"), Some(n(-1)));

        let first = program.binding_location("count.1");
        assert!(matches!(first, Some(BindingLocation::Scoped(..))));
        assert_ne!(first, program.binding_location("count.2"));
        assert_ne!(first, library.binding_location("count"));
    }

    #[test]
    fn a_scoped_alias_never_falls_back_to_the_spelling() {
        // The scopes are an identity, matched exactly. Resolved as a
        // *reference* instead, an identity the library does not hold would
        // fall back by name and read — and assign — the plain `count`.
        let library = tree_walker_library();
        let program = Rc::new(Environment::with_heap(library.heap().clone()));
        program.define_scoped_alias("count.9", Rc::clone(&library), "count".into(), scopes(&[9]));

        assert_eq!(program.get("count.9"), None);
        assert_eq!(program.binding_location("count.9"), None);
        // Refused with the name, as `set` refuses a dangling plain alias.
        assert_eq!(program.set("count.9", n(1)), Err("count".to_string()));
        assert_eq!(library.get("count"), Some(n(100)));
    }

    #[test]
    fn the_vms_renamed_global_is_found_by_the_same_question() {
        // The VM renames an introduced definition to a global of its own and
        // records the identity; the bare name is an alias to the latest.
        let library = Rc::new(Environment::new());
        for (run, renamed, value) in [(1, "count.g1", 10), (2, "count.g2", 20)] {
            library.define(renamed, n(value));
            library.define_introduced_global("count".into(), scopes(&[run]), renamed.into());
            library.define_alias("count", Rc::clone(&library), renamed.into());
        }
        let (home, found) = library
            .introduced_definition("count", &scopes(&[1, 7]))
            .unwrap();
        assert!(Rc::ptr_eq(&home, &library));
        assert_eq!(found, IntroducedDefinition::Renamed("count.g1".into()));
        assert!(library.has_introduced_definition("count"));
    }

    #[test]
    fn a_lexical_binding_is_not_an_introduced_definition() {
        // A `let`-bound variable around the macro's definition is a scoped
        // binding in a *child* frame. It is not something an alias can name —
        // it lives in a run-time frame — so it is left to scoped resolution.
        let library = Rc::new(Environment::new());
        let body = Rc::new(Environment::with_parent(Rc::clone(&library)));
        body.define_with_scopes("f", scopes(&[3]), n(1));
        assert!(body.introduced_definition("f", &scopes(&[3, 7])).is_none());
        assert!(!body.has_introduced_definition("f"));

        // From inside a body, the root's definitions are still found, and the
        // home is the root.
        library.define_scoped_definition("g", scopes(&[4]), n(2));
        let (home, _) = body.introduced_definition("g", &scopes(&[4, 7])).unwrap();
        assert!(Rc::ptr_eq(&home, &library));
    }
}

/// `import_alias` gives a root a name of its own for an imported *location*,
/// so that a reference bound to it survives the importer defining the
/// spelling itself (#438).
#[cfg(test)]
mod import_alias_tests {
    use super::*;

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    fn library_and_program() -> (Rc<Environment>, Rc<Environment>) {
        let library = Rc::new(Environment::new());
        library.define("count", n(0));
        let program = Rc::new(Environment::with_heap(library.heap().clone()));
        program.share_binding("count", &library, "count");
        (library, program)
    }

    #[test]
    fn the_alias_outlives_a_definition_over_the_import() {
        let (library, program) = library_and_program();
        let alias = program
            .import_alias("count", || "count.alias".into())
            .unwrap();

        // The program defines the spelling itself: a binding of its own.
        program.define("count", n(100));
        library.set("count", n(1)).unwrap();

        assert_eq!(program.get("count"), Some(n(100)));
        assert_eq!(program.get(&alias), Some(n(1)));
        // And it writes where it reads.
        program.set(&alias, n(2)).unwrap();
        assert_eq!(library.get("count"), Some(n(2)));
        assert_eq!(program.get("count"), Some(n(100)));
        assert_eq!(
            program.binding_location(&alias),
            library.binding_location("count")
        );
    }

    #[test]
    fn one_name_has_one_alias_however_often_it_is_asked_for() {
        let (_, program) = library_and_program();
        let mut minted = 0;
        let mut mint = || {
            minted += 1;
            Rc::<str>::from(format!("count.{minted}"))
        };
        let first = program.import_alias("count", &mut mint).unwrap();
        let again = program.import_alias("count", &mut mint).unwrap();
        assert_eq!(first, again);

        assert_eq!(minted, 1, "an alias per name, not per request");
    }

    #[test]
    fn a_second_name_for_the_location_has_an_alias_of_its_own() {
        let (library, program) = library_and_program();
        let mut minted = 0;
        let mut mint = || {
            minted += 1;
            Rc::<str>::from(format!("alias.{minted}"))
        };
        let first = program.import_alias("count", &mut mint).unwrap();

        // A second name for the same location — a prefixed import. The
        // desugarer reads the spelling a reference was written with back off
        // its alias, so the two names cannot share one.
        program.share_binding("c:count", &library, "count");
        let prefixed = program.import_alias("c:count", &mut mint).unwrap();
        assert_ne!(first, prefixed);
        assert_eq!(
            program.import_alias("c:count", &mut mint).unwrap(),
            prefixed
        );
        assert_eq!(program.import_alias("count", &mut mint).unwrap(), first);
        assert_eq!(minted, 2);

        // Both are the library's location, and both are bookkeeping.
        library.set("count", n(9)).unwrap();
        assert_eq!(program.get(&first), Some(n(9)));
        assert_eq!(program.get(&prefixed), Some(n(9)));
        assert_eq!(program.local_names(), ["count", "c:count"]);
    }

    #[test]
    fn an_alias_is_not_among_the_names_a_program_bound() {
        let (_, program) = library_and_program();
        program.define("before", n(1));
        program
            .import_alias("count", || "count.alias".into())
            .unwrap();
        program.define("after", n(2));

        assert_eq!(program.local_names(), ["count", "before", "after"]);
        // And leaving it out does not shift the values after it.
        assert_eq!(
            program.bindings(),
            [
                ("count".to_string(), n(0)),
                ("before".to_string(), n(1)),
                ("after".to_string(), n(2)),
            ]
        );
    }

    #[test]
    fn an_alias_defined_over_is_replaced_not_handed_out_again() {
        let (library, program) = library_and_program();
        let first = program
            .import_alias("count", || "count.alias".into())
            .unwrap();

        // Something defines the alias's own spelling: the slot is the
        // definer's from now on, and no longer the library's location.
        program.define(Rc::clone(&first), n(7));
        // A name the program bound from that moment, not from whenever the
        // location is next asked for.
        assert_eq!(program.local_names(), ["count", "count.alias"]);
        assert_eq!(
            program.bindings(),
            [
                ("count".to_string(), n(0)),
                ("count.alias".to_string(), n(7))
            ]
        );
        let second = program
            .import_alias("count", || "count.again".into())
            .unwrap();

        assert_ne!(first, second);
        library.set("count", n(1)).unwrap();
        assert_eq!(program.get(&second), Some(n(1)));
        assert_eq!(program.get(&first), Some(n(7)));
        // The definition is a name the program bound; the new alias is not.
        assert_eq!(program.local_names(), ["count", "count.alias"]);
    }

    #[test]
    fn minting_never_forwards_over_a_name_already_bound() {
        let (_, program) = library_and_program();
        program.define("count.0", n(42));
        let mut offered = ["count.0", "count.1"].into_iter();
        let alias = program
            .import_alias("count", || offered.next().unwrap().into())
            .unwrap();

        assert_eq!(&*alias, "count.1");
        assert_eq!(program.get("count.0"), Some(n(42)));
    }

    #[test]
    fn every_enumeration_of_names_leaves_an_alias_out() {
        let (_, program) = library_and_program();
        program
            .import_alias("count", || "count.alias".into())
            .unwrap();
        assert_eq!(program.get_all_names(), ["count"]);
    }

    #[test]
    fn only_an_import_has_one() {
        let (_, program) = library_and_program();
        program.define("own", n(5));
        let never = || -> Rc<str> { unreachable!("nothing to alias") };
        assert_eq!(program.import_alias("own", never), None);
        assert_eq!(program.import_alias("missing", never), None);

        // Defined over, the name is no longer an import.
        program.define("count", n(100));
        assert_eq!(program.import_alias("count", never), None);
    }
}

#[cfg(test)]
mod layout_tests {
    /// Not a style rule: this struct's size is a measured cost. The
    /// tree-walker allocates one per call and per `let`-bound temporary, and
    /// 8 bytes here — 256 to 264, past four cache lines — was 3–4% of that
    /// backend's running time on programs that never touched what the bytes
    /// were for (`Environment::rare` has the numbers). A table that only a
    /// global or library environment fills belongs in `RareTables`.
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn environment_size_is_watched() {
        let size = std::mem::size_of::<super::Environment>();
        assert!(
            size <= 224,
            "Environment grew to {size} bytes; measure the tree-walker before raising this"
        );
    }
}
