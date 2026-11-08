use crate::Trace;
use crate::binding_record::{BindingCell, Record};
use crate::census::Site;
use crate::heap::{ExternalBytes, Heap, SharedHeap};
use crate::placeholder_store::{PlaceholderHandle, PlaceholderStore, StoreKind};
use crate::scope::{ScopeId, ScopeSet};
use crate::scope_index::{FRAME_INDEX_ABOVE, INDEX_ABOVE, ScopeIndex};
use crate::scope_resolve::AmbiguousReference;
use crate::tagged_value::TaggedValue;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::cell::{Cell, OnceCell, Ref, RefCell, RefMut};
use std::collections::hash_map::Entry;
use std::mem::size_of;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A binding with its associated scope set
/// Used for scope-based hygiene lookup
#[derive(Debug, Clone, Trace)]
pub struct ScopedBinding {
    pub scopes: ScopeSet,
    /// Internal storage uses TaggedValue for efficiency
    pub(crate) tagged_value: TaggedValue,
    /// Whether a name-only lookup may fall back to this binding: false for a
    /// macro-introduced parameter, true for a macro-introduced definition.
    /// `define_scoped_definition` is where that difference is decided and
    /// explained.
    #[trace(skip, reason = "a flag")]
    pub(crate) visible_by_name: bool,
}

/// The bindings of one name in one environment: one per scope set it is bound
/// at, which is one in all but the rare shadowing case. Inline, so binding a
/// macro-introduced parameter does not allocate a `Vec` per call.
type ScopedBindingList = SmallVec<[ScopedBinding; 1]>;

/// What a spelling's list of scoped bindings holds outside its entry in the
/// table: its buffer at capacity, once it has spilled out of line, as a
/// namespace charges it ([`NamespaceCharge`]). Nothing while it is inline.
fn list_bytes(list: &ScopedBindingList) -> usize {
    if list.spilled() {
        list.capacity() * size_of::<ScopedBinding>()
    } else {
        0
    }
}

/// The value a slot's or a scoped entry's `word` stands for: in a namespace
/// (`step` is its borrow, `Some` exactly for a namespace, from `read_step`)
/// what the word's record's cell holds (#834); in a frame, the word itself.
#[inline(always)]
fn resolve(step: Option<&Heap>, word: TaggedValue) -> TaggedValue {
    step.map_or(word, |heap| Record::from_word(word).value(heap))
}

/// Write `value` into the binding a scoped entry is: in a namespace (`step`
/// is its mutable borrow, from `write_step`) a store into the cell the
/// entry's record names, logged at `site` with the cell as holder, as every
/// store into a cell is (#834); in a frame, the entry's word itself, off the
/// heap, logged at `site` with no holder where `log_frame` says so. Only
/// [`Environment::set`]'s by-name step logs a frame's entry; the scoped
/// assignments write it unlogged, as they did before #834.
#[inline]
fn write_entry(
    step: Option<&mut Heap>,
    entry: &mut ScopedBinding,
    value: TaggedValue,
    site: Site,
    log_frame: bool,
) {
    match step {
        Some(heap) => {
            let cell = Record::from_word(entry.tagged_value).cell(heap);
            cell.store(heap, value, site);
        }
        None => {
            if log_frame {
                crate::census::store(site, None, value);
            }
            entry.tagged_value = value;
        }
    }
}

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
#[derive(Debug, Default, Trace)]
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
#[derive(Debug, Clone, Trace)]
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

    fn get(&self, holder: &Environment, heap: &RefCell<Heap>) -> Option<TaggedValue> {
        let env = self.env(holder);
        match &self.scopes {
            None => env.get(heap, &self.name),
            Some(scopes) => env.scoped_definition_value(heap, &self.name, scopes),
        }
    }

    /// `Err` carries the name, as [`Environment::set`]'s does.
    fn set(
        &self,
        holder: &Environment,
        heap: &RefCell<Heap>,
        value: TaggedValue,
    ) -> Result<(), String> {
        let env = self.env(holder);
        match &self.scopes {
            None => env.set(heap, &self.name, value),
            Some(scopes) => env.set_scoped_definition(heap, &self.name, scopes, value),
        }
    }

    fn location(&self, holder: &Environment, heap: &RefCell<Heap>) -> Option<BindingLocation> {
        let env = self.env(holder);
        match &self.scopes {
            None => env.binding_location(heap, &self.name),
            Some(scopes) => env.scoped_definition_location(heap, &self.name, scopes),
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
type IntroducedGlobals = FxHashMap<Rc<str>, Introduced>;

/// One spelling's macro-introduced top-level definitions the VM renamed, in
/// the order they were recorded: the scope set each was introduced at, which
/// is its identity, and the global it was renamed to.
///
/// Indexed by scope once there are more than a scan should visit
/// ([`ScopeIndex`]), so that neither recording one nor resolving a reference
/// visits every definition of the spelling. Each was a scan once, and each
/// made `n` expansions of one spelling quadratic: recording, until the
/// identities were keyed by their scope set, and resolving, until the index
/// (#613).
///
/// Four words, as the map of identities it replaced was, so a namespace's
/// charge for its table of these is what it was; the assertion below holds
/// it there.
#[derive(Debug, Default)]
struct Introduced {
    identities: Vec<(ScopeSet, Rc<str>)>,
    index: Option<Box<ScopeIndex>>,
}

const _: () = assert!(size_of::<Introduced>() == size_of::<FxHashMap<ScopeSet, Rc<str>>>());

impl Introduced {
    /// Where the identity at exactly `scopes` is.
    fn position(&self, scopes: &ScopeSet) -> Option<usize> {
        match &self.index {
            Some(index) => index
                .exact(scopes)
                .find(|&i| self.identities[i].0 == *scopes),
            None => self.identities.iter().position(|(s, _)| s == scopes),
        }
    }

    /// The global the definition at exactly `scopes` was renamed to.
    fn get(&self, scopes: &ScopeSet) -> Option<&Rc<str>> {
        self.position(scopes).map(|i| &self.identities[i].1)
    }

    /// The bytes of this spelling's buffers at their capacities, as a
    /// namespace charges them ([`NamespaceCharge`]): the identities, and the
    /// boxed index once there is one. Not the names the identities hold,
    /// each the name of the global's slot, which the namespace counts there.
    fn buffer_bytes(&self) -> usize {
        self.identities.capacity() * size_of::<(ScopeSet, Rc<str>)>()
            + self
                .index
                .as_ref()
                .map_or(0, |index| size_of::<ScopeIndex>() + index.buffer_bytes())
    }

    /// Record a definition. One already recorded at the same scopes is
    /// overwritten, which happens when one expansion is compiled twice.
    fn record(&mut self, scopes: ScopeSet, renamed_to: Rc<str>) {
        if let Some(i) = self.position(&scopes) {
            self.identities[i].1 = renamed_to;
            return;
        }
        if let Some(index) = &mut self.index {
            index.push(&scopes);
        }
        self.identities.push((scopes, renamed_to));
        if self.index.is_none() && self.identities.len() > INDEX_ABOVE {
            let sets = self.identities.iter().map(|(scopes, _)| scopes);
            self.index = Some(Box::new(ScopeIndex::of(sets)));
        }
    }

    /// Call `f` with each definition that is a candidate for `reference`,
    /// newest first.
    fn for_each_candidate(&self, reference: &ScopeSet, mut f: impl FnMut(&ScopeSet, &Rc<str>)) {
        visit_candidates(
            &self.identities,
            |(scopes, _)| scopes,
            self.index.as_deref(),
            reference,
            |_, (scopes, renamed_to), candidate| {
                if candidate {
                    f(scopes, renamed_to);
                }
            },
        );
    }
}

/// Visit each of `items`, a list of bindings in the order they were made,
/// that can be a candidate for `reference`: newest first, which is the order
/// [`resolve_index`] wants within one environment, with its position and
/// whether it is one.
///
/// Without an index that is every item. With one, it is the items
/// [`ScopeIndex::candidates`] names, which include every candidate, so a
/// caller that keeps the candidates keeps the same ones in the same order
/// either way, and every answer the rule gives — which binding wins, a tie,
/// an ambiguity and the order its rivals are reported in — is the same.
///
/// [`resolve_index`]: crate::scope_resolve::resolve_index
fn visit_candidates<T>(
    items: &[T],
    scopes: impl Fn(&T) -> &ScopeSet,
    index: Option<&ScopeIndex>,
    reference: &ScopeSet,
    mut f: impl FnMut(usize, &T, bool),
) {
    let mut visit = |position: usize| {
        let item = &items[position];
        f(
            position,
            item,
            crate::scope_resolve::is_candidate(scopes(item), reference),
        );
    };
    match index {
        Some(index) => {
            for position in index.candidates(reference) {
                visit(position as usize);
            }
        }
        None => {
            for position in (0..items.len()).rev() {
                visit(position);
            }
        }
    }
}

/// Simple (non-scoped) binding storage: an append-only list of slots, each
/// holding its own name, with a hash index built only for large environments.
///
/// Slots are append-only: a binding's slot never moves or disappears once
/// created, and redefining a name keeps its slot (below). The VM's
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
///
/// In a namespace a slot's word is the binding's record (`binding_record.rs`,
/// #834), and a definition or an import changes what the record names,
/// never the slot's word: `define_record` and `bind_cell` re-point, so the
/// VM's cached `(env_id, slot)` names one record for the namespace's life.
/// In a frame a slot's word is the value, or `TaggedValue::IMPORTED`.
#[derive(Debug, Default, Trace)]
struct Bindings {
    /// The bindings in slot order: a name's slot *is* its index here. One
    /// vector rather than parallel name and value vectors, so there is no
    /// length invariant for `read_slot` to index past and no second
    /// allocation when a frame outgrows its inline capacity. In a namespace
    /// each word is the binding's record; in a frame, the value, or
    /// `TaggedValue::IMPORTED`.
    slots: SmallVec<[(Rc<str>, TaggedValue); 3]>,
    /// name → slot, present only above `LINEAR_MAX` entries.
    #[trace(skip, reason = "name → slot number; the values are in `slots`")]
    index: Option<Box<FxHashMap<Rc<str>, u32>>>,
}

/// Frames up to this many bindings are searched linearly, without a hash index.
const LINEAR_MAX: usize = 8;

/// What [`Bindings::insert`] did.
struct Inserted {
    /// The slot written.
    slot: u32,
    /// Whether it held a frame's import marker before.
    was_imported: bool,
    /// Whether the slot vector or the index reallocated to take it.
    grew: bool,
}

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

    /// What the slot holds: in a namespace, the binding's record; in a
    /// frame, the value, or `TaggedValue::IMPORTED` for an import. Neither a
    /// record nor the marker is a value. Only `Environment`'s resolving
    /// reads call this; there is deliberately no by-name `get` here, so
    /// nothing can read around them.
    fn read_slot(&self, slot: u32) -> TaggedValue {
        self.slots[slot as usize].1
    }

    fn write_slot(&mut self, slot: u32, value: TaggedValue) {
        self.slots[slot as usize].1 = value;
    }

    /// Define semantics: overwrite the existing slot or append a new one. A
    /// frame's; a namespace never overwrites a slot's word (`append`).
    ///
    /// Reports the slot, whether what it overwrote was a frame's import
    /// marker — in which case the caller has an entry of `frame_imports` to
    /// drop — and whether a table reallocated to make room, which a
    /// namespace charges (`NamespaceCharge`). Both asked here rather than of
    /// a side table or of the environment, so the frames the tree-walker
    /// builds by the million pay a compare for each and nothing else.
    fn insert(&mut self, name: Rc<str>, value: TaggedValue) -> Inserted {
        // A fresh frame is the common case on the tree-walker's hot path —
        // one is built per `let`-bound temporary and per call — and it has
        // nothing to search, no index (one exists only past `LINEAR_MAX`)
        // and room inline, so nothing reallocates.
        if self.slots.is_empty() {
            self.slots.push((name, value));
            return Inserted {
                slot: 0,
                was_imported: false,
                grew: false,
            };
        }
        if let Some(slot) = self.slot_of(&name) {
            let old = std::mem::replace(&mut self.slots[slot as usize].1, value);
            return Inserted {
                slot,
                was_imported: old == TaggedValue::IMPORTED,
                grew: false,
            };
        }
        self.append(name, value)
    }

    /// Append a slot for `name`, which has none: [`insert`](Self::insert)'s
    /// append half, and a namespace's fresh name, which is hashed once
    /// (#834). A namespace never overwrites a slot's word, so its records
    /// stay in their slots for its life, which the VM's per-site global
    /// caches rest on.
    fn append(&mut self, name: Rc<str>, value: TaggedValue) -> Inserted {
        debug_assert!(
            self.slot_of(&name).is_none(),
            "`append` of `{name}`, which has a slot"
        );
        let slot = self.slots.len() as u32;
        // A full vector reallocates to take one more.
        let mut grew = self.slots.len() == self.slots.capacity();
        match &mut self.index {
            Some(index) => {
                // So does a full table (`capacity` counts the room left).
                grew |= index.len() == index.capacity();
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
                return Inserted {
                    slot,
                    was_imported: false,
                    grew: true,
                };
            }
            None => {}
        }
        self.slots.push((name, value));
        Inserted {
            slot,
            was_imported: false,
            grew,
        }
    }

    /// Every bound name, in slot order.
    fn names(&self) -> impl Iterator<Item = &Rc<str>> {
        self.slots.iter().map(|(n, _)| n)
    }
}

/// Which location a name reaches by its by-name view — [`Environment::get`]'s
/// answer as a place rather than as what the place holds.
///
/// Two environments mean one binding by a name exactly when these are equal,
/// which a comparison of values cannot say: two variables that are both `0`
/// are two bindings. Comparable across environments because an import is the
/// exporting library's location, a cell its record and the importer's name
/// (`binding_record.rs`, #834): a name imported twice, by any routes and
/// under any import sets, is one `Cell`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingLocation {
    /// A frame's own plain binding: the frame's id, and its slot there.
    Slot(u64, u32),
    /// The name-only view of a definition a macro introduced in a frame:
    /// the frame, its spelling there, and its place among that spelling's
    /// scoped bindings.
    Scoped(u64, Rc<str>, usize),
    /// A cell, by its word: a namespace's binding (a slot or a scoped entry,
    /// own or imported) and any frame's import. An identity within one
    /// form's desugaring, beneath a context, and never held across a
    /// collection: the word of a cell made in a swept cell's slot may be the
    /// same (the rulings of 2026-11-03).
    Cell(u64),
}

/// One macro-introduced top-level definition's record, as
/// [`Environment::introduced_records`] answers it: raw, for 5c and the
/// tests (#834).
#[derive(Debug, Clone)]
pub struct IntroducedRecord {
    /// The scope set it was introduced at, its identity.
    pub identity: ScopeSet,
    /// `identity.introducing_scope()`: the scope of the expansion that
    /// introduced it (ruling D9).
    pub scope: Option<ScopeId>,
    /// What its cell holds now.
    pub value: TaggedValue,
    /// The record's identity: its word less a check build's stamp.
    pub record: u64,
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

/// The tables that nearly only a global or a library environment fills, kept
/// out of line so that the environments which never do — and the tree-walker
/// builds one per call and per `let`-bound temporary — do not carry them. A
/// frame comes to have them when an alias is installed in it, or when it
/// binds more than [`FRAME_INDEX_ABOVE`] scoped bindings of one spelling
/// (`scoped_index`). `Environment::rare` has the measurement.
#[derive(Debug, Default, Trace)]
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
    /// Installed beside the aliases by the VM's `compile_pipeline`, into the
    /// environment a form is compiled in: a namespace of either kind, a host
    /// namespace included (#835). Nothing asserts which. `introduced_global`
    /// and `for_each_introduced_candidate` read this environment's table
    /// alone; `has_introduced_definition` reads its root's, which for a host
    /// form is the global namespace's (the owner's ruling (c) of 2026-11-07
    /// on #835's question 3 moved only the import lookup to the nearest
    /// namespace).
    #[trace(
        skip,
        reason = "spellings, scope sets and the names of renamed globals"
    )]
    introduced_global_names: RefCell<IntroducedGlobals>,
    /// For each spelling with more than [`INDEX_ABOVE`] scoped bindings here,
    /// where among them a reference should look: positions in that
    /// spelling's list in `scoped_bindings`, by scope ([`ScopeIndex`], #613).
    ///
    /// Here, rather than beside the list it indexes, because of what that
    /// list's entry costs. Every frame the tree-walker builds holds a scoped
    /// table of those entries, sized into the `CAPTURED_FRAME_BYTES` charged
    /// to every closure it makes, so a word more per entry would be paid by
    /// every call. Which lists are indexed is [`Environment::indexes`]'s:
    /// a namespace's, of either kind, past [`INDEX_ABOVE`], a frame's past
    /// [`FRAME_INDEX_ABOVE`]. `insert_scoped`, the one writer of the lists,
    /// is the one writer of these, and `scoped_index` checks that an index
    /// covers its list.
    #[trace(
        skip,
        reason = "positions in `scoped_bindings`, which is traced in full"
    )]
    scoped_index: RefCell<FxHashMap<Rc<str>, ScopeIndex>>,
    /// For a frame, per slot, the cell an import names; the slot holds
    /// `TaggedValue::IMPORTED`. Indexed by slot, grown on demand: `None`, or
    /// a slot past the end, is the frame's own binding, which nearly every
    /// slot of nearly every frame is. Never filled in a namespace, whose
    /// slot words are records (#834, ruling D5/D6).
    ///
    /// Reads never ask whether this table exists: an imported slot announces
    /// itself by its marker, so the read path pays one compare, and so does
    /// the one writer that must drop an entry — `define` over an import.
    frame_imports: RefCell<Vec<Option<BindingCell>>>,
    /// For each imported location something here has been *early-bound* to,
    /// the hidden names that hold it — see [`Environment::import_alias`].
    /// Keyed by the imported cell's key ([`BindingCell::key`], its word less
    /// a check build's stamp), then by the spelling that was asked about:
    /// one name for one location has one alias however many expansions ask.
    /// Nearly always one entry; a location imported under two names has
    /// two, because the desugarer reads a spelling back off an alias
    /// (`Desugarer::settle_early_bindings`).
    #[trace(
        skip,
        reason = "cell keys less the stamp, and names: never dereferenced; an entry lives only while its alias's record names the cell (`forget_alias`)"
    )]
    import_aliases: RefCell<FxHashMap<u64, ImportAliases>>,
    /// For each binding the desugarer's relinking aliased here, the alias it
    /// minted — see [`Environment::relink_alias`]. One target has one alias
    /// however many expansions ask (#611), where each expansion used to mint
    /// its own: a program of `n` top-level `guard` forms held `3n` aliases to
    /// one private helper, and their symbols.
    #[trace(
        skip,
        reason = "environment ids, names, scope ids and alias spellings; the binding each alias reaches is an edge of `alias_bindings`"
    )]
    relink_aliases: RefCell<FxHashMap<RelinkKey, Rc<str>>>,
    /// This environment's mutable specifier, made the first time one was
    /// asked for ([`Environment::mutable_specifier`]) and handed out after
    /// that: `(interaction-environment)` answers one object, as chibi,
    /// Gauche and Chez do. A heap value held like a binding's, so it lives
    /// exactly as long as the bindings do: while this environment is traced.
    specifier: Cell<Option<TaggedValue>>,
    /// What this environment has charged the heap's external bytes for its
    /// tables, when it is a namespace ([`NamespaceCharge`]); `None` for every
    /// other environment that comes to hold one of these tables.
    #[trace(skip, reason = "a count of bytes and a handle to the heap's account")]
    namespace: Option<NamespaceCharge>,
    /// A host namespace's read-through records ([`ReadThrough`], #835), made
    /// the first time a linker asks for one: never in another kind. One
    /// pointer wide either way.
    read_through: OnceCell<Box<RefCell<ReadThrough>>>,
    /// A namespace's weak entries (#836, R1, R7): a parentless namespace's
    /// placeholders, or a host namespace's read-through records over a
    /// parent placeholder, in a store made the first time a linker asks for
    /// one and listed on the heap's `PlaceholderList`; never in a frame. The
    /// handle takes the store off the list when this namespace drops (M2).
    /// The store counts and charges its own (`placeholder_store.rs`).
    #[trace(
        skip,
        reason = "weak (R1, R7, #836): a link table marks an entry, and the sweep's pass drops each it did not"
    )]
    placeholders: OnceCell<PlaceholderHandle>,
}

/// A host namespace's read-through records (#835): for each name its parent
/// binds in a slot that a linker asked for ([`Environment::link_record`]),
/// its child record over the parent's, and the one cell they all name, which
/// holds `TaggedValue::UNBOUND`. At most one per name the parent binds, so
/// they are bounded by the parent's bindings. A define or an import of the
/// name here adopts its record and takes it out of this table. Dies with the
/// host namespace: nothing else holds the table, and the parent's records do
/// not name their children.
///
/// These are the strong children. A child over a parent's placeholder, made
/// for a name the parent did not bind (R7, #836), is weak, and lives in the
/// host namespace's placeholder store instead (`RareTables::placeholders`),
/// though it names this table's `unbound` cell. A link of its name once the
/// parent binds it promotes it here, out of the store, so a name has one
/// child across the two.
#[derive(Debug, Trace)]
struct ReadThrough {
    /// The free cell every record here names in its car: `(UNBOUND . 0)`.
    unbound: BindingCell,
    /// Each name's read-through record, keyed by the parent's slot name,
    /// shared.
    records: FxHashMap<Rc<str>, Record>,
}

/// Where a record a define or an import adopts was counted until then
/// (`Environment::take_adoptable`, #835, #836).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Adopted {
    /// A host namespace's strong read-through record, from [`ReadThrough`]:
    /// counted on the namespace's charge since it was made.
    Counted,
    /// A placeholder, or a host namespace's weak read-through record, from
    /// the namespace's placeholder store, which has let go of its count: the
    /// namespace counts it from now on.
    FromStore,
}

impl Adopted {
    /// The records the adopting namespace counts now.
    #[inline]
    fn records(self) -> usize {
        match self {
            Adopted::Counted => 0,
            Adopted::FromStore => 1,
        }
    }
}

/// A check build's failure: a namespace slot given a placeholder or a
/// read-through record (`Environment::append_slot`, Rule 1, #836).
#[cold]
#[inline(never)]
fn flagged_record_in_a_slot(name: &str) -> ! {
    panic!(
        "a slot for {name:?} given a placeholder or a read-through record: a record in a slot has \
         flags 0, or the sweep's pass could drop it (#836)"
    )
}

/// The external bytes (`heap/account.rs`, GC_PRD §15) a namespace has charged
/// for its tables, given back when it drops (#615).
///
/// **A namespace is an environment of either namespace kind** ([`Kind`]): a
/// parentless one ([`Environment::with_heap`]) — the global environment, a
/// library's, and the ones `environment`, `scheme-report-environment` and
/// `null-environment` build — or a host namespace
/// (`Heap::new_host_namespace`, #835), whose parent is the global one and
/// which charges the same account. Their tables are Rust allocations the
/// trigger would otherwise not see: an `(environment '(scheme base))` holds
/// about 32 KB in them (about
/// 40 KB until #834 took out its import links and owners) and costs the heap
/// one specifier slot, so a loop of those piled up 2.9 GiB of dead
/// namespaces between collections. A frame ([`Environment::child_frame`])
/// is never one: the tree-walker builds one per call and per `let`-bound
/// temporary, and what its bindings hold is short-lived and small, so its
/// path does as little of the measuring as it can. A scoped binding or an
/// alias — the tree-walker binds a call's parameters under their scopes —
/// asks [`Environment::is_namespace`] before it reads a table's capacity,
/// and a frame reads none. A plain binding compares the slot vector's
/// length with its capacity, except a fresh frame's first, which compares
/// nothing; and when a frame's vector spills or its index grows, one load
/// finds that it has no charge to settle. VM frames are registers and are
/// not environments at all.
///
/// **What is counted** ([`Environment::table_bytes`]): the environment and
/// its out-of-line tables' box; every table's own buffer at its capacity —
/// the slot vector once it is out of line, the name index, the scoped and
/// alias tables, the import and relink aliases, the table of introduced
/// globals and the table of the scoped bindings' indexes, a host
/// namespace's table of read-through records (#835), whose keys are its
/// parent's slot names and are counted there, and a frame's import cells,
/// which a namespace never has; the name of each slot, an `Rc<str>` of two
/// counts and its bytes,
/// counted whether or not something else shares it (an `environment` built
/// by import has a fresh name for each of its several hundred bindings,
/// which are most of what it holds); and each spelling's own buffers, which
/// a macro that introduces a definition grows by an entry at every
/// expansion (#833): its list of scoped bindings once spilled out of line,
/// that list's index ([`ScopeIndex`]: its heads map and its previous
/// vector), and its introduced globals' identities and their boxed index.
/// The tree-walker fills the lists and their indexes, the VM the
/// identities. Not counted: what an entry owns beyond its place in its
/// buffer — a scope set's spilled part, an alias's or a scoped binding's
/// name, and a name another table shares, as each identity's renamed global
/// is its slot's name — and the values, which live in the arenas and are
/// charged there, as are the binding records and cells (#834): each a pair,
/// charged in the live bytes as the pair arena's and never here. An
/// estimate, then, of the tables a namespace fills by
/// the hundred, and close for those: allocator overhead is not modelled.
/// Outside any namespace, and so not counted here: the VM's code units and
/// the objects arena's sweep vector, which a stream of expansions grows too,
/// to a high-water.
///
/// **When it is charged**: once when the namespace is made, and again
/// whenever one of the counted tables reallocates, which is the only time
/// their size changes — each mutation that can grow a table reports whether
/// it did, and only then is the namespace asked what it holds. The names are
/// counted from where the last count stopped (slots are append-only), so a
/// namespace's charge trails its names by at most the slots defined since a
/// table last grew. A spelling's buffers are counted as a running sum,
/// `entries`, which their writers — `insert_scoped`, `index_scoped` and
/// `define_introduced_global` — add to when one of them reallocates, so no
/// recharge walks the spellings. Nothing removes a scoped binding or an
/// identity, so the buffers and the sum only grow, and `Drop` gives the sum
/// back with the rest. Growth after a namespace is made is charged like the
/// rest: the global environment growing under `eval`, a library's under its
/// body.
///
/// **Borrow-free**: the charge goes through an [`ExternalBytes`] handle,
/// which shares the trigger's count with the heap, because a table grows
/// wherever a definition lands, some under a heap borrow; and `Drop` gives
/// it back through the same handle, because a namespace usually dies inside a
/// sweep, which holds the heap mutably while it drops a dead specifier's
/// `Rc`. Each namespace is charged once however many specifiers name it:
/// `interaction-environment` and `load` wrap the one global environment.
///
/// **Counted** (#834): `new` counts the namespace in `Heap::namespaces`, and
/// `Drop` takes it off again with its `records` and `cells`, through the same
/// handle and for the same reason.
///
/// **What it counts** (#834): `records`, every binding record the namespace
/// holds — one per slot, per scoped entry (the splicing `let-syntax`
/// keywords filed at top level among them) and per import alias, each of
/// which is a slot, and per read-through record of a host namespace's,
/// which is in no slot until a define or an import adopts it (#835); and
/// `cells`, the cells of its own its records name now. Its weak entries
/// (#836) are not among them: its placeholder store counts a placeholder and
/// its cell, and a host namespace's weak read-through record, in the heap's
/// sums itself, and charges its own buffer and names, until an entry is
/// dropped or adopted into a slot, where this charge counts it from then.
/// The slots stay append-only either way (D1).
/// Each is a running sum, raised as a record or a cell is made
/// (`Environment::count`), `cells` lowered as a record is re-pointed away
/// from an own cell (`Environment::count_cell_let_go`), and both given back
/// to the heap's sums when the namespace drops. Not counted: a frame's
/// imports, a free cell (the one a host namespace's read-through records
/// name among them), and a cell only an importer's record or an export
/// keeps once this namespace re-pointed away from it. `Environment::recount`
/// checks the sums against the tables in the tests.
#[derive(Debug)]
struct NamespaceCharge {
    /// The heap's external-bytes account.
    account: ExternalBytes,
    /// What this namespace has charged and not given back.
    charged: Cell<usize>,
    /// How many slots' names are in `charged`, and their bytes.
    names: Cell<(usize, usize)>,
    /// What the spellings' own buffers hold at their capacities: each
    /// spelling's spilled list of scoped bindings, its index, and its
    /// introduced globals' identities and index. A running sum, kept by the
    /// buffers' writers ([`Environment::charge_entries`]).
    entries: Cell<usize>,
    /// The binding records this namespace holds, as the heap's running sum
    /// counts them (`Heap::binding_records`, #834): given back when it drops.
    records: Cell<usize>,
    /// The cells of its own its records name now, as the heap's running sum
    /// counts them (`Heap::cells`, #834): given back when it drops.
    cells: Cell<usize>,
}

impl NamespaceCharge {
    /// A namespace's charge, which counts it among the heap's namespaces
    /// (`Heap::namespaces`) until it drops.
    fn new(account: ExternalBytes) -> Self {
        account.namespace_made();
        Self {
            account,
            charged: Cell::new(0),
            names: Cell::new((0, 0)),
            entries: Cell::new(0),
            records: Cell::new(0),
            cells: Cell::new(0),
        }
    }

    /// The namespace made `records` binding records and `cells` cells of
    /// its own: added to its sums and the heap's, borrow-free.
    fn count(&self, records: usize, cells: usize) {
        if records > 0 {
            self.records.set(self.records.get() + records);
            self.account.records_made(records);
        }
        if cells > 0 {
            self.cells.set(self.cells.get() + cells);
            self.account.cells_made(cells);
        }
    }

    /// One of the namespace's records was re-pointed away from an own cell.
    fn let_go_cell(&self) {
        debug_assert!(self.cells.get() > 0, "an own cell let go, but none counted");
        self.cells.set(self.cells.get().saturating_sub(1));
        self.account.cells_let_go(1);
    }

    /// One spelling's buffers went from `before` bytes to `after`: add the
    /// growth to the running sum, and report whether there was any.
    fn grow_entries(&self, before: usize, after: usize) -> bool {
        debug_assert!(after >= before, "a spelling's buffers only grow");
        // In this order, so that a shrink, which nothing makes, could not
        // underflow either.
        self.entries.set(self.entries.get() + after - before);
        after != before
    }

    /// The bytes of the names of `slots`, counting only the ones not counted
    /// before.
    fn name_bytes(&self, slots: &[(Rc<str>, TaggedValue)]) -> usize {
        let (counted, bytes) = self.names.get();
        let bytes = bytes
            + slots[counted.min(slots.len())..]
                .iter()
                .map(|(name, _)| RC_COUNTS + name.len())
                .sum::<usize>();
        self.names.set((slots.len(), bytes));
        bytes
    }

    /// Charge the difference between what the namespace holds now, `bytes`,
    /// and what it charged before.
    fn settle(&self, bytes: usize) {
        let charged = self.charged.replace(bytes);
        if bytes > charged {
            self.account.charge(bytes - charged);
        } else {
            self.account.release(charged - bytes);
        }
    }
}

impl Drop for NamespaceCharge {
    fn drop(&mut self) {
        self.account
            .namespace_dropped(self.records.get(), self.cells.get());
        self.account.release(self.charged.get());
    }
}

/// The two counts at the head of an `Rc`'s allocation.
pub(crate) const RC_COUNTS: usize = 2 * size_of::<usize>();

/// What a tree-walker closure is charged for the frame it captures (#637;
/// `heap/account.rs`, `cps_lambda_payload`): a frame binding one parameter
/// under its scopes, as the tree-walker binds a call's — the frame in its
/// `Rc`'s allocation, and the smallest scoped table, which `hashbrown` makes
/// four buckets wide, a capacity of three. 612 bytes on a 64-bit target, the
/// one-parameter frame #637 measured, which
/// `captured_frame_bytes_is_a_one_parameter_frame` builds to check that the
/// two agree.
///
/// A heuristic: the frame a closure captures may bind more, or bind by name
/// alone; closures made in one frame are each charged for it; and a closure
/// made where no frame is — at top level, in a library body, or by `eval` in
/// a namespace — captures a namespace, whose tables [`NamespaceCharge`]
/// charges already, and is charged one it does not capture.
pub(crate) const CAPTURED_FRAME_BYTES: usize =
    RC_COUNTS + size_of::<Environment>() + hash_table_bytes::<Rc<str>, ScopedBindingList>(3);

/// The bytes of a hash table — `std`'s and `FxHashMap` are `hashbrown`'s —
/// whose `capacity` reports `capacity`: its buckets, each an entry and a
/// control byte, and a group of trailing control bytes. Zero for a table that
/// never allocated.
pub(crate) const fn hash_table_bytes<K, V>(capacity: usize) -> usize {
    if capacity == 0 {
        return 0;
    }
    // `capacity` is the bucket count less an eighth, or less one below 8.
    let buckets = if capacity < 8 {
        capacity + 1
    } else {
        (capacity / 7 * 8).next_power_of_two()
    };
    buckets * (size_of::<(K, V)>() + 1) + 16
}

/// The aliases for one imported location: the spelling each was asked for
/// under, and the alias. A scan, not a table — it holds one entry, or two.
type ImportAliases = SmallVec<[(Rc<str>, Rc<str>); 1]>;

/// What a relink alias stands for ([`Environment::relink_alias`]): the
/// environment that holds the binding, by id, the binding's name there, the
/// scopes that select it (`None` for one the name alone reaches), and the
/// identity of the macro's mention it renames.
type RelinkKey = (u64, Rc<str>, Option<ScopeSet>, ScopeSet);

/// Mint a process-unique, never-reused environment id (0 is reserved as the
/// "empty" sentinel in the VM's global caches).
fn fresh_env_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Environment for variable bindings
///
/// **The raw layer** (#831): the type's name is inside the embedding API's
/// promise, since promised signatures name it, but its methods are not
/// (`docs/README.md`, "Changing the embedding API"). They change without a
/// deprecated twin, and promise only the rules documented on them.
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
/// ## The heap a namespace's binding takes (#834)
///
/// Each read or write of a namespace's binding takes `heap: &RefCell<Heap>`,
/// the first parameter after the receiver, and borrows it at the
/// **namespace step**: shared where a hit is read, mutable where a binding
/// is written. The borrow covers the binding's record, which is the word in
/// a namespace's slot or scoped entry, and the cell it names
/// (`binding_record.rs`), with a debug check that `heap` is this
/// environment's. A caller that holds the heap across a namespace access
/// panics, in every build.
///
/// A frame's own binding takes no borrow. A frame's import borrows the heap,
/// shared to read it and mutably to write it, since it reads or writes a
/// cell on the heap: the library's, or a declaration's free cell that a
/// declaration view imports through `Cx::import_cell_in_frame`. Installing
/// it takes none. The step is found by the environment's kind where a
/// binding is hit, never by whether it has a parent, and a miss takes no
/// borrow.
///
/// The `_in` cores take the heap already borrowed: `slot_value_in` and
/// `set_slot_value_in` are the step's own bodies, and `define_in` and the
/// test-only `local_value_in` serve the tests in this crate that hold the
/// heap borrowed.
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
///
/// ## Its trace
///
/// Derived (#686): the value in each of its slots, which in a namespace is
/// the binding's record, and so its cell and value; each cell a frame
/// imports (`frame_imports`); the target of each macro-expansion alias; a
/// host namespace's read-through table (`RareTables::read_through`), its
/// `unbound` cell and each read-through record, and through the record the
/// parent's (#835); not its parent. `SlotVisitor::env` walks an
/// environment with the chain it heads, and the collector follows the
/// parent by reference, since nearly every environment has one: queueing it
/// as well cost a hash lookup and a reference count per environment, about
/// 1% of the tree-walker's chibi suite under stress 1. The sentinel tests
/// `gc_edge_tests::environment_edges` and, for the read-through table,
/// `gc_edge_tests::read_through_edges` put a value behind each edge that
/// nothing else reaches.
#[derive(Debug, Trace)]
pub struct Environment {
    /// The number of the heap its values live on
    /// ([`Heap::id`](crate::heap::Heap::id)): how a heap refuses another's
    /// environment (`Heap::assert_own`), and what a namespace step checks
    /// the heap it borrows against in a debug build. An environment keeps no
    /// handle to its heap (#704); a frame takes its parent's number.
    #[trace(skip, reason = "a number")]
    heap_id: u64,
    /// Simple name-based bindings (for built-ins and top-level)
    /// Stores TaggedValue internally for memory efficiency
    bindings: RefCell<Bindings>,
    /// Process-unique id for this environment, minted at construction and
    /// shared by every holder of its `Rc`. Never reused, unlike an address —
    /// see `GlobalCacheEntry` in patina-vm for the cache soundness argument,
    /// and `gc_identity` for the address-based identity the GC uses to dedup
    /// *live* environments.
    #[trace(skip, reason = "a number minted at construction")]
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
    #[trace(
        skip,
        reason = "a summary of `alias_bindings`, which is traced in full"
    )]
    has_aliases: Cell<bool>,
    #[trace(
        skip,
        reason = "a summary of `scoped_bindings`, which is traced in full"
    )]
    has_visible_scoped: Cell<bool>,
    /// What this environment is ([`Kind`]): a namespace, a frame, or a frame
    /// of the tree-walker's temporaries. Set at construction and never
    /// changed.
    #[trace(skip, reason = "what the environment is, set at construction")]
    kind: Kind,
    /// 1 for a frame of either kind, 0 for a namespace of either kind, set
    /// from `kind` at construction ([`Kind::frame_flag`]).
    /// [`Environment::is_namespace`], asked at each read and write of a
    /// binding, tests it against zero: one load and a `cbz` or `cbnz`, laid
    /// out by LLVM's zero heuristic as `kind == Kind::Namespace` was before
    /// host namespaces, the frame case the fall-through at 39 of its 47
    /// inlined sites. A test of the kind for two namespace values, a range
    /// compare or a single-bit test, gives LLVM no such heuristic: the frame
    /// case was the taken branch at 43 of the 47, and the bit test read +1.25%
    /// tree-walker cycles against main on the aligned GBS (#835).
    #[trace(skip, reason = "a flag set from the kind at construction")]
    frame: u8,
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
    #[trace(
        skip,
        reason = "the parent, which `SlotVisitor::env` walks with the chain it heads: the collector follows it by reference (`GcVisitor::visit_env`)"
    )]
    parent: Option<Rc<Environment>>,
}

/// What an environment is, set by its constructor and never changed (4b's
/// start decision 4 on #782): one type serves namespaces and frames, and the
/// kind says which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A parentless namespace: the global environment, a library's or a
    /// specifier's, each made without a parent and on a heap
    /// ([`Environment::with_heap`]). It charges the heap's account for its
    /// tables ([`NamespaceCharge`]), indexes a spelling's scoped bindings
    /// past [`INDEX_ABOVE`], and borrows the heap at each read or write of a
    /// binding (the namespace step, #834).
    Namespace,
    /// A host namespace (#835): the namespace `Interpreter::new_environment`
    /// answers, whose parent is the global namespace, a parentless one
    /// ([`Environment::host_namespace`]). Every namespace path is its own:
    /// its definitions and imports are records over cells in its slots
    /// (#834, D1), and it borrows the heap at each read or write of a
    /// binding. It also holds read-through records over its parent's
    /// (`RareTables::read_through`), which are in no slot. It charges its
    /// heap's account for its tables as a parentless namespace does
    /// ([`NamespaceCharge`]), and indexes past [`INDEX_ABOVE`] as one does.
    HostNamespace,
    /// A frame ([`Environment::child_frame`]): a call's or a body's, and the
    /// desugarer's scopes. It never charges, and indexes past
    /// [`FRAME_INDEX_ABOVE`].
    Frame,
    /// A frame the tree-walker made for its CPS code's temporaries, a `let`
    /// temporary's or a continuation's parameter
    /// ([`Environment::for_temporaries`]), where no definition belongs: a
    /// `define` evaluated with it binds in the nearest environment above it
    /// that is not one (#680). Otherwise a frame.
    Temporaries,
}

impl Kind {
    /// [`Environment`]'s `frame` flag for an environment of this kind: 0 for
    /// a namespace of either kind, 1 for a frame of either kind.
    const fn frame_flag(self) -> u8 {
        match self {
            Kind::Namespace | Kind::HostNamespace => 0,
            Kind::Frame | Kind::Temporaries => 1,
        }
    }
}

impl Environment {
    /// Create a new empty namespace whose values live on `heap`: the global
    /// environment, a library's, or one `environment` or an R5RS constructor
    /// builds. It keeps no handle to the heap (#704): whoever reads or
    /// writes its values reaches the heap by another way.
    ///
    /// A namespace charges the heap's external bytes for its tables, now and
    /// as they grow, and gives them back when it drops (`NamespaceCharge`),
    /// so a collection comes as often as the memory namespaces hold calls
    /// for. Borrows `heap` for its account, so it must not be borrowed
    /// mutably here: a test in this crate that holds it passes the account
    /// itself, to `Environment::namespace`.
    pub fn with_heap(heap: &SharedHeap) -> Self {
        Self::namespace(&heap.borrow())
    }

    /// [`Environment::with_heap`] with the heap in hand: within the crate,
    /// for the tests that hold it borrowed. It takes the heap's number and
    /// its account ([`Heap::external_bytes_handle`](crate::heap::Heap::external_bytes_handle))
    /// together, so a namespace cannot be charged to another heap's.
    pub(crate) fn namespace(heap: &crate::Heap) -> Self {
        Self::parentless(heap.id(), heap.external_bytes_handle())
    }

    /// A parentless environment of the heap numbered `heap_id`, charging
    /// `account` for its tables.
    fn parentless(heap_id: u64, account: ExternalBytes) -> Self {
        Self::namespace_with(
            heap_id,
            Kind::Namespace,
            Some(NamespaceCharge::new(account)),
            None,
        )
    }

    /// A host namespace over `parent` (#835): what `Heap::new_host_namespace`
    /// makes and holds for `Interpreter::new_environment`. Panics unless
    /// `parent` is a parentless namespace, so no host namespace sits under a
    /// frame or under another host namespace. Its heap is its parent's. Its
    /// `RareTables` is built here under every charging rule, as a parentless
    /// namespace's is: `import_alias` and `link_record` need it.
    pub(crate) fn host_namespace(parent: Rc<Environment>) -> Self {
        assert!(
            parent.kind == Kind::Namespace,
            "a host namespace's parent is a parentless namespace, not a {:?} (#835)",
            parent.kind
        );
        // Charged to its parent's account, its heap's, which every
        // parentless namespace has (#834), as a parentless namespace is
        // charged (the owner's ruling (a) of 2026-11-07 on #835's question
        // 2).
        let charge = Some(NamespaceCharge::new(
            parent
                .rare
                .get()
                .and_then(|rare| rare.namespace.as_ref())
                .expect("a parentless namespace has its charge (#834)")
                .account
                .clone(),
        ));
        Self::namespace_with(parent.heap_id, Kind::HostNamespace, charge, Some(parent))
    }

    /// A namespace of either kind, with its `RareTables` built: the one
    /// struct literal [`Environment::parentless`] and
    /// [`Environment::host_namespace`] share, so the two cannot drift.
    /// Charges what its tables hold from the start when it has a charge.
    fn namespace_with(
        heap_id: u64,
        kind: Kind,
        charge: Option<NamespaceCharge>,
        parent: Option<Rc<Environment>>,
    ) -> Self {
        let env = Environment {
            heap_id,
            bindings: RefCell::new(Bindings::default()),
            env_id: fresh_env_id(),
            scoped_bindings: RefCell::new(ScopedTable::default()),
            alias_bindings: RefCell::new(FxHashMap::default()),
            has_aliases: Cell::new(false),
            has_visible_scoped: Cell::new(false),
            kind,
            frame: kind.frame_flag(),
            rare: OnceCell::from(Box::new(RareTables {
                namespace: charge,
                ..RareTables::default()
            })),
            parent,
        };
        env.recharge();
        env
    }

    /// Create a new environment with a parent: a frame, which never charges
    /// for its tables (`NamespaceCharge`).
    ///
    /// Deprecated for hosts (#831): nothing roots a frame built here between
    /// calls (#620), and `Interpreter::new_environment` answers a host
    /// namespace held (#835). It goes at stage 5e. The workspace builds its
    /// frames with [`Environment::child_frame`]. The VM refuses to evaluate
    /// in a frame built here, with an error naming its kind, where the
    /// tree-walker evaluates in one by name (#835).
    #[deprecated(
        note = "use `Interpreter::new_environment`, which answers an environment held between \
                calls; nothing roots a frame built here (#620), and this goes at stage 5e (#831)"
    )]
    pub fn with_parent(parent: Rc<Environment>) -> Self {
        Self::frame(parent, Kind::Frame)
    }

    /// A frame whose parent is `parent`, which never charges for its tables
    /// (`NamespaceCharge`): the desugarer's scopes, and a frame of the
    /// tree-walker's per call. What [`Environment::with_parent`] made, for
    /// the workspace's own callers once it was deprecated (#831). Hidden,
    /// since a host keeps an environment through `Interpreter::new_environment`,
    /// and raw, as every `Environment` method is.
    #[doc(hidden)]
    pub fn child_frame(parent: Rc<Environment>) -> Self {
        Self::frame(parent, Kind::Frame)
    }

    /// [`Environment::child_frame`] for a frame of the tree-walker's CPS
    /// temporaries, a `let` temporary's or a continuation's parameter, which
    /// is never where a definition binds ([`Environment::holds_temporaries`],
    /// #680). Hidden with it (#831).
    #[doc(hidden)]
    pub fn for_temporaries(parent: Rc<Environment>) -> Self {
        Self::frame(parent, Kind::Temporaries)
    }

    fn frame(parent: Rc<Environment>, kind: Kind) -> Self {
        Environment {
            heap_id: parent.heap_id,
            bindings: RefCell::new(Bindings::default()),
            env_id: fresh_env_id(),
            scoped_bindings: RefCell::new(ScopedTable::default()),
            alias_bindings: RefCell::new(FxHashMap::default()),
            has_aliases: Cell::new(false),
            has_visible_scoped: Cell::new(false),
            kind,
            frame: kind.frame_flag(),
            rare: OnceCell::new(),
            parent: Some(parent),
        }
    }

    /// The number of the heap this environment's values live on
    /// ([`Heap::id`](crate::heap::Heap::id)); a frame's is its parent's.
    #[inline]
    pub fn heap_id(&self) -> u64 {
        self.heap_id
    }

    /// Whether this is a frame of the tree-walker's CPS temporaries
    /// ([`Environment::for_temporaries`]), where no definition binds.
    #[inline]
    pub fn holds_temporaries(&self) -> bool {
        self.kind == Kind::Temporaries
    }

    /// What frame this is, for an error that names it: `None` for a
    /// namespace of either kind, which the VM evaluates in, and
    /// `Some("frame")` or `Some("frame of temporaries")` otherwise, which it
    /// refuses (`VmBackend`'s `eval_datum`, #835, and `compile_for_eval`,
    /// which compiles Scheme `eval`'s and `load`'s code and a host
    /// primitive's `Step::Eval`, #853). Hidden, as
    /// [`Environment::child_frame`] and [`Environment::for_temporaries`]
    /// are, and raw, as every `Environment` method is. The predicate the
    /// holder asks, `is_namespace`, stays crate-private (#831's ruling on
    /// question 1; the owner's ruling (a) of 2026-11-07 on #835's question
    /// 5).
    #[doc(hidden)]
    pub fn frame_kind(&self) -> Option<&'static str> {
        match self.kind {
            Kind::Namespace | Kind::HostNamespace => None,
            Kind::Frame => Some("frame"),
            Kind::Temporaries => Some("frame of temporaries"),
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
    /// of the environment: redefinition keeps the slot, storing into the
    /// cell its record names in a namespace and overwriting it in a frame.
    #[inline]
    pub fn local_slot(&self, name: &str) -> Option<u32> {
        self.bindings.borrow().slot_of(name)
    }

    /// Read the value in a slot previously obtained from `local_slot`.
    ///
    /// An imported binding is read from the cell it shares with the library
    /// that owns it, so this is the *binding's* current value, never a
    /// snapshot.
    ///
    /// The namespace step's read (#834): a namespace borrows `heap` shared,
    /// and reads the value through the slot's record and its cell; a frame
    /// borrows it only for an import, to read the import's cell.
    #[inline]
    pub fn slot_value(&self, heap: &RefCell<Heap>, slot: u32) -> TaggedValue {
        if let Some(step) = self.read_step(heap) {
            return self.slot_value_in(&step, slot);
        }
        self.frame_slot_value(heap, slot)
    }

    /// [`slot_value`](Self::slot_value) for a frame, out of line: the VM's
    /// `LoadGlobal` inlines `slot_value`, and nearly every global it reads
    /// is a namespace's.
    #[inline(never)]
    fn frame_slot_value(&self, heap: &RefCell<Heap>, slot: u32) -> TaggedValue {
        let tv = self.bindings.borrow().read_slot(slot);
        if tv == TaggedValue::IMPORTED {
            return self.frame_import_value(heap, slot);
        }
        tv
    }

    /// [`slot_value`](Self::slot_value) with the heap already borrowed: the
    /// namespace step's own body, and the VM's `LoadGlobal`, which reads a
    /// cached slot under the borrow it took to find the frame's globals
    /// (#834). Any environment's slot, a frame's included. Raw, as every
    /// `Environment` method is.
    #[inline]
    pub fn slot_value_in(&self, heap: &Heap, slot: u32) -> TaggedValue {
        self.check_heap(heap);
        let tv = self.bindings.borrow().read_slot(slot);
        if self.is_namespace() {
            return Record::from_word(tv).value(heap);
        }
        if tv == TaggedValue::IMPORTED {
            return self.frame_import_value_in(heap, slot);
        }
        tv
    }

    /// Overwrite the value in a slot previously obtained from `local_slot`.
    ///
    /// An imported binding is written where it lives, into the cell it
    /// shares with the library: the library sees the assignment, as it does
    /// under chibi and Gauche. R7RS §5.2 calls assigning an imported binding
    /// "an error", so any answer conforms; this is the one that keeps a
    /// location one location.
    ///
    /// The namespace step's write (#834): a namespace borrows `heap`
    /// mutably, and stores into the cell the slot's record names; a frame
    /// borrows it only for an import, to store into the import's cell.
    #[inline]
    pub fn set_slot_value(&self, heap: &RefCell<Heap>, slot: u32, value: TaggedValue) {
        if let Some(mut step) = self.write_step(heap) {
            self.set_slot_value_in(&mut step, slot, value);
            return;
        }
        self.frame_set_slot_value(heap, slot, value);
    }

    /// [`set_slot_value`](Self::set_slot_value) for a frame, out of line as
    /// [`frame_slot_value`](Self::frame_slot_value) is. A plain slot is
    /// written with no borrow; an import borrows `heap` mutably before
    /// anything is recorded, and stores into its cell.
    #[inline(never)]
    fn frame_set_slot_value(&self, heap: &RefCell<Heap>, slot: u32, value: TaggedValue) {
        let imported = {
            let mut bindings = self.bindings.borrow_mut();
            let imported = bindings.read_slot(slot) == TaggedValue::IMPORTED;
            if !imported {
                crate::census::store(Site::EnvSetSlot, None, value);
                bindings.write_slot(slot, value);
            }
            imported
        };
        if imported {
            let mut step = self.borrow_write(heap);
            let cell = self.frame_import_cell(slot);
            cell.store(&mut step, value, Site::EnvSetSlot);
        }
    }

    /// [`set_slot_value`](Self::set_slot_value) with the heap already
    /// borrowed mutably: the namespace step's body, a store into the cell
    /// the slot's record names. A frame's slot is written as
    /// [`frame_set_slot_value`](Self::frame_set_slot_value) writes it, with
    /// the heap in hand.
    fn set_slot_value_in(&self, heap: &mut Heap, slot: u32, value: TaggedValue) {
        self.check_heap(heap);
        let word = self.bindings.borrow().read_slot(slot);
        if self.is_namespace() {
            let cell = Record::from_word(word).cell(heap);
            cell.store(heap, value, Site::EnvSetSlot);
        } else if word == TaggedValue::IMPORTED {
            let cell = self.frame_import_cell(slot);
            cell.store(heap, value, Site::EnvSetSlot);
        } else {
            crate::census::store(Site::EnvSetSlot, None, value);
            self.bindings.borrow_mut().write_slot(slot, value);
        }
    }

    /// The cell a frame's imported slot names: its entry in
    /// `frame_imports`. An imported slot is written with its entry
    /// (`frame_import`) and a definition over it drops both
    /// (`define_here`), so a marker without a cell is a bug here, not a
    /// state a program can reach.
    fn frame_import_cell(&self, slot: u32) -> BindingCell {
        self.rare
            .get()
            .and_then(|rare| rare.frame_imports.borrow().get(slot as usize).copied())
            .flatten()
            .unwrap_or_else(|| {
                unreachable!("slot {slot} holds the import marker but names no cell")
            })
    }

    /// A frame's import, read with the heap borrowed. Kept out of line:
    /// `slot_value_in` is inlined into the VM's `LoadGlobal`, and a frame's
    /// import is rare there, where the environment is a namespace, a host's
    /// included since #835.
    #[inline(never)]
    fn frame_import_value_in(&self, heap: &Heap, slot: u32) -> TaggedValue {
        self.frame_import_cell(slot).value(heap)
    }

    /// A frame's import, read: it borrows `heap` shared, to read the cell.
    #[inline(never)]
    fn frame_import_value(&self, heap: &RefCell<Heap>, slot: u32) -> TaggedValue {
        let cell = self.frame_import_cell(slot);
        cell.value(&self.borrow_read(heap))
    }

    /// Make `slot` of this frame an import of `cell`, or, for `None`, the
    /// frame's own binding again. Allocates nothing on the heap and takes
    /// no borrow of it. Frames never charge, so nothing is recharged.
    fn set_frame_import(&self, slot: u32, cell: Option<BindingCell>) {
        debug_assert!(!self.is_namespace(), "a namespace's import is a record");
        let slot = slot as usize;
        match cell {
            None => {
                if let Some(rare) = self.rare.get()
                    && let Some(entry) = rare.frame_imports.borrow_mut().get_mut(slot)
                {
                    *entry = None;
                }
            }
            Some(cell) => {
                let rare = self.rare.get_or_init(Default::default);
                let mut imports = rare.frame_imports.borrow_mut();
                if imports.len() <= slot {
                    imports.resize(slot + 1, None);
                }
                imports[slot] = Some(cell);
            }
        }
    }

    /// Bind `name` in this frame as an import of `cell`: the marker in its
    /// slot and the cell in `frame_imports`. No borrow, no allocation.
    fn frame_import(&self, name: Rc<str>, cell: BindingCell) {
        let (slot, _) = self.define_slot(name, TaggedValue::IMPORTED);
        self.set_frame_import(slot, Some(cell));
    }

    /// This namespace made `records` binding records and `cells` cells of
    /// its own (#834): counted on its charge and in the heap's sums. A
    /// frame counts nothing.
    fn count(&self, records: usize, cells: usize) {
        self.count_in(self.rare.get().map(Box::as_ref), records, cells);
    }

    /// [`count`](Self::count) with this environment's `rare` tables in
    /// hand: for an insert arm that has loaded them once already, to ask
    /// whether a placeholder store holds the name (`take_adoptable`).
    #[inline]
    fn count_in(&self, rare: Option<&RareTables>, records: usize, cells: usize) {
        let charge = rare.and_then(|rare| rare.namespace.as_ref());
        debug_assert!(
            charge.is_some() || !self.is_namespace(),
            "a namespace counts its records on its charge"
        );
        if let Some(charge) = charge {
            charge.count(records, cells);
        }
    }

    /// One of this namespace's records was re-pointed away from an own cell
    /// (#834), which an importer's record or an export may still keep.
    fn count_cell_let_go(&self) {
        let charge = self.rare.get().and_then(|rare| rare.namespace.as_ref());
        debug_assert!(
            charge.is_some() || !self.is_namespace(),
            "a namespace counts its cells on its charge"
        );
        if let Some(charge) = charge {
            charge.let_go_cell();
        }
    }

    /// Whether this environment is a namespace of either kind
    /// ([`Kind::Namespace`], [`Kind::HostNamespace`]): records in its slots,
    /// the namespace step, and the namespace-only holder's test
    /// (`Heap::hold_namespace`, #831's ruling on question 1). Either kind
    /// charges for its tables when it has an account ([`NamespaceCharge`])
    /// and indexes at the namespace's threshold ([`Environment::indexes`]).
    /// Asked before a table that frames fill too is measured, so that a frame
    /// skips the measuring for the price of a field read: the kind is in the
    /// struct, where the charge is behind the `rare` box. It tests the
    /// `frame` flag set from the kind, a load and a `cbz` with the frame case
    /// the fall-through (#835).
    #[inline(always)]
    pub(crate) fn is_namespace(&self) -> bool {
        debug_assert_eq!(self.frame, self.kind.frame_flag());
        self.frame == 0
    }

    /// The namespace step's shared borrow (#834): taken where a hit in a
    /// namespace is read, through the record the slot word names. `None` for
    /// a frame, whose own bindings take no borrow.
    #[inline(always)]
    #[must_use = "the step's borrow must span the access"]
    fn read_step<'h>(&self, heap: &'h RefCell<Heap>) -> Option<Ref<'h, Heap>> {
        if !self.is_namespace() {
            return None;
        }
        Some(self.borrow_read(heap))
    }

    /// The namespace step's mutable borrow (#834): taken where a namespace's
    /// binding is written, before anything is. `None` for a frame.
    #[inline(always)]
    #[must_use = "the step's borrow must span the access"]
    fn write_step<'h>(&self, heap: &'h RefCell<Heap>) -> Option<RefMut<'h, Heap>> {
        if !self.is_namespace() {
            return None;
        }
        Some(self.borrow_write(heap))
    }

    /// The step's shared borrow whatever the kind, with the debug identity
    /// check: for a frame's import, which reads its cell.
    #[inline(always)]
    fn borrow_read<'h>(&self, heap: &'h RefCell<Heap>) -> Ref<'h, Heap> {
        let heap = heap.borrow();
        self.check_heap(&heap);
        heap
    }

    /// The step's mutable borrow whatever the kind, with the debug identity
    /// check.
    #[inline(always)]
    fn borrow_write<'h>(&self, heap: &'h RefCell<Heap>) -> RefMut<'h, Heap> {
        let heap = heap.borrow_mut();
        self.check_heap(&heap);
        heap
    }

    /// That `heap` is the one this environment's values live on, in a debug
    /// build.
    #[inline(always)]
    fn check_heap(&self, heap: &Heap) {
        debug_assert_eq!(
            heap.id(),
            self.heap_id,
            "a namespace step with another heap's borrow"
        );
    }

    /// After a table reallocated: a namespace charges the heap for what its
    /// tables hold now ([`NamespaceCharge`]). Anything else — a frame —
    /// has nothing to charge, and finds that out in a load.
    #[inline]
    fn recharge(&self) {
        if let Some(charge) = self.rare.get().and_then(|rare| rare.namespace.as_ref()) {
            self.settle_charge(charge);
        }
    }

    #[cold]
    #[inline(never)]
    fn settle_charge(&self, charge: &NamespaceCharge) {
        charge.settle(self.table_bytes(charge));
    }

    /// After one spelling's own buffers went from `before` bytes to `after`
    /// (`list_bytes`, `ScopeIndex::buffer_bytes`, `Introduced::buffer_bytes`):
    /// a namespace adds the growth to its running sum
    /// ([`NamespaceCharge`]'s `entries`), and reports whether there was any,
    /// for its caller to recharge. Anything else has no sum, and reports
    /// none.
    fn charge_entries(&self, before: usize, after: usize) -> bool {
        match self.rare.get().and_then(|rare| rare.namespace.as_ref()) {
            Some(charge) => charge.grow_entries(before, after),
            None => false,
        }
    }

    /// What this namespace's tables hold outside the heap's arenas, as
    /// [`NamespaceCharge`] counts it.
    ///
    /// Every field of this struct and of [`RareTables`] is named, so a new
    /// one does not compile here until it says whether it is counted.
    fn table_bytes(&self, charge: &NamespaceCharge) -> usize {
        let Environment {
            // A number.
            heap_id: _,
            bindings,
            // A number.
            env_id: _,
            scoped_bindings,
            alias_bindings,
            // Flags.
            has_aliases: _,
            has_visible_scoped: _,
            kind: _,
            frame: _,
            rare,
            // An environment of its own, which charges for itself if it is a
            // namespace.
            parent: _,
        } = self;
        // The environment itself, in its `Rc`'s allocation; and each
        // spelling's own buffers, which their writers sum as they grow.
        let mut bytes = RC_COUNTS + size_of::<Environment>() + charge.entries.get();
        {
            let bindings = bindings.borrow();
            let Bindings { slots, index } = &*bindings;
            if slots.spilled() {
                bytes += slots.capacity() * size_of::<(Rc<str>, TaggedValue)>();
            }
            bytes += charge.name_bytes(slots);
            if let Some(index) = index {
                // Its keys are the slots' names, counted above.
                bytes += size_of::<FxHashMap<Rc<str>, u32>>()
                    + hash_table_bytes::<Rc<str>, u32>(index.capacity());
            }
        }
        bytes +=
            hash_table_bytes::<Rc<str>, ScopedBindingList>(scoped_bindings.borrow().capacity());
        bytes += hash_table_bytes::<Rc<str>, AliasTarget>(alias_bindings.borrow().capacity());
        if let Some(rare) = rare.get() {
            let RareTables {
                introduced_global_names,
                scoped_index,
                frame_imports,
                import_aliases,
                relink_aliases,
                // A heap value, in the arenas.
                specifier: _,
                // This account.
                namespace: _,
                read_through,
                // The weak entries' store charges itself, through the same
                // account (#836).
                placeholders: _,
            } = &**rare;
            bytes += size_of::<RareTables>()
                + hash_table_bytes::<Rc<str>, Introduced>(
                    introduced_global_names.borrow().capacity(),
                )
                + hash_table_bytes::<Rc<str>, ScopeIndex>(scoped_index.borrow().capacity())
                // Always empty in a namespace, whose imports are records.
                + frame_imports.borrow().capacity() * size_of::<Option<BindingCell>>()
                + hash_table_bytes::<u64, ImportAliases>(import_aliases.borrow().capacity())
                + hash_table_bytes::<RelinkKey, Rc<str>>(relink_aliases.borrow().capacity())
                // A host namespace's read-through records (#835): the box and
                // the table's buffer. Its keys are its parent's slot names,
                // shared, and counted there; its records and their cell are
                // pairs, charged in the live bytes.
                + read_through.get().map_or(0, |table| {
                    size_of::<RefCell<ReadThrough>>()
                        + hash_table_bytes::<Rc<str>, Record>(table.borrow().records.capacity())
                });
        }
        bytes
    }

    /// The bytes this namespace has charged the heap's external bytes for
    /// its tables, or `None` for an environment that is not a namespace
    /// (`NamespaceCharge`).
    pub fn charged_bytes(&self) -> Option<usize> {
        self.rare
            .get()
            .and_then(|rare| rare.namespace.as_ref())
            .map(|charge| charge.charged.get())
    }

    /// This environment's mutable specifier: the same heap object each time
    /// it is asked for, allocated the first time. `interaction-environment`
    /// answers it, so that two calls answer one object as they do under
    /// chibi, Gauche and Chez, and a program that calls it in a loop
    /// allocates nothing; `load` evaluates in it.
    ///
    /// Held in this environment and traced with its bindings, so it is as
    /// safe as they are: whatever environment it is asked of, a root reaches
    /// it while the program can use it. Both backends ask it of the
    /// program's global environment, during a library's load as after it:
    /// the VM answered the library's environment there until its load
    /// stopped swapping the machine's globals (#823).
    pub fn mutable_specifier(self: &Rc<Self>, heap: &RefCell<Heap>) -> TaggedValue {
        let rare = self.rare.get_or_init(Default::default);
        if let Some(specifier) = rare.specifier.get() {
            return specifier;
        }
        let specifier = heap
            .borrow_mut()
            .alloc_environment_specifier(Rc::clone(self), true);
        rare.specifier.set(Some(specifier));
        specifier
    }

    /// Define a new binding in this environment
    ///
    /// Use this for top-level defines, built-ins, and other simple bindings.
    /// This is the primary API - accepts TaggedValue directly.
    ///
    /// A namespace borrows `heap` mutably on entry, for an insert, a store
    /// or a re-point alike (#834), each a write of the name's record
    /// (`define_record`); a frame takes no borrow.
    pub fn define(&self, heap: &RefCell<Heap>, name: impl Into<Rc<str>>, value: TaggedValue) {
        if let Some(mut step) = self.write_step(heap) {
            self.define_record(&mut step, name.into(), value);
            return;
        }
        self.define_here(name.into(), value);
    }

    /// [`define`](Self::define) with the heap already borrowed mutably: a
    /// definition for this crate's tests that hold the heap.
    #[cfg(test)]
    pub(crate) fn define_in(&self, heap: &mut Heap, name: impl Into<Rc<str>>, value: TaggedValue) {
        self.check_heap(heap);
        if self.is_namespace() {
            self.define_record(heap, name.into(), value);
        } else {
            self.define_here(name.into(), value);
        }
    }

    /// `define`'s body for a frame, which takes no step: a frame's own
    /// binding is the word in its slot.
    #[inline]
    fn define_here(&self, name: Rc<str>, value: TaggedValue) {
        debug_assert!(
            !self.is_namespace(),
            "a namespace's definition is a record (`define_record`)"
        );
        crate::census::store(Site::EnvDefine, None, value);
        let (slot, was_imported) = self.define_slot(name, value);
        // A definition is a new binding of this environment's own, whatever
        // the name was before — so an importer that defines a name it
        // imported shadows it here and leaves the library's alone, which is
        // what chibi and Gauche do.
        if was_imported {
            self.set_frame_import(slot, None);
        }
    }

    /// `define` in a namespace, under its mutable step (#834): an insert, a
    /// store or a re-point of `name`'s record.
    ///
    /// - A **store** when the record names a cell of this namespace's own:
    ///   the value goes into it, and the record and the slot stay.
    /// - A **re-point** when it names another's (a definition over an
    ///   import): a fresh cell of this namespace's own, holding `value`, and
    ///   the record names it. The library's cell is left alone, as chibi and
    ///   Gauche leave it, and the slot keeps its record.
    /// - An **insert** for a fresh name: a cell and a record, in a new slot.
    fn define_record(&self, heap: &mut Heap, name: Rc<str>, value: TaggedValue) {
        // A plain binding is reachable by name and by nothing else, so
        // `byname=true` here is a property of the table, not a decision.
        crate::scope_trace::bind(&name, &ScopeSet::new(), true);
        match self.local_slot(&name) {
            Some(slot) => {
                let record = Record::from_word(self.bindings.borrow().read_slot(slot));
                let cell = record.cell(heap);
                if cell.owned_by(heap, self.env_id) {
                    cell.store(heap, value, Site::EnvDefine);
                } else {
                    crate::census::store(Site::EnvDefine, None, value);
                    self.forget_alias(&name, cell);
                    let fresh = BindingCell::new_own(heap, value, self.env_id);
                    record.repoint(heap, fresh);
                    self.count(0, 1);
                }
            }
            None => {
                crate::census::store(Site::EnvDefine, None, value);
                // Both allocations are under one `&mut Heap`, which a
                // collection needs, and allocation never collects
                // (`docs/GC_DESIGN.md` §6): the cell, held only in this
                // local until the record holds it, cannot be swept between.
                let cell = BindingCell::new_own(heap, value, self.env_id);
                // Loaded once, for the adoption's test and the count.
                let rare = self.rare.get().map(Box::as_ref);
                match self.take_adoptable(rare, &name) {
                    // A definition of a name a linker asked for (#835,
                    // #836): the read-through record or the placeholder
                    // becomes this namespace's own, in a slot, over the fresh
                    // cell, keeping its word, so code that linked it reads
                    // the cell from now on. A parent's record and cell are
                    // left alone, and a placeholder's own `UNBOUND` cell is
                    // garbage. A strong read-through record was counted when
                    // it was made; a placeholder or a weak one is counted
                    // here, as its store has let go of it.
                    Some((record, adopted)) => {
                        record.adopt(heap, cell);
                        self.append_slot(heap, name, record.word());
                        self.count_in(rare, adopted.records(), 1);
                    }
                    None => {
                        let record = Record::new(heap, cell);
                        self.append_slot(heap, name, record.word());
                        self.count_in(rare, 1, 1);
                    }
                }
            }
        }
    }

    /// A slot for `name`, which has none here, holding `word`: a namespace's
    /// fresh record (#834), or one it adopted (#835, #836). Recharges if a
    /// table grew.
    ///
    /// The one writer of a namespace's slots, so a check build asserts here
    /// that no slot holds a placeholder or a read-through record: a record
    /// in a slot has flags 0 (Rule 1, #836), so every entry the trace skips
    /// is one the sweep's pass may drop, and no slot names a record the pass
    /// dropped. Nothing writes a slot's record's cdr after this but
    /// `Heap::adopt_record`, which writes 0.
    fn append_slot(&self, heap: &Heap, name: Rc<str>, word: TaggedValue) -> u32 {
        if crate::heap::GC_CHECK && heap.cdr(word) != TaggedValue::fixnum(0) {
            flagged_record_in_a_slot(&name);
        }
        let Inserted { slot, grew, .. } = self.bindings.borrow_mut().append(name, word);
        if grew {
            self.recharge();
        }
        slot
    }

    /// This namespace's record for `name`, or `None` for a frame or a name
    /// with no slot here.
    pub(crate) fn local_record(&self, name: &str) -> Option<Record> {
        if !self.is_namespace() {
            return None;
        }
        let bindings = self.bindings.borrow();
        let slot = bindings.slot_of(name)?;
        Some(Record::from_word(bindings.read_slot(slot)))
    }

    /// The record code compiled in this namespace links for `name` (#835,
    /// #836), made the first time and the same one after:
    /// - its own ([`local_record`](Self::local_record)), in a slot;
    /// - `None` for a frame, and for a name this namespace reaches by an
    ///   alias or a visible scoped definition of its own, which
    ///   [`get`](Self::get) finds first and which is read by name;
    /// - in a parentless namespace that does not bind `name`, a placeholder
    ///   (#836): a record of its own with its flag set, over a cell of its
    ///   own holding `TaggedValue::UNBOUND`, kept weakly in its placeholder
    ///   store, never in a slot, until a define or an import adopts it;
    /// - in a host namespace that does not bind `name`, a read-through record
    ///   over its parent's: over the parent's record in a slot, strong, in
    ///   [`ReadThrough`] (a weak one made earlier is promoted there); `None`
    ///   where the parent reaches `name` by an alias or a visible scoped
    ///   definition; otherwise over the parent's placeholder, weak, in this
    ///   namespace's placeholder store (R7).
    ///
    /// Takes the write step. The reads by name (`get`, `get_with_scopes`,
    /// `Interpreter::lookup_in`, the tree-walker's) never call it, so a read
    /// makes no record. A placeholder and a weak read-through record live
    /// while code that links them is traced, and the sweep's pass drops them
    /// once none is (`placeholder_store.rs`).
    pub(crate) fn link_record(&self, heap: &RefCell<Heap>, name: &str) -> Option<Record> {
        let mut step = self.write_step(heap)?;
        if let Some(own) = self.local_record(name) {
            return Some(own);
        }
        // `get`'s order: an alias here, then a visible scoped definition
        // here, both before the parent and before an unbound name. A record
        // would read another binding than `get` does.
        if self.read_by_name(name) {
            return None;
        }
        if self.kind != Kind::HostNamespace {
            return Some(self.placeholder(&mut step, name).1);
        }
        let parent = self
            .parent
            .as_deref()
            .expect("a host namespace has its parent");
        // The parent's slot name, shared: this table counts no name of its
        // own.
        let bound = {
            let bindings = parent.bindings.borrow();
            bindings.slot_of(name).map(|slot| {
                let (key, word) = &bindings.slots[slot as usize];
                (Rc::clone(key), Record::from_word(*word))
            })
        };
        match bound {
            Some((key, parent_record)) => Some(self.strong_child(&mut step, key, parent_record)),
            // The parent's own `get` finds an alias or a visible scoped
            // definition of its own there: read by name, and no placeholder.
            None if parent.read_by_name(name) => None,
            None => {
                let (key, placeholder) = parent.placeholder(&mut step, name);
                Some(self.weak_child(&mut step, key, placeholder))
            }
        }
    }

    /// Whether `get` finds `name` here by an alias or a visible scoped
    /// definition, before any slot's binding but this namespace's own:
    /// [`link_record`](Self::link_record) leaves such a name to be read by
    /// name.
    fn read_by_name(&self, name: &str) -> bool {
        (self.has_aliases.get() && self.alias_target(name).is_some())
            || self.visible_scoped_index(name).is_some()
    }

    /// This namespace's weak entries' store, if a linker has made one
    /// (#836).
    #[inline]
    pub(crate) fn weak_entries(&self) -> Option<&PlaceholderStore> {
        self.rare
            .get()?
            .placeholders
            .get()
            .map(PlaceholderHandle::store)
    }

    /// This namespace's weak entries' store, made of `kind` and listed on
    /// `step`'s placeholder list the first time (#836), counting and
    /// charging through this namespace's account.
    fn weak_entries_or_make(&self, step: &Heap, kind: StoreKind) -> &PlaceholderStore {
        let rare = self
            .rare
            .get()
            .expect("a namespace has its tables from construction");
        let store = rare
            .placeholders
            .get_or_init(|| {
                let account = rare
                    .namespace
                    .as_ref()
                    .expect("a namespace has its charge (#834)")
                    .account
                    .clone();
                PlaceholderHandle::new(
                    step.placeholder_list(),
                    PlaceholderStore::new(self.env_id, account, kind),
                )
            })
            .store();
        debug_assert_eq!(
            store.kind(),
            kind,
            "a namespace's weak entries are of one kind"
        );
        store
    }

    /// This parentless namespace's placeholder for `name`, which it does not
    /// bind (#836), with the name its store holds it under: the one in its
    /// store, or a new one, made and held there, counted a record and a cell
    /// in the heap's sums by the store and charged there.
    fn placeholder(&self, step: &mut Heap, name: &str) -> (Rc<str>, Record) {
        debug_assert_eq!(
            self.kind,
            Kind::Namespace,
            "a host namespace makes no placeholder"
        );
        let store = self.weak_entries_or_make(step, StoreKind::Placeholders);
        if let Some(found) = store.get_key_value(name) {
            return found;
        }
        // Allocation never collects (`docs/GC_DESIGN.md` §6), and both
        // allocations are under the one step: the cell, held in a local,
        // cannot be swept before the record holds it, nor the record before
        // the store does.
        let cell = BindingCell::new_own(step, TaggedValue::UNBOUND, self.env_id);
        let record = Record::placeholder(step, cell);
        let key: Rc<str> = Rc::from(name);
        store.insert(Rc::clone(&key), record);
        (key, record)
    }

    /// This host namespace's read-through records, made the first time, and
    /// whether this call made them.
    fn read_through_table<'a>(&'a self, step: &mut Heap) -> (&'a RefCell<ReadThrough>, bool) {
        let rare = self
            .rare
            .get()
            .expect("a host namespace has its tables from construction");
        let made = rare.read_through.get().is_none();
        let table = rare.read_through.get_or_init(|| {
            let unbound = BindingCell::free(step, TaggedValue::UNBOUND);
            Box::new(RefCell::new(ReadThrough {
                unbound,
                records: FxHashMap::default(),
            }))
        });
        (table, made)
    }

    /// This host namespace's read-through record over `parent_record`, its
    /// parent's record for the name in the slot `key` names (#835): the one
    /// in its table, or one made earlier over the parent's placeholder,
    /// which the parent has since adopted, promoted there out of the
    /// placeholder store (#836), or a new one.
    fn strong_child(&self, step: &mut Heap, key: Rc<str>, parent_record: Record) -> Record {
        let (table, made) = self.read_through_table(step);
        let mut table = table.borrow_mut();
        if let Some(&child) = table.records.get(&*key) {
            return child;
        }
        // A weak child (R7) is promoted, so that it is held and dies with
        // this namespace as any child over a bound parent does (SD3): its
        // count moves from the store to this namespace's charge, as the
        // store lets go of it and the count below takes it on. One child a
        // name across the two tables, or a define here would adopt one and
        // leave the other reading the parent.
        let promoted = self.weak_entries().and_then(|store| store.take(&key));
        let child = match promoted {
            Some(child) => {
                debug_assert_eq!(
                    child.parent_record(step),
                    Some(parent_record),
                    "a weak child reads through the placeholder its parent adopted"
                );
                child
            }
            // Allocation never collects (`docs/GC_DESIGN.md` §6), and both
            // allocations are under the one step: the cell is held by the
            // table before the record is made.
            None => Record::read_through(step, table.unbound, parent_record),
        };
        let capacity = table.records.capacity();
        table.records.insert(key, child);
        let grew = made || table.records.capacity() != capacity;
        drop(table);
        self.count(1, 0);
        if grew {
            self.recharge();
        }
        child
    }

    /// This host namespace's weak read-through record over `placeholder`,
    /// its parent's for the name `key` (R7, #836): the one in its placeholder
    /// store, or a new one held there, counted a record in the heap's sums by
    /// the store. It names the host's one `unbound` cell, as a strong child
    /// does.
    fn weak_child(&self, step: &mut Heap, key: Rc<str>, placeholder: Record) -> Record {
        let (table, made) = self.read_through_table(step);
        let unbound = table.borrow().unbound;
        if made {
            self.recharge();
        }
        let store = self.weak_entries_or_make(step, StoreKind::Children);
        if let Some(child) = store.get(&key) {
            debug_assert_eq!(
                child.parent_record(step),
                Some(placeholder),
                "a weak child reads through its parent's placeholder"
            );
            return child;
        }
        let child = Record::read_through(step, unbound, placeholder);
        store.insert(key, child);
        child
    }

    /// The record a define or an import of `name` here adopts, taken out of
    /// what held it, and where it was counted (#835, #836): a parentless
    /// namespace's placeholder, from its store; a host namespace's strong
    /// read-through record, from [`ReadThrough`], or else its weak one, from
    /// its store. `rare` is this namespace's tables, which its caller loaded.
    ///
    /// The kind test first, as before: a parentless namespace with no store,
    /// as every one is until something links, answers `None` for one load
    /// of the store's cell, with no hash, borrow or `Rc` clone.
    #[inline]
    fn take_adoptable(&self, rare: Option<&RareTables>, name: &str) -> Option<(Record, Adopted)> {
        if self.kind != Kind::HostNamespace {
            let store = rare?.placeholders.get()?;
            return store
                .store()
                .take(name)
                .map(|record| (record, Adopted::FromStore));
        }
        self.take_read_through(rare?, name)
    }

    /// [`take_adoptable`](Self::take_adoptable) in a host namespace: its
    /// strong read-through record for `name`, or else its weak one. At most
    /// one of the two exists (`link_record` promotes a weak one).
    fn take_read_through(&self, rare: &RareTables, name: &str) -> Option<(Record, Adopted)> {
        let strong = rare
            .read_through
            .get()
            .and_then(|table| table.borrow_mut().records.remove(name));
        if let Some(child) = strong {
            return Some((child, Adopted::Counted));
        }
        let store = rare.placeholders.get()?;
        store
            .store()
            .take(name)
            .map(|child| (child, Adopted::FromStore))
    }

    /// Whether a placeholder or a weak read-through record holds `name` here
    /// (#836). An alias is never minted at such a name: an import alias's
    /// install would adopt the entry, and code that linked an unbound name
    /// would read the alias's location; a relink alias would leave linked
    /// code reading the entry where `get` reads the alias.
    fn holds_placeholder(&self, name: &str) -> bool {
        self.weak_entries()
            .is_some_and(|store| store.contains(name))
    }

    /// Make `name` here the binding `cell` is: an import (R7RS §5.2, #406).
    /// What every import in both backends comes to (`Library::import_into`).
    ///
    /// In a namespace, under its mutable step (#834):
    /// - an **insert** when the name has no slot here: a record naming
    ///   `cell`;
    /// - a **re-point** when it has one, a definition or another import:
    ///   the slot keeps its record, which names `cell` from now on;
    /// - a cell of this namespace's own, imported under another name, is
    ///   **copied** by `define`'s rules: two names of one namespace sharing a
    ///   cell would make a later `define` of either assign both.
    ///
    /// In a frame, the marker in its slot and the cell beside it
    /// (`frame_imports`): no borrow of the heap and no allocation.
    ///
    /// See [`BindingCell`] for why every import shares, primitives included.
    pub fn import_cell(&self, heap: &RefCell<Heap>, name: impl Into<Rc<str>>, cell: BindingCell) {
        match self.write_step(heap) {
            Some(mut step) => self.bind_cell(&mut step, name.into(), cell),
            None => self.frame_import(name.into(), cell),
        }
    }

    /// [`import_cell`](Self::import_cell) in a namespace, under its step.
    fn bind_cell(&self, heap: &mut Heap, name: Rc<str>, cell: BindingCell) {
        let existing = self.local_record(&name);
        if cell.owned_by(heap, self.env_id) {
            // Two names of one namespace never share a cell, or a later
            // `define` of either would assign both: copied, as a forward to
            // itself was copied before records.
            if existing.is_none_or(|record| record.cell(heap) != cell) {
                let value = cell.value(heap);
                self.define_record(heap, name, value);
            }
            return;
        }
        // Every import is traced as a binding by name, a re-point included,
        // as it was when it went through `define_slot`.
        crate::scope_trace::bind(&name, &ScopeSet::new(), true);
        match existing {
            Some(record) => {
                let old = record.cell(heap);
                if old == cell {
                    return;
                }
                self.forget_alias(&name, old);
                if old.owned_by(heap, self.env_id) {
                    self.count_cell_let_go();
                }
                record.repoint(heap, cell);
            }
            None => {
                // Loaded once, for the adoption's test and the count.
                let rare = self.rare.get().map(Box::as_ref);
                match self.take_adoptable(rare, &name) {
                    // An import of a name a linker asked for (#835, #836):
                    // the read-through record or the placeholder names the
                    // exporter's cell from now on, in a slot, keeping its
                    // word; a placeholder's own `UNBOUND` cell is garbage. A
                    // strong read-through record was counted when it was
                    // made; a placeholder or a weak one is counted here, as
                    // its store has let go of it.
                    Some((record, adopted)) => {
                        record.adopt(heap, cell);
                        self.append_slot(heap, name, record.word());
                        self.count_in(rare, adopted.records(), 0);
                    }
                    None => {
                        let record = Record::new(heap, cell);
                        self.append_slot(heap, name, record.word());
                        self.count_in(rare, 1, 0);
                    }
                }
            }
        }
    }

    /// `name`'s record is re-pointed away from `cell`, so it is no longer an
    /// alias [`import_alias`](Self::import_alias) minted for `cell`, and its
    /// entry goes. An entry lives only while its alias's record names its
    /// cell, which keeps the cell alive, so no key outlives its cell and
    /// meets a cell made in its slot after a collection (the rulings of
    /// 2026-11-03). Another spelling's entry for `cell` stays.
    fn forget_alias(&self, name: &str, cell: BindingCell) {
        let Some(rare) = self.rare.get() else {
            return;
        };
        let key = cell.key();
        let mut table = rare.import_aliases.borrow_mut();
        if let Some(aliases) = table.get_mut(&key) {
            aliases.retain(|(_, alias)| &**alias != name);
            if aliases.is_empty() {
                table.remove(&key);
            }
        }
    }

    /// The cell this environment's own binding of `name` names: a namespace
    /// slot's record's cell; a namespace visible scoped entry's record's
    /// cell, the name-only view [`get`](Self::get) reads; a frame's import's
    /// cell. `None` for a frame's own binding, a name reached only through a
    /// macro-expansion alias (refused for export by #847), or through the
    /// parent.
    ///
    /// What a library exports (`build_library`), and what a test hands
    /// [`import_cell`](Self::import_cell). A namespace borrows `heap`
    /// shared at its step.
    pub fn cell_of(&self, heap: &RefCell<Heap>, name: &str) -> Option<BindingCell> {
        if let Some(slot) = self.local_slot(name) {
            let word = self.bindings.borrow().read_slot(slot);
            if let Some(step) = self.read_step(heap) {
                return Some(Record::from_word(word).cell(&step));
            }
            return (word == TaggedValue::IMPORTED).then(|| self.frame_import_cell(slot));
        }
        // `get` follows an alias before the name-only view: so does this,
        // to agree with it, and an alias is no binding to name.
        if self.has_aliases.get() && self.alias_target(name).is_some() {
            return None;
        }
        let i = self.visible_scoped_index(name)?;
        let step = self.read_step(heap)?;
        let table = self.scoped_bindings.borrow();
        let entry = table.get(name)?.get(i)?;
        Some(Record::from_word(entry.tagged_value).cell(&step))
    }

    /// Each name [`local_names`](Self::local_names) lists, with its cell:
    /// the tree-walker's export of a Rust-built library. A namespace; a
    /// frame answers its imports alone.
    pub fn cells(&self, heap: &RefCell<Heap>) -> Vec<(String, BindingCell)> {
        let slots = self.visible_slots(heap);
        let step = self.read_step(heap);
        let bindings = self.bindings.borrow();
        slots
            .into_iter()
            .filter_map(|(slot, name)| {
                let word = bindings.read_slot(slot);
                let cell = match &step {
                    Some(step) => Record::from_word(word).cell(step),
                    None if word == TaggedValue::IMPORTED => self.frame_import_cell(slot),
                    None => return None,
                };
                Some((name, cell))
            })
            .collect()
    }

    /// Carry `staged_name` from the staging frame `staging` into this
    /// environment as `name`: a staged import arrives as an import of the
    /// same cell (by [`import_cell`](Self::import_cell)'s rules); a staged
    /// own binding as a [`define`](Self::define). `false`, having done
    /// nothing, when `staging` does not bind the name itself.
    ///
    /// For the desugarer's staged import (`desugar_with_imports`): a staging
    /// environment owns nothing, it only carries bindings through, so its
    /// imports arrive as the libraries' cells and not as bindings of an
    /// environment about to be dropped.
    pub fn copy_binding(
        &self,
        heap: &RefCell<Heap>,
        name: impl Into<Rc<str>>,
        staging: &Rc<Environment>,
        staged_name: &str,
    ) -> bool {
        debug_assert!(!staging.is_namespace(), "a staging environment is a frame");
        let Some(slot) = staging.local_slot(staged_name) else {
            return false;
        };
        let word = staging.bindings.borrow().read_slot(slot);
        if word == TaggedValue::IMPORTED {
            self.import_cell(heap, name, staging.frame_import_cell(slot));
        } else {
            self.define(heap, name, word);
        }
        true
    }

    /// A frame's `define` without its bookkeeping: writes the slot, and
    /// reports it and whether an import was there before.
    #[inline]
    fn define_slot(&self, name: Rc<str>, value: TaggedValue) -> (u32, bool) {
        // A plain binding is reachable by name and by nothing else, so
        // `byname=true` here is a property of the table, not a decision.
        crate::scope_trace::bind(&name, &ScopeSet::new(), true);
        let Inserted {
            slot,
            was_imported,
            grew,
        } = self.bindings.borrow_mut().insert(name, value);
        if grew {
            self.recharge();
        }
        (slot, was_imported)
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
    /// The alias is a second record over the same cell (#834), so it reads,
    /// writes and caches like the import it stands beside; it is minted once
    /// per name for a location (`mint` runs only then) and shared by every
    /// expansion after. It stays a name until 4b.10.
    ///
    /// Per *name*, not per location alone: a location this environment
    /// imported under two names — `car`, and `first` by a `rename` — gets an
    /// alias for each. The desugarer takes an early binding back by reading
    /// the spelling off the alias, and one alias for both spellings settled a
    /// reference written `first` by what the form said about `car`.
    ///
    /// Being an ordinary slot, it can be defined over like one, by a program
    /// that happens to spell it — `car.17` is a legal identifier, and
    /// machine-written Scheme is full of names like it. A definition over it
    /// re-points its record and forgets it as an alias (`forget_alias`), so
    /// the next request mints another: what was bound to the old one is lost
    /// to that definition, but nothing expanded afterwards follows it there.
    /// For the same reason `mint` is asked again while the spelling it
    /// offers is already bound here. (Only a *reference* is ever given this
    /// name — `Desugarer::early_bound` — so nothing Patina does defines over
    /// one.)
    ///
    /// A namespace's only: a frame answers `None`, and `Desugarer::import_of`
    /// asks a namespace, the nearest and then each above it (#835). Borrows
    /// `heap` shared over the lookup, and mutably to insert a fresh alias
    /// (#834).
    pub fn import_alias(
        &self,
        heap: &RefCell<Heap>,
        name: &str,
        mut mint: impl FnMut() -> Rc<str>,
    ) -> Option<Rc<str>> {
        let record = self.local_record(name)?;
        let rare = self.rare.get()?;
        let (cell, key) = {
            // One step over the lookup: the imported cell, the alias
            // remembered for it, and whether that still names it.
            let step = self.borrow_read(heap);
            let cell = record.cell(&step);
            if cell.owned_by(&step, self.env_id) {
                return None;
            }
            let key = cell.key();
            let known = rare.import_aliases.borrow().get(&key).and_then(|aliases| {
                aliases
                    .iter()
                    .find(|(spelling, _)| &**spelling == name)
                    .map(|(_, alias)| Rc::clone(alias))
            });
            if let Some(alias) = known {
                // `forget_alias` makes a mismatch impossible; checked anyway,
                // as the defence the reuse tests exercise.
                let still_names = self
                    .local_record(&alias)
                    .is_some_and(|alias_record| alias_record.cell(&step) == cell);
                if still_names {
                    return Some(alias);
                }
            }
            (cell, key)
        };
        // A name a placeholder holds is taken too (#836): the alias's
        // install would adopt it.
        let alias = loop {
            let candidate = mint();
            if self.local_slot(&candidate).is_none() && !self.holds_placeholder(&candidate) {
                break candidate;
            }
        };
        self.bind_cell(&mut self.borrow_write(heap), Rc::clone(&alias), cell);
        let grew = {
            let mut table = rare.import_aliases.borrow_mut();
            let capacity = table.capacity();
            let aliases = table.entry(key).or_default();
            match aliases.iter_mut().find(|(spelling, _)| &**spelling == name) {
                // Replacing one that was defined over.
                Some((_, stale)) => *stale = Rc::clone(&alias),
                None => aliases.push((Rc::from(name), Rc::clone(&alias))),
            }
            table.capacity() != capacity
        };
        if grew {
            self.recharge();
        }
        Some(alias)
    }

    /// The value of a binding in this environment's own table, read through
    /// its record in a namespace and through its cell for a frame's import.
    /// Every by-name read of the table goes through here; `Bindings` has no
    /// `get` of its own, so none can hand out a record or the marker.
    ///
    /// A hit in a namespace is read at its step, under a shared borrow of
    /// `heap`; a miss takes none, and nor does a frame's own binding (#834).
    /// A frame's import borrows it shared. Both branches stay inline: the
    /// tree-walker's frames are hot.
    #[inline(always)]
    fn local_value(&self, heap: &RefCell<Heap>, name: &str) -> Option<TaggedValue> {
        let (slot, tv) = {
            let bindings = self.bindings.borrow();
            let slot = bindings.slot_of(name)?;
            (slot, bindings.read_slot(slot))
        };
        if let Some(step) = self.read_step(heap) {
            return Some(Record::from_word(tv).value(&step));
        }
        if tv == TaggedValue::IMPORTED {
            return Some(self.frame_import_value(heap, slot));
        }
        Some(tv)
    }

    /// [`local_value`](Self::local_value) with the heap already borrowed:
    /// for this crate's tests that hold it.
    #[cfg(test)]
    pub(crate) fn local_value_in(&self, heap: &Heap, name: &str) -> Option<TaggedValue> {
        self.check_heap(heap);
        let (slot, tv) = {
            let bindings = self.bindings.borrow();
            let slot = bindings.slot_of(name)?;
            (slot, bindings.read_slot(slot))
        };
        if self.is_namespace() {
            return Some(Record::from_word(tv).value(heap));
        }
        if tv == TaggedValue::IMPORTED {
            return Some(self.frame_import_value_in(heap, slot));
        }
        Some(tv)
    }

    /// Define a primitive procedure in this environment, through the
    /// namespace view ([`NamespaceView::define_primitive`](crate::NamespaceView::define_primitive)),
    /// which says what it binds.
    ///
    /// The `library` parameter (e.g., `["scheme", "base"]`) is joined with "."
    /// and combined with `name` to produce a `qualified_name` (e.g., `"scheme.base/+"`).
    /// This qualified name is computed once here at init time, avoiding repeated
    /// `format!()` allocations on every primitive call.
    pub fn define_primitive(
        &self,
        heap: &RefCell<Heap>,
        name: &'static str,
        arity: crate::procedure::Arity,
        library: Vec<String>,
    ) {
        crate::NamespaceView::of(self).define_primitive(heap, name, arity, library);
    }

    /// Set an existing binding (searches parent environments)
    /// This is the primary API - accepts TaggedValue directly.
    ///
    /// The binding it writes, in a namespace, is written at that
    /// namespace's step, under a mutable borrow of `heap` (#834).
    pub fn set(&self, heap: &RefCell<Heap>, name: &str, value: TaggedValue) -> Result<(), String> {
        // A loop up the chain, not a call per parent: a chain is as long as
        // the code is nested, and recursing on it overflowed the native stack
        // (#617).
        let mut env = self;
        loop {
            if let Some(slot) = env.local_slot(name) {
                env.set_slot_value(heap, slot, value);
                return Ok(());
            }
            // Assign through a macro-expansion alias, so a template that
            // mutates a binding private to its defining library works. Reads
            // follow aliases in `get`; writes have to as well or the two
            // disagree.
            if let Some(target) = env.alias_target(name) {
                return target.set(env, heap, value);
            }
            // The write side of the same fallback `get` takes, and it has to
            // be here for the reason `alias_bindings` gives for its own pair:
            // a name that reads through to a scoped definition and does not
            // write through to it leaves the two disagreeing. Reached when a
            // macro-generated macro's template assigns to a definition its
            // expansion introduced, which arrives relinked to the bare name.
            if let Some(i) = env.visible_scoped_index(name) {
                let mut step = env.write_step(heap);
                if let Some(bindings) = env.scoped_bindings.borrow_mut().get_mut(name) {
                    write_entry(
                        step.as_deref_mut(),
                        &mut bindings[i],
                        value,
                        Site::EnvSetScoped,
                        true,
                    );
                    return Ok(());
                }
            }
            match &env.parent {
                Some(parent) => env = parent,
                None => return Err(name.to_string()),
            }
        }
    }

    /// Get a TaggedValue from the environment (searches parent environments)
    ///
    /// This is the primary API - returns TaggedValue directly.
    /// For the simple name-based lookup for identifiers.
    ///
    /// A hit in a namespace is read at that namespace's step, under a shared
    /// borrow of `heap`; a miss, and a frame's own binding, take none (#834).
    pub fn get(&self, heap: &RefCell<Heap>, name: &str) -> Option<TaggedValue> {
        // Walked iteratively rather than by recursing into the parent: the
        // tree-walker builds a frame per `let`-bound temporary, so this chain
        // is the hottest loop in the backend.
        let mut env = self;
        loop {
            if let Some(tv) = env.local_value(heap, name) {
                return Some(tv);
            }
            // Follow a macro-expansion alias into the environment the macro was
            // defined in. Checked after real bindings so a local definition always
            // wins, and before the parent so the alias is not shadowed by an
            // unrelated outer binding of the same (unique) name.
            if env.has_aliases.get()
                && let Some(target) = env.alias_target(name)
            {
                return target.get(env, heap);
            }
            // A macro-introduced definition lives under its scopes; this is the
            // name-only view of it. Checked after real bindings and aliases, so a
            // binding written in source always wins.
            if env.has_visible_scoped.get()
                && let Some(i) = env.visible_scoped_index(name)
            {
                let step = env.read_step(heap);
                return env.scoped_bindings.borrow()[name]
                    .get(i)
                    .map(|b| resolve(step.as_deref(), b.tagged_value));
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
    /// A hit in a namespace is located at its step, under a shared borrow of
    /// `heap`: the cell its record names, own or imported (#834). A frame's
    /// hit takes none: a frame's import names its cell, and a frame's own
    /// binding is its slot, and neither reads the heap.
    ///
    /// [`get`]: Self::get
    pub fn binding_location(&self, heap: &RefCell<Heap>, name: &str) -> Option<BindingLocation> {
        let mut env = self;
        loop {
            if let Some(slot) = env.local_slot(name) {
                return Some(env.slot_location(heap, slot));
            }
            if env.has_aliases.get()
                && let Some(target) = env.alias_target(name)
            {
                return target.location(env, heap);
            }
            if let Some(i) = env.visible_scoped_index(name) {
                let step = env.read_step(heap);
                let table = env.scoped_bindings.borrow();
                let (spelling, bindings) = table.get_key_value(name)?;
                return Some(match step {
                    Some(step) => Record::from_word(bindings.get(i)?.tagged_value)
                        .cell(&step)
                        .location(),
                    None => BindingLocation::Scoped(env.env_id(), Rc::clone(spelling), i),
                });
            }
            env = env.parent.as_deref()?;
        }
    }

    /// Where this environment's `slot` lives, as `binding_location` names
    /// it: a namespace's record's cell, read at its step; a frame's
    /// import's cell; or a frame's own slot.
    fn slot_location(&self, heap: &RefCell<Heap>, slot: u32) -> BindingLocation {
        let step = self.read_step(heap);
        let word = self.bindings.borrow().read_slot(slot);
        if let Some(step) = step {
            return Record::from_word(word).cell(&step).location();
        }
        if word == TaggedValue::IMPORTED {
            return self.frame_import_cell(slot).location();
        }
        BindingLocation::Slot(self.env_id(), slot)
    }

    /// Whether this environment indexes a spelling's scoped bindings when it
    /// has `len` of them: past [`INDEX_ABOVE`] in a namespace of either
    /// kind, a host namespace's included (#835), past [`FRAME_INDEX_ABOVE`]
    /// in a frame of either frame kind ([`Kind`]), which is why a frame of
    /// parameters does not build one per call. The one place the scoped
    /// table's thresholds are applied, so that the writer that builds an
    /// index and the readers that consult it agree on which lists have one.
    /// (The VM's identities, `Introduced`, are recorded only in a namespace,
    /// and carry their own index past [`INDEX_ABOVE`].) The
    /// first test is the one nearly every list fails, so a short list costs a
    /// compare.
    #[inline]
    fn indexes(&self, len: usize) -> bool {
        len > INDEX_ABOVE && (self.is_namespace() || len > FRAME_INDEX_ABOVE)
    }

    /// The index of this environment's scoped bindings of `name`, which the
    /// caller has borrowed and which are `len` long, or `None` for a list
    /// that is not indexed ([`indexes`]) and that a scan answers.
    ///
    /// An indexed list without an index, or with one that does not cover it,
    /// is a broken invariant (`insert_scoped` builds and grows both), and
    /// fails here in a debug build. A release build scans instead, since a
    /// scan answers every question the index does.
    ///
    /// [`indexes`]: Self::indexes
    fn scoped_index(&self, name: &str, len: usize) -> Option<Ref<'_, ScopeIndex>> {
        if !self.indexes(len) {
            return None;
        }
        let index = self.rare.get().and_then(|rare| {
            Ref::filter_map(rare.scoped_index.borrow(), |indexes| indexes.get(name)).ok()
        });
        debug_assert!(
            index.as_ref().is_some_and(|index| index.len() == len),
            "the scoped bindings of `{name}` are {len} long, and their index covers {:?}",
            index.as_ref().map(|index| index.len())
        );
        index.filter(|index| index.len() == len)
    }

    /// Where among `bindings`, this environment's scoped bindings of `name`,
    /// the one at exactly `scopes` is. There is at most one: `insert_scoped`
    /// overwrites a binding at a set already present.
    fn scoped_position(
        &self,
        name: &str,
        bindings: &[ScopedBinding],
        scopes: &ScopeSet,
    ) -> Option<usize> {
        match self.scoped_index(name, bindings.len()) {
            Some(index) => index.exact(scopes).find(|&i| bindings[i].scopes == *scopes),
            None => bindings.iter().position(|b| b.scopes == *scopes),
        }
    }

    /// Visit each of `bindings`, this environment's scoped bindings of
    /// `name`, that can be a candidate for `reference`, newest first, through
    /// the spelling's index when it has one ([`visit_candidates`]).
    fn visit_scoped_candidates(
        &self,
        name: &str,
        bindings: &[ScopedBinding],
        reference: &ScopeSet,
        f: impl FnMut(usize, &ScopedBinding, bool),
    ) {
        let index = self.scoped_index(name, bindings.len());
        visit_candidates(
            bindings,
            |binding| &binding.scopes,
            index.as_deref(),
            reference,
            f,
        );
    }

    /// The location of the scoped definition of `name` whose identity is
    /// exactly `scopes`, in this environment's own table. A namespace reads
    /// it at its step, under a shared borrow of `heap`.
    fn scoped_definition_location(
        &self,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Option<BindingLocation> {
        let step = self.read_step(heap);
        let table = self.scoped_bindings.borrow();
        let (spelling, bindings) = table.get_key_value(name)?;
        let index = self.scoped_position(name, bindings, scopes)?;
        Some(match step {
            // A namespace's entry is a record: its cell (#834).
            Some(step) => Record::from_word(bindings[index].tagged_value)
                .cell(&step)
                .location(),
            None => BindingLocation::Scoped(self.env_id(), Rc::clone(spelling), index),
        })
    }

    /// What that definition holds — [`scoped_definition_location`]'s read.
    ///
    /// [`scoped_definition_location`]: Self::scoped_definition_location
    fn scoped_definition_value(
        &self,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Option<TaggedValue> {
        let step = self.read_step(heap);
        let table = self.scoped_bindings.borrow();
        let bindings = table.get(name)?;
        let position = self.scoped_position(name, bindings, scopes)?;
        Some(resolve(step.as_deref(), bindings[position].tagged_value))
    }

    /// Assign that definition — [`scoped_definition_location`]'s write. `Err`
    /// carries the name, as [`set`]'s does. A namespace writes it at its
    /// step, under a mutable borrow of `heap`.
    ///
    /// [`scoped_definition_location`]: Self::scoped_definition_location
    /// [`set`]: Self::set
    fn set_scoped_definition(
        &self,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
        value: TaggedValue,
    ) -> Result<(), String> {
        let mut step = self.write_step(heap);
        let mut table = self.scoped_bindings.borrow_mut();
        let binding = table.get_mut(name).and_then(|bindings| {
            let position = self.scoped_position(name, bindings, scopes)?;
            bindings.get_mut(position)
        });
        match binding {
            Some(binding) => {
                write_entry(
                    step.as_deref_mut(),
                    binding,
                    value,
                    Site::EnvSetScoped,
                    false,
                );
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
                .is_some_and(|introduced| !introduced.identities.is_empty())
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
    /// The definition's home is always this chain's root, which every caller
    /// holds already ([`root`](Self::root)), so the answer does not repeat
    /// it. Reads at each namespace's step, under a shared borrow of `heap`
    /// (#834).
    ///
    /// [`define_introduced_global`]: Self::define_introduced_global
    pub fn introduced_definition(
        &self,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Option<IntroducedDefinition> {
        let chosen = self.scoped_binding_of(heap, name, scopes).ok()??;
        // Walked by reference: `root` clones an `Rc` per frame.
        let mut root = self;
        while let Some(parent) = root.parent.as_deref() {
            root = parent;
        }
        if let Some(renamed) = root.introduced_global(name, &chosen) {
            return Some(IntroducedDefinition::Renamed(renamed));
        }
        root.scoped_definition_location(heap, name, &chosen)
            .is_some()
            .then_some(IntroducedDefinition::Scoped(chosen))
    }

    /// The global the macro-introduced top-level definition of `name` at
    /// exactly `scopes` was renamed to, if this environment recorded one
    /// ([`define_introduced_global`]). A probe of the spelling's index, so
    /// that nothing has to scan its identities ([`Introduced`]).
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

    /// Whether `name` is reached here only through a macro-expansion alias:
    /// no binding of this environment's own, and an alias the desugarer
    /// installed under that name. `get` follows such an alias, but the alias
    /// is not a binding, so a library cannot export it (#846).
    pub fn reaches_only_by_alias(&self, name: &str) -> bool {
        self.has_aliases.get()
            && self.local_slot(name).is_none()
            && self.alias_target(name).is_some()
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
    /// Records the binding identity that later forms resolve at compile and
    /// desugar time, and that definition-environment relinking uses to find
    /// the renamed global. No bare-name alias is installed: that would let a
    /// reference from another expansion reach a definition its scopes reject
    /// (#427, triage family 40).
    ///
    /// Two different expansions get different scope sets and so are different
    /// entries, which is the whole point — and which means a program that
    /// expands such a macro `n` times records `n` entries. That is
    /// proportional to something that already grows: each of those expansions
    /// also defines its own global, under the name recorded here, and those
    /// are never reclaimed either.
    ///
    /// Found by the spelling's index rather than scanned for, which is not a
    /// micro-optimisation: a linear scan per insert made compiling `n`
    /// expansions of one spelling quadratic, measured at 12x main for
    /// n = 12000. A repeated identical `(name, scopes)` overwrites, which
    /// happens when one expansion is compiled twice (`Introduced`).
    ///
    pub fn define_introduced_global(&self, name: Rc<str>, scopes: ScopeSet, renamed_to: Rc<str>) {
        let grew = {
            let mut table = self
                .rare
                .get_or_init(Default::default)
                .introduced_global_names
                .borrow_mut();
            let capacity = table.capacity();
            let introduced = table.entry(name).or_default();
            let before = introduced.buffer_bytes();
            introduced.record(scopes, renamed_to);
            // The spelling's identities and index, which a namespace sums.
            let after = introduced.buffer_bytes();
            let table_grew = table.capacity() != capacity;
            self.charge_entries(before, after) || table_grew
        };
        if grew {
            self.recharge();
        }
    }

    /// Call `f` with each macro-introduced top-level definition of `name`
    /// ([`define_introduced_global`]) that is a candidate for a reference at
    /// `reference`: the scope set it was introduced at, and the global it was
    /// renamed to.
    ///
    /// A callback rather than a returned collection because the caller keeps
    /// only the candidates for one reference, and building a list first meant
    /// cloning every entry — a scope set and an `Rc` apiece — on every scoped
    /// reference to the spelling.
    ///
    /// Through the spelling's index once it has more than eight definitions:
    /// a reference probes one bucket per scope it carries, and is shown only
    /// the definitions filed under them (`scope_index.rs`).
    /// This was a scan, one visit per definition of the spelling for each
    /// reference, and with the same scan of the scoped table it made `n`
    /// expansions of one macro cost time quadratic in `n` (#613).
    ///
    /// Newest first, as `resolve_index` wants each environment's candidates.
    /// No resolution depends on that: each scope set is recorded once, and of
    /// two candidates for one reference [`crate::scope_resolve::resolve_index`]
    /// takes the one that contains the other, or refuses both as ambiguous,
    /// rather than settling them by position. What the order does decide is
    /// how an ambiguity is reported: which binding it names as the pick (the
    /// first of the largest), and so which rivals it lists (those the pick
    /// does not contain), and in what order. Before #613 that followed the
    /// identities map's unspecified order.
    ///
    /// Parents are not walked: a caller that resolves through a chain, as
    /// the VM's alpha-renaming does for a host namespace and the global one
    /// above it (#620, #835), asks each environment of it.
    ///
    /// [`define_introduced_global`]: Self::define_introduced_global
    pub fn for_each_introduced_candidate(
        &self,
        name: &str,
        reference: &ScopeSet,
        f: impl FnMut(&ScopeSet, &Rc<str>),
    ) {
        let Some(rare) = self.rare.get() else {
            return;
        };
        if let Some(introduced) = rare.introduced_global_names.borrow().get(name) {
            introduced.for_each_candidate(reference, f);
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
    /// finishes, so aliases go to the root instead. For a host form the root
    /// is the global namespace, past its host namespace: a relink alias goes
    /// there, and only an import alias is asked of the host namespace first
    /// ([`namespace_root`](Self::namespace_root), #835).
    pub fn root(self: &Rc<Self>) -> Rc<Environment> {
        let mut env = self.clone();
        while let Some(parent) = env.parent.clone() {
            env = parent;
        }
        env
    }

    /// The nearest namespace at or above this environment: itself if it is a
    /// namespace of either kind, else the namespace its frames hang from. For
    /// a host form that is its host namespace; otherwise the parentless root
    /// (the owner's ruling (c) of 2026-11-07 on #835's question 3). Raw, as
    /// [`root`](Self::root) is.
    pub fn namespace_root(self: &Rc<Self>) -> Rc<Environment> {
        let mut env = Rc::clone(self);
        while !env.is_namespace() {
            env = Rc::clone(env.parent.as_ref().expect("a frame has its parent"));
        }
        env
    }

    /// Install a macro-expansion alias: `alias` resolves to `target_name` as
    /// bound in `target_env`, looked up afresh on every access.
    ///
    /// The desugarer installs a generated, unique alias so it cannot shadow
    /// anything the program wrote. For an introduced definition, its scopes
    /// select the target: the renamed global on the VM, or a scoped alias on
    /// the tree-walker. The VM no longer installs aliases under the bare
    /// spelling of introduced globals (#427).
    ///
    /// Keyed by `alias`, so a second install under the same name replaces the
    /// first.
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

    /// The alias here for the binding of `name` in `target_env` — the one
    /// `scopes` select, for a binding the name alone does not reach — as a
    /// macro's mention of it under `mention`, its identity, renames it:
    /// minted once per target and mention (#611).
    ///
    /// The desugarer's relinking (`link_definition_env_refs`) used to mint an
    /// alias for every expansion that needed one, so a library macro whose
    /// template names a private helper, as `guard` names `%guard-aux`, left an
    /// alias and an interned symbol in the program per top-level form, which
    /// nothing removes. One target now has one alias here, as one imported
    /// location has one in [`import_alias`]. Two targets, or two identities of
    /// one spelling that select different bindings, still get one each.
    ///
    /// And two mentions of one target get one each. The alias's spelling is
    /// also how a renamed mention is told from another of the same name: a
    /// generated macro can mention a spelling once as a binder of its own
    /// template and once as the library's binding, both reaching the same
    /// target, and one spelling for the two had the binder capture the other
    /// (`hygiene_matrix.rs`, `one_spelling_mentioned_as_two_bindings_…`). A
    /// macro's mention is the same from one expansion to the next — written
    /// templates mention a name under their definition scopes — so this costs
    /// nothing in what is shared. Two *expansions* of one mention share the
    /// spelling and are told apart by the scopes their occurrences keep
    /// (`Desugarer::rewrite_refs`).
    ///
    /// An alias is an ordinary spelling (`%guard-aux.17` is a legal
    /// identifier), so a program can define over it. A remembered alias is
    /// reused only while it is still an alias to the same target and nothing
    /// here binds its spelling, and is replaced otherwise; and `mint` is asked
    /// again while the spelling it offers is bound here already. Otherwise it
    /// is installed as [`define_alias`] or [`define_scoped_alias`] would.
    ///
    /// A frame first reuses an alias its root holds for the target: a lookup
    /// from the frame reaches the root's, and the frame installs nothing. The
    /// desugarer's speculative body scan works in a frame, which it drops with
    /// whatever was minted there.
    ///
    /// [`import_alias`]: Self::import_alias
    /// [`define_alias`]: Self::define_alias
    /// [`define_scoped_alias`]: Self::define_scoped_alias
    pub fn relink_alias(
        &self,
        target_env: Rc<Environment>,
        name: Rc<str>,
        scopes: Option<ScopeSet>,
        mention: &ScopeSet,
        mut mint: impl FnMut() -> Rc<str>,
    ) -> Rc<str> {
        let key: RelinkKey = (target_env.env_id(), name, scopes, mention.clone());
        if let Some(alias) = self.remembered_relink(&key) {
            return alias;
        }
        let mut root = self.parent.as_ref();
        while let Some(parent) = root.and_then(|env| env.parent.as_ref()) {
            root = Some(parent);
        }
        if let Some(alias) = root.and_then(|root| root.remembered_relink(&key)) {
            return alias;
        }
        // A name a placeholder holds is taken too (#836): code that linked
        // it would read the placeholder where `get` reads the alias.
        let alias = loop {
            let candidate = mint();
            if self.local_slot(&candidate).is_none()
                && self.alias_target(&candidate).is_none()
                && !self.holds_placeholder(&candidate)
            {
                break candidate;
            }
        };
        self.install_alias(Rc::clone(&alias), target_env, key.1.clone(), key.2.clone());
        let rare = self.rare.get_or_init(Box::default);
        let grew = {
            let mut table = rare.relink_aliases.borrow_mut();
            let capacity = table.capacity();
            table.insert(key, Rc::clone(&alias));
            table.capacity() != capacity
        };
        if grew {
            self.recharge();
        }
        alias
    }

    /// [`relink_alias`] to the definition [`introduced_definition`] found, as
    /// [`define_alias_to_introduced`] installs one.
    ///
    /// [`relink_alias`]: Self::relink_alias
    /// [`introduced_definition`]: Self::introduced_definition
    /// [`define_alias_to_introduced`]: Self::define_alias_to_introduced
    pub fn relink_alias_to_introduced(
        &self,
        home: Rc<Environment>,
        name: Rc<str>,
        found: IntroducedDefinition,
        mention: &ScopeSet,
        mint: impl FnMut() -> Rc<str>,
    ) -> Rc<str> {
        match found {
            IntroducedDefinition::Renamed(renamed) => {
                self.relink_alias(home, renamed, None, mention, mint)
            }
            IntroducedDefinition::Scoped(scopes) => {
                self.relink_alias(home, name, Some(scopes), mention, mint)
            }
        }
    }

    /// The alias remembered here for `key`, while it is still an alias to
    /// that target and nothing here binds its spelling.
    fn remembered_relink(&self, key: &RelinkKey) -> Option<Rc<str>> {
        let alias = self.rare.get()?.relink_aliases.borrow().get(key).cloned()?;
        let (target_id, name, scopes, _) = key;
        let still = self.local_slot(&alias).is_none()
            && self.alias_target(&alias).is_some_and(|target| {
                target
                    .env
                    .as_ref()
                    .map_or(self.env_id(), |env| env.env_id())
                    == *target_id
                    && target.name == *name
                    && target.scopes == *scopes
            });
        // No placeholder can sit at a remembered alias's spelling (#836):
        // the mint skips a name one holds, and a link of an alias's spelling
        // makes none (`link_record`), so this is asserted, not tested, on a
        // path every relinking expansion takes.
        debug_assert!(
            !still || !self.holds_placeholder(&alias),
            "a remembered relink alias over a placeholder (#836)"
        );
        still.then_some(alias)
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
        let grew = {
            let mut aliases = self.alias_bindings.borrow_mut();
            let capacity = self.is_namespace().then(|| aliases.capacity());
            aliases.insert(alias, AliasTarget { env, name, scopes });
            capacity.is_some_and(|capacity| aliases.capacity() != capacity)
        };
        if grew {
            self.recharge();
        }
    }

    /// A frame's own binding of `name` (#834): what [`define`](Self::define)
    /// makes in a frame, with no heap, no kind test and no step, for the
    /// tree-walker's per-call binds — a call's parameters and its CPS
    /// temporaries — which are always a frame's. A namespace's binding goes
    /// through `define`, which takes the heap.
    #[inline]
    pub fn bind(&self, name: impl Into<Rc<str>>, value: TaggedValue) {
        debug_assert!(!self.is_namespace(), "`bind` is for a frame's own binding");
        self.define_here(name.into(), value);
    }

    /// [`define_with_scopes`](Self::define_with_scopes) for a frame's own
    /// binding, as [`bind`](Self::bind) is `define`'s.
    #[inline]
    pub fn bind_with_scopes(&self, name: impl Into<Rc<str>>, scopes: ScopeSet, value: TaggedValue) {
        debug_assert!(
            !self.is_namespace(),
            "`bind_with_scopes` is for a frame's own binding"
        );
        let name = name.into();
        if scopes.is_empty() {
            self.define_here(name, value);
        } else {
            self.insert_scoped_here(name, scopes, value, false);
        }
    }

    /// [`define_scoped_definition`](Self::define_scoped_definition) for a
    /// frame's own binding, as [`bind`](Self::bind) is `define`'s.
    #[inline]
    pub fn bind_scoped_definition(
        &self,
        name: impl Into<Rc<str>>,
        scopes: ScopeSet,
        value: TaggedValue,
    ) {
        debug_assert!(
            !self.is_namespace(),
            "`bind_scoped_definition` is for a frame's own binding"
        );
        let name = name.into();
        if scopes.is_empty() {
            self.define_here(name, value);
        } else {
            self.insert_scoped_here(name, scopes, value, true);
        }
    }

    /// Define a binding with a scope set (for scope-based hygiene)
    ///
    /// Use this when creating bindings from binding forms (lambda, let, etc.)
    /// where you want to track the lexical scope for hygiene.
    /// This is the primary API - accepts TaggedValue directly.
    ///
    /// A namespace borrows `heap` mutably on entry (#834); a frame, where
    /// nearly every scoped binding is made, takes no borrow.
    pub fn define_with_scopes(
        &self,
        heap: &RefCell<Heap>,
        name: impl Into<Rc<str>>,
        scopes: ScopeSet,
        value: TaggedValue,
    ) {
        use crate::macro_debug;

        let name = name.into();
        if macro_debug::is_enabled() {
            println!(
                "[ENV] Defining '{}' with scopes {} = {:?}",
                name, scopes, value
            );
        }

        self.insert_scoped(heap, name, scopes, value, false);
    }

    /// Store a binding under its scopes, or by name when it has none.
    ///
    /// The one place either kind of scoped binding is written, so the two
    /// public entry points differ only in `visible_by_name` and their docs.
    ///
    /// A scope set already present is *overwritten* rather than pushed
    /// beside. `set_with_scopes` finds a binding by exact scope-set match, so
    /// a second entry for the same set would be unreachable — and every entry
    /// is a GC root (`for_each_gc_edge`), so re-evaluating a top-level
    /// form that expands a macro would otherwise pin one dead value per
    /// evaluation for the life of the process.
    ///
    /// Also the one writer of the spellings' indexes (`RareTables::
    /// scoped_index`): a list that grows past its threshold
    /// ([`Environment::indexes`]) gets one here, and a longer one has each new
    /// position filed in it as it is pushed.
    fn insert_scoped(
        &self,
        heap: &RefCell<Heap>,
        name: Rc<str>,
        scopes: ScopeSet,
        value: TaggedValue,
        visible_by_name: bool,
    ) {
        if scopes.is_empty() {
            self.define(heap, name, value);
            return;
        }
        // The step first, before the trace, the latch or the table: a
        // refused borrow then panics with nothing written.
        match self.write_step(heap) {
            Some(mut step) => {
                self.insert_scoped_record(&mut step, name, scopes, value, visible_by_name)
            }
            None => self.insert_scoped_here(name, scopes, value, visible_by_name),
        }
    }

    /// [`insert_scoped`](Self::insert_scoped) in a namespace, under its
    /// mutable step (#834): every scoped entry a namespace holds is a
    /// record over a cell of its own, the tree-walker's introduced
    /// definitions, the splicing `let-syntax` keywords the desugarer files
    /// at top level and a visible definition alike, so that one invariant
    /// covers the table and an overwrite never changes what an entry is.
    ///
    /// A vacant spelling, or a scope set the spelling has no entry at, gets
    /// a fresh record and cell; an entry at exactly `scopes` is stored into,
    /// and keeps its record.
    fn insert_scoped_record(
        &self,
        heap: &mut Heap,
        name: Rc<str>,
        scopes: ScopeSet,
        value: TaggedValue,
        visible_by_name: bool,
    ) {
        crate::scope_trace::bind(&name, &scopes, visible_by_name);
        if visible_by_name {
            // Only the tree-walker makes a visible scoped definition, and it
            // links nothing, so no read-through record, strong or weak, is
            // under one (#835, #836).
            debug_assert!(
                self.kind != Kind::HostNamespace
                    || (self
                        .rare
                        .get()
                        .and_then(|rare| rare.read_through.get())
                        .is_none_or(|table| !table.borrow().records.contains_key(&*name))
                        && !self.holds_placeholder(&name)),
                "a visible scoped definition over a read-through record (#835, #836)"
            );
            self.has_visible_scoped.set(true);
        }
        let env_id = self.env_id;
        // Both allocations are under one `&mut Heap`, which a collection
        // needs, and allocation never collects (`docs/GC_DESIGN.md` §6): the
        // cell, held only in a local until its record holds it, cannot be
        // swept between. Allocating with the scoped table borrowed is
        // sound: the heap is another `RefCell`.
        let fresh = |heap: &mut Heap| {
            let cell = BindingCell::new_own(heap, value, env_id);
            Record::new(heap, cell).word()
        };
        let (grew, made) = {
            let mut table = self.scoped_bindings.borrow_mut();
            let capacity = table.capacity();
            let (spelling_grew, made) = match table.entry(name) {
                Entry::Vacant(vacant) => {
                    let record = fresh(heap);
                    vacant.insert(smallvec::smallvec![ScopedBinding {
                        scopes,
                        tagged_value: record,
                        visible_by_name,
                    }]);
                    (false, true)
                }
                Entry::Occupied(mut occupied) => {
                    match self.scoped_position(occupied.key(), occupied.get(), &scopes) {
                        Some(position) => {
                            let existing = &mut occupied.get_mut()[position];
                            existing.visible_by_name = visible_by_name;
                            let cell = Record::from_word(existing.tagged_value).cell(heap);
                            cell.store(heap, value, Site::EnvDefine);
                            (false, false)
                        }
                        None => {
                            let before = list_bytes(occupied.get());
                            let record = fresh(heap);
                            occupied.get_mut().push(ScopedBinding {
                                scopes,
                                tagged_value: record,
                                visible_by_name,
                            });
                            let (spelling, bindings) = (occupied.key(), occupied.get());
                            let list_grew = self.charge_entries(before, list_bytes(bindings));
                            let index_grew = self.indexes(bindings.len())
                                && self.index_scoped(spelling, bindings);
                            (list_grew || index_grew, true)
                        }
                    }
                }
            };
            (table.capacity() != capacity || spelling_grew, made)
        };
        if made {
            self.count(1, 1);
        }
        if grew {
            self.recharge();
        }
    }

    /// [`insert_scoped`](Self::insert_scoped)'s body for a frame, for
    /// non-empty `scopes`: no heap and no step. A namespace's goes through
    /// [`insert_scoped_record`](Self::insert_scoped_record).
    #[inline]
    fn insert_scoped_here(
        &self,
        name: Rc<str>,
        scopes: ScopeSet,
        value: TaggedValue,
        visible_by_name: bool,
    ) {
        crate::scope_trace::bind(&name, &scopes, visible_by_name);
        if visible_by_name {
            self.has_visible_scoped.set(true);
        }
        let grew = {
            let mut table = self.scoped_bindings.borrow_mut();
            // Measured only for a namespace, which charges for the table and
            // for each spelling's list: the tree-walker binds each call's
            // parameters here, in a frame, and a frame's first scoped binding
            // always allocates the table.
            let measured = self.is_namespace();
            let capacity = measured.then(|| table.capacity());
            let binding = ScopedBinding {
                scopes,
                tagged_value: value,
                visible_by_name,
            };
            // By entry, so that the frame binding a call's parameters, which
            // finds each spelling vacant, hashes each name once. A vacant
            // spelling's list is inline, in the table's own buffer, which a
            // namespace counts at its capacity already, so it adds nothing
            // to the spellings' sum until it spills.
            let spelling_grew = match table.entry(name) {
                Entry::Vacant(vacant) => {
                    vacant.insert(smallvec::smallvec![binding]);
                    false
                }
                Entry::Occupied(mut occupied) => {
                    match self.scoped_position(occupied.key(), occupied.get(), &binding.scopes) {
                        Some(position) => {
                            let existing = &mut occupied.get_mut()[position];
                            existing.tagged_value = binding.tagged_value;
                            existing.visible_by_name = binding.visible_by_name;
                            false
                        }
                        None => {
                            let before = if measured {
                                list_bytes(occupied.get())
                            } else {
                                0
                            };
                            occupied.get_mut().push(binding);
                            let (spelling, bindings) = (occupied.key(), occupied.get());
                            let list_grew =
                                measured && self.charge_entries(before, list_bytes(bindings));
                            let index_grew = self.indexes(bindings.len())
                                && self.index_scoped(spelling, bindings);
                            list_grew || index_grew
                        }
                    }
                }
            };
            capacity.is_some_and(|capacity| table.capacity() != capacity) || spelling_grew
        };
        if grew {
            self.recharge();
        }
    }

    /// File the binding just pushed onto `bindings`, the scoped bindings of
    /// `name`, in the spelling's index — or build the index, for a list that
    /// has just grown past its threshold. Reports whether a namespace's table
    /// of indexes reallocated, or the spelling's index grew, each of which
    /// it charges for.
    fn index_scoped(&self, name: &Rc<str>, bindings: &[ScopedBinding]) -> bool {
        let rare = self.rare.get_or_init(Default::default);
        let mut indexes = rare.scoped_index.borrow_mut();
        let measured = self.is_namespace();
        let capacity = measured.then(|| indexes.capacity());
        let (before, after) = match indexes.get_mut(&**name) {
            Some(index) => {
                let before = index.buffer_bytes();
                let newest = bindings.last().expect("a binding was just pushed");
                index.push(&newest.scopes);
                debug_assert_eq!(index.len(), bindings.len());
                (before, index.buffer_bytes())
            }
            None => {
                let sets = bindings.iter().map(|binding| &binding.scopes);
                let index = ScopeIndex::of(sets);
                let after = index.buffer_bytes();
                indexes.insert(Rc::clone(name), index);
                (0, after)
            }
        };
        let table_grew = capacity.is_some_and(|capacity| indexes.capacity() != capacity);
        let index_grew = measured && self.charge_entries(before, after);
        table_grew || index_grew
    }

    /// Define a binding that a name-only lookup can also reach.
    ///
    /// Source-written parameters and internal definitions need this view:
    /// they acquire body scopes while their source references can still be
    /// name-only. All introduced definitions use `define_with_scopes`
    /// instead (#269, #427); their generated getters and setters reach the
    /// scoped binding directly, including through scoped library aliases.
    ///
    /// The name-only view's reach is plain access — [`get`], [`set`], and
    /// the relinker resolving through them — not scoped resolution's
    /// fallback. A scoped reference whose resolution rejected this binding
    /// stays refused (`get_scoped_fallback` / `set_scoped_fallback`).
    ///
    /// [`get`]: Self::get
    /// [`set`]: Self::set
    ///
    /// It is still **one** binding. An earlier version stored the value under
    /// the bare name as well, and a `set!` through the scoped path then left
    /// the two disagreeing — the freeze `alias_bindings` names as the reason
    /// its own indirection exists.
    ///
    /// Borrows `heap` as [`define_with_scopes`](Self::define_with_scopes)
    /// does.
    pub fn define_scoped_definition(
        &self,
        heap: &RefCell<Heap>,
        name: impl Into<Rc<str>>,
        scopes: ScopeSet,
        value: TaggedValue,
    ) {
        self.insert_scoped(heap, name.into(), scopes, value, true);
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
    /// The binding it writes, in a namespace, is written at that
    /// namespace's step, under a mutable borrow of `heap` (#834); the
    /// resolution reads scope sets only, and takes none.
    ///
    /// [`get_with_scopes`]: Self::get_with_scopes
    pub fn set_with_scopes(
        &self,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
        value: TaggedValue,
    ) -> Result<(), ScopedSetError> {
        if scopes.is_empty() {
            // Empty scopes - use simple lookup
            return self
                .set(heap, name, value)
                .map_err(ScopedSetError::Undefined);
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
                    env.visit_scoped_candidates(
                        name,
                        bindings,
                        ref_scopes,
                        |index, binding, candidate| {
                            if candidate {
                                out.push((binding.scopes.clone(), (env, index)));
                            }
                        },
                    );
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
                let mut step = env.write_step(heap);
                let mut scoped = env.scoped_bindings.borrow_mut();
                match scoped.get_mut(name).and_then(|bs| bs.get_mut(position)) {
                    Some(binding) => {
                        write_entry(
                            step.as_deref_mut(),
                            binding,
                            value,
                            Site::EnvSetScoped,
                            false,
                        );
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
                let landed = self.set_scoped_fallback(heap, name, scopes, value);
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
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
        value: TaggedValue,
    ) -> Result<(), String> {
        // A loop up the chain, as `set` walks it (#617).
        let mut env = self;
        loop {
            if let Some(slot) = env.local_slot(name) {
                env.set_slot_value(heap, slot, value);
                return Ok(());
            }
            if env.has_aliases.get()
                && let Some(target) = env.alias_target(name)
            {
                return target.set(env, heap, value);
            }
            if env.has_visible_scoped.get()
                && let Some(i) = env.visible_scoped_index(name)
            {
                let table = env.scoped_bindings.borrow();
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
                        let mut step = env.write_step(heap);
                        let mut table = env.scoped_bindings.borrow_mut();
                        if let Some(binding) = table.get_mut(name).and_then(|bs| bs.get_mut(i)) {
                            write_entry(
                                step.as_deref_mut(),
                                binding,
                                value,
                                Site::EnvSetScoped,
                                false,
                            );
                            return Ok(());
                        }
                        return Err(name.to_string());
                    }
                }
                // Rejected for these scopes: fall through to the parent rather
                // than clobber it by spelling.
            }
            match &env.parent {
                Some(parent) => env = parent,
                None => return Err(name.to_string()),
            }
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
    ///
    /// Each namespace's hit is read at its step, under a shared borrow of
    /// `heap` (#834).
    pub fn get_with_scopes(
        &self,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<Option<TaggedValue>, Box<AmbiguousReference>> {
        self.resolve_with_scopes(heap, name, scopes)
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
    /// Borrows `heap` as [`get_with_scopes`] does.
    ///
    /// [`get_with_scopes`]: Self::get_with_scopes
    pub fn resolve_with_scopes(
        &self,
        heap: &RefCell<Heap>,
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
            let result = self.get(heap, name);
            if debug {
                match result {
                    Some(tv) => println!("[ENV]   Result: {tv:?}"),
                    None => println!("[ENV]   Result: NOT FOUND"),
                }
            }
            return Ok((result, false));
        }

        // Every binding of this name in this environment and its parents,
        // in the order `resolve_scoped` wants them.
        let mut candidates: Vec<(ScopeSet, TaggedValue)> = Vec::new();
        self.collect_scoped_candidates(heap, name, scopes, &mut candidates, debug);

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
            Ok(None) => self.get_scoped_fallback(heap, name, scopes),
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
                Some(tv) => println!("[ENV]   Result (scoped or fallback): {tv:?}"),
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
    fn get_scoped_fallback(
        &self,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Option<TaggedValue> {
        // A loop up the chain, as `get` walks it (#617).
        let mut env = self;
        loop {
            if let Some(tv) = env.local_value(heap, name) {
                return Some(tv);
            }
            if env.has_aliases.get()
                && let Some(target) = env.alias_target(name)
            {
                return target.get(env, heap);
            }
            if env.has_visible_scoped.get()
                && let Some(i) = env.visible_scoped_index(name)
            {
                let step = env.read_step(heap);
                let table = env.scoped_bindings.borrow();
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
                        return Some(resolve(step.as_deref(), binding.tagged_value));
                    }
                }
                // Rejected for these scopes: fall through to the parent rather
                // than resurrect it by name.
            }
            env = env.parent.as_deref()?;
        }
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
    /// Borrows `heap` as [`get_with_scopes`] does, since its walk is that
    /// read's.
    ///
    /// [`get_with_scopes`]: Self::get_with_scopes
    pub fn scoped_binding_of(
        &self,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<Option<ScopeSet>, Box<AmbiguousReference>> {
        if scopes.is_empty() {
            return Ok(None);
        }
        let mut candidates: Vec<(ScopeSet, TaggedValue)> = Vec::new();
        self.collect_scoped_candidates(heap, name, scopes, &mut candidates, false);
        let chosen = crate::scope_resolve::resolve_index(name, scopes, &candidates)?;
        Ok(chosen.map(|index| candidates.swap_remove(index).0))
    }

    /// Locate a scoped macro's binding for a transformer that escapes its
    /// local syntax region (#424). Unlike a runtime lexical variable, this
    /// binding lives in the expansion environment and can be shared by alias.
    ///
    /// `heap` is the one its values live on, which says what is a macro, and
    /// which each namespace's step borrows shared (#834).
    pub fn scoped_macro_binding(
        self: &Rc<Self>,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Option<(Rc<Self>, ScopeSet)> {
        let identity = self.scoped_binding_of(heap, name, scopes).ok()??;
        let mut home = self.clone();
        loop {
            if let Some(value) = home.scoped_definition_value(heap, name, &identity) {
                let is_macro = heap.borrow().get_macro(value).is_some();
                return is_macro.then_some((home, identity));
            }
            home = home.parent.clone()?;
        }
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
    /// definition is, when it is the one resolution chose. An alias to a
    /// renamed introduced global can also name the chosen binding, recognised
    /// by its recorded identity ([`define_introduced_global`]). This check
    /// supports such aliases, though the compiler no longer installs them
    /// under an introduced definition's bare spelling.
    ///
    /// `Err` as [`scoped_binding_of`] gives it: an ambiguous reference
    /// denotes no binding.
    ///
    /// Borrows `heap` as [`scoped_binding_of`] does; its own walk reads no
    /// value.
    ///
    /// [`get`]: Self::get
    /// [`get_scoped_fallback`]: Self::get_scoped_fallback
    /// [`define_introduced_global`]: Self::define_introduced_global
    /// [`scoped_binding_of`]: Self::scoped_binding_of
    pub fn name_reaches_binding_of(
        &self,
        heap: &RefCell<Heap>,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<bool, Box<AmbiguousReference>> {
        if scopes.is_empty() {
            // The reference *is* the name-only view.
            return Ok(true);
        }
        let chosen = self.scoped_binding_of(heap, name, scopes)?;
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
        heap: &RefCell<Heap>,
        name: &str,
        ref_scopes: &ScopeSet,
        candidates: &mut Vec<(ScopeSet, TaggedValue)>,
        debug: bool,
    ) {
        let mut env = self;
        loop {
            let scoped = env.scoped_bindings.borrow();
            if let Some(bindings) = scoped.get(name) {
                // Each candidate's value is read at the namespace's step,
                // through its record (#834).
                let step = env.read_step(heap);
                // A spelling's index shows only the bindings filed under the
                // reference's scopes, so with one the debug trace lists those.
                env.visit_scoped_candidates(
                    name,
                    bindings,
                    ref_scopes,
                    |_, binding, is_candidate| {
                        if debug {
                            println!(
                                "[ENV]   Candidate {} ⊆ {} : {}",
                                binding.scopes,
                                ref_scopes,
                                if is_candidate { "YES" } else { "NO" }
                            );
                        }
                        if is_candidate {
                            candidates.push((
                                binding.scopes.clone(),
                                resolve(step.as_deref(), binding.tagged_value),
                            ));
                        }
                    },
                );
            }
            drop(scoped);
            match &env.parent {
                Some(parent) => env = parent,
                None => {
                    return env
                        .collect_introduced_globals(heap, name, ref_scopes, candidates, debug);
                }
            }
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
        heap: &RefCell<Heap>,
        name: &str,
        ref_scopes: &ScopeSet,
        candidates: &mut Vec<(ScopeSet, TaggedValue)>,
        debug: bool,
    ) {
        // The tree-walker's case, on every scoped read: nothing recorded, so
        // return before `for_each_introduced_candidate` hashes the name.
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
        self.for_each_introduced_candidate(name, ref_scopes, |scopes, renamed_to| {
            found.push((scopes.clone(), renamed_to.clone()));
        });
        for (scopes, renamed_to) in found {
            let value = self.local_value(heap, &renamed_to);
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
        let mut env = self;
        loop {
            if env.bindings.borrow().slot_of(name).is_some() {
                return true;
            }
            match env.parent.as_deref() {
                Some(parent) => env = parent,
                None => return false,
            }
        }
    }

    /// Get all variable names defined in this environment and parent environments
    ///
    /// Borrows `heap` as [`local_names`](Self::local_names) does, at each
    /// namespace on the chain.
    pub fn get_all_names(&self, heap: &RefCell<Heap>) -> Vec<String> {
        let mut names = Vec::new();
        let mut env = Some(self);
        while let Some(current) = env {
            // Through `local_names`, so an `import_alias` is left out here too.
            names.extend(current.local_names(heap));
            // Include names from scoped bindings
            names.extend(
                current
                    .scoped_bindings
                    .borrow()
                    .keys()
                    .map(|name| name.to_string()),
            );
            env = current.parent.as_deref();
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
    /// Borrows `heap` shared at a namespace's step, to tell an alias from a
    /// name a program bound, when it holds any alias (#834).
    ///
    /// [`import_alias`]: Self::import_alias
    pub fn local_names(&self, heap: &RefCell<Heap>) -> Vec<String> {
        self.visible_slots(heap)
            .into_iter()
            .map(|(_, name)| name)
            .collect()
    }

    /// Each name a program bound here with its slot, in slot order. With the
    /// slot, because leaving a name out makes position and slot two things.
    fn visible_slots(&self, heap: &RefCell<Heap>) -> Vec<(u32, String)> {
        // A table, not a scan per name: an importer that expands library
        // macros holds an alias for each location their templates mention.
        // Each alias's spelling, with the key of the cell it was minted for.
        let hidden: FxHashMap<Rc<str>, u64> =
            self.rare.get().map_or_else(FxHashMap::default, |rare| {
                rare.import_aliases
                    .borrow()
                    .iter()
                    .flat_map(|(key, aliases)| {
                        aliases.iter().map(|(_, alias)| (Rc::clone(alias), *key))
                    })
                    .collect()
            });
        // Whether an alias still names the cell it was minted for is read at
        // the step, through its record (#834). Only a namespace mints one.
        let step = if hidden.is_empty() {
            None
        } else {
            self.read_step(heap)
        };
        let bindings = self.bindings.borrow();
        bindings
            .names()
            .enumerate()
            .filter(|(slot, name)| {
                // Hidden only while it still names the cell it was minted
                // for. `forget_alias` drops the entry of an alias a program
                // defined over or imported over, so a mismatch cannot
                // happen; compared all the same, as the defence the reuse
                // tests exercise.
                match (hidden.get(&***name), &step) {
                    (Some(&key), Some(step)) => {
                        Record::from_word(bindings.read_slot(*slot as u32))
                            .cell(step)
                            .key()
                            != key
                    }
                    _ => true,
                }
            })
            .map(|(slot, name)| (slot as u32, name.to_string()))
            .collect()
    }

    /// Get all bindings in this environment only (not including parent)
    ///
    /// Returns a vector of (name, TaggedValue) pairs for all bindings defined locally.
    /// This is useful for library imports where we need to iterate over all exports.
    ///
    /// Each value is read as [`slot_value`](Self::slot_value) reads it, at
    /// a namespace's step (#834).
    pub fn bindings(&self, heap: &RefCell<Heap>) -> Vec<(String, TaggedValue)> {
        // Names first and values after: an imported binding's value is read
        // from its cell, and that read must not happen under this borrow.
        self.visible_slots(heap)
            .into_iter()
            .map(|(slot, name)| (name, self.slot_value(heap, slot)))
            .collect()
    }

    /// Each macro-introduced top-level definition of `name` this namespace
    /// holds a record for (#834): the VM's (`Introduced` → the renamed
    /// global's slot, once its `Define` ran) and the tree-walker's (the
    /// scoped list's entries that are not name-visible and hold no macro,
    /// which leaves the splicing keywords out). Empty for a frame. Reads at
    /// the namespace's step. Raw, for 5c and the tests.
    pub fn introduced_records(&self, heap: &RefCell<Heap>, name: &str) -> Vec<IntroducedRecord> {
        let Some(step) = self.read_step(heap) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let entry = |identity: &ScopeSet, record: Record, value: TaggedValue| IntroducedRecord {
            identity: identity.clone(),
            scope: identity.introducing_scope(),
            value,
            record: record.key(),
        };
        if let Some(rare) = self.rare.get()
            && let Some(introduced) = rare.introduced_global_names.borrow().get(name)
        {
            for (identity, renamed) in &introduced.identities {
                if let Some(record) = self.local_record(renamed) {
                    out.push(entry(identity, record, record.value(&step)));
                }
            }
        }
        if let Some(list) = self.scoped_bindings.borrow().get(name) {
            for binding in list.iter().filter(|binding| !binding.visible_by_name) {
                let record = Record::from_word(binding.tagged_value);
                let value = record.value(&step);
                if step.get_macro(value).is_none() {
                    out.push(entry(&binding.scopes, record, value));
                }
            }
        }
        out
    }

    /// The records and own cells this namespace holds, counted from its
    /// tables rather than kept as running sums: for the tests, to check the
    /// sums (`NamespaceCharge`). A frame holds none.
    #[cfg(test)]
    pub(crate) fn recount(&self, heap: &Heap) -> (usize, usize) {
        if !self.is_namespace() {
            return (0, 0);
        }
        let mut records = 0;
        let mut cells = 0;
        let mut tally = |word: TaggedValue| {
            records += 1;
            if Record::from_word(word)
                .cell(heap)
                .owned_by(heap, self.env_id)
            {
                cells += 1;
            }
        };
        for (_, word) in &self.bindings.borrow().slots {
            tally(*word);
        }
        for list in self.scoped_bindings.borrow().values() {
            for binding in list {
                tally(binding.tagged_value);
            }
        }
        // A host namespace's read-through records (#835): records, each
        // over the one free cell, which no namespace owns.
        if let Some(table) = self.rare.get().and_then(|rare| rare.read_through.get()) {
            records += table.borrow().records.len();
        }
        (records, cells)
    }

    /// The running sums of records and own cells this namespace's charge
    /// keeps, for the tests beside [`recount`](Self::recount).
    #[cfg(test)]
    pub(crate) fn counted(&self) -> (usize, usize) {
        self.rare
            .get()
            .and_then(|rare| rare.namespace.as_ref())
            .map_or((0, 0), |charge| (charge.records.get(), charge.cells.get()))
    }

    /// What a scoped entry of this environment's stands for: in a namespace
    /// the value its record's cell holds, read at the step; in a frame, its
    /// word. For the tests that walk the scoped table.
    #[cfg(test)]
    pub(crate) fn scoped_entry_value(
        &self,
        heap: &RefCell<Heap>,
        entry: &ScopedBinding,
    ) -> TaggedValue {
        resolve(self.read_step(heap).as_deref(), entry.tagged_value)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::new_shared_heap;

    #[test]
    fn test_define_and_get() {
        let heap = new_shared_heap();
        let env = Environment::with_heap(&heap);
        env.define(&heap, "x".to_string(), TaggedValue::fixnum(42));
        assert_eq!(env.get(&heap, "x"), Some(TaggedValue::fixnum(42)));
    }

    #[test]
    fn test_slot_stability_across_redefine() {
        let heap = new_shared_heap();
        let env = Environment::with_heap(&heap);
        env.define(&heap, "x".to_string(), TaggedValue::fixnum(1));
        let slot = env.local_slot("x").unwrap();

        // Redefinition overwrites the slot in place — same index, new value.
        env.define(&heap, "x".to_string(), TaggedValue::fixnum(2));
        assert_eq!(env.local_slot("x"), Some(slot));
        assert_eq!(env.slot_value(&heap, slot), TaggedValue::fixnum(2));

        // set! through the slot API is visible to name lookup and vice versa.
        env.set_slot_value(&heap, slot, TaggedValue::fixnum(3));
        assert_eq!(env.get(&heap, "x"), Some(TaggedValue::fixnum(3)));
        env.set(&heap, "x", TaggedValue::fixnum(4)).unwrap();
        assert_eq!(env.slot_value(&heap, slot), TaggedValue::fixnum(4));

        // New defines append fresh slots without disturbing existing ones.
        env.define(&heap, "y".to_string(), TaggedValue::fixnum(10));
        assert_ne!(env.local_slot("y"), Some(slot));
        assert_eq!(env.slot_value(&heap, slot), TaggedValue::fixnum(4));
    }

    #[test]
    fn test_local_slot_ignores_parent() {
        let heap = new_shared_heap();
        let parent = Rc::new(Environment::with_heap(&heap));
        parent.define(&heap, "x".to_string(), TaggedValue::fixnum(42));
        let child = Environment::child_frame(parent);
        // x resolves via the parent chain but has no local slot in the child.
        assert_eq!(child.get(&heap, "x"), Some(TaggedValue::fixnum(42)));
        assert_eq!(child.local_slot("x"), None);
    }

    #[test]
    fn linear_and_indexed_frames_agree_on_slots() {
        let heap = new_shared_heap();
        // `Bindings` answers by linear scan until a frame passes
        // `LINEAR_MAX`, then by hash index. Only the global environment
        // crosses that line at runtime, and it crosses it during startup,
        // where a name the index misses looks like a missing primitive
        // rather than an indexing bug — so the crossover is pinned here.
        let env = Environment::with_heap(&heap);
        let names: Vec<String> = (0..LINEAR_MAX + 2).map(|i| format!("v{i}")).collect();
        for (i, name) in names.iter().enumerate() {
            env.define(&heap, name.as_str(), TaggedValue::fixnum(i as i64));
        }
        // Every name keeps the slot its insertion order gave it, on both
        // sides of the crossover, and reads through the index agree with the
        // values.
        for (i, name) in names.iter().enumerate() {
            assert_eq!(env.local_slot(name), Some(i as u32), "slot of {name}");
            assert_eq!(env.get(&heap, name), Some(TaggedValue::fixnum(i as i64)));
        }
        // Redefining after the index exists must overwrite in place, not
        // append a second slot the index then shadows.
        env.define(&heap, "v0", TaggedValue::fixnum(99));
        assert_eq!(env.local_slot("v0"), Some(0));
        assert_eq!(env.get(&heap, "v0"), Some(TaggedValue::fixnum(99)));
        assert_eq!(env.bindings(&heap).len(), names.len());
    }

    #[test]
    fn test_env_ids_unique_and_nonzero() {
        let heap = new_shared_heap();
        let a = Rc::new(Environment::with_heap(&heap));
        let b = Environment::with_heap(&heap);
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
        let heap = new_shared_heap();
        let parent = Rc::new(Environment::with_heap(&heap));
        parent.define(&heap, "x".to_string(), TaggedValue::fixnum(42));

        let child = Environment::child_frame(parent);
        assert_eq!(child.get(&heap, "x"), Some(TaggedValue::fixnum(42)));
    }

    #[test]
    fn test_bindings() {
        let heap = new_shared_heap();
        let env = Environment::with_heap(&heap);
        env.define(&heap, "x".to_string(), TaggedValue::fixnum(42));
        env.define(&heap, "y".to_string(), TaggedValue::TRUE);
        env.define(&heap, "z".to_string(), TaggedValue::fixnum(100));

        let bindings = env.bindings(&heap);
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
        let heap = new_shared_heap();
        let parent = Rc::new(Environment::with_heap(&heap));
        parent.define(&heap, "x".to_string(), TaggedValue::fixnum(42));
        parent.define(&heap, "y".to_string(), TaggedValue::fixnum(100));

        let child = Environment::child_frame(parent);
        child.define(&heap, "z".to_string(), TaggedValue::TRUE);

        // bindings() should only return local bindings
        let bindings = child.bindings(&heap);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].0, "z");
        assert_eq!(bindings[0].1, TaggedValue::TRUE);
    }

    #[test]
    fn test_scoped_bindings_basic() {
        let heap = new_shared_heap();
        use crate::scope::ScopeId;

        let env = Environment::with_heap(&heap);
        let s1 = ScopeId(1);
        let scopes = ScopeSet::singleton(s1);

        env.define_with_scopes(
            &heap,
            "x".to_string(),
            scopes.clone(),
            TaggedValue::fixnum(42),
        );

        // Lookup with matching scopes should find the binding
        let result = env.get_with_scopes(&heap, "x", &scopes).unwrap();
        assert_eq!(result, Some(TaggedValue::fixnum(42)));

        // Lookup with superset scopes should also find it (binding.scopes ⊆ ref.scopes)
        let s2 = ScopeId(2);
        let larger_scopes = ScopeSet::from_iter([s1, s2]);
        let result = env.get_with_scopes(&heap, "x", &larger_scopes).unwrap();
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

        let heap = crate::heap::new_shared_heap();
        let env = Environment::with_heap(&heap);
        let s1 = ScopeId(1);
        let s2 = ScopeId(2);
        let s3 = ScopeId(3);

        // Outer x binding: introduced at S1 (Symbol requires heap allocation)
        let outer_x_scopes = ScopeSet::singleton(s1);
        let outer_tv = heap.borrow_mut().intern_symbol("outer");
        env.define_with_scopes(&heap, "x".to_string(), outer_x_scopes.clone(), outer_tv);

        // Inner x binding: inside S1, S2, S3
        let inner_x_scopes = ScopeSet::from_iter([s1, s2, s3]);
        let inner_tv = heap.borrow_mut().intern_symbol("inner");
        env.define_with_scopes(&heap, "x".to_string(), inner_x_scopes.clone(), inner_tv);

        // Free var x in macro m: inside S1, S2 (captured when macro was defined)
        let macro_x_scopes = ScopeSet::from_iter([s1, s2]);

        // Lookup should find outer x, NOT inner x
        let result_tv = env
            .get_with_scopes(&heap, "x", &macro_x_scopes)
            .unwrap()
            .unwrap();
        let name = heap
            .borrow()
            .get_symbol_name(result_tv)
            .map(|s| s.to_string());
        assert_eq!(name.as_deref(), Some("outer"));
    }

    #[test]
    fn test_scoped_bindings_most_specific() {
        let heap = new_shared_heap();
        use crate::scope::ScopeId;

        let env = Environment::with_heap(&heap);
        let s1 = ScopeId(1);
        let s2 = ScopeId(2);

        // Less specific binding
        env.define_with_scopes(
            &heap,
            "x".to_string(),
            ScopeSet::singleton(s1),
            TaggedValue::fixnum(1),
        );

        // More specific binding
        env.define_with_scopes(
            &heap,
            "x".to_string(),
            ScopeSet::from_iter([s1, s2]),
            TaggedValue::fixnum(2),
        );

        // Lookup with {S1, S2} should find the more specific binding
        let lookup_scopes = ScopeSet::from_iter([s1, s2]);
        assert_eq!(
            env.get_with_scopes(&heap, "x", &lookup_scopes).unwrap(),
            Some(TaggedValue::fixnum(2))
        );

        // Lookup with just {S1} should find the less specific binding
        let lookup_scopes_s1 = ScopeSet::singleton(s1);
        assert_eq!(
            env.get_with_scopes(&heap, "x", &lookup_scopes_s1).unwrap(),
            Some(TaggedValue::fixnum(1))
        );
    }

    #[test]
    fn test_scoped_bindings_fallback_to_unmarked() {
        use crate::scope::ScopeId;

        let heap = crate::heap::new_shared_heap();
        let env = Environment::with_heap(&heap);

        // Define an unmarked binding (Symbol requires heap allocation)
        let tv = heap.borrow_mut().intern_symbol("primitive-cons");
        env.define(&heap, "cons".to_string(), tv);

        // Lookup with scopes should fall back to unmarked binding
        let s1 = ScopeId(1);
        let scopes = ScopeSet::singleton(s1);
        let result_tv = env
            .get_with_scopes(&heap, "cons", &scopes)
            .unwrap()
            .unwrap();
        let name = heap
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
        let heap = new_shared_heap();
        use crate::scope::ScopeId;

        let env = Environment::with_heap(&heap);
        let (s1, s2) = (ScopeId(1), ScopeId(2));

        env.define_with_scopes(
            &heap,
            "x".to_string(),
            ScopeSet::singleton(s1),
            TaggedValue::fixnum(1),
        );
        env.define_with_scopes(
            &heap,
            "x".to_string(),
            ScopeSet::singleton(s2),
            TaggedValue::fixnum(2),
        );

        let both = ScopeSet::from_iter([s1, s2]);
        let err = env
            .get_with_scopes(&heap, "x", &both)
            .expect_err("neither binding contains the other");
        assert_eq!(err.name, "x");

        // Each is still reachable on its own: ambiguity is a property of the
        // reference, not of the bindings.
        assert_eq!(
            env.get_with_scopes(&heap, "x", &ScopeSet::singleton(s1))
                .unwrap(),
            Some(TaggedValue::fixnum(1))
        );
        assert_eq!(
            env.get_with_scopes(&heap, "x", &ScopeSet::singleton(s2))
                .unwrap(),
            Some(TaggedValue::fixnum(2))
        );
    }
}

#[cfg(test)]
mod scoped_write_tests {
    use super::*;
    use crate::heap::new_shared_heap;

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
        let heap = new_shared_heap();
        let env = Environment::with_heap(&heap);
        env.define_with_scopes(&heap, "x", scopes(&[1]), TaggedValue::fixnum(1));
        env.define_with_scopes(&heap, "x", scopes(&[2]), TaggedValue::fixnum(2));
        match env.set_with_scopes(&heap, "x", &scopes(&[1, 2]), TaggedValue::fixnum(9)) {
            Err(ScopedSetError::Ambiguous(e)) => assert_eq!(e.name, "x"),
            other => panic!("expected an ambiguity, got {other:?}"),
        }
        // And nothing moved.
        assert_eq!(
            env.get_with_scopes(&heap, "x", &scopes(&[1])).unwrap(),
            Some(TaggedValue::fixnum(1))
        );
        assert_eq!(
            env.get_with_scopes(&heap, "x", &scopes(&[2])).unwrap(),
            Some(TaggedValue::fixnum(2))
        );
    }

    /// The other arm: a name nothing binds is undefined, not ambiguous.
    #[test]
    fn a_write_to_nothing_reports_undefined() {
        let heap = new_shared_heap();
        let env = Environment::with_heap(&heap);
        match env.set_with_scopes(&heap, "absent", &scopes(&[1]), TaggedValue::fixnum(9)) {
            Err(ScopedSetError::Undefined(name)) => assert_eq!(name, "absent"),
            other => panic!("expected undefined, got {other:?}"),
        }
    }

    /// Both arms render a sentence, so a caller that formats rather than
    /// matching gets a diagnostic either way.
    #[test]
    fn both_arms_render_a_message() {
        let heap = new_shared_heap();
        let env = Environment::with_heap(&heap);
        let undefined = env
            .set_with_scopes(&heap, "absent", &scopes(&[1]), TaggedValue::fixnum(9))
            .unwrap_err()
            .to_string();
        assert!(undefined.contains("undefined variable"), "{undefined}");
        assert!(undefined.contains("absent"), "{undefined}");

        env.define_with_scopes(&heap, "x", scopes(&[1]), TaggedValue::fixnum(1));
        env.define_with_scopes(&heap, "x", scopes(&[2]), TaggedValue::fixnum(2));
        let ambiguous = env
            .set_with_scopes(&heap, "x", &scopes(&[1, 2]), TaggedValue::fixnum(9))
            .unwrap_err()
            .to_string();
        assert!(ambiguous.contains("ambiguous reference"), "{ambiguous}");
    }
}

#[cfg(test)]
mod introduced_global_tests {
    use super::*;
    use crate::heap::new_shared_heap;

    fn scopes(ids: &[usize]) -> ScopeSet {
        let mut set = ScopeSet::new();
        for id in ids {
            set.add_scope(crate::scope::ScopeId(*id));
        }
        set
    }

    /// Every identity recorded for `name`: the candidates for a reference
    /// carrying every scope these tests use.
    fn recorded(env: &Environment, name: &str) -> Vec<(ScopeSet, Rc<str>)> {
        let every_scope = scopes(&(0..256).collect::<Vec<_>>());
        let mut out = Vec::new();
        env.for_each_introduced_candidate(name, &every_scope, |scopes, renamed| {
            out.push((scopes.clone(), renamed.clone()))
        });
        out.sort_by_key(|(scopes, _)| scopes.len());
        out
    }

    #[test]
    fn a_name_with_no_introduced_definition_yields_nothing() {
        let heap = new_shared_heap();
        let env = Environment::with_heap(&heap);
        env.define_introduced_global("x".into(), scopes(&[1]), "x #1".into());
        assert!(recorded(&env, "y").is_empty());
    }

    /// The property the fix rests on: two expansions of one macro introduce
    /// the same spelling and must stay two bindings. Collapsing them is what
    /// the removed bare-name alias did; scoped references need the identities.
    #[test]
    fn two_expansions_of_one_spelling_are_two_entries() {
        let heap = new_shared_heap();
        let env = Environment::with_heap(&heap);
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
        let heap = new_shared_heap();
        let env = Environment::with_heap(&heap);
        env.define_introduced_global("count".into(), scopes(&[1, 2]), "stale".into());
        env.define_introduced_global("count".into(), scopes(&[1, 2]), "fresh".into());
        assert_eq!(
            recorded(&env, "count"),
            vec![(scopes(&[1, 2]), Rc::from("fresh"))]
        );
    }

    /// Past `INDEX_ABOVE` the identities are found through their index
    /// (#613): each by its scopes, a reference inside one expansion shown
    /// that expansion's alone, newest first for a reference that sees them
    /// all, and an identity recorded again overwritten in place, before the
    /// index is built and after.
    #[test]
    fn identities_past_the_threshold_are_found_through_the_index() {
        let heap = new_shared_heap();
        let env = Environment::with_heap(&heap);
        let identity = |i: usize| scopes(&[1, 100 + i]);
        for i in 0..20 {
            env.define_introduced_global("count".into(), identity(i), format!("count #{i}").into());
        }
        for i in 0..20 {
            let renamed = env.introduced_global("count", &identity(i));
            assert_eq!(renamed.as_deref(), Some(format!("count #{i}").as_str()));
        }
        let mut found = Vec::new();
        env.for_each_introduced_candidate("count", &scopes(&[1, 105, 250]), |scopes, renamed| {
            found.push((scopes.clone(), renamed.clone()))
        });
        assert_eq!(found, vec![(identity(5), Rc::from("count #5"))]);

        for i in [3, 17] {
            env.define_introduced_global("count".into(), identity(i), "fresh".into());
        }
        assert_eq!(recorded(&env, "count").len(), 20);
        for i in [3, 17] {
            assert_eq!(
                env.introduced_global("count", &identity(i)).as_deref(),
                Some("fresh")
            );
        }

        let every_scope = scopes(&(0..256).collect::<Vec<_>>());
        let mut order = Vec::new();
        env.for_each_introduced_candidate("count", &every_scope, |scopes, _| {
            order.push(scopes.clone())
        });
        assert_eq!(order, (0..20).rev().map(identity).collect::<Vec<_>>());
    }

    /// A child environment does not see them: `introduced_global` and
    /// `for_each_introduced_candidate` read the recording environment's own
    /// table, which is the one a form was compiled in (a namespace of either
    /// kind, a host namespace since #835). Nothing asserts which.
    #[test]
    fn entries_are_local_to_the_environment_that_recorded_them() {
        let heap = new_shared_heap();
        let parent = Rc::new(Environment::with_heap(&heap));
        parent.define_introduced_global("x".into(), scopes(&[1]), "x #1".into());
        let child = Environment::child_frame(Rc::clone(&parent));
        assert!(recorded(&child, "x").is_empty());
        assert_eq!(recorded(&parent, "x").len(), 1);
    }
}

/// A spelling with more scoped bindings than its environment's threshold is
/// read and written through its index (#613). The hygiene properties check
/// the candidate walks against an oracle; these check the exact-set readers,
/// redefinition, and which environments build an index.
#[cfg(test)]
mod scoped_index_tests {
    use super::*;
    use crate::heap::new_shared_heap;

    fn scopes(ids: &[usize]) -> ScopeSet {
        let mut set = ScopeSet::new();
        for id in ids {
            set.add_scope(crate::scope::ScopeId(*id));
        }
        set
    }

    fn n(value: i64) -> TaggedValue {
        TaggedValue::fixnum(value)
    }

    fn indexed_len(env: &Environment, name: &str) -> Option<usize> {
        let len = env.scoped_bindings.borrow().get(name)?.len();
        env.scoped_index(name, len).map(|index| index.len())
    }

    /// Redefinition overwrites in place, before the index is built and after;
    /// a scoped alias reads, writes and locates the binding it names through
    /// the exact-set readers; and a reference from one expansion reads its
    /// own.
    #[test]
    fn an_indexed_spelling_is_found_by_its_exact_readers() {
        let heap = new_shared_heap();
        let library = Rc::new(Environment::with_heap(&heap));
        let identity = |k: usize| scopes(&[k, 100 + k]);
        for k in 0..12 {
            library.define_with_scopes(&heap, "count", identity(k), n(k as i64));
        }
        library.define_with_scopes(&heap, "count", identity(2), n(-2));
        library.define_with_scopes(&heap, "count", identity(11), n(-11));
        assert_eq!(library.scoped_bindings.borrow()["count"].len(), 12);
        assert_eq!(indexed_len(&library, "count"), Some(12));
        for (k, value) in [(0, 0), (2, -2), (7, 7), (11, -11)] {
            let reference = scopes(&[k, 100 + k, 500]);
            assert_eq!(
                library.get_with_scopes(&heap, "count", &reference).unwrap(),
                Some(n(value))
            );
        }

        let user = Environment::with_heap(&heap);
        for (alias, k) in [("oldest", 0), ("newest", 11)] {
            user.define_scoped_alias(alias, Rc::clone(&library), "count".into(), identity(k));
        }
        assert_eq!(user.get(&heap, "oldest"), Some(n(0)));
        assert_eq!(user.get(&heap, "newest"), Some(n(-11)));
        user.set(&heap, "oldest", n(40)).unwrap();
        user.set(&heap, "newest", n(41)).unwrap();
        assert_eq!(
            library.scoped_definition_value(&heap, "count", &identity(0)),
            Some(n(40))
        );
        assert_eq!(
            library.scoped_definition_value(&heap, "count", &identity(11)),
            Some(n(41))
        );
        // Each alias locates the cell its definition's record names (#834).
        let located = |alias| user.binding_location(&heap, alias);
        let definition = |k| library.scoped_definition_location(&heap, "count", &identity(k));
        assert!(matches!(located("oldest"), Some(BindingLocation::Cell(..))));
        assert_eq!(located("oldest"), definition(0));
        assert_eq!(located("newest"), definition(11));
        assert_ne!(located("oldest"), located("newest"));

        library
            .set_with_scopes(&heap, "count", &scopes(&[4, 104, 500]), n(44))
            .unwrap();
        assert_eq!(
            library.scoped_definition_value(&heap, "count", &identity(4)),
            Some(n(44))
        );
        assert_eq!(library.scoped_bindings.borrow()["count"].len(), 12);
    }

    /// A namespace indexes a spelling past `INDEX_ABOVE`; a frame only past
    /// `FRAME_INDEX_ABOVE`, so a call binding nine parameters of one
    /// spelling, as a nine-formal `let-values` does, builds no index and no
    /// out-of-line tables.
    #[test]
    fn a_frame_is_indexed_only_past_its_own_threshold() {
        let heap = new_shared_heap();
        let root = Rc::new(Environment::with_heap(&heap));
        for k in 0..=INDEX_ABOVE {
            root.define_with_scopes(&heap, "x", scopes(&[k + 1]), n(k as i64));
        }
        assert_eq!(indexed_len(&root, "x"), Some(INDEX_ABOVE + 1));

        let frame = Environment::child_frame(Rc::clone(&root));
        for k in 0..=INDEX_ABOVE {
            frame.define_with_scopes(&heap, "x", scopes(&[1000 + k]), n(k as i64));
        }
        assert_eq!(indexed_len(&frame, "x"), None);
        assert!(frame.rare.get().is_none());
        for k in INDEX_ABOVE + 1..FRAME_INDEX_ABOVE {
            frame.define_with_scopes(&heap, "x", scopes(&[1000 + k]), n(k as i64));
        }
        assert_eq!(frame.scoped_bindings.borrow()["x"].len(), FRAME_INDEX_ABOVE);
        assert_eq!(indexed_len(&frame, "x"), None);
        frame.define_with_scopes(&heap, "x", scopes(&[5000]), n(-1));
        assert_eq!(indexed_len(&frame, "x"), Some(FRAME_INDEX_ABOVE + 1));
        assert_eq!(
            frame
                .get_with_scopes(&heap, "x", &scopes(&[1003, 9999]))
                .unwrap(),
            Some(n(3))
        );
        assert_eq!(
            frame.get_with_scopes(&heap, "x", &scopes(&[5000])).unwrap(),
            Some(n(-1))
        );
    }

    /// A host namespace indexes a spelling past `INDEX_ABOVE`, as every
    /// namespace does, and not at a frame's threshold (the owner's ruling (a)
    /// of 2026-11-07 on #835's question 2); the index is charged with the
    /// rest of its tables.
    #[test]
    fn a_host_namespace_is_indexed_at_the_namespace_threshold() {
        let heap = new_shared_heap();
        let root = Rc::new(Environment::with_heap(&heap));
        let host = Environment::host_namespace(Rc::clone(&root));
        assert!(!host.indexes(INDEX_ABOVE) && host.indexes(INDEX_ABOVE + 1));
        for k in 0..INDEX_ABOVE {
            host.define_with_scopes(&heap, "x", scopes(&[k + 1]), n(k as i64));
        }
        assert_eq!(indexed_len(&host, "x"), None);
        let unindexed = host.charged_bytes().unwrap();
        host.define_with_scopes(&heap, "x", scopes(&[100]), n(-1));
        assert_eq!(indexed_len(&host, "x"), Some(INDEX_ABOVE + 1));
        assert!(
            host.charged_bytes().unwrap() > unindexed,
            "the index is charged"
        );
        assert_eq!(
            host.get_with_scopes(&heap, "x", &scopes(&[3, 9999]))
                .unwrap(),
            Some(n(2))
        );
        assert_eq!(
            host.get_with_scopes(&heap, "x", &scopes(&[100])).unwrap(),
            Some(n(-1))
        );
    }
}

/// `name_reaches_binding_of` answers by *binding*. Every case here holds the
/// same value in both bindings on purpose: comparing values is what the
/// relinker did first, and it called these one binding.
#[cfg(test)]
mod name_view_tests {
    use super::*;
    use crate::heap::new_shared_heap;

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

    fn reaches(heap: &SharedHeap, env: &Environment, name: &str, ids: &[usize]) -> bool {
        env.name_reaches_binding_of(heap, name, &scopes(ids))
            .expect("not ambiguous")
    }

    /// For two name-visible scoped definitions, the name means the most
    /// recent. Introduced definitions no longer use this view (#427).
    #[test]
    fn the_name_reaches_only_the_latest_of_two_introduced_definitions() {
        let heap = new_shared_heap();
        let env = Environment::with_heap(&heap);
        env.define_scoped_definition(&heap, "count", scopes(&[1]), zero());
        env.define_scoped_definition(&heap, "count", scopes(&[2]), zero());
        assert!(!reaches(&heap, &env, "count", &[1]));
        assert!(reaches(&heap, &env, "count", &[2]));
    }

    /// A plain definition beside an introduced one takes the name, so the
    /// name reaches what an unscoped mention means and not what a mention
    /// carrying the introducing expansion's scope means.
    #[test]
    fn a_plain_sibling_takes_the_name_from_an_introduced_definition() {
        let heap = new_shared_heap();
        let env = Environment::with_heap(&heap);
        env.define(&heap, "x", zero());
        env.define_scoped_definition(&heap, "x", scopes(&[1]), zero());
        assert!(!reaches(&heap, &env, "x", &[1]));
        assert!(reaches(&heap, &env, "x", &[]));
        // Scopes no binding claims fall back to the plain one, as a read does.
        assert!(reaches(&heap, &env, "x", &[7]));
    }

    /// An explicitly installed alias can name an introduced global's
    /// identity. The VM no longer creates these bare aliases (#427), but the
    /// environment API still needs to compare their bindings correctly.
    #[test]
    fn a_bare_alias_is_the_introduced_global_it_was_installed_for() {
        let heap = new_shared_heap();
        let env = Rc::new(Environment::with_heap(&heap));
        for (id, renamed) in [(1, "count #1"), (2, "count #2")] {
            env.define(&heap, renamed, zero());
            env.define_introduced_global("count".into(), scopes(&[id]), renamed.into());
            env.define_alias("count", Rc::clone(&env), renamed.into());
        }
        assert!(!reaches(&heap, &env, "count", &[1]));
        assert!(reaches(&heap, &env, "count", &[2]));
    }

    /// A child frame that binds nothing of the name defers to its parent.
    #[test]
    fn the_walk_continues_into_the_parent() {
        let heap = new_shared_heap();
        let parent = Rc::new(Environment::with_heap(&heap));
        parent.define(&heap, "x", zero());
        parent.define_scoped_definition(&heap, "x", scopes(&[1]), zero());
        let child = Environment::child_frame(parent);
        assert!(!reaches(&heap, &child, "x", &[1]));
        assert!(reaches(&heap, &child, "x", &[2]));
    }
}

#[cfg(test)]
#[path = "hygiene_properties.rs"]
mod hygiene_properties;

/// Import `source_name` from `source` into `env` as `name`, as a library's
/// export is imported: `env` takes the cell `source`'s binding names
/// (`cell_of`, `import_cell`; #834). For the tests that imported with
/// `share_binding` before binding records.
#[cfg(test)]
fn share(
    heap: &SharedHeap,
    env: &Environment,
    name: &str,
    source: &Environment,
    source_name: &str,
) {
    let cell = source
        .cell_of(heap, source_name)
        .unwrap_or_else(|| panic!("`{source_name}` names no cell"));
    env.import_cell(heap, name, cell);
}

/// What an import installs (#406): the library's location, not a copy of what
/// it held. `tests/scheme/stdlib/library-bindings.scm` says the same from
/// Scheme, with Gauche arbitrating; these pin the parts only Rust can see —
/// which records name the library's cell, which copy, and that an importer
/// holds a cell and no environment (#834).
#[cfg(test)]
mod shared_binding_tests {
    use super::*;
    use crate::heap::new_shared_heap;

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    /// A library environment holding `count`, and an importer on its heap.
    fn library_and_importer(heap: &SharedHeap) -> (Rc<Environment>, Rc<Environment>) {
        let library = Rc::new(Environment::with_heap(heap));
        library.define(heap, "count", n(0));
        let importer = Rc::new(Environment::with_heap(heap));
        (library, importer)
    }

    /// The environments the collector is shown from `env`, by its trace and
    /// as its parent. None of these tests gives an importer a parent or an
    /// alias, and an import is a cell, not an edge to its library (#834),
    /// so there are none.
    fn edges_to_environments(env: &Environment) -> Vec<*const Environment> {
        /// Records the environments it is given, and ignores the rest.
        struct Envs(Vec<*const Environment>);
        impl crate::SlotVisitor for Envs {
            fn slot(&mut self, _: &Cell<TaggedValue>) {}
            fn pinned(&mut self, _: TaggedValue) {}
            fn env(&mut self, env: &Rc<Environment>) {
                self.0.push(Rc::as_ptr(env));
            }
            fn continuation(&mut self, _: &Rc<crate::CpsContinuation>) {}
            fn cont_env(&mut self, _: &crate::cont_value::ContEnv) {}
            fn expr_literals(&mut self, _: &crate::cps_expr::CpsExpr) {}
        }
        let mut envs = Envs(Vec::new());
        env.trace(&mut envs);
        envs.0.extend(env.parent().map(Rc::as_ptr));
        envs.0
    }

    #[test]
    fn a_shared_binding_reads_what_its_owner_holds_now() {
        let heap = new_shared_heap();
        let (library, importer) = library_and_importer(&heap);
        share(&heap, &importer, "count", &library, "count");
        library.set(&heap, "count", n(2)).unwrap();

        assert_eq!(importer.get(&heap, "count"), Some(n(2)));
        // Every read, not only `get`: the VM reads by slot, a scoped reference
        // falls back by name, and an import set enumerates.
        let slot = importer.local_slot("count").unwrap();
        assert_eq!(importer.slot_value(&heap, slot), n(2));
        assert_eq!(
            importer
                .get_with_scopes(&heap, "count", &ScopeSet::new())
                .unwrap(),
            Some(n(2))
        );
        assert_eq!(importer.bindings(&heap), vec![("count".to_string(), n(2))]);
    }

    #[test]
    fn a_write_to_a_shared_binding_lands_in_its_owner() {
        let heap = new_shared_heap();
        let (library, importer) = library_and_importer(&heap);
        share(&heap, &importer, "count", &library, "count");

        importer.set(&heap, "count", n(10)).unwrap();
        assert_eq!(library.get(&heap, "count"), Some(n(10)));

        let slot = importer.local_slot("count").unwrap();
        importer.set_slot_value(&heap, slot, n(11));
        assert_eq!(library.get(&heap, "count"), Some(n(11)));
        assert_eq!(importer.get(&heap, "count"), Some(n(11)));
    }

    #[test]
    fn a_definition_over_a_shared_binding_is_the_importers_own() {
        let heap = new_shared_heap();
        let (library, importer) = library_and_importer(&heap);
        share(&heap, &importer, "count", &library, "count");
        let slot = importer.local_slot("count").unwrap();

        importer.define(&heap, "count", n(99));
        assert_eq!(importer.get(&heap, "count"), Some(n(99)));
        assert_eq!(library.get(&heap, "count"), Some(n(0)));
        // In place, which is what the VM's per-site global caches rest on.
        assert_eq!(importer.local_slot("count"), Some(slot));

        // And it stays the importer's: a later write goes no further.
        importer.set(&heap, "count", n(100)).unwrap();
        assert_eq!(library.get(&heap, "count"), Some(n(0)));
    }

    #[test]
    fn sharing_over_a_definition_replaces_it_in_place() {
        let heap = new_shared_heap();
        let (library, importer) = library_and_importer(&heap);
        importer.define(&heap, "count", n(7));
        let slot = importer.local_slot("count").unwrap();

        share(&heap, &importer, "count", &library, "count");
        assert_eq!(importer.local_slot("count"), Some(slot));
        assert_eq!(importer.slot_value(&heap, slot), n(0));
    }

    #[test]
    fn a_re_export_forwards_to_the_owner_and_not_to_the_library_between() {
        let heap = new_shared_heap();
        let (library, relay) = library_and_importer(&heap);
        share(&heap, &relay, "relayed", &library, "count");
        let importer = Rc::new(Environment::with_heap(&heap));
        share(&heap, &importer, "again", &relay, "relayed");

        library.set(&heap, "count", n(3)).unwrap();
        assert_eq!(importer.get(&heap, "again"), Some(n(3)));

        // One hop: the relay can go back to being its own binding without the
        // importer noticing.
        relay.define(&heap, "relayed", n(-1));
        library.set(&heap, "count", n(4)).unwrap();
        assert_eq!(importer.get(&heap, "again"), Some(n(4)));

        // The importer holds the library's cell, and no environment.
        assert_eq!(
            edges_to_environments(&importer),
            Vec::<*const Environment>::new()
        );
        assert_eq!(
            importer.binding_location(&heap, "again"),
            library.binding_location(&heap, "count")
        );
    }

    #[test]
    fn a_staging_environment_carries_a_binding_without_owning_it() {
        let heap = new_shared_heap();
        let (library, importer) = library_and_importer(&heap);
        // A staging frame, as the desugarer stages an import over the
        // environment it then copies into.
        let staging = Rc::new(Environment::child_frame(Rc::clone(&importer)));
        share(&heap, &staging, "count", &library, "count");
        // A binding of the staging frame's own, given its value outright.
        staging.define(&heap, "loose", n(5));

        assert!(importer.copy_binding(&heap, "c:count", &staging, "count"));
        assert!(importer.copy_binding(&heap, "c:loose", &staging, "loose"));
        assert!(!importer.copy_binding(&heap, "c:absent", &staging, "absent"));
        assert_eq!(importer.get(&heap, "c:absent"), None);

        library.set(&heap, "count", n(2)).unwrap();
        assert_eq!(importer.get(&heap, "c:count"), Some(n(2)));

        // A value, not a binding of an environment about to be dropped; and
        // the staged import arrived as the library's cell.
        staging.set(&heap, "loose", n(6)).unwrap();
        assert_eq!(importer.get(&heap, "c:loose"), Some(n(5)));
        assert_eq!(
            importer.binding_location(&heap, "c:count"),
            library.binding_location(&heap, "count")
        );
        assert_eq!(
            edges_to_environments(&importer),
            Vec::<*const Environment>::new()
        );
    }

    #[test]
    fn a_re_exported_binding_is_one_location_whoever_assigns_and_whenever() {
        let heap = new_shared_heap();
        // `primitives` stands for a library registered from Rust, `base` for
        // the Scheme library that re-exports it. A first version copied such
        // a binding down the chain, and an importer then saw a library's
        // assignment only if the import happened to come after it.
        let (primitives, base) = library_and_importer(&heap);
        share(&heap, &base, "count", &primitives, "count");
        let early = Rc::new(Environment::with_heap(&heap));
        share(&heap, &early, "count", &base, "count");

        base.set(&heap, "count", n(50)).unwrap();
        let late = Rc::new(Environment::with_heap(&heap));
        share(&heap, &late, "count", &base, "count");

        for env in [&primitives, &base, &early, &late] {
            assert_eq!(env.get(&heap, "count"), Some(n(50)));
        }
        early.set(&heap, "count", n(51)).unwrap();
        for env in [&primitives, &base, &early, &late] {
            assert_eq!(env.get(&heap, "count"), Some(n(51)));
        }
    }

    #[test]
    fn a_name_the_source_does_not_bind_plainly_is_left_to_the_caller() {
        let heap = new_shared_heap();
        let (library, importer) = library_and_importer(&heap);
        assert_eq!(library.cell_of(&heap, "missing"), None);
        assert_eq!(importer.get(&heap, "missing"), None);

        // A parent's binding is not the source's own location to share.
        let child = Rc::new(Environment::child_frame(Rc::clone(&library)));
        assert_eq!(child.cell_of(&heap, "count"), None);
        // Nor is a frame's own binding a cell.
        child.define(&heap, "own", n(1));
        assert_eq!(child.cell_of(&heap, "own"), None);
    }

    /// A namespace's own cell imported under another name is copied: two
    /// names of one namespace never share a cell, or a later `define` of
    /// either would assign both. Imported under its own name, it is left
    /// alone.
    #[test]
    fn an_own_cell_imported_under_another_name_is_copied() {
        let heap = new_shared_heap();
        let (library, _) = library_and_importer(&heap);
        share(&heap, &library, "count", &library, "count");
        share(&heap, &library, "also", &library, "count");
        assert_eq!(library.get(&heap, "count"), Some(n(0)));
        assert_eq!(library.get(&heap, "also"), Some(n(0)));
        assert_ne!(
            library.binding_location(&heap, "also"),
            library.binding_location(&heap, "count")
        );
        library.define(&heap, "also", n(1));
        assert_eq!(library.get(&heap, "count"), Some(n(0)));
        assert_eq!(library.recount(&heap.borrow()), library.counted());
        assert_eq!(
            edges_to_environments(&library),
            Vec::<*const Environment>::new(),
            "a self-reference would leak the environment"
        );
    }

    /// An importer holds the library's cell, not the library (#834): its
    /// trace shows the collector no environment, and each word it pins is a
    /// record naming the library's cell. So once the library drops, a
    /// collection rooted at the importer alone keeps the cell and its value,
    /// where an import owner kept the whole library before.
    #[test]
    fn an_importer_holds_the_cell_and_not_the_library() {
        use crate::heap::sentinels::Sentinels;
        use crate::heap::trace_sentinels::collect_only;

        /// The environments and the pinned words a trace shows.
        #[derive(Default)]
        struct Shown {
            envs: usize,
            pinned: Vec<TaggedValue>,
        }
        impl crate::SlotVisitor for Shown {
            fn slot(&mut self, cell: &Cell<TaggedValue>) {
                self.pinned.push(cell.get());
            }
            fn pinned(&mut self, value: TaggedValue) {
                self.pinned.push(value);
            }
            fn env(&mut self, _: &Rc<Environment>) {
                self.envs += 1;
            }
            fn continuation(&mut self, _: &Rc<crate::CpsContinuation>) {}
            fn cont_env(&mut self, _: &crate::cont_value::ContEnv) {}
            fn expr_literals(&mut self, _: &crate::cps_expr::CpsExpr) {}
        }

        let heap = new_shared_heap();
        let mut s = Sentinels::new(&mut heap.borrow_mut());
        let value = s.pair(&mut heap.borrow_mut(), "the library's cell's value");
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "count", value);
        let cell = library.cell_of(&heap, "count").unwrap();
        let importer = Rc::new(Environment::with_heap(&heap));
        share(&heap, &importer, "count", &library, "count");

        let mut shown = Shown::default();
        // The environment's own trace, not its `Rc`'s, which is the edge.
        Environment::trace(&importer, &mut shown);
        assert_eq!(shown.envs, 0, "an import is no edge to its library");
        assert_eq!(
            shown.pinned.len(),
            1,
            "the import's record: {:?}",
            shown.pinned
        );
        {
            let h = heap.borrow();
            for &word in &shown.pinned {
                assert!(
                    word.is_pair(),
                    "a namespace's slot holds a record: {word:?}"
                );
                assert_eq!(Record::from_word(word).cell(&h), cell);
            }
        }

        drop(library);
        assert_eq!(heap.borrow().namespaces(), 1, "the library dropped");
        collect_only(&mut heap.borrow_mut(), |visitor| visitor.env(&importer));
        s.assert_survived(&heap.borrow());
        assert_eq!(importer.get(&heap, "count"), Some(value));
        assert_eq!(importer.cell_of(&heap, "count"), Some(cell));
    }

    /// A definition over an import re-points the importer's record at a cell
    /// of its own (#834): the slot keeps its word, the same record, which is
    /// what the VM's per-site global caches rest on, and the library's cell
    /// keeps the library's value.
    #[test]
    fn a_define_over_an_import_repoints_its_record() {
        let heap = new_shared_heap();
        let (library, importer) = library_and_importer(&heap);
        share(&heap, &importer, "count", &library, "count");
        let library_cell = library.cell_of(&heap, "count").unwrap();
        let slot = importer.local_slot("count").unwrap();
        let record = importer.local_record("count").unwrap();
        let word = importer.bindings.borrow().read_slot(slot);
        assert_eq!(record.cell(&heap.borrow()), library_cell);

        importer.define(&heap, "count", n(99));

        assert_eq!(importer.local_slot("count"), Some(slot));
        assert_eq!(importer.local_record("count"), Some(record));
        assert_eq!(
            importer.bindings.borrow().read_slot(slot),
            word,
            "the slot's word is unchanged"
        );
        {
            let h = heap.borrow();
            assert_eq!(record.value(&h), n(99));
            let own = record.cell(&h);
            assert_ne!(own, library_cell, "re-pointed");
            assert!(own.owned_by(&h, importer.env_id()), "at a cell of its own");
            assert_eq!(library_cell.value(&h), n(0));
        }
        assert_eq!(library.get(&heap, "count"), Some(n(0)));
        assert_eq!(importer.recount(&heap.borrow()), importer.counted());
    }
}

/// `binding_location` answers "is this one binding?" across environments,
/// which a comparison of values cannot (#407).
#[cfg(test)]
mod binding_location_tests {
    use super::*;
    use crate::heap::new_shared_heap;

    fn zero() -> TaggedValue {
        TaggedValue::fixnum(0)
    }

    #[test]
    fn equal_values_in_two_environments_are_two_locations() {
        let heap = new_shared_heap();
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "X", zero());
        let program = Rc::new(Environment::with_heap(&heap));
        program.define(&heap, "X", zero());

        assert_eq!(library.get(&heap, "X"), program.get(&heap, "X"));
        assert_ne!(
            library.binding_location(&heap, "X"),
            program.binding_location(&heap, "X")
        );
        assert_eq!(program.binding_location(&heap, "missing"), None);
    }

    #[test]
    fn an_import_is_its_librarys_location_by_any_route_and_any_name() {
        let heap = new_shared_heap();
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "X", zero());
        let relay = Rc::new(Environment::with_heap(&heap));
        share(&heap, &relay, "relayed", &library, "X");
        let program = Rc::new(Environment::with_heap(&heap));
        share(&heap, &program, "direct", &library, "X");
        share(&heap, &program, "through-relay", &relay, "relayed");

        let home = library.binding_location(&heap, "X");
        assert!(home.is_some());
        assert_eq!(program.binding_location(&heap, "direct"), home);
        assert_eq!(program.binding_location(&heap, "through-relay"), home);
        assert_eq!(relay.binding_location(&heap, "relayed"), home);

        // Defined over, it is the program's own again.
        program.define(&heap, "direct", zero());
        assert_ne!(program.binding_location(&heap, "direct"), home);
    }

    #[test]
    fn the_walk_is_gets_walk() {
        let heap = new_shared_heap();
        let root = Rc::new(Environment::with_heap(&heap));
        root.define(&heap, "X", zero());
        let child = Rc::new(Environment::child_frame(Rc::clone(&root)));
        // Through the parent.
        assert_eq!(
            child.binding_location(&heap, "X"),
            root.binding_location(&heap, "X")
        );

        // Through a macro-expansion alias, into another environment.
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "helper", zero());
        root.define_alias("helper.1", Rc::clone(&library), "helper".into());
        assert_eq!(
            child.binding_location(&heap, "helper.1"),
            library.binding_location(&heap, "helper")
        );

        // Through an alias whose target is the environment holding it, which
        // is stored without one: the walk restarts there, as `get`'s does.
        root.define(&heap, "renamed.7", zero());
        root.define_alias("bare", Rc::clone(&root), "renamed.7".into());
        assert_eq!(child.get(&heap, "bare"), Some(zero()));
        assert_eq!(
            child.binding_location(&heap, "bare"),
            root.binding_location(&heap, "renamed.7")
        );
        // A dangling alias is nothing, to both.
        root.define_alias("dangling", Rc::clone(&root), "nowhere".into());
        assert_eq!(child.get(&heap, "dangling"), None);
        assert_eq!(child.binding_location(&heap, "dangling"), None);

        // A plain binding wins over a parent's, as it does for `get`.
        child.define(&heap, "X", zero());
        assert_ne!(
            child.binding_location(&heap, "X"),
            root.binding_location(&heap, "X")
        );
    }

    #[test]
    fn a_macro_introduced_definition_has_a_location_of_its_own() {
        let heap = new_shared_heap();
        let env = Rc::new(Environment::with_heap(&heap));
        let scopes = ScopeSet::from_iter([crate::scope::ScopeId(1)]);
        env.define_scoped_definition(&heap, "X", scopes, zero());
        let other = Rc::new(Environment::with_heap(&heap));
        other.define(&heap, "X", zero());

        // `get` reaches it by name where nothing plain has the name, and then
        // it is that definition, not whatever else holds an equal value.
        assert_eq!(env.get(&heap, "X"), Some(zero()));
        // A namespace's scoped entry is a record over a cell (#834).
        let here = env.binding_location(&heap, "X");
        assert!(matches!(here, Some(BindingLocation::Cell(..))));
        assert_ne!(here, other.binding_location(&heap, "X"));

        // A plain binding of the name takes the by-name view, as it does for
        // `get`, and is a cell of its own.
        env.define(&heap, "X", zero());
        let plain = env.binding_location(&heap, "X");
        assert!(matches!(plain, Some(BindingLocation::Cell(..))));
        assert_ne!(plain, here);
    }
}

/// An alias can name a *binding* a macro introduced, not only a spelling
/// (#408). `hygiene_matrix.rs`'s `introducing` rows say it from Scheme; these
/// pin how each backend's storage is found and what is refused.
#[cfg(test)]
mod introduced_definition_tests {
    use super::*;
    use crate::heap::new_shared_heap;

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
    fn tree_walker_library(heap: &SharedHeap) -> Rc<Environment> {
        let library = Rc::new(Environment::with_heap(heap));
        library.define(heap, "count", n(100));
        library.define_with_scopes(heap, "count", scopes(&[1]), n(10));
        library.define_with_scopes(heap, "count", scopes(&[2]), n(20));
        library
    }

    #[test]
    fn a_mentions_scopes_select_its_own_generators_definition() {
        let heap = new_shared_heap();
        let library = tree_walker_library(&heap);
        // A use of the generated macro adds its own expansion scope.
        for (mention, run) in [(scopes(&[1, 7]), 1), (scopes(&[2, 8]), 2)] {
            let found = library
                .introduced_definition(&heap, "count", &mention)
                .unwrap();
            assert_eq!(found, IntroducedDefinition::Scoped(scopes(&[run])));
        }
        // No scopes select the plain one, which is not an introduced
        // definition; nor is a spelling the library does not have.
        assert!(
            library
                .introduced_definition(&heap, "count", &scopes(&[9]))
                .is_none()
        );
        assert!(
            library
                .introduced_definition(&heap, "other", &scopes(&[1]))
                .is_none()
        );
        assert!(library.has_introduced_definition("count"));
        assert!(!library.has_introduced_definition("other"));
    }

    #[test]
    fn a_scoped_alias_reads_writes_and_locates_that_definition() {
        let heap = new_shared_heap();
        let library = tree_walker_library(&heap);
        let program = Rc::new(Environment::with_heap(&heap));
        program.define(&heap, "count", n(-1));
        program.define_scoped_alias("count.1", Rc::clone(&library), "count".into(), scopes(&[1]));
        program.define_scoped_alias("count.2", Rc::clone(&library), "count".into(), scopes(&[2]));

        assert_eq!(program.get(&heap, "count.1"), Some(n(10)));
        assert_eq!(program.get(&heap, "count.2"), Some(n(20)));

        program.set(&heap, "count.1", n(11)).unwrap();
        assert_eq!(program.get(&heap, "count.1"), Some(n(11)));
        // Nothing else of the spelling moved: not the second run's, not the
        // library's plain one, not the program's.
        assert_eq!(program.get(&heap, "count.2"), Some(n(20)));
        assert_eq!(library.get(&heap, "count"), Some(n(100)));
        assert_eq!(program.get(&heap, "count"), Some(n(-1)));

        let first = program.binding_location(&heap, "count.1");
        assert!(matches!(first, Some(BindingLocation::Cell(..))));
        assert_ne!(first, program.binding_location(&heap, "count.2"));
        assert_ne!(first, library.binding_location(&heap, "count"));
    }

    #[test]
    fn a_scoped_alias_never_falls_back_to_the_spelling() {
        let heap = new_shared_heap();
        // The scopes are an identity, matched exactly. Resolved as a
        // *reference* instead, an identity the library does not hold would
        // fall back by name and read — and assign — the plain `count`.
        let library = tree_walker_library(&heap);
        let program = Rc::new(Environment::with_heap(&heap));
        program.define_scoped_alias("count.9", Rc::clone(&library), "count".into(), scopes(&[9]));

        assert_eq!(program.get(&heap, "count.9"), None);
        assert_eq!(program.binding_location(&heap, "count.9"), None);
        // Refused with the name, as `set` refuses a dangling plain alias.
        assert_eq!(
            program.set(&heap, "count.9", n(1)),
            Err("count".to_string())
        );
        assert_eq!(library.get(&heap, "count"), Some(n(100)));
    }

    #[test]
    fn the_vms_renamed_global_is_found_by_the_same_question() {
        let heap = new_shared_heap();
        // The VM renames an introduced definition to a global of its own and
        // records the identity, without exposing the bare name.
        let library = Rc::new(Environment::with_heap(&heap));
        for (run, renamed, value) in [(1, "count.g1", 10), (2, "count.g2", 20)] {
            library.define(&heap, renamed, n(value));
            library.define_introduced_global("count".into(), scopes(&[run]), renamed.into());
        }
        let found = library
            .introduced_definition(&heap, "count", &scopes(&[1, 7]))
            .unwrap();
        assert_eq!(found, IntroducedDefinition::Renamed("count.g1".into()));
        assert!(library.has_introduced_definition("count"));
    }

    #[test]
    fn a_lexical_binding_is_not_an_introduced_definition() {
        let heap = new_shared_heap();
        // A `let`-bound variable around the macro's definition is a scoped
        // binding in a *child* frame. It is not something an alias can name —
        // it lives in a run-time frame — so it is left to scoped resolution.
        let library = Rc::new(Environment::with_heap(&heap));
        let body = Rc::new(Environment::child_frame(Rc::clone(&library)));
        body.define_with_scopes(&heap, "f", scopes(&[3]), n(1));
        assert!(
            body.introduced_definition(&heap, "f", &scopes(&[3, 7]))
                .is_none()
        );
        assert!(!body.has_introduced_definition("f"));

        // From inside a body, the root's definitions are still found.
        library.define_scoped_definition(&heap, "g", scopes(&[4]), n(2));
        assert_eq!(
            body.introduced_definition(&heap, "g", &scopes(&[4, 7])),
            Some(IntroducedDefinition::Scoped(scopes(&[4])))
        );
    }
}

/// `import_alias` gives a root a name of its own for an imported *location*,
/// so that a reference bound to it survives the importer defining the
/// spelling itself (#438).
#[cfg(test)]
mod import_alias_tests {
    use super::*;
    use crate::heap::new_shared_heap;

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    fn library_and_program(heap: &SharedHeap) -> (Rc<Environment>, Rc<Environment>) {
        let library = Rc::new(Environment::with_heap(heap));
        library.define(heap, "count", n(0));
        let program = Rc::new(Environment::with_heap(heap));
        share(heap, &program, "count", &library, "count");
        (library, program)
    }

    #[test]
    fn the_alias_outlives_a_definition_over_the_import() {
        let heap = new_shared_heap();
        let (library, program) = library_and_program(&heap);
        let alias = program
            .import_alias(&heap, "count", || "count.alias".into())
            .unwrap();

        // The program defines the spelling itself: a binding of its own.
        program.define(&heap, "count", n(100));
        library.set(&heap, "count", n(1)).unwrap();

        assert_eq!(program.get(&heap, "count"), Some(n(100)));
        assert_eq!(program.get(&heap, &alias), Some(n(1)));
        // And it writes where it reads.
        program.set(&heap, &alias, n(2)).unwrap();
        assert_eq!(library.get(&heap, "count"), Some(n(2)));
        assert_eq!(program.get(&heap, "count"), Some(n(100)));
        assert_eq!(
            program.binding_location(&heap, &alias),
            library.binding_location(&heap, "count")
        );
    }

    #[test]
    fn one_name_has_one_alias_however_often_it_is_asked_for() {
        let heap = new_shared_heap();
        let (_, program) = library_and_program(&heap);
        let mut minted = 0;
        let mut mint = || {
            minted += 1;
            Rc::<str>::from(format!("count.{minted}"))
        };
        let first = program.import_alias(&heap, "count", &mut mint).unwrap();
        let again = program.import_alias(&heap, "count", &mut mint).unwrap();
        assert_eq!(first, again);

        assert_eq!(minted, 1, "an alias per name, not per request");
    }

    #[test]
    fn a_second_name_for_the_location_has_an_alias_of_its_own() {
        let heap = new_shared_heap();
        let (library, program) = library_and_program(&heap);
        let mut minted = 0;
        let mut mint = || {
            minted += 1;
            Rc::<str>::from(format!("alias.{minted}"))
        };
        let first = program.import_alias(&heap, "count", &mut mint).unwrap();

        // A second name for the same location — a prefixed import. The
        // desugarer reads the spelling a reference was written with back off
        // its alias, so the two names cannot share one.
        share(&heap, &program, "c:count", &library, "count");
        let prefixed = program.import_alias(&heap, "c:count", &mut mint).unwrap();
        assert_ne!(first, prefixed);
        assert_eq!(
            program.import_alias(&heap, "c:count", &mut mint).unwrap(),
            prefixed
        );
        assert_eq!(
            program.import_alias(&heap, "count", &mut mint).unwrap(),
            first
        );
        assert_eq!(minted, 2);

        // Both are the library's location, and both are bookkeeping.
        library.set(&heap, "count", n(9)).unwrap();
        assert_eq!(program.get(&heap, &first), Some(n(9)));
        assert_eq!(program.get(&heap, &prefixed), Some(n(9)));
        assert_eq!(program.local_names(&heap), ["count", "c:count"]);
    }

    #[test]
    fn an_alias_is_not_among_the_names_a_program_bound() {
        let heap = new_shared_heap();
        let (_, program) = library_and_program(&heap);
        program.define(&heap, "before", n(1));
        program
            .import_alias(&heap, "count", || "count.alias".into())
            .unwrap();
        program.define(&heap, "after", n(2));

        assert_eq!(program.local_names(&heap), ["count", "before", "after"]);
        // And leaving it out does not shift the values after it.
        assert_eq!(
            program.bindings(&heap),
            [
                ("count".to_string(), n(0)),
                ("before".to_string(), n(1)),
                ("after".to_string(), n(2)),
            ]
        );
    }

    #[test]
    fn an_alias_defined_over_is_replaced_not_handed_out_again() {
        let heap = new_shared_heap();
        let (library, program) = library_and_program(&heap);
        let first = program
            .import_alias(&heap, "count", || "count.alias".into())
            .unwrap();

        // Something defines the alias's own spelling: the slot is the
        // definer's from now on, and no longer the library's location.
        program.define(&heap, Rc::clone(&first), n(7));
        // A name the program bound from that moment, not from whenever the
        // location is next asked for.
        assert_eq!(program.local_names(&heap), ["count", "count.alias"]);
        assert_eq!(
            program.bindings(&heap),
            [
                ("count".to_string(), n(0)),
                ("count.alias".to_string(), n(7))
            ]
        );
        let second = program
            .import_alias(&heap, "count", || "count.again".into())
            .unwrap();

        assert_ne!(first, second);
        library.set(&heap, "count", n(1)).unwrap();
        assert_eq!(program.get(&heap, &second), Some(n(1)));
        assert_eq!(program.get(&heap, &first), Some(n(7)));
        // The definition is a name the program bound; the new alias is not.
        assert_eq!(program.local_names(&heap), ["count", "count.alias"]);
    }

    #[test]
    fn minting_never_forwards_over_a_name_already_bound() {
        let heap = new_shared_heap();
        let (_, program) = library_and_program(&heap);
        program.define(&heap, "count.0", n(42));
        let mut offered = ["count.0", "count.1"].into_iter();
        let alias = program
            .import_alias(&heap, "count", || offered.next().unwrap().into())
            .unwrap();

        assert_eq!(&*alias, "count.1");
        assert_eq!(program.get(&heap, "count.0"), Some(n(42)));
    }

    /// [`minting_never_forwards_over_a_name_already_bound`]'s twin (#836):
    /// a name a placeholder holds is taken too, or the alias's install would
    /// adopt the placeholder, and code that linked the unbound name would
    /// read the import.
    #[test]
    fn minting_never_forwards_over_a_name_a_placeholder_holds() {
        let heap = new_shared_heap();
        let (_, program) = library_and_program(&heap);
        let placeholder = program
            .link_record(&heap, "count.0")
            .expect("a placeholder");
        let mut offered = ["count.0", "count.1"].into_iter();
        let alias = program
            .import_alias(&heap, "count", || offered.next().unwrap().into())
            .unwrap();

        assert_eq!(&*alias, "count.1");
        assert_eq!(program.get(&heap, "count.0"), None, "still unbound");
        assert_eq!(
            program.link_record(&heap, "count.0"),
            Some(placeholder),
            "and still the placeholder"
        );
        assert!(placeholder.is_placeholder(&heap.borrow()));
    }

    /// The same in a host namespace (#836): a name its weak read-through
    /// record holds, over the parent's placeholder, is taken too, or the
    /// alias's install would adopt the record.
    #[test]
    fn minting_never_forwards_over_a_name_a_weak_read_through_record_holds() {
        let heap = new_shared_heap();
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "count", n(0));
        let global = Rc::new(Environment::with_heap(&heap));
        let host = Rc::new(Environment::host_namespace(Rc::clone(&global)));
        share(&heap, &host, "count", &library, "count");
        let child = host.link_record(&heap, "count.0").expect("a weak child");
        let mut offered = ["count.0", "count.1"].into_iter();
        let alias = host
            .import_alias(&heap, "count", || offered.next().unwrap().into())
            .unwrap();

        assert_eq!(&*alias, "count.1");
        assert_eq!(host.get(&heap, "count.0"), None, "still unbound");
        assert_eq!(
            host.weak_entries().and_then(|store| store.get("count.0")),
            Some(child),
            "and still the weak record, in the store"
        );
        assert_eq!(host.link_record(&heap, "count.0"), Some(child));
    }

    #[test]
    fn every_enumeration_of_names_leaves_an_alias_out() {
        let heap = new_shared_heap();
        let (_, program) = library_and_program(&heap);
        program
            .import_alias(&heap, "count", || "count.alias".into())
            .unwrap();
        assert_eq!(program.get_all_names(&heap), ["count"]);
    }

    #[test]
    fn only_an_import_has_one() {
        let heap = new_shared_heap();
        let (_, program) = library_and_program(&heap);
        program.define(&heap, "own", n(5));
        let never = || -> Rc<str> { unreachable!("nothing to alias") };
        assert_eq!(program.import_alias(&heap, "own", never), None);
        assert_eq!(program.import_alias(&heap, "missing", never), None);

        // Defined over, the name is no longer an import.
        program.define(&heap, "count", n(100));
        assert_eq!(program.import_alias(&heap, "count", never), None);
    }
}

/// Spike 4b.1's condition (#829; the owner's rulings of 2026-11-03 on
/// #782): `import_aliases` keys an imported cell by its word less a check
/// build's generation stamp (`BindingCell::key`), and the arena reuses a
/// slot once its cell is swept. A key must not outlive every record that
/// names its cell, or it meets the key of a cell made in that slot after a
/// collection: it would then hide a name the program bound under the
/// alias's spelling (`visible_slots`), and be taken for that cell's alias
/// (`import_alias`). `forget_alias` drops it as the alias's record leaves
/// the cell, on each of the two paths that re-point a record; each test
/// below holds one of them, and fails without it in either build.
#[cfg(test)]
mod import_alias_reuse_tests {
    use super::*;
    use crate::heap::new_shared_heap;
    use crate::heap::trace_sentinels::collect_only;

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    /// The cell `env`'s record for `name` names.
    fn cell_of(heap: &SharedHeap, env: &Environment, name: &str) -> BindingCell {
        let record = env.local_record(name).expect("a namespace's name");
        record.cell(&heap.borrow())
    }

    /// An importer mints the alias `f.1` for a library's `f`, the library
    /// drops, as a redefined one's does since #834, and `repoint` moves
    /// both spellings off the library's cell, so that nothing names it but
    /// the alias's key, if that is still there. A collection sweeps the
    /// cell, a second library makes a cell in its slot, and the program
    /// imports that cell as `f.1` and as `f`.
    fn reuse_case(repoint: impl FnOnce(&SharedHeap, &Environment)) {
        let heap = new_shared_heap();
        let importer = Rc::new(Environment::with_heap(&heap));
        let minted = Cell::new(0);
        let mint = || {
            minted.set(minted.get() + 1);
            Rc::<str>::from(format!("f.{}", minted.get()))
        };
        let old = {
            let library = Rc::new(Environment::with_heap(&heap));
            library.define(&heap, "f", n(1));
            share(&heap, &importer, "f", &library, "f");
            assert_eq!(
                importer.import_alias(&heap, "f", mint).as_deref(),
                Some("f.1")
            );
            cell_of(&heap, &importer, "f")
        };
        repoint(&heap, &importer);
        collect_only(&mut heap.borrow_mut(), |visitor| visitor.env(&importer));

        // The keys are never traced, so nothing kept the cell: swept, and
        // the next pair allocated takes its slot.
        let slot = old.word().heap_index();
        assert!(
            heap.borrow().free_pair_slots().contains(&slot),
            "nothing names the library's cell, so the collection swept it"
        );
        let exporter = Rc::new(Environment::with_heap(&heap));
        {
            let mut held = heap.borrow_mut();
            while held.free_pair_slots().last() != Some(&slot) {
                held.alloc_pair(TaggedValue::UNSPECIFIED, TaggedValue::UNSPECIFIED);
            }
        }
        exporter.define(&heap, "h", n(4));
        let fresh = cell_of(&heap, &exporter, "h");
        assert_eq!(fresh.word().heap_index(), slot, "the slot is reused");
        assert_eq!(
            fresh.key(),
            old.key(),
            "and the key with it, in every build"
        );

        share(&heap, &importer, "f.1", &exporter, "h");
        share(&heap, &importer, "f", &exporter, "h");
        assert!(
            importer.local_names(&heap).iter().any(|name| name == "f.1"),
            "`f.1` is the program's import, not an alias: {:?}",
            importer.local_names(&heap)
        );
        let alias = importer.import_alias(&heap, "f", mint);
        assert!(
            alias.as_deref().is_some_and(|alias| alias != "f.1"),
            "a fresh alias, not the program's `f.1`: {alias:?}"
        );
        assert_eq!(importer.get(&heap, "f.1"), Some(n(4)));
        assert_eq!(importer.get(&heap, &alias.unwrap()), Some(n(4)));
        assert_eq!(importer.recount(&heap.borrow()), importer.counted());
    }

    /// Both spellings defined over: the alias leaves its cell through
    /// `define_record`'s re-point.
    #[test]
    fn an_alias_key_never_meets_a_cell_made_in_its_cells_slot() {
        reuse_case(|heap, importer| {
            importer.define(heap, "f", n(2));
            importer.define(heap, "f.1", n(3));
        });
    }

    /// Both spellings imported over: the alias leaves its cell through
    /// `bind_cell`'s re-point.
    #[test]
    fn an_alias_imported_over_leaves_no_key_either() {
        reuse_case(|heap, importer| {
            let other = Rc::new(Environment::with_heap(heap));
            other.define(heap, "g", n(5));
            share(heap, importer, "f", &other, "g");
            share(heap, importer, "f.1", &other, "g");
        });
    }
}

/// No read of a namespace answers a record or a cell (#834, critique D2): a
/// namespace's every binding is a record over a cell, and every path that
/// reads one dereferences it, whatever kind of binding it is. Each kind
/// holds a fixnum of its own, so an answer that is a record, a cell or
/// another binding's value fails by value.
#[cfg(test)]
mod record_read_tests {
    use super::*;
    use crate::NamespaceView;
    use crate::heap::new_shared_heap;
    use crate::library::Library;

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    fn scopes(ids: &[usize]) -> ScopeSet {
        let mut set = ScopeSet::new();
        for id in ids {
            set.add_scope(ScopeId(*id));
        }
        set
    }

    #[test]
    fn no_namespace_read_answers_a_record() {
        let heap = new_shared_heap();
        // Where the imports come from, and where the two macro-expansion
        // aliases point.
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "lib", n(2));
        library.define(&heap, "aliased-lib", n(8));
        let other = Rc::new(Environment::with_heap(&heap));
        other.define(&heap, "target", n(6));
        other.define_with_scopes(&heap, "deep", scopes(&[7]), n(7));
        let mut exporting = Library::with_env(vec!["t13".to_string()], Rc::clone(&library));
        exporting.export_value(&heap, "free".to_string(), n(9));

        let ns = Rc::new(Environment::with_heap(&heap));
        // Its own binding, and an import.
        ns.define(&heap, "own", n(1));
        share(&heap, &ns, "imported", &library, "lib");
        // A visible scoped definition, and an invisible one: the tree-walker's
        // introduced definition.
        ns.define_scoped_definition(&heap, "visible", scopes(&[3]), n(3));
        ns.define_with_scopes(&heap, "hidden", scopes(&[4]), n(4));
        // The VM's introduced definition: its identity recorded at compile
        // time, its renamed global defined when the code runs.
        ns.define_introduced_global("renamed".into(), scopes(&[5]), "renamed #5".into());
        ns.define(&heap, "renamed #5", n(5));
        // A plain alias and a scoped one.
        ns.define_alias("plain-alias", Rc::clone(&other), "target".into());
        ns.define_scoped_alias(
            "scoped-alias",
            Rc::clone(&other),
            "deep".into(),
            scopes(&[7]),
        );
        // An import alias (#438), a second record over an import's cell.
        share(&heap, &ns, "aliased", &library, "aliased-lib");
        let alias = ns
            .import_alias(&heap, "aliased", || "aliased.1".into())
            .unwrap();
        // A free cell's export.
        assert!(exporting.import_into(&heap, &ns, "free", "free"));

        let by_name: Vec<(&str, i64)> = vec![
            ("own", 1),
            ("imported", 2),
            ("visible", 3),
            ("renamed #5", 5),
            ("plain-alias", 6),
            ("scoped-alias", 7),
            ("aliased", 8),
            (&*alias, 8),
            ("free", 9),
        ];
        let unscoped = ScopeSet::new();
        for &(name, value) in &by_name {
            let want = Some(n(value));
            assert_eq!(ns.get(&heap, name), want, "get {name}");
            assert_eq!(
                ns.get_with_scopes(&heap, name, &unscoped).unwrap(),
                want,
                "get_with_scopes {name}"
            );
            assert_eq!(
                ns.resolve_with_scopes(&heap, name, &unscoped).unwrap().0,
                want,
                "resolve_with_scopes {name}"
            );
            assert_eq!(
                NamespaceView::of(&ns).get(&heap, name),
                want,
                "NamespaceView::get {name}"
            );
            let (looked_up, resolved) = crate::NoGcScope::with_cx(&heap, |cx| {
                let looked_up = cx.lookup(&ns, name).map(|value| value.tagged());
                let resolved = cx
                    .resolve_with_scopes(&ns, name, &unscoped)
                    .unwrap()
                    .0
                    .map(|value| value.tagged());
                (looked_up, resolved)
            });
            assert_eq!(looked_up, want, "Cx::lookup {name}");
            assert_eq!(resolved, want, "Cx::resolve_with_scopes {name}");
        }

        // The scoped definitions, by their identities, and by a reference
        // that carries one more scope.
        for (name, identity, value) in [("visible", 3, 3), ("hidden", 4, 4)] {
            let want = Some(n(value));
            for reference in [scopes(&[identity]), scopes(&[identity, 40])] {
                assert_eq!(
                    ns.get_with_scopes(&heap, name, &reference).unwrap(),
                    want,
                    "get_with_scopes {name} {reference}"
                );
                assert_eq!(
                    ns.resolve_with_scopes(&heap, name, &reference).unwrap(),
                    (want, true),
                    "resolve_with_scopes {name} {reference}"
                );
                let resolved = crate::NoGcScope::with_cx(&heap, |cx| {
                    cx.resolve_with_scopes(&ns, name, &reference)
                        .unwrap()
                        .0
                        .map(|value| value.tagged())
                });
                assert_eq!(resolved, want, "Cx::resolve_with_scopes {name} {reference}");
            }
            assert_eq!(
                ns.scoped_definition_value(&heap, name, &scopes(&[identity])),
                want,
                "scoped_definition_value {name}"
            );
        }
        assert_eq!(ns.get(&heap, "hidden"), None, "not visible by name");

        // An alias's target, read as each walk reads it.
        let target = |alias: &str| ns.alias_target(alias).unwrap().get(&ns, &heap);
        assert_eq!(target("plain-alias"), Some(n(6)));
        assert_eq!(target("scoped-alias"), Some(n(7)));

        // By slot, as the VM reads.
        for (name, value) in [
            ("own", 1),
            ("imported", 2),
            ("renamed #5", 5),
            ("aliased", 8),
            (&*alias, 8),
            ("free", 9),
        ] {
            let slot = ns.local_slot(name).unwrap();
            let view = NamespaceView::of(&ns);
            assert_eq!(ns.slot_value(&heap, slot), n(value), "slot_value {name}");
            assert_eq!(
                ns.slot_value_in(&heap.borrow(), slot),
                n(value),
                "slot_value_in {name}"
            );
            assert_eq!(
                view.slot_value(&heap, slot),
                n(value),
                "NamespaceView::slot_value {name}"
            );
            assert_eq!(
                view.slot_value_in(&heap.borrow(), slot),
                n(value),
                "NamespaceView::slot_value_in {name}"
            );
        }

        // Enumerated, as an import set reads a library: the import alias is
        // bookkeeping and left out.
        let mut bindings = ns.bindings(&heap);
        bindings.sort_by(|a, b| a.0.cmp(&b.0));
        let expected: Vec<(String, TaggedValue)> = [
            ("aliased", 8),
            ("free", 9),
            ("imported", 2),
            ("own", 1),
            ("renamed #5", 5),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), n(value)))
        .collect();
        assert_eq!(bindings, expected);

        // The introduced definitions' records, the VM's and the
        // tree-walker's, each read through its cell.
        let renamed = ns.introduced_records(&heap, "renamed");
        assert_eq!(renamed.len(), 1, "{renamed:?}");
        assert_eq!(
            (&renamed[0].identity, renamed[0].scope, renamed[0].value),
            (&scopes(&[5]), Some(ScopeId(5)), n(5))
        );
        let hidden = ns.introduced_records(&heap, "hidden");
        assert_eq!(hidden.len(), 1, "{hidden:?}");
        assert_eq!(
            (&hidden[0].identity, hidden[0].scope, hidden[0].value),
            (&scopes(&[4]), Some(ScopeId(4)), n(4))
        );
        assert!(
            ns.introduced_records(&heap, "visible").is_empty(),
            "a visible definition is no introduced one"
        );

        // A library's export, read off its cell.
        assert_eq!(exporting.get_export_tagged(&heap, "free"), Some(n(9)));
        let exports: Vec<(String, TaggedValue)> = exporting
            .exports_iter_tagged(&heap)
            .map(|(name, value)| (name.clone(), value))
            .collect();
        assert_eq!(exports, [("free".to_string(), n(9))]);

        assert_eq!(ns.recount(&heap.borrow()), ns.counted());
    }
}

/// A namespace's charge on the heap's external bytes (#615): made, grown,
/// given back; and a frame's, which is nothing.
#[cfg(test)]
mod namespace_charge_tests {
    use super::*;
    use crate::heap::new_shared_heap;

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    fn external(heap: &SharedHeap) -> usize {
        heap.borrow().external_bytes()
    }

    #[test]
    fn a_namespace_charges_its_tables_as_they_grow_and_gives_them_back() {
        let heap = new_shared_heap();
        let env = Rc::new(Environment::with_heap(&heap));
        let made = external(&heap);
        assert!(made >= size_of::<Environment>() + size_of::<RareTables>());
        assert_eq!(env.charged_bytes(), Some(made));

        for i in 0..1000 {
            env.define(&heap, format!("name-{i}"), n(i));
        }
        let grown = external(&heap);
        // A slot and a name apiece at the least; the index is on top.
        let least = 1000 * (size_of::<(Rc<str>, TaggedValue)>() + RC_COUNTS + "name-0".len());
        assert!(grown >= made + least, "{made} then {grown}");
        assert_eq!(env.charged_bytes(), Some(grown));

        // Redefining a name writes its slot in place: no table grows.
        for i in 0..1000 {
            env.define(&heap, format!("name-{i}"), n(-i));
        }
        assert_eq!(external(&heap), grown);

        drop(env);
        assert_eq!(external(&heap), 0);
    }

    /// The heap counts a namespace while it lives, and its records and own
    /// cells with it (#834): made, +1; dropped, −1 and what it held. A frame
    /// is not a namespace, and its own bindings are not records.
    #[test]
    fn a_namespace_is_counted_while_it_lives() {
        let heap = new_shared_heap();
        let counts = || {
            let h = heap.borrow();
            (h.namespaces(), h.binding_records(), h.cells())
        };
        assert_eq!(counts(), (0, 0, 0));
        let ns = Rc::new(Environment::with_heap(&heap));
        assert_eq!(counts(), (1, 0, 0));
        let frame = Rc::new(Environment::child_frame(Rc::clone(&ns)));
        frame.define(&heap, "the frame's own", n(1));
        assert_eq!(counts(), (1, 0, 0), "a frame is counted nowhere");
        ns.define(&heap, "x", n(2));
        assert_eq!(counts(), (1, 1, 1));
        drop(frame);
        assert_eq!(counts(), (1, 1, 1));
        drop(ns);
        assert_eq!(counts(), (0, 0, 0));
    }

    /// A host namespace is counted while it lives, as a parentless one is
    /// (the owner's ruling (a) of 2026-11-07 on #835's question 2): made,
    /// +1; a define, a record and a cell; a read-through record, a record,
    /// its table charged; its adoption by a define, a cell; dropped, every
    /// count and its charge back where they started.
    #[test]
    fn a_host_namespace_is_counted_while_it_lives() {
        let heap = new_shared_heap();
        let counts = || {
            let h = heap.borrow();
            (h.namespaces(), h.binding_records(), h.cells())
        };
        let global = Rc::new(Environment::with_heap(&heap));
        global.define(&heap, "x", n(1));
        let (namespaces, records, cells) = counts();
        let start = external(&heap);

        let host = Rc::new(Environment::host_namespace(Rc::clone(&global)));
        assert_eq!(counts(), (namespaces + 1, records, cells));
        assert!(external(&heap) > start, "charged for its tables");
        host.define(&heap, "own", n(2));
        assert_eq!(counts(), (namespaces + 1, records + 1, cells + 1));

        let unlinked = host.charged_bytes().unwrap();
        let child = host.link_record(&heap, "x").unwrap();
        assert_eq!(counts(), (namespaces + 1, records + 2, cells + 1));
        assert!(
            host.charged_bytes().unwrap() >= unlinked + size_of::<RefCell<ReadThrough>>(),
            "the read-through table is charged"
        );
        assert_eq!(host.link_record(&heap, "x"), Some(child));
        assert_eq!(counts(), (namespaces + 1, records + 2, cells + 1));

        host.define(&heap, "x", n(3));
        assert_eq!(host.local_record("x"), Some(child), "adopted");
        assert_eq!(counts(), (namespaces + 1, records + 2, cells + 2));
        assert_eq!(host.recount(&heap.borrow()), host.counted());

        drop(host);
        assert_eq!(counts(), (namespaces, records, cells));
        assert_eq!(external(&heap), start);
    }

    /// A set of one scope, `id`: the identity of one expansion's definition.
    fn one_scope(id: usize) -> ScopeSet {
        let mut scopes = ScopeSet::new();
        scopes.add_scope(crate::scope::ScopeId(id));
        scopes
    }

    /// What an index of `positions` bindings, each filed under a key of its
    /// own, holds at the least, each buffer on its own: the heads map, a
    /// table of at least that capacity, and the previous vector, a link per
    /// position.
    fn least_index(positions: usize) -> usize {
        hash_table_bytes::<crate::scope::ScopeId, u32>(positions) + positions * size_of::<u32>()
    }

    /// The running sum of a namespace's spellings' buffers.
    fn entries(env: &Environment) -> usize {
        let charge = env.rare.get().and_then(|rare| rare.namespace.as_ref());
        charge.expect("a namespace with an account").entries.get()
    }

    /// The same, by a walk of every spelling, which the sum spares the
    /// recharges.
    fn walked(env: &Environment) -> usize {
        let lists: usize = env.scoped_bindings.borrow().values().map(list_bytes).sum();
        let Some(rare) = env.rare.get() else {
            return lists;
        };
        let indexes: usize = rare
            .scoped_index
            .borrow()
            .values()
            .map(ScopeIndex::buffer_bytes)
            .sum();
        let introduced: usize = rare
            .introduced_global_names
            .borrow()
            .values()
            .map(Introduced::buffer_bytes)
            .sum();
        lists + indexes + introduced
    }

    /// The tree-walker binds a macro's introduced definition under the
    /// expansion's scopes, so each expansion of `hidden-define`'s macro adds
    /// one binding to the spelling's list, and a position to its index
    /// (#833). The namespace charges both as they grow, and gives them back.
    #[test]
    fn a_namespace_charges_a_spelling_s_scoped_list_and_its_index() {
        let heap = new_shared_heap();
        let env = Rc::new(Environment::with_heap(&heap));
        let made = external(&heap);
        for i in 0..1000 {
            env.define_scoped_definition(&heap, "state", one_scope(100 + i), n(i as i64));
        }
        let grown = external(&heap);
        let list = env.scoped_bindings.borrow()["state"].capacity() * size_of::<ScopedBinding>();
        assert!(list >= 1000 * size_of::<ScopedBinding>());
        let least = list + least_index(1000);
        assert!(
            grown >= made + least,
            "{made} then {grown}, at least {least} more"
        );
        assert_eq!(env.charged_bytes(), Some(grown));

        // Bound again at the same scopes, a binding is written in place.
        for i in 0..1000 {
            env.define_scoped_definition(&heap, "state", one_scope(100 + i), n(-(i as i64)));
        }
        assert_eq!(external(&heap), grown);

        drop(env);
        assert_eq!(external(&heap), 0);
    }

    /// The VM renames each expansion's definition to a fresh global and
    /// records its identity under the spelling (`Introduced`), indexed past
    /// `INDEX_ABOVE`. The namespace charges the identities and the index as
    /// they grow, and gives them back.
    #[test]
    fn a_namespace_charges_its_introduced_globals() {
        let heap = new_shared_heap();
        let env = Rc::new(Environment::with_heap(&heap));
        let made = external(&heap);
        let record = |i: usize, renamed: &str| {
            let renamed = Rc::from(format!("{renamed}.{i}"));
            env.define_introduced_global(Rc::from("state"), one_scope(100 + i), renamed);
        };
        for i in 0..1000 {
            record(i, "state");
        }
        let grown = external(&heap);
        let least =
            1000 * size_of::<(ScopeSet, Rc<str>)>() + size_of::<ScopeIndex>() + least_index(1000);
        assert!(
            grown >= made + least,
            "{made} then {grown}, at least {least} more"
        );
        assert_eq!(env.charged_bytes(), Some(grown));

        // Recorded again at the same scopes, an identity is overwritten.
        for i in 0..1000 {
            record(i, "again");
        }
        assert_eq!(external(&heap), grown);

        drop(env);
        assert_eq!(external(&heap), 0);
    }

    /// The running sum is what a walk of the spellings finds, and the
    /// charge what the tables hold, after every write: spellings of one
    /// binding, whose list is inline, of a few, of one past the namespace's
    /// threshold, where the index is built, and of many, each bound by both
    /// scoped writers and recorded as introduced globals, with redefinitions
    /// between.
    #[test]
    fn the_running_sum_is_what_the_tables_hold() {
        let heap = new_shared_heap();
        let env = Rc::new(Environment::with_heap(&heap));
        assert_eq!(entries(&env), 0);
        let sizes = [1, 2, 5, INDEX_ABOVE, INDEX_ABOVE + 1, 40, 300];
        for (k, size) in sizes.into_iter().enumerate() {
            for i in 0..size {
                let scopes = one_scope(1000 * (k + 1) + i);
                let value = n(i as i64);
                env.define_with_scopes(&heap, format!("p{k}"), scopes.clone(), value);
                env.define_scoped_definition(&heap, format!("d{k}"), scopes.clone(), value);
                let renamed = Rc::from(format!("g{k}.{i}"));
                env.define_introduced_global(Rc::from(format!("g{k}")), scopes.clone(), renamed);
                if i % 3 == 0 {
                    env.define_with_scopes(&heap, format!("p{k}"), scopes.clone(), n(-1));
                    env.define_introduced_global(Rc::from(format!("g{k}")), scopes, "again".into());
                }
                assert_eq!(entries(&env), walked(&env), "spelling {k}, binding {i}");
                // And the charge settled it: what the tables hold now, a
                // spelling's list that grew alone among them. No slot is
                // defined here, so the names do not trail.
                let charge = env.rare.get().and_then(|rare| rare.namespace.as_ref());
                let now = env.table_bytes(charge.expect("an account"));
                assert_eq!(env.charged_bytes(), Some(now), "spelling {k}, binding {i}");
            }
        }
        assert!(entries(&env) > 0);
        // A plain definition, and a scoped one at the empty set, which is
        // stored by name, grow no spelling's buffers.
        env.define(&heap, "plain", n(1));
        env.define_with_scopes(&heap, "bare", ScopeSet::new(), n(2));
        assert_eq!(entries(&env), walked(&env));
    }

    /// An `environment` is built by import: what it holds is its slots,
    /// their names and the index — each charged to the importer, none to
    /// the library. Each import's record is a pair, charged in the live
    /// bytes and not here (#834).
    #[test]
    fn an_imported_binding_charges_the_importer() {
        let heap = new_shared_heap();
        let library = Rc::new(Environment::with_heap(&heap));
        for i in 0..300 {
            library.define(&heap, format!("export-{i}"), n(i));
        }
        let library_charge = library.charged_bytes().unwrap();
        let importer = Rc::new(Environment::with_heap(&heap));
        let before = importer.charged_bytes().unwrap();
        for i in 0..300 {
            let name = format!("export-{i}");
            share(&heap, &importer, name.as_str(), &library, &name);
        }
        let after = importer.charged_bytes().unwrap();
        let least = 300 * size_of::<(Rc<str>, TaggedValue)>();
        assert!(after >= before + least, "{before} then {after}");
        assert_eq!(library.charged_bytes(), Some(library_charge));
        assert_eq!(external(&heap), library_charge + after);

        drop(importer);
        assert_eq!(external(&heap), library_charge);
    }

    /// The tree-walker builds a frame per call and per `let`-bound
    /// temporary; none of them is a namespace, whatever is bound in it.
    /// Read while each frame is alive, with every table it has grown: a
    /// frame that charged and gave the charge back when it dropped would
    /// leave the account where it started all the same.
    #[test]
    fn a_frame_charges_nothing() {
        let heap = new_shared_heap();
        let global = Rc::new(Environment::with_heap(&heap));
        let start = external(&heap);
        let mut scopes = ScopeSet::new();
        scopes.add_scope(crate::scope::ScopeId(3));
        for depth in 0..100 {
            let frame = Rc::new(Environment::child_frame(Rc::clone(&global)));
            for i in 0..depth {
                frame.define(&heap, format!("v{i}"), n(i));
                frame.define_with_scopes(&heap, format!("s{i}"), scopes.clone(), n(i));
            }
            frame.define_alias("alias", Rc::clone(&global), Rc::from("x"));
            assert_eq!(frame.charged_bytes(), None);
            assert!(!frame.is_namespace());
            assert_eq!(external(&heap), start, "a frame of {depth}");
        }

        // A frame that grows a spelling's buffers: one spelling bound past a
        // frame's threshold, so that it is indexed, and recorded as
        // introduced globals, as the VM recorded them in a host's
        // environment while that was a frame (until #835). A frame sums
        // none of them (#833).
        let frame = Rc::new(Environment::child_frame(Rc::clone(&global)));
        for k in 0..=FRAME_INDEX_ABOVE {
            let mut scopes = ScopeSet::new();
            scopes.add_scope(crate::scope::ScopeId(100 + k));
            frame.define_with_scopes(&heap, "deep", scopes.clone(), n(k as i64));
            let renamed = Rc::from(format!("deep.{k}"));
            frame.define_introduced_global(Rc::from("deep"), scopes, renamed);
        }
        let rare = frame
            .rare
            .get()
            .expect("an indexed frame has the rare tables");
        assert!(rare.scoped_index.borrow().contains_key("deep"));
        assert!(frame.scoped_bindings.borrow()["deep"].spilled());
        assert_eq!(frame.charged_bytes(), None);
        assert_eq!(external(&heap), start, "a frame of one indexed spelling");
    }

    /// The kind is the constructor's (4b's start decision 4 on #782): the
    /// namespace constructors make parentless namespaces, the host
    /// namespace's makes a namespace whose parent is a parentless one
    /// (#835), and the frame constructors make frames, which have a parent.
    /// What answers to the kind answers as it did when it read the parent: a
    /// namespace of either kind charges its heap's account, which every
    /// parentless namespace has since #834. `frame_kind` names each frame
    /// and no namespace, as the VM's refusal reads it (#835).
    #[test]
    fn each_constructor_sets_its_kind() {
        let heap = new_shared_heap();
        let global = Rc::new(Environment::with_heap(&heap));
        let in_hand = Rc::new(Environment::namespace(&heap.borrow()));
        for namespace in [&global, &in_hand] {
            assert_eq!(namespace.kind, Kind::Namespace);
            assert!(namespace.is_namespace() && !namespace.holds_temporaries());
            assert_eq!(namespace.frame_kind(), None);
            assert!(namespace.parent().is_none());
        }
        assert!(global.charged_bytes().is_some() && in_hand.charged_bytes().is_some());

        let frame = Environment::child_frame(Rc::clone(&global));
        assert_eq!(frame.kind, Kind::Frame);
        assert!(!frame.is_namespace() && !frame.holds_temporaries());
        assert_eq!(frame.frame_kind(), Some("frame"));
        let temporaries = Environment::for_temporaries(Rc::clone(&global));
        assert_eq!(temporaries.kind, Kind::Temporaries);
        assert!(!temporaries.is_namespace() && temporaries.holds_temporaries());
        assert_eq!(temporaries.frame_kind(), Some("frame of temporaries"));
        for frame in [&frame, &temporaries] {
            assert!(
                frame
                    .parent()
                    .is_some_and(|parent| Rc::ptr_eq(parent, &global))
            );
            assert_eq!(frame.charged_bytes(), None);
        }

        let host = Environment::host_namespace(Rc::clone(&global));
        assert_eq!(host.kind, Kind::HostNamespace);
        assert!(host.is_namespace() && !host.holds_temporaries());
        assert_eq!(host.frame_kind(), None);
        assert!(
            host.parent()
                .is_some_and(|parent| Rc::ptr_eq(parent, &global))
        );
        assert_eq!(host.heap_id(), global.heap_id(), "its parent's heap");
        assert!(
            host.rare.get().is_some(),
            "its rare tables are built eagerly"
        );
        // Charged, as a parentless namespace is (the owner's ruling (a) of
        // 2026-11-07 on #835's question 2).
        assert!(host.charged_bytes().is_some());
    }

    /// A host namespace's parent is a parentless namespace (#835): a frame,
    /// a frame of temporaries and another host namespace are refused.
    #[test]
    #[should_panic(expected = "a host namespace's parent is a parentless namespace")]
    fn a_host_namespace_over_a_frame_is_refused() {
        let heap = new_shared_heap();
        let global = Rc::new(Environment::with_heap(&heap));
        let frame = Rc::new(Environment::child_frame(global));
        Environment::host_namespace(frame);
    }

    #[test]
    #[should_panic(expected = "a host namespace's parent is a parentless namespace")]
    fn a_host_namespace_over_a_frame_of_temporaries_is_refused() {
        let heap = new_shared_heap();
        let global = Rc::new(Environment::with_heap(&heap));
        let temporaries = Rc::new(Environment::for_temporaries(global));
        Environment::host_namespace(temporaries);
    }

    #[test]
    #[should_panic(expected = "a host namespace's parent is a parentless namespace")]
    fn a_host_namespace_over_a_host_namespace_is_refused() {
        let heap = new_shared_heap();
        let global = Rc::new(Environment::with_heap(&heap));
        let host = Rc::new(Environment::host_namespace(global));
        Environment::host_namespace(host);
    }

    /// The estimate a tree-walker closure is charged for its frame (#637) is
    /// the frame it describes: one the tree-walker builds for a call of one
    /// parameter, bound under its scopes, measured as a namespace's tables
    /// are. Its slot vector is inline and it has no index, aliases or rare
    /// tables, so the frame and its scoped table are all it holds.
    #[test]
    fn captured_frame_bytes_is_a_one_parameter_frame() {
        let heap = new_shared_heap();
        let global = Rc::new(Environment::with_heap(&heap));
        let frame = Environment::child_frame(global);
        let mut scopes = ScopeSet::new();
        scopes.add_scope(crate::scope::ScopeId(3));
        frame.define_with_scopes(&heap, "x", scopes, n(1));

        let bindings = frame.bindings.borrow();
        assert!(!bindings.slots.spilled());
        assert!(bindings.index.is_none());
        assert_eq!(frame.alias_bindings.borrow().capacity(), 0);
        assert!(frame.rare.get().is_none());
        let measured = RC_COUNTS
            + size_of::<Environment>()
            + hash_table_bytes::<Rc<str>, ScopedBindingList>(
                frame.scoped_bindings.borrow().capacity(),
            );
        assert_eq!(measured, CAPTURED_FRAME_BYTES);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            CAPTURED_FRAME_BYTES, 612,
            "the frame changed size: update the number CAPTURED_FRAME_BYTES's comment gives"
        );
    }

    /// However many specifiers name a namespace, it is charged once; and its
    /// mutable specifier is one object.
    #[test]
    fn a_namespace_is_charged_once_however_many_specifiers_name_it() {
        let heap = new_shared_heap();
        let env = Rc::new(Environment::with_heap(&heap));
        let made = external(&heap);
        let first = env.mutable_specifier(&heap);
        for _ in 0..10 {
            assert_eq!(env.mutable_specifier(&heap), first);
            heap.borrow_mut()
                .alloc_environment_specifier(Rc::clone(&env), false);
        }
        assert_eq!(external(&heap), made);
    }
}

/// A host namespace's read-through records (#835): made only by
/// [`Environment::link_record`], never in a slot, reading through the
/// parent's record to whatever it names now, and adopted by a define or an
/// import in the host namespace. Until 4b.9's linker calls `link_record`,
/// these tests are its only callers.
#[cfg(test)]
mod read_through_tests {
    use super::*;
    use crate::NamespaceView;
    use crate::heap::new_shared_heap;
    use crate::heap::trace_sentinels::collect_only;

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    /// The global namespace binding `x` to 1, and a host namespace over it.
    fn global_and_host(heap: &SharedHeap) -> (Rc<Environment>, Rc<Environment>) {
        let global = Rc::new(Environment::with_heap(heap));
        global.define(heap, "x", n(1));
        let host = Rc::new(Environment::host_namespace(Rc::clone(&global)));
        (global, host)
    }

    /// What `record` resolves to now.
    fn resolved(heap: &SharedHeap, record: Record) -> TaggedValue {
        let h = heap.borrow();
        record.resolved_cell(&h).value(&h)
    }

    /// The host namespace's read-through table, if a linker made it.
    fn table(host: &Environment) -> Option<&RefCell<ReadThrough>> {
        host.rare
            .get()
            .and_then(|rare| rare.read_through.get())
            .map(|table| &**table)
    }

    fn holds(host: &Environment, name: &str) -> bool {
        table(host).is_some_and(|table| table.borrow().records.contains_key(name))
    }

    /// A parent's later store, import and define are each seen through the
    /// child record, which reads through the parent's one record the whole
    /// time: a store into its cell, and a re-point of it.
    #[test]
    fn a_read_through_record_reads_the_parents_later_define_and_import() {
        let heap = new_shared_heap();
        let (global, host) = global_and_host(&heap);
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "x", n(100));
        let child = host.link_record(&heap, "x").expect("the global binds x");
        let parent = global.local_record("x").unwrap();
        let reads_through_parent = || {
            let h = heap.borrow();
            child.parent_record(&h).map(Record::key) == Some(parent.key())
        };
        assert_eq!(resolved(&heap, child), n(1));
        assert!(reads_through_parent());

        // A store into the global's own cell.
        global.define(&heap, "x", n(2));
        assert_eq!(resolved(&heap, child), n(2));
        assert!(reads_through_parent());

        // An import over the definition: the global's record names the
        // library's cell.
        share(&heap, &global, "x", &library, "x");
        assert_eq!(resolved(&heap, child), n(100));
        assert!(reads_through_parent());

        // A definition over the import: a fresh cell of the global's own.
        global.define(&heap, "x", n(3));
        assert_eq!(resolved(&heap, child), n(3));
        assert_eq!(library.get(&heap, "x"), Some(n(100)));
        assert!(reads_through_parent());
        assert_eq!(global.local_record("x"), Some(parent), "one record");
    }

    /// A define in the host namespace adopts the read-through record: the
    /// same word, now a record of its own in a slot over a fresh cell. The
    /// global's record and cell are left alone, and the table lets go.
    #[test]
    fn a_child_define_repoints_the_read_through_record_and_leaves_the_parents() {
        let heap = new_shared_heap();
        let (global, host) = global_and_host(&heap);
        let global_cell = global.cell_of(&heap, "x").unwrap();
        let child = host.link_record(&heap, "x").unwrap();
        assert_eq!(host.counted(), (1, 0), "a record, over no cell of its own");

        host.define(&heap, "x", n(9));
        assert_eq!(
            host.local_record("x"),
            Some(child),
            "the record keeps its word"
        );
        {
            let h = heap.borrow();
            assert_eq!(child.parent_record(&h), None);
            assert!(child.cell(&h).owned_by(&h, host.env_id()));
            let parents = global.local_record("x").unwrap();
            assert_eq!(parents.cell(&h), global_cell);
            assert_eq!(global_cell.value(&h), n(1));
        }
        assert_eq!(resolved(&heap, child), n(9));
        assert_eq!(host.get(&heap, "x"), Some(n(9)));
        assert_eq!(global.get(&heap, "x"), Some(n(1)));
        assert!(!holds(&host, "x"), "the table let go of it");
        assert_eq!(host.counted(), (1, 1));
        assert_eq!(host.recount(&heap.borrow()), host.counted());
    }

    /// An import in the host namespace adopts the read-through record over
    /// the exporter's cell: what the library stores later the host reads,
    /// and the global's binding of the name is left alone.
    #[test]
    fn a_child_import_repoints_the_read_through_record_to_the_exporters_cell() {
        let heap = new_shared_heap();
        let (global, host) = global_and_host(&heap);
        global.define(&heap, "y", n(2));
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "y", n(20));
        let exported = library.cell_of(&heap, "y").unwrap();
        let child = host.link_record(&heap, "y").unwrap();

        host.import_cell(&heap, "y", exported);
        assert_eq!(host.local_record("y"), Some(child));
        assert_eq!(child.cell(&heap.borrow()), exported);
        assert!(!holds(&host, "y"));
        assert_eq!(host.get(&heap, "y"), Some(n(20)));

        library.define(&heap, "y", n(21));
        assert_eq!(
            host.get(&heap, "y"),
            Some(n(21)),
            "the library's later store"
        );
        assert_eq!(resolved(&heap, child), n(21));
        assert_eq!(global.get(&heap, "y"), Some(n(2)));
        assert_eq!(host.counted(), (1, 0), "an import makes no cell");
        assert_eq!(host.recount(&heap.borrow()), host.counted());
    }

    /// An assignment through a read-through record lands in its parent's
    /// cell, the location `resolved_cell` names; and an assignment by name
    /// in the host namespace, which does not bind the name, lands there too.
    #[test]
    fn a_set_through_a_read_through_record_stores_into_the_parents_cell() {
        let heap = new_shared_heap();
        let (global, host) = global_and_host(&heap);
        let child = host.link_record(&heap, "x").unwrap();
        {
            let mut h = heap.borrow_mut();
            let cell = child.resolved_cell(&h);
            cell.store(&mut h, n(5), Site::EnvSetSlot);
        }
        assert_eq!(global.get(&heap, "x"), Some(n(5)));

        host.set(&heap, "x", n(6)).unwrap();
        assert_eq!(resolved(&heap, child), n(6));
        assert_eq!(global.get(&heap, "x"), Some(n(6)));
        assert_eq!(host.local_slot("x"), None, "a set binds nothing here");
        assert!(holds(&host, "x"));
    }

    /// A read makes no record: none of the reads by name of a name only the
    /// parent binds makes the table, or counts a record.
    #[test]
    fn a_read_makes_no_read_through_record() {
        let heap = new_shared_heap();
        let (_global, host) = global_and_host(&heap);
        let records = heap.borrow().binding_records();
        let mut scopes = ScopeSet::new();
        scopes.add_scope(crate::scope::ScopeId(7));

        assert_eq!(host.get(&heap, "x"), Some(n(1)));
        assert_eq!(
            host.get_with_scopes(&heap, "x", &scopes).unwrap(),
            Some(n(1))
        );
        assert!(host.binding_location(&heap, "x").is_some());
        assert_eq!(host.cell_of(&heap, "x"), None, "through the parent");
        assert!(host.has("x"));
        assert!(host.local_names(&heap).is_empty());
        assert!(NamespaceView::of(&host).get(&heap, "x").is_some());

        assert!(table(&host).is_none(), "no read made the table");
        assert_eq!(heap.borrow().binding_records(), records);
        assert_eq!(host.counted(), (0, 0));
    }

    /// One read-through record per name, however often it is asked for. An
    /// own name answers its own record and a frame `None`. A name neither
    /// namespace binds answers a weak read-through record over the global
    /// namespace's placeholder, and a parentless namespace asked for a name
    /// it does not bind its placeholder (#836): each the same one when asked
    /// again, and in no slot.
    #[test]
    fn link_record_answers_one_record_per_name() {
        let heap = new_shared_heap();
        let (global, host) = global_and_host(&heap);

        let nowhere = host
            .link_record(&heap, "nowhere")
            .expect("a weak child over the global's placeholder (#836)");
        assert_eq!(host.link_record(&heap, "nowhere"), Some(nowhere));
        assert!(
            table(&host).is_some_and(|table| table.borrow().records.is_empty()),
            "the weak child names the table's cell and is not in the table"
        );
        assert_eq!(host.local_slot("nowhere"), None);
        assert_eq!(global.local_slot("nowhere"), None);

        host.define(&heap, "own", n(2));
        let own = host.local_record("own").unwrap();
        assert_eq!(host.link_record(&heap, "own"), Some(own));
        assert!(
            table(&host).is_some_and(|table| table.borrow().records.is_empty()),
            "an own name makes no child"
        );

        let first = host.link_record(&heap, "x").unwrap();
        let second = host.link_record(&heap, "x").unwrap();
        assert_eq!(first, second);
        assert_eq!(first.key(), second.key());
        assert_eq!(table(&host).unwrap().borrow().records.len(), 1);
        assert_eq!(
            host.counted(),
            (2, 1),
            "own's record and cell, and x's record"
        );
        assert_eq!(host.recount(&heap.borrow()), host.counted());

        let frame = Environment::child_frame(Rc::clone(&global));
        assert_eq!(frame.link_record(&heap, "x"), None, "a frame links nothing");
        let missing = global
            .link_record(&heap, "missing")
            .expect("a placeholder (#836)");
        assert_eq!(global.link_record(&heap, "missing"), Some(missing));
        assert_eq!(global.local_slot("missing"), None, "in no slot");
        assert_eq!(global.link_record(&heap, "x"), global.local_record("x"));
    }

    /// A name the host namespace reaches by an alias or a visible scoped
    /// definition of its own, over a name the global namespace binds in a
    /// slot, is left to be read by name: `get` finds either before the
    /// parent's binding, and a read-through record would read the parent's.
    #[test]
    fn link_record_leaves_a_host_alias_or_scoped_definition_to_be_read_by_name() {
        let heap = new_shared_heap();
        let (global, host) = global_and_host(&heap);
        global.define(&heap, "y", n(2));
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "target", n(100));

        host.define_alias("x", Rc::clone(&library), Rc::from("target"));
        assert_eq!(host.get(&heap, "x"), Some(n(100)), "the alias first");
        assert_eq!(host.link_record(&heap, "x"), None, "an alias here");

        let mut scopes = ScopeSet::new();
        scopes.add_scope(crate::scope::ScopeId(7));
        host.define_scoped_definition(&heap, "y", scopes, n(20));
        assert_eq!(host.get(&heap, "y"), Some(n(20)), "the scoped one first");
        assert_eq!(
            host.link_record(&heap, "y"),
            None,
            "a visible scoped definition here"
        );

        assert!(table(&host).is_none(), "neither made a record");
    }

    /// The table is the host namespace's alone: once it drops, a collection
    /// rooted at the global namespace frees the read-through record and the
    /// cell it named, and keeps the global's record.
    #[test]
    fn a_read_through_record_dies_with_its_host_namespace() {
        let heap = new_shared_heap();
        let (global, host) = global_and_host(&heap);
        let child = host.link_record(&heap, "x").unwrap();
        let unbound = table(&host).unwrap().borrow().unbound;
        let parent = global.local_record("x").unwrap();
        assert_eq!(heap.borrow().binding_records(), 2);
        drop(host);
        assert_eq!(
            heap.borrow().binding_records(),
            1,
            "given back with the host"
        );

        collect_only(&mut heap.borrow_mut(), |visitor| visitor.env(&global));
        let h = heap.borrow();
        let free = h.free_pair_slots();
        assert!(
            free.contains(&child.word().heap_index()),
            "the child record"
        );
        assert!(free.contains(&unbound.word().heap_index()), "its cell");
        assert!(!free.contains(&parent.word().heap_index()));
        assert_eq!(parent.value(&h), n(1));
    }

    /// A read-through record's car names no binding, and a check build
    /// refuses to read it as a record of its own.
    #[test]
    #[cfg(any(debug_assertions, feature = "gc-check"))]
    #[should_panic(expected = "read-through")]
    fn a_read_through_record_read_as_its_own_panics() {
        let heap = new_shared_heap();
        let (_global, host) = global_and_host(&heap);
        let child = host.link_record(&heap, "x").unwrap();
        let h = heap.borrow();
        let _ = child.cell(&h);
    }

    /// A read-through record is in no slot, so nothing that asks for this
    /// namespace's own bindings sees it: `local_slot`, which the VM's
    /// `define_global` and `resolve_primitive_calls` gate on, the name
    /// lists, and `cell_of`. A define puts it in a slot, with its word.
    #[test]
    fn a_read_through_record_is_no_own_binding() {
        let heap = new_shared_heap();
        let (_global, host) = global_and_host(&heap);
        let child = host.link_record(&heap, "x").unwrap();

        assert_eq!(host.local_slot("x"), None);
        assert_eq!(NamespaceView::of(&host).local_slot("x"), None);
        assert!(!host.local_names(&heap).iter().any(|name| name == "x"));
        assert!(!host.bindings(&heap).iter().any(|(name, _)| name == "x"));
        assert!(!host.cells(&heap).iter().any(|(name, _)| name == "x"));
        assert_eq!(host.cell_of(&heap, "x"), None);

        host.define(&heap, "x", n(1));
        let slot = host.local_slot("x").expect("a define binds it here");
        assert_eq!(host.bindings.borrow().read_slot(slot), child.word());
        assert_eq!(host.local_names(&heap), vec!["x".to_string()]);
    }
}

#[cfg(test)]
mod placeholder_tests {
    //! Placeholders and weak read-through records (#836): adopted by every
    //! bind, kept while linked, dropped by the sweep's pass once nothing
    //! links them, and counted in the heap's sums by their stores.
    use super::*;
    use crate::heap::new_shared_heap;
    use crate::heap::trace_sentinels::collect_only;
    use crate::link_table::LinkTable;

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    /// The heap's sums of binding records and cells.
    fn sums(heap: &SharedHeap) -> (usize, usize) {
        let h = heap.borrow();
        (h.binding_records(), h.cells())
    }

    /// What `env`'s placeholder store counts, if it has one.
    fn stored(env: &Environment) -> (usize, usize) {
        env.weak_entries().map_or((0, 0), PlaceholderStore::counted)
    }

    /// Whether `env`'s placeholder store holds `name`.
    fn in_store(env: &Environment, name: &str) -> bool {
        env.weak_entries()
            .is_some_and(|store| store.get(name).is_some())
    }

    /// Whether `host`'s table of strong read-through records holds `name`.
    fn in_read_through(host: &Environment, name: &str) -> bool {
        host.rare
            .get()
            .and_then(|rare| rare.read_through.get())
            .is_some_and(|table| table.borrow().records.contains_key(name))
    }

    /// What `env`'s placeholder store has charged, if it has one.
    fn store_charged(env: &Environment) -> usize {
        env.weak_entries().map_or(0, PlaceholderStore::charged)
    }

    /// What a store charges for holding `name`: an `Rc<str>`'s two counts
    /// and its bytes. An entry taken out gives back exactly these, at once,
    /// since the store's table keeps its capacity.
    fn name_bytes(name: &str) -> usize {
        RC_COUNTS + name.len()
    }

    /// Every count agrees: each namespace's running sums with its tables,
    /// each store's with its entries, the heap's sums with theirs, and the
    /// heap's external bytes with what the namespaces and their stores have
    /// charged. `envs` is every namespace alive on `heap`.
    fn assert_counts_agree(heap: &SharedHeap, envs: &[&Environment]) {
        let h = heap.borrow();
        let mut total = (0, 0);
        let mut charged = 0;
        for env in envs {
            assert_eq!(env.recount(&h), env.counted(), "a namespace's sums");
            if let Some(store) = env.weak_entries() {
                assert_eq!(store.recount(&h), store.counted(), "a store's sums");
            }
            let ((records, cells), (in_store, its_cells)) = (env.counted(), stored(env));
            total = (total.0 + records + in_store, total.1 + cells + its_cells);
            charged += env.charged_bytes().expect("a namespace") + store_charged(env);
        }
        assert_eq!(
            (h.binding_records(), h.cells()),
            total,
            "the heap's sums are the namespaces' and their stores'"
        );
        assert_eq!(
            h.external_bytes(),
            charged,
            "the heap's external bytes are the namespaces' and their stores' charges"
        );
    }

    /// Whether the pair `word` named was freed.
    fn freed(heap: &SharedHeap, word: TaggedValue) -> bool {
        heap.borrow().free_pair_slots().contains(&word.heap_index())
    }

    /// The global namespace binding `x` to 1, and a host namespace over it.
    fn global_and_host(heap: &SharedHeap) -> (Rc<Environment>, Rc<Environment>) {
        let global = Rc::new(Environment::with_heap(heap));
        global.define(heap, "x", n(1));
        let host = Rc::new(Environment::host_namespace(Rc::clone(&global)));
        (global, host)
    }

    /// What `record` resolves to now.
    fn resolved(heap: &SharedHeap, record: Record) -> TaggedValue {
        let h = heap.borrow();
        record.resolved_cell(&h).value(&h)
    }

    /// A define adopts a placeholder: its word, in a fresh slot, flags 0,
    /// over a fresh cell of the namespace's own holding the value, so code
    /// that linked it reads the binding from now on. The store lets go of a
    /// record and a cell and the namespace counts them, so the heap's sums
    /// do not move, and gives back its name's bytes at once. Bound, it is
    /// never dropped: a collection rooted at the namespace alone keeps it,
    /// and frees the placeholder's own cell.
    #[test]
    fn a_define_adopts_a_placeholder_and_keeps_its_word() {
        let heap = new_shared_heap();
        let ns = Rc::new(Environment::with_heap(&heap));
        let placeholder = ns.link_record(&heap, "later").expect("a placeholder");
        let unbound = heap.borrow().car(placeholder.word());
        let before = sums(&heap);
        let (records, cells) = ns.counted();
        assert_eq!(stored(&ns), (1, 1));
        let store_before = store_charged(&ns);

        ns.define(&heap, "later", n(5));
        assert_eq!(
            store_before - store_charged(&ns),
            name_bytes("later"),
            "the store gives back its name's bytes"
        );
        assert_eq!(
            ns.local_record("later"),
            Some(placeholder),
            "its word, in a slot"
        );
        {
            let h = heap.borrow();
            assert_eq!(h.cdr(placeholder.word()), n(0), "its flag cleared");
            let cell = placeholder.cell(&h);
            assert!(cell.owned_by(&h, ns.env_id()));
            assert_ne!(cell.word(), unbound, "a fresh cell");
            assert_eq!(cell.value(&h), n(5));
        }
        assert_eq!(ns.get(&heap, "later"), Some(n(5)));
        assert!(!ns.holds_placeholder("later"));
        assert_eq!(stored(&ns), (0, 0), "the store let go of it");
        assert_eq!(sums(&heap), before, "moved from the store to the namespace");
        assert_eq!(ns.counted(), (records + 1, cells + 1));
        assert_counts_agree(&heap, &[&ns]);
        assert_eq!(ns.link_record(&heap, "later"), Some(placeholder));

        collect_only(&mut heap.borrow_mut(), |visitor| visitor.env(&ns));
        assert!(
            !freed(&heap, placeholder.word()),
            "a bound placeholder stays"
        );
        assert!(freed(&heap, unbound), "its own cell is garbage");
        assert_eq!(ns.get(&heap, "later"), Some(n(5)));
        assert_counts_agree(&heap, &[&ns]);
    }

    /// An import adopts a placeholder: its word, in a fresh slot, re-pointed
    /// at the exporter's cell, so what the library assigns later the code
    /// that linked it reads. The namespace counts a record and no cell, so
    /// the heap's cells fall by the placeholder's own. A cell of the
    /// namespace's own imported under a placeholder's name is copied, as
    /// `define` would, and adopts it the same way.
    #[test]
    fn an_import_adopts_a_placeholder_over_the_exporters_cell() {
        let heap = new_shared_heap();
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "v", n(20));
        let exported = library.cell_of(&heap, "v").unwrap();
        let ns = Rc::new(Environment::with_heap(&heap));
        let placeholder = ns.link_record(&heap, "later").expect("a placeholder");
        let (records, cells) = sums(&heap);
        let counted = ns.counted();
        let store_before = store_charged(&ns);

        ns.import_cell(&heap, "later", exported);
        assert_eq!(ns.local_record("later"), Some(placeholder));
        assert_eq!(
            store_before - store_charged(&ns),
            name_bytes("later"),
            "the store gives back its name's bytes"
        );
        {
            let h = heap.borrow();
            assert_eq!(h.cdr(placeholder.word()), n(0), "its flag cleared");
            assert_eq!(placeholder.cell(&h), exported);
        }
        library.define(&heap, "v", n(21));
        assert_eq!(ns.get(&heap, "later"), Some(n(21)), "the library's store");
        assert_eq!(stored(&ns), (0, 0));
        assert_eq!(sums(&heap), (records, cells - 1), "its own cell let go");
        assert_eq!(ns.counted(), (counted.0 + 1, counted.1));
        assert_counts_agree(&heap, &[&library, &ns]);

        ns.define(&heap, "own", n(7));
        let own = ns.cell_of(&heap, "own").unwrap();
        let copied = ns.link_record(&heap, "copied").expect("a placeholder");
        let store_before = store_charged(&ns);
        ns.import_cell(&heap, "copied", own);
        assert_eq!(ns.local_record("copied"), Some(copied));
        assert_eq!(store_before - store_charged(&ns), name_bytes("copied"));
        {
            let h = heap.borrow();
            assert_ne!(copied.cell(&h), own, "a copy");
            assert!(copied.cell(&h).owned_by(&h, ns.env_id()));
        }
        assert_eq!(ns.get(&heap, "copied"), Some(n(7)));
        assert_counts_agree(&heap, &[&library, &ns]);
    }

    /// A host namespace that links a name its parent does not bind (R7)
    /// gets a weak read-through record over the parent's placeholder, in its
    /// store and not its table, counted there, and the same one when asked
    /// again. Once the parent binds the name, adopting the placeholder, the
    /// record reads the binding through it, and the next link promotes it
    /// into the table, counted by the namespace from then. One of the two
    /// tables holds it at every step, never both (S9). The host namespace
    /// is charged the read-through table its first weak record makes for
    /// the cell it names, and each store gives back a name's bytes at once
    /// as the parent adopts its placeholder and as the record is promoted.
    /// A name the parent reaches by an alias gets neither a record nor a
    /// placeholder.
    #[test]
    fn a_host_links_a_weak_child_over_the_parents_placeholder_and_promotes_it() {
        let heap = new_shared_heap();
        let (global, host) = global_and_host(&heap);
        let (records, cells) = sums(&heap);
        let counted = (global.counted(), host.counted());
        let one_table = |name: &str| {
            assert!(
                !(in_read_through(&host, name) && in_store(&host, name)),
                "{name} in both of the host's tables"
            );
        };
        let unlinked = host.charged_bytes().unwrap();

        let child = host.link_record(&heap, "y").expect("a weak child");
        assert!(
            host.charged_bytes().unwrap() >= unlinked + size_of::<RefCell<ReadThrough>>(),
            "the read-through table is charged"
        );
        let placeholder = global
            .weak_entries()
            .and_then(|store| store.get("y"))
            .expect("the global's placeholder");
        {
            let h = heap.borrow();
            assert_eq!(child.parent_record(&h), Some(placeholder));
            assert!(placeholder.is_placeholder(&h));
        }
        assert!(in_store(&host, "y") && !in_read_through(&host, "y"), "weak");
        one_table("y");
        assert_eq!(
            sums(&heap),
            (records + 2, cells + 1),
            "the child, and the placeholder and its cell"
        );
        assert_eq!(
            (global.counted(), host.counted()),
            counted,
            "the stores count them"
        );
        assert_eq!(host.link_record(&heap, "y"), Some(child), "the same one");
        assert_counts_agree(&heap, &[&global, &host]);

        let global_store = store_charged(&global);
        global.define(&heap, "y", n(5));
        assert_eq!(global.local_record("y"), Some(placeholder), "adopted");
        assert_eq!(global_store - store_charged(&global), name_bytes("y"));
        assert_eq!(resolved(&heap, child), n(5), "read through it");
        assert!(in_store(&host, "y"), "still weak until linked again");
        one_table("y");
        assert_counts_agree(&heap, &[&global, &host]);

        let (host_records, host_cells) = host.counted();
        let host_store = store_charged(&host);
        assert_eq!(
            host.link_record(&heap, "y"),
            Some(child),
            "promoted, not remade"
        );
        assert_eq!(
            host_store - store_charged(&host),
            name_bytes("y"),
            "the host's store gives back the promoted record's name"
        );
        assert!(in_read_through(&host, "y") && !in_store(&host, "y"));
        one_table("y");
        assert_eq!(host.counted(), (host_records + 1, host_cells));
        assert_eq!(sums(&heap), (records + 2, cells + 1));
        assert_counts_agree(&heap, &[&global, &host]);

        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "target", n(100));
        global.define_alias("aliased", Rc::clone(&library), Rc::from("target"));
        assert_eq!(host.link_record(&heap, "aliased"), None, "read by name");
        assert!(!in_store(&global, "aliased") && !in_store(&host, "aliased"));
        assert_counts_agree(&heap, &[&global, &host, &library]);
    }

    /// #836's question 11, pending the owner's ruling: a weak read-through
    /// record whose parent placeholder was bound since stays weak until it
    /// is linked again, so a collection that no link table marks it in
    /// drops it, and keeps the parent's record, bound in its slot. The next
    /// link makes a strong record over that.
    #[test]
    fn an_unlinked_weak_child_over_a_parent_bound_since_is_dropped() {
        let heap = new_shared_heap();
        let (global, host) = global_and_host(&heap);
        let child = host.link_record(&heap, "z").expect("a weak child");
        global.define(&heap, "z", n(3));
        let parent = global.local_record("z").unwrap();

        collect_only(&mut heap.borrow_mut(), |visitor| {
            visitor.env(&global);
            visitor.env(&host);
        });
        assert!(
            freed(&heap, child.word()),
            "dropped, though its parent is bound"
        );
        assert!(!freed(&heap, parent.word()));
        assert!(!in_store(&host, "z"));
        assert_eq!(global.get(&heap, "z"), Some(n(3)));
        assert_counts_agree(&heap, &[&global, &host]);

        let strong = host.link_record(&heap, "z").expect("a strong child");
        assert!(in_read_through(&host, "z") && !in_store(&host, "z"));
        assert_eq!(strong.parent_record(&heap.borrow()), Some(parent));
        assert_eq!(resolved(&heap, strong), n(3));
        assert_counts_agree(&heap, &[&global, &host]);
    }

    /// A weak read-through record goes alone: while code linked in the
    /// parent links its placeholder, a collection that marks the
    /// placeholder through that link table, and not the record, drops the
    /// record and keeps the placeholder in the parent's store.
    #[test]
    fn a_weak_child_goes_alone_while_its_parents_placeholder_is_linked() {
        let heap = new_shared_heap();
        let (global, host) = global_and_host(&heap);
        let child = host.link_record(&heap, "w").expect("a weak child");
        let linked = LinkTable::new(vec![Rc::<str>::from("w")].into_boxed_slice());
        linked.link_in(&heap, &global);
        let placeholder = Record::from_word(linked.entry(0));
        assert_eq!(child.parent_record(&heap.borrow()), Some(placeholder));

        collect_only(&mut heap.borrow_mut(), |visitor| {
            visitor.env(&global);
            visitor.env(&host);
            linked.trace(visitor);
        });
        assert!(freed(&heap, child.word()), "the record goes");
        assert!(!freed(&heap, placeholder.word()), "its parent stays");
        assert!(in_store(&global, "w") && !in_store(&host, "w"));
        assert_counts_agree(&heap, &[&global, &host]);
    }

    /// A define in the host namespace adopts its weak read-through record
    /// as it does a strong one: the record's word, in a slot, flags 0, over
    /// a fresh cell; the host's store lets go of it, and the namespace counts
    /// a record and a cell. The parent's placeholder it read through is
    /// left, and dropped by the next collection nothing links it in. An
    /// import adopts one the same way, over the exporter's cell, counting a
    /// record and no cell.
    #[test]
    fn a_host_define_or_import_adopts_its_weak_child() {
        let heap = new_shared_heap();
        let (global, host) = global_and_host(&heap);
        let child = host.link_record(&heap, "y").expect("a weak child");
        let placeholder = global
            .weak_entries()
            .and_then(|store| store.get("y"))
            .unwrap();
        let (records, cells) = host.counted();
        let host_store = store_charged(&host);

        host.define(&heap, "y", n(9));
        assert_eq!(host.local_record("y"), Some(child), "its word, in a slot");
        assert_eq!(
            host_store - store_charged(&host),
            name_bytes("y"),
            "the host's store gives back its name's bytes"
        );
        {
            let h = heap.borrow();
            assert_eq!(h.cdr(child.word()), n(0), "an own record now");
            assert!(child.cell(&h).owned_by(&h, host.env_id()));
            assert_eq!(child.value(&h), n(9));
        }
        assert!(!in_store(&host, "y") && !in_read_through(&host, "y"));
        assert_eq!(host.counted(), (records + 1, cells + 1));
        assert_eq!(global.get(&heap, "y"), None);
        assert_counts_agree(&heap, &[&global, &host]);

        collect_only(&mut heap.borrow_mut(), |visitor| {
            visitor.env(&global);
            visitor.env(&host);
        });
        assert!(
            freed(&heap, placeholder.word()),
            "the placeholder, orphaned"
        );
        assert!(!freed(&heap, child.word()));
        assert!(!in_store(&global, "y"));
        assert_eq!(host.get(&heap, "y"), Some(n(9)));
        assert_counts_agree(&heap, &[&global, &host]);

        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "v", n(20));
        let exported = library.cell_of(&heap, "v").unwrap();
        let imported = host.link_record(&heap, "y2").expect("a weak child");
        let (records, cells) = host.counted();
        let host_store = store_charged(&host);
        host.import_cell(&heap, "y2", exported);
        assert_eq!(host.local_record("y2"), Some(imported));
        assert_eq!(host_store - store_charged(&host), name_bytes("y2"));
        {
            let h = heap.borrow();
            assert_eq!(h.cdr(imported.word()), n(0), "an own record now");
            assert_eq!(imported.cell(&h), exported);
        }
        assert_eq!(
            host.counted(),
            (records + 1, cells),
            "an import makes no cell"
        );
        assert!(!in_store(&host, "y2"));
        assert_eq!(host.get(&heap, "y2"), Some(n(20)));
        assert_counts_agree(&heap, &[&global, &host, &library]);
    }

    /// A scoped definition of a name a placeholder holds leaves it in its
    /// store (S14): flagged and over `UNBOUND` still, in no slot, while a
    /// read under the scopes finds the scoped binding, as before. Only a
    /// define or an import by name adopts one.
    #[test]
    fn a_scoped_definition_leaves_the_placeholder_in_its_store() {
        let heap = new_shared_heap();
        let ns = Rc::new(Environment::with_heap(&heap));
        let placeholder = ns.link_record(&heap, "foo").expect("a placeholder");
        let mut scopes = ScopeSet::new();
        scopes.add_scope(ScopeId(7));
        ns.define_with_scopes(&heap, "foo", scopes.clone(), n(4));

        assert!(in_store(&ns, "foo"));
        {
            let h = heap.borrow();
            assert!(placeholder.is_placeholder(&h));
            assert_eq!(placeholder.value(&h), TaggedValue::UNBOUND);
        }
        assert_eq!(
            ns.get_with_scopes(&heap, "foo", &scopes).unwrap(),
            Some(n(4))
        );
        assert_eq!(ns.local_slot("foo"), None);
        assert_eq!(ns.link_record(&heap, "foo"), Some(placeholder));
        assert_counts_agree(&heap, &[&ns]);
    }

    /// A host namespace dropped while its store still holds weak
    /// read-through records, none adopted, promoted or dropped by a pass,
    /// gives back everything it and its store took: its store's drop lets
    /// go of a record each and no cell, and its charge. The heap's sums
    /// and external bytes return to what they were before the host
    /// namespace, and its parent's placeholders stay in the parent's store.
    #[test]
    fn a_host_namespace_s_drop_gives_back_its_weak_children() {
        let heap = new_shared_heap();
        let global = Rc::new(Environment::with_heap(&heap));
        global.define(&heap, "x", n(1));
        for name in ["y", "z"] {
            global.link_record(&heap, name).expect("a placeholder");
        }
        let held = || {
            let h = heap.borrow();
            (
                h.namespaces(),
                h.binding_records(),
                h.cells(),
                h.external_bytes(),
            )
        };
        let before = held();

        let host = Rc::new(Environment::host_namespace(Rc::clone(&global)));
        for name in ["y", "z"] {
            host.link_record(&heap, name).expect("a weak child");
        }
        assert!(in_store(&host, "y") && in_store(&host, "z"));
        assert_eq!(stored(&host), (2, 0), "a record each, and no cell");
        assert_counts_agree(&heap, &[&global, &host]);

        drop(host);
        assert_eq!(held(), before, "everything given back");
        assert!(in_store(&global, "y") && in_store(&global, "z"));
        assert_counts_agree(&heap, &[&global]);
    }

    /// `append_slot`'s check (Rule 1, #836): the one writer of a namespace's
    /// slots refuses a placeholder, as an adoption that left its flag set
    /// would hand it, so no slot holds a record the sweep's pass could drop.
    #[test]
    #[cfg_attr(
        not(any(debug_assertions, feature = "gc-check")),
        ignore = "needs a check build"
    )]
    #[should_panic(expected = "a slot for \"later\" given a placeholder or a read-through record")]
    fn a_check_build_refuses_a_placeholder_in_a_slot() {
        let heap = new_shared_heap();
        let ns = Rc::new(Environment::with_heap(&heap));
        let placeholder = ns.link_record(&heap, "later").expect("a placeholder");
        ns.append_slot(&heap.borrow(), Rc::from("later"), placeholder.word());
    }

    /// `insert_scoped_record`'s assertion (#836): a visible scoped
    /// definition, which only the tree-walker makes, and it links nothing,
    /// is refused over a host namespace's weak read-through record, as over
    /// a strong one.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "a visible scoped definition over a read-through record")]
    fn a_visible_scoped_definition_over_a_weak_child_is_refused() {
        let heap = new_shared_heap();
        let (_global, host) = global_and_host(&heap);
        host.link_record(&heap, "w").expect("a weak child");
        let mut scopes = ScopeSet::new();
        scopes.add_scope(ScopeId(7));
        host.define_scoped_definition(&heap, "w", scopes, n(1));
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

/// Every edge the collector follows out of an environment, each to a value
/// nothing else reaches (#623): a plain binding, a scoped one, the binding an
/// alias forwards to (#38), a frame's imported cell (#834), and the parent's
/// binding. The environment is built by a struct literal, so a new field
/// breaks this test as well as `for_each_gc_edge`; the reads before the
/// collection show the literal is the environment the API would build.
#[cfg(test)]
mod gc_edge_tests {
    use super::*;
    use crate::heap::new_shared_heap;
    use crate::heap::sentinels::Sentinels;
    use crate::heap::trace_sentinels::collect_only;
    use crate::scope::ScopeId;
    use smallvec::smallvec;

    #[test]
    fn environment_edges() {
        let heap = new_shared_heap();
        let mut h = heap.borrow_mut();
        let mut s = Sentinels::new(&mut h);
        let plain = s.pair(&mut h, "Environment.bindings");
        let scoped = s.vector(&mut h, "Environment.scoped_bindings");
        let aliased = s.string(&mut h, "Environment.alias_bindings");
        let imported = s.object(&mut h, "Environment.rare: RareTables.frame_imports");
        let specifier = s.object(&mut h, "Environment.rare: RareTables.specifier");
        let inherited = s.pair(&mut h, "Environment.parent");
        drop(h);

        let parent = Rc::new(Environment::with_heap(&heap));
        parent.define(&heap, "inherited", inherited);
        let alias_target = Rc::new(Environment::with_heap(&heap));
        alias_target.define(&heap, "private", aliased);
        // The library itself is never traced: the frame's import keeps its
        // cell, and so the value, alone.
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "exported", imported);
        let exported = library.cell_of(&heap, "exported").unwrap();

        let mut scopes = ScopeSet::new();
        scopes.add_scope(ScopeId(7));
        let mut scoped_table = ScopedTable::default();
        scoped_table.insert(
            Rc::from("hidden"),
            smallvec![ScopedBinding {
                scopes: scopes.clone(),
                tagged_value: scoped,
                visible_by_name: true,
            }],
        );
        let mut aliases = AliasBindings::default();
        aliases.insert(
            Rc::from("alias"),
            AliasTarget {
                env: Some(Rc::clone(&alias_target)),
                name: Rc::from("private"),
                scopes: None,
            },
        );
        let env = Environment {
            heap_id: heap.borrow().id(),
            bindings: RefCell::new(Bindings {
                slots: smallvec![
                    (Rc::from("plain"), plain),
                    (Rc::from("exported"), TaggedValue::IMPORTED),
                ],
                index: None,
            }),
            env_id: fresh_env_id(),
            scoped_bindings: RefCell::new(scoped_table),
            alias_bindings: RefCell::new(aliases),
            has_aliases: Cell::new(true),
            has_visible_scoped: Cell::new(true),
            kind: Kind::Frame,
            frame: Kind::Frame.frame_flag(),
            rare: OnceCell::from(Box::new(RareTables {
                introduced_global_names: RefCell::default(),
                scoped_index: RefCell::default(),
                frame_imports: RefCell::new(vec![None, Some(exported)]),
                import_aliases: RefCell::default(),
                relink_aliases: RefCell::default(),
                specifier: Cell::new(Some(specifier)),
                namespace: None,
                // A host namespace's: `read_through_edges`.
                read_through: OnceCell::new(),
                // Weak, never traced (#836): a namespace's alone.
                placeholders: OnceCell::new(),
            })),
            parent: Some(parent),
        };
        assert_eq!(env.get(&heap, "plain"), Some(plain));
        assert_eq!(
            env.get_with_scopes(&heap, "hidden", &scopes).unwrap(),
            Some(scoped)
        );
        assert_eq!(env.get(&heap, "alias"), Some(aliased));
        assert_eq!(env.get(&heap, "exported"), Some(imported));
        assert_eq!(env.get(&heap, "inherited"), Some(inherited));
        let env = Rc::new(env);
        assert_eq!(env.mutable_specifier(&heap), specifier);

        let mut h = heap.borrow_mut();
        collect_only(&mut h, |visitor| visitor.env(&env));
        s.assert_survived(&h);
    }

    /// A host namespace's read-through table (#835), every edge: the cell
    /// its records name in their car, and each record, whose cdr is its
    /// parent's record, which holds the parent's cell and its value. The
    /// parent namespace whose record it reads through is dropped and the
    /// host's own parent is empty, so the table alone reaches either
    /// sentinel. Built by a struct literal; the reads before the collection
    /// show the table is what `link_record` would have made.
    #[test]
    fn read_through_edges() {
        let heap = new_shared_heap();
        let mut h = heap.borrow_mut();
        let mut s = Sentinels::new(&mut h);
        let unbound = s.pair(
            &mut h,
            "Environment.rare: RareTables.read_through: ReadThrough.unbound",
        );
        let inherited = s.vector(
            &mut h,
            "Environment.rare: RareTables.read_through: ReadThrough.records, through the parent's record",
        );
        let unbound = BindingCell::free(&mut h, unbound);
        drop(h);

        let child = {
            let elsewhere = Rc::new(Environment::with_heap(&heap));
            elsewhere.define(&heap, "x", inherited);
            let parent = elsewhere.local_record("x").unwrap();
            let mut h = heap.borrow_mut();
            let marker = BindingCell::free(&mut h, TaggedValue::UNBOUND);
            Record::read_through(&mut h, marker, parent)
        };
        let mut records = FxHashMap::default();
        records.insert(Rc::from("x"), child);
        let empty = Rc::new(Environment::with_heap(&heap));
        let env = Rc::new(Environment {
            heap_id: heap.borrow().id(),
            bindings: RefCell::new(Bindings::default()),
            env_id: fresh_env_id(),
            scoped_bindings: RefCell::new(ScopedTable::default()),
            alias_bindings: RefCell::new(AliasBindings::default()),
            has_aliases: Cell::new(false),
            has_visible_scoped: Cell::new(false),
            kind: Kind::HostNamespace,
            frame: Kind::HostNamespace.frame_flag(),
            rare: OnceCell::from(Box::new(RareTables {
                read_through: OnceCell::from(Box::new(RefCell::new(ReadThrough {
                    unbound,
                    records,
                }))),
                ..RareTables::default()
            })),
            parent: Some(empty),
        });
        assert_eq!(env.recount(&heap.borrow()), (1, 0), "one record, no cell");
        assert_eq!(env.local_slot("x"), None, "in no slot");
        assert_eq!(
            child.resolved_cell(&heap.borrow()).value(&heap.borrow()),
            inherited
        );

        let mut h = heap.borrow_mut();
        collect_only(&mut h, |visitor| visitor.env(&env));
        s.assert_survived(&h);
        assert_eq!(child.resolved_cell(&h).value(&h), inherited);
    }

    /// A namespace's slot and scoped entry each hold a record, which holds
    /// its cell, which holds the value (#834): the collector marks all three
    /// from the namespace's trace alone, with no trace code of the records'
    /// own.
    #[test]
    fn a_namespace_record_keeps_its_cell_and_value() {
        let heap = new_shared_heap();
        let mut h = heap.borrow_mut();
        let mut s = Sentinels::new(&mut h);
        let slotted = s.pair(&mut h, "Environment.bindings: a slot's record's cell");
        let scoped = s.vector(
            &mut h,
            "Environment.scoped_bindings: an entry's record's cell",
        );
        drop(h);

        let ns = Rc::new(Environment::with_heap(&heap));
        ns.define(&heap, "slotted", slotted);
        let mut identity = ScopeSet::new();
        identity.add_scope(ScopeId(9));
        ns.define_with_scopes(&heap, "scoped", identity.clone(), scoped);
        assert!(ns.local_record("slotted").is_some());

        collect_only(&mut heap.borrow_mut(), |visitor| visitor.env(&ns));
        s.assert_survived(&heap.borrow());
        assert_eq!(ns.get(&heap, "slotted"), Some(slotted));
        assert_eq!(
            ns.get_with_scopes(&heap, "scoped", &identity).unwrap(),
            Some(scoped)
        );
    }

    /// A cell an importer's record names outlives the namespace that made
    /// it (#834): the importer holds the cell and not its exporter, so once
    /// the exporter drops, a collection rooted at the importer alone keeps
    /// the cell and its value.
    #[test]
    fn a_cell_only_an_importer_holds_outlives_its_exporter() {
        let heap = new_shared_heap();
        let mut h = heap.borrow_mut();
        let mut s = Sentinels::new(&mut h);
        let exported = s.string(&mut h, "an imported cell, its exporter dropped");
        drop(h);

        let importer = Rc::new(Environment::with_heap(&heap));
        let cell = {
            let exporter = Rc::new(Environment::with_heap(&heap));
            exporter.define(&heap, "exported", exported);
            let cell = exporter.cell_of(&heap, "exported").unwrap();
            importer.import_cell(&heap, "imported", cell);
            cell
        };
        assert_eq!(heap.borrow().namespaces(), 1, "the exporter dropped");

        collect_only(&mut heap.borrow_mut(), |visitor| visitor.env(&importer));
        s.assert_survived(&heap.borrow());
        assert_eq!(importer.get(&heap, "imported"), Some(exported));
        assert_eq!(importer.cell_of(&heap, "imported"), Some(cell));
    }

    /// The stale-reference check (#621) guards a read through a record
    /// (#834, critique D4): a namespace's record and cell, once the
    /// namespace drops and a collection sweeps them, panic as freed when
    /// read, before anything is allocated in their slots. Every read goes
    /// through `Record::value` and `BindingCell::value`, so an unchecked
    /// fast read added later fails here.
    #[test]
    #[cfg(any(debug_assertions, feature = "gc-check"))]
    fn a_freed_record_is_refused_in_a_check_build() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        /// The message of the panic `read` raises.
        fn refused(read: impl FnOnce()) -> String {
            let payload = catch_unwind(AssertUnwindSafe(read)).expect_err("a panic");
            match payload.downcast::<String>() {
                Ok(message) => *message,
                Err(payload) => payload
                    .downcast_ref::<&str>()
                    .map(|message| message.to_string())
                    .unwrap_or_default(),
            }
        }

        let heap = new_shared_heap();
        let ns = Rc::new(Environment::with_heap(&heap));
        ns.define(&heap, "x", TaggedValue::fixnum(1));
        let record = ns.local_record("x").unwrap();
        let cell = ns.cell_of(&heap, "x").unwrap();
        assert_eq!(record.value(&heap.borrow()), TaggedValue::fixnum(1));
        drop(ns);
        collect_only(&mut heap.borrow_mut(), |_| {});

        let h = heap.borrow();
        let read_record = refused(|| {
            record.value(&h);
        });
        assert!(
            read_record.contains("use-after-free"),
            "a freed record: {read_record}"
        );
        let read_cell = refused(|| {
            cell.value(&h);
        });
        assert!(
            read_cell.contains("use-after-free"),
            "a freed cell: {read_cell}"
        );
    }
}

#[cfg(test)]
mod relink_alias_tests {
    //! [`Environment::relink_alias`] (#611): one alias per target, however
    //! many expansions ask, and never one shared by two targets.
    use super::*;
    use crate::heap::new_shared_heap;
    use crate::scope::{ScopeId, ScopeSet};

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    /// A library with two private helpers, and a program that cannot name
    /// either.
    fn library_and_program(heap: &SharedHeap) -> (Rc<Environment>, Rc<Environment>) {
        let library = Rc::new(Environment::with_heap(heap));
        library.define(heap, "helper", n(1));
        library.define(heap, "other", n(2));
        let program = Rc::new(Environment::with_heap(heap));
        (library, program)
    }

    #[test]
    fn one_target_asked_for_repeatedly_gets_one_alias() {
        let heap = new_shared_heap();
        let (library, program) = library_and_program(&heap);
        let minted = Cell::new(0);
        let mut mint = || {
            minted.set(minted.get() + 1);
            Rc::<str>::from(format!("helper.{}", minted.get()))
        };
        let first = program.relink_alias(
            Rc::clone(&library),
            "helper".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        for _ in 0..100 {
            let again = program.relink_alias(
                Rc::clone(&library),
                "helper".into(),
                None,
                &ScopeSet::new(),
                &mut mint,
            );
            assert_eq!(again, first);
        }
        assert_eq!(minted.get(), 1, "an alias per target, not per expansion");
        // Still an alias, resolved afresh: it sees what the library assigns.
        library.set(&heap, "helper", n(10)).unwrap();
        assert_eq!(program.get(&heap, &first), Some(n(10)));
    }

    #[test]
    fn two_targets_get_two_aliases() {
        let heap = new_shared_heap();
        let (library, program) = library_and_program(&heap);
        let elsewhere = Rc::new(Environment::with_heap(&heap));
        elsewhere.define(&heap, "helper", n(3));
        let minted = Cell::new(0);
        let mut mint = || {
            minted.set(minted.get() + 1);
            Rc::<str>::from(format!("alias.{}", minted.get()))
        };
        let helper = program.relink_alias(
            Rc::clone(&library),
            "helper".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        let other = program.relink_alias(
            Rc::clone(&library),
            "other".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        // The same spelling in another environment is another target.
        let elsewhere_helper = program.relink_alias(
            Rc::clone(&elsewhere),
            "helper".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        assert_eq!(minted.get(), 3);
        assert_eq!(program.get(&heap, &helper), Some(n(1)));
        assert_eq!(program.get(&heap, &other), Some(n(2)));
        assert_eq!(program.get(&heap, &elsewhere_helper), Some(n(3)));
    }

    #[test]
    fn two_identities_of_one_spelling_get_two_aliases() {
        let heap = new_shared_heap();
        let (library, program) = library_and_program(&heap);
        let a = ScopeSet::singleton(ScopeId::fresh());
        let b = ScopeSet::singleton(ScopeId::fresh());
        let minted = Cell::new(0);
        let mut mint = || {
            minted.set(minted.get() + 1);
            Rc::<str>::from(format!("tmp.{}", minted.get()))
        };
        let by_name = program.relink_alias(
            Rc::clone(&library),
            "tmp".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        let at_a = program.relink_alias(
            Rc::clone(&library),
            "tmp".into(),
            Some(a.clone()),
            &ScopeSet::new(),
            &mut mint,
        );
        let at_b = program.relink_alias(
            Rc::clone(&library),
            "tmp".into(),
            Some(b),
            &ScopeSet::new(),
            &mut mint,
        );
        assert_eq!(minted.get(), 3, "{by_name} {at_a} {at_b}");
        assert_eq!(
            program.relink_alias(
                Rc::clone(&library),
                "tmp".into(),
                Some(a),
                &ScopeSet::new(),
                &mut mint
            ),
            at_a
        );
        assert_eq!(minted.get(), 3);
    }

    /// Two mentions of one target — a generated macro's own binder of a
    /// spelling and the library's binding it was passed — each get an alias,
    /// since the spelling is what tells the renamed mentions apart.
    #[test]
    fn two_mentions_of_one_target_get_two_aliases() {
        let heap = new_shared_heap();
        let (library, program) = library_and_program(&heap);
        let generated = ScopeSet::singleton(ScopeId::fresh());
        let minted = Cell::new(0);
        let mut mint = || {
            minted.set(minted.get() + 1);
            Rc::<str>::from(format!("helper.{}", minted.get()))
        };
        let as_written = program.relink_alias(
            Rc::clone(&library),
            "helper".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        let as_generated = program.relink_alias(
            Rc::clone(&library),
            "helper".into(),
            None,
            &generated,
            &mut mint,
        );
        assert_ne!(as_written, as_generated);
        assert_eq!(minted.get(), 2);
        // Each mention keeps its own from then on, and both reach the helper.
        assert_eq!(
            program.relink_alias(
                Rc::clone(&library),
                "helper".into(),
                None,
                &generated,
                &mut mint
            ),
            as_generated
        );
        assert_eq!(minted.get(), 2);
        assert_eq!(program.get(&heap, &as_written), Some(n(1)));
        assert_eq!(program.get(&heap, &as_generated), Some(n(1)));
    }

    /// The mint skips a name a placeholder holds (#836): an alias there
    /// would leave code that linked the name reading the placeholder where
    /// `get` reads the alias.
    #[test]
    fn minting_skips_a_name_a_placeholder_holds() {
        let heap = new_shared_heap();
        let (library, program) = library_and_program(&heap);
        let placeholder = program
            .link_record(&heap, "helper.1")
            .expect("a placeholder");
        let minted = Cell::new(0);
        let mut mint = || {
            minted.set(minted.get() + 1);
            Rc::<str>::from(format!("helper.{}", minted.get()))
        };
        let alias = program.relink_alias(
            Rc::clone(&library),
            "helper".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        assert_eq!(&*alias, "helper.2");
        assert_eq!(program.get(&heap, &alias), Some(n(1)));
        assert_eq!(program.get(&heap, "helper.1"), None, "still unbound");
        assert_eq!(program.link_record(&heap, "helper.1"), Some(placeholder));
    }

    /// The same in a host namespace (#836): the mint skips a name its weak
    /// read-through record holds, over the parent's placeholder.
    #[test]
    fn minting_skips_a_name_a_weak_read_through_record_holds() {
        let heap = new_shared_heap();
        let (library, _) = library_and_program(&heap);
        let global = Rc::new(Environment::with_heap(&heap));
        let host = Rc::new(Environment::host_namespace(Rc::clone(&global)));
        let child = host.link_record(&heap, "helper.1").expect("a weak child");
        let minted = Cell::new(0);
        let mut mint = || {
            minted.set(minted.get() + 1);
            Rc::<str>::from(format!("helper.{}", minted.get()))
        };
        let alias = host.relink_alias(
            Rc::clone(&library),
            "helper".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        assert_eq!(&*alias, "helper.2");
        assert_eq!(host.get(&heap, &alias), Some(n(1)));
        assert_eq!(host.get(&heap, "helper.1"), None, "still unbound");
        assert_eq!(host.link_record(&heap, "helper.1"), Some(child));
    }

    /// `remembered_relink`'s assertion (#836): no path puts a placeholder at
    /// a remembered alias's spelling, since the mint skips a name one holds
    /// and a link of an alias's spelling makes none, so one forged into the
    /// store there is refused at the next relink to the same target.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "a remembered relink alias over a placeholder")]
    fn a_remembered_relink_alias_over_a_placeholder_is_refused() {
        let heap = new_shared_heap();
        let (library, program) = library_and_program(&heap);
        let mut mint = || Rc::<str>::from("helper.1");
        let alias = program.relink_alias(
            Rc::clone(&library),
            "helper".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        program
            .link_record(&heap, "elsewhere")
            .expect("a placeholder, and the store");
        let forged = {
            let mut h = heap.borrow_mut();
            let cell = BindingCell::new_own(&mut h, TaggedValue::UNBOUND, program.env_id());
            Record::placeholder(&mut h, cell)
        };
        let store = program.weak_entries().expect("the store");
        store.insert(alias, forged);
        program.relink_alias(
            Rc::clone(&library),
            "helper".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
    }

    #[test]
    fn a_remembered_alias_that_was_defined_over_is_replaced() {
        let heap = new_shared_heap();
        let (library, program) = library_and_program(&heap);
        let minted = Cell::new(0);
        let mut mint = || {
            minted.set(minted.get() + 1);
            Rc::<str>::from(format!("helper.{}", minted.get()))
        };
        let first = program.relink_alias(
            Rc::clone(&library),
            "helper".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        // `helper.1` is a legal identifier: the program binds it itself.
        program.define(&heap, Rc::clone(&first), n(99));
        let second = program.relink_alias(
            Rc::clone(&library),
            "helper".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        assert_ne!(second, first);
        assert_eq!(minted.get(), 2);
        assert_eq!(program.get(&heap, &first), Some(n(99)));
        assert_eq!(program.get(&heap, &second), Some(n(1)));
        // And the replacement is the one remembered from then on.
        assert_eq!(
            program.relink_alias(
                Rc::clone(&library),
                "helper".into(),
                None,
                &ScopeSet::new(),
                &mut mint
            ),
            second
        );
        assert_eq!(minted.get(), 2);
    }

    #[test]
    fn a_frame_reuses_its_roots_alias_and_keeps_its_own_to_itself() {
        let heap = new_shared_heap();
        let (library, program) = library_and_program(&heap);
        let minted = Cell::new(0);
        let mut mint = || {
            minted.set(minted.get() + 1);
            Rc::<str>::from(format!("alias.{}", minted.get()))
        };
        let rooted = program.relink_alias(
            Rc::clone(&library),
            "helper".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        let body = Rc::new(Environment::child_frame(Rc::clone(&program)));
        let frame = Rc::new(Environment::child_frame(Rc::clone(&body)));
        assert_eq!(
            frame.relink_alias(
                Rc::clone(&library),
                "helper".into(),
                None,
                &ScopeSet::new(),
                &mut mint
            ),
            rooted
        );
        assert_eq!(minted.get(), 1);
        assert!(
            frame.alias_target(&rooted).is_none(),
            "the frame installs nothing"
        );
        assert_eq!(frame.get(&heap, &rooted), Some(n(1)));
        // A target the root has not aliased is minted into the frame alone.
        let own = frame.relink_alias(
            Rc::clone(&library),
            "other".into(),
            None,
            &ScopeSet::new(),
            &mut mint,
        );
        assert_eq!(minted.get(), 2);
        assert!(program.alias_target(&own).is_none());
        assert_eq!(frame.get(&heap, &own), Some(n(2)));
    }
}

/// The namespace step (#834): every read or write of a namespace's binding
/// borrows the heap, shared for a read and mutably for a write, in every
/// build; a frame's own binding takes no borrow. The borrow covers the
/// binding's record and cell (#834), so what this pins is that it is taken
/// before anything is read or written. Run with `--release` too, where the
/// borrows must still be taken.
#[cfg(test)]
mod heap_step_tests {
    use super::*;
    use crate::heap::new_shared_heap;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    fn n(i: i64) -> TaggedValue {
        TaggedValue::fixnum(i)
    }

    fn scopes(ids: &[usize]) -> ScopeSet {
        let mut set = ScopeSet::new();
        for id in ids {
            set.add_scope(crate::scope::ScopeId(*id));
        }
        set
    }

    /// A named access, run to see whether it panics.
    type Case<'a> = (&'static str, Box<dyn FnOnce() + 'a>);

    fn panics(f: impl FnOnce()) -> bool {
        catch_unwind(AssertUnwindSafe(f)).is_err()
    }

    /// What an access answered, which only its panic or its absence says
    /// anything about here.
    fn ignore<T>(_: T) {}

    /// The tree-walker's per-call binds take no borrow of the heap: with it
    /// borrowed mutably elsewhere, each still binds, and reads back as
    /// `define`'s binding would (#834).
    #[test]
    fn a_frames_binds_take_no_borrow() {
        let heap = new_shared_heap();
        let ns = Rc::new(Environment::with_heap(&heap));
        let frame = Rc::new(Environment::child_frame(Rc::clone(&ns)));
        {
            let _held = heap.borrow_mut();
            frame.bind("plain", n(1));
            frame.bind_with_scopes("scoped", scopes(&[7]), n(2));
            frame.bind_with_scopes("unscoped", ScopeSet::new(), n(3));
            frame.bind_scoped_definition("both", scopes(&[8]), n(4));
        }
        assert_eq!(frame.get(&heap, "plain"), Some(n(1)));
        assert_eq!(frame.get(&heap, "unscoped"), Some(n(3)));
        assert_eq!(frame.get(&heap, "both"), Some(n(4)), "by name");
        assert_eq!(
            frame
                .get_with_scopes(&heap, "scoped", &scopes(&[7]))
                .unwrap(),
            Some(n(2))
        );
        assert_eq!(
            frame.get_with_scopes(&heap, "both", &scopes(&[8])).unwrap(),
            Some(n(4))
        );
    }

    #[test]
    fn every_namespace_hit_borrows_the_heap_and_a_frames_own_does_not() {
        let heap = new_shared_heap();
        let library = Rc::new(Environment::with_heap(&heap));
        library.define(&heap, "lib", n(1));
        let ns = Rc::new(Environment::with_heap(&heap));
        ns.define(&heap, "own", n(2));
        ns.define_with_scopes(&heap, "x", scopes(&[1]), n(3));
        ns.define_scoped_definition(&heap, "vis", scopes(&[2]), n(4));
        share(&heap, &ns, "imported", &library, "lib");
        let frame = Rc::new(Environment::child_frame(Rc::clone(&ns)));
        frame.define(&heap, "local", n(5));
        share(&heap, &frame, "frame-import", &library, "lib");
        // A host namespace is a namespace (#835): its own binding is read
        // and written at the step too.
        let host = Rc::new(Environment::host_namespace(Rc::clone(&ns)));
        host.define(&heap, "host-own", n(9));
        // Taken before any borrow: it reads the library at its step.
        let lib_cell = library.cell_of(&heap, "lib").unwrap();
        let own = ns.local_slot("own").unwrap();
        let local = frame.local_slot("local").unwrap();
        let host_own = host.local_slot("host-own").unwrap();

        // A read of a namespace's binding, under the heap borrowed mutably.
        {
            let held = heap.borrow_mut();
            let reads: [Case<'_>; 13] = [
                ("ns.get own", Box::new(|| ignore(ns.get(&heap, "own")))),
                (
                    "ns.get imported",
                    Box::new(|| ignore(ns.get(&heap, "imported"))),
                ),
                (
                    "frame.get, at the namespace",
                    Box::new(|| ignore(frame.get(&heap, "own"))),
                ),
                (
                    "frame.get of its import",
                    Box::new(|| ignore(frame.get(&heap, "frame-import"))),
                ),
                (
                    "ns.slot_value",
                    Box::new(|| ignore(ns.slot_value(&heap, own))),
                ),
                (
                    "ns.binding_location",
                    Box::new(|| ignore(ns.binding_location(&heap, "own"))),
                ),
                (
                    "ns.get_with_scopes",
                    Box::new(|| ignore(ns.get_with_scopes(&heap, "x", &scopes(&[1, 9])))),
                ),
                (
                    "ns.scoped_binding_of",
                    Box::new(|| ignore(ns.scoped_binding_of(&heap, "x", &scopes(&[1, 9])))),
                ),
                ("ns.bindings", Box::new(|| ignore(ns.bindings(&heap)))),
                (
                    "NamespaceView::get",
                    Box::new(|| ignore(crate::NamespaceView::of(&ns).get(&heap, "own"))),
                ),
                (
                    "ns.get of the visible scoped binding",
                    Box::new(|| ignore(ns.get(&heap, "vis"))),
                ),
                (
                    "host.get of its own",
                    Box::new(|| ignore(host.get(&heap, "host-own"))),
                ),
                (
                    "host.slot_value",
                    Box::new(|| ignore(host.slot_value(&heap, host_own))),
                ),
            ];
            for (what, read) in reads {
                assert!(panics(read), "{what} took no shared borrow");
            }
            drop(held);
        }

        // A write of a namespace's binding, under the heap borrowed shared.
        {
            let held = heap.borrow();
            let writes: [Case<'_>; 10] = [
                (
                    "ns.define over a name",
                    Box::new(|| ns.define(&heap, "own", n(20))),
                ),
                (
                    "ns.define of a fresh name",
                    Box::new(|| ns.define(&heap, "fresh", n(21))),
                ),
                ("ns.set", Box::new(|| ns.set(&heap, "own", n(22)).unwrap())),
                (
                    "ns.set_slot_value",
                    Box::new(|| ns.set_slot_value(&heap, own, n(23))),
                ),
                (
                    "ns.define_with_scopes",
                    Box::new(|| ns.define_with_scopes(&heap, "x", scopes(&[1]), n(24))),
                ),
                (
                    "ns.set_with_scopes",
                    Box::new(|| {
                        ns.set_with_scopes(&heap, "x", &scopes(&[1, 9]), n(25))
                            .unwrap()
                    }),
                ),
                (
                    "ns.import_cell re-pointing an import",
                    Box::new(|| ns.import_cell(&heap, "imported", lib_cell)),
                ),
                (
                    "frame.set of its import",
                    Box::new(|| frame.set(&heap, "frame-import", n(26)).unwrap()),
                ),
                (
                    "ns.set of the visible scoped binding",
                    Box::new(|| ns.set(&heap, "vis", n(27)).unwrap()),
                ),
                (
                    "host.define of a fresh name",
                    Box::new(|| host.define(&heap, "host-fresh", n(28))),
                ),
            ];
            for (what, write) in writes {
                assert!(panics(write), "{what} took no mutable borrow");
            }
            drop(held);
        }

        // Each step came before anything was written.
        assert_eq!(ns.get(&heap, "own"), Some(n(2)));
        assert_eq!(ns.get(&heap, "imported"), Some(n(1)));
        assert_eq!(library.get(&heap, "lib"), Some(n(1)));
        assert_eq!(
            ns.get_with_scopes(&heap, "x", &scopes(&[1, 9])).unwrap(),
            Some(n(3))
        );
        assert_eq!(ns.get(&heap, "vis"), Some(n(4)));
        assert_eq!(ns.local_slot("fresh"), None);
        assert_eq!(host.get(&heap, "host-own"), Some(n(9)));
        assert_eq!(host.local_slot("host-fresh"), None);

        // A frame's own binding, under the heap borrowed mutably: no borrow.
        // Nor does installing a frame's import, which allocates nothing
        // (#834, ruling D5/D6).
        let held = heap.borrow_mut();
        assert_eq!(frame.get(&heap, "local"), Some(n(5)));
        frame.define(&heap, "frame-fresh", n(6));
        frame.set(&heap, "local", n(7)).unwrap();
        frame.define_with_scopes(&heap, "y", scopes(&[3]), n(8));
        assert_eq!(frame.slot_value(&heap, local), n(7));
        frame.import_cell(&heap, "frame-import-2", lib_cell);
        drop(held);
        assert_eq!(frame.get(&heap, "frame-fresh"), Some(n(6)));
        assert_eq!(frame.get(&heap, "frame-import-2"), Some(n(1)));
        assert_eq!(
            frame.get_with_scopes(&heap, "y", &scopes(&[3])).unwrap(),
            Some(n(8))
        );
    }

    /// The step's borrow is checked against the environment's heap: a
    /// namespace read with another heap's `RefCell` is refused in a debug
    /// build.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "another heap")]
    fn a_namespace_step_refuses_another_heaps_borrow() {
        let heap = new_shared_heap();
        let other = new_shared_heap();
        let ns = Environment::with_heap(&heap);
        ns.define(&heap, "x", n(1));
        let _ = ns.get(&other, "x");
    }
}
