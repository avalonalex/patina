use crate::heap::SharedHeap;
use crate::scope::ScopeSet;
use crate::scope_resolve::AmbiguousReference;
use crate::tagged_value::TaggedValue;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::cell::{Cell, RefCell};
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
#[derive(Debug, Default)]
struct ScopedTable {
    map: FxHashMap<Rc<str>, ScopedBindingList>,
    /// Whether any binding here is visible to a name-only lookup.
    any_visible_by_name: bool,
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
/// binding, and the name it has there.
/// `None` for the environment holding the alias.
///
/// Not an optimisation: an `Rc<Environment>` pointing at the environment that
/// owns the table is a cycle refcounting can never break, so a self-alias
/// would pin that environment — and its heap — for the process. It also saves
/// an `Rc` clone and drop on every alias hit.
type AliasTarget = (Option<Rc<Environment>>, Rc<str>);

/// Alias name -> the binding it forwards to.
type AliasBindings = FxHashMap<Rc<str>, AliasTarget>;

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
    /// Bound names in slot order: a name's slot *is* its index here.
    names: SmallVec<[Rc<str>; 3]>,
    /// Values, indexed by slot. Kept beside `names` rather than paired with
    /// them so `for_each_local_value` can hand the GC a plain slice.
    slots: SmallVec<[TaggedValue; 3]>,
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
                .names
                .iter()
                .position(|n| name_matches(n, name))
                .map(|i| i as u32),
        }
    }

    fn read_slot(&self, slot: u32) -> TaggedValue {
        self.slots[slot as usize]
    }

    fn write_slot(&mut self, slot: u32, value: TaggedValue) {
        self.slots[slot as usize] = value;
    }

    fn get(&self, name: &str) -> Option<TaggedValue> {
        self.slot_of(name).map(|i| self.read_slot(i))
    }

    /// Define semantics: overwrite the existing slot or append a new one.
    fn insert(&mut self, name: Rc<str>, value: TaggedValue) {
        // A fresh frame is the common case on the tree-walker's hot path —
        // one is built per `let`-bound temporary and per call — and it has
        // nothing to search.
        if !self.names.is_empty()
            && let Some(slot) = self.slot_of(&name)
        {
            self.slots[slot as usize] = value;
            return;
        }
        let slot = self.names.len() as u32;
        self.slots.push(value);
        match &mut self.index {
            Some(index) => {
                index.insert(Rc::clone(&name), slot);
                self.names.push(name);
            }
            None => {
                self.names.push(name);
                if self.names.len() > LINEAR_MAX {
                    let mut index = FxHashMap::default();
                    index.reserve(self.names.len());
                    for (i, n) in self.names.iter().enumerate() {
                        index.insert(Rc::clone(n), i as u32);
                    }
                    self.index = Some(Box::new(index));
                }
            }
        }
    }

    /// Every bound name, in slot order.
    fn names(&self) -> impl Iterator<Item = &Rc<str>> {
        self.names.iter()
    }
}

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
    has_aliases: Cell<bool>,
    has_visible_scoped: Cell<bool>,
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
    #[inline]
    pub fn slot_value(&self, slot: u32) -> TaggedValue {
        self.bindings.borrow().read_slot(slot)
    }

    /// Overwrite the value in a slot previously obtained from `local_slot`.
    #[inline]
    pub fn set_slot_value(&self, slot: u32, value: TaggedValue) {
        self.bindings.borrow_mut().write_slot(slot, value);
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
        let name = name.into();
        // A plain binding is reachable by name and by nothing else, so
        // `byname=true` here is a property of the table, not a decision.
        crate::scope_trace::bind(&name, &ScopeSet::new(), true);
        self.bindings.borrow_mut().insert(name, value);
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
        self.define(name.to_string(), tv);
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
        if let Some((target_env, target_name)) = self.alias_target(name) {
            return match target_env {
                Some(env) => env.set(&target_name, value),
                None => self.set(&target_name, value),
            };
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
            if let Some(tv) = env.bindings.borrow().get(name) {
                return Some(tv);
            }
            // Follow a macro-expansion alias into the environment the macro was
            // defined in. Checked after real bindings so a local definition always
            // wins, and before the parent so the alias is not shadowed by an
            // unrelated outer binding of the same (unique) name.
            if env.has_aliases.get()
                && let Some((target_env, target_name)) = env.alias_target(name)
            {
                return match target_env {
                    Some(target) => target.get(&target_name),
                    None => env.get(&target_name),
                };
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
    ///   temporary from overwriting a user's global of the same spelling, and
    ///   is *also* why a user's later global of that spelling steals the
    ///   macro's private definition (`(jab get 10) (define mh 99) (get)` is
    ///   99 here, 10 in chibi and Gauche). Both directions follow from the
    ///   bare name, and neither is fixable while relinking resolves by name;
    ///   Track L §6 records that as the open defect it is.
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
        // A target that is this environment is stored as `None`: see
        // `AliasTarget`.
        let target_env = (target_env.env_id() != self.env_id()).then_some(target_env);
        self.has_aliases.set(true);
        self.alias_bindings
            .borrow_mut()
            .insert(alias.into(), (target_env, target_name));
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
        table.any_visible_by_name |= visible_by_name;
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
    /// This is what a `define` needs and what `define_with_scopes`
    /// deliberately withholds. A macro-introduced *parameter* must stay
    /// invisible to a reference written in source, so it is filed under its
    /// scopes alone. A macro-introduced *definition* cannot be: the
    /// definition-environment relinking that lets a macro-generated macro
    /// reach its defining environment resolves its target by name
    /// (`link_definition_env_refs`, and Track L §6 records that it does), so a
    /// definition reachable only under scopes is unreachable from exactly the
    /// code that needs it — the R7RS suite's `jabberwocky` test.
    ///
    /// The name-only view's reach is *plain* access — [`get`], [`set`], and
    /// the relinker resolving through them — not scoped resolution's
    /// fallback. A **scoped** reference whose resolution rejected this
    /// binding stays refused (`get_scoped_fallback` / `set_scoped_terminal`):
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
    /// This is the name-only view of a macro-introduced definition. It is one
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
        let table = self.scoped_bindings.borrow();
        if !table.any_visible_by_name {
            return None;
        }
        table.get(name)?.iter().rposition(|b| b.visible_by_name)
    }

    /// Set an existing scoped binding (searches parent environments)
    ///
    /// Finds the binding matching the scope set and updates its value.
    /// This is the primary API - accepts TaggedValue directly.
    pub fn set_with_scopes(
        &self,
        name: &str,
        scopes: &ScopeSet,
        value: TaggedValue,
    ) -> Result<(), String> {
        if scopes.is_empty() {
            // Empty scopes - use simple lookup
            return self.set(name, value);
        }

        // Resolve the way a read does — largest matching subset, most
        // recent on a tie — so that a reference can write the binding it can
        // read. Requiring an exact match meant a `set!` a macro introduced,
        // which carries that macro's scopes on top of the binder's, found
        // nothing and fell through to the root's by-name `set`. Larceny
        // triage family 38.
        let here = {
            let scoped = self.scoped_bindings.borrow();
            scoped.get(name).and_then(|bindings| {
                let mut best: Option<(usize, usize)> = None;
                for (index, binding) in bindings.iter().enumerate().rev() {
                    if !crate::scope_resolve::is_candidate(&binding.scopes, scopes) {
                        continue;
                    }
                    let len = binding.scopes.len();
                    if best.is_none_or(|(best_len, _)| len > best_len) {
                        best = Some((len, index));
                    }
                }
                best
            })
        };
        if crate::scope_trace::enabled() {
            // One record per environment, because this walk resolves per
            // environment rather than over the whole chain — so the trace of a
            // write is a sequence where a read's is a single line. That
            // asymmetry is triage family 38, visible rather than argued.
            let scoped = self.scoped_bindings.borrow();
            // Candidates, not bindings: `cands` has to mean the same thing
            // here as on the read path or the two cannot be compared, which is
            // the whole reason to record both.
            let (count, picked) = match scoped.get(name) {
                Some(bindings) => (
                    bindings
                        .iter()
                        .filter(|b| crate::scope_resolve::is_candidate(&b.scopes, scopes))
                        .count(),
                    here.map(|(_, index)| bindings[index].scopes.clone()),
                ),
                None => (0, None),
            };
            drop(scoped);
            use crate::scope_trace::{Op, Outcome};
            let outcome = if picked.is_some() {
                Outcome::Scoped
            } else {
                Outcome::ByName
            };
            crate::scope_trace::resolve(name, scopes, count, picked.as_ref(), Op::Set, outcome);
        }
        if let Some((_, index)) = here {
            let mut scoped = self.scoped_bindings.borrow_mut();
            if let Some(binding) = scoped.get_mut(name).and_then(|bs| bs.get_mut(index)) {
                binding.tagged_value = value;
                drop(scoped);
                crate::scope_trace::wrote(name, scopes, "scoped");
                return Ok(());
            }
        }
        // Check parent
        if let Some(parent) = &self.parent {
            parent.set_with_scopes(name, scopes, value)
        } else {
            // Fall back to unmarked binding. This is the *root*, the recursion
            // having walked here — triage family 38's open half — so the
            // terminal record says which of the two ways the walk ended.
            let landed = self.set_scoped_terminal(name, scopes, value);
            crate::scope_trace::wrote(
                name,
                scopes,
                if landed.is_ok() {
                    "byname"
                } else {
                    "undefined"
                },
            );
            landed
        }
    }

    /// The terminal of [`set_with_scopes`]'s fallback: [`set`], except that
    /// the name-only view of a scoped binding this resolution rejected is not
    /// written through. That is [`get_scoped_fallback`]'s rule on the write
    /// side, and it has to hold on both or a reference could clobber by
    /// spelling a binding it is not allowed to read. Reached only at the
    /// root — the per-frame walk above already resolved every scoped binding
    /// on the chain — so the parent arm mirrors [`set`]'s for shape, not for
    /// traffic.
    ///
    /// [`set`]: Self::set
    /// [`set_with_scopes`]: Self::set_with_scopes
    /// [`get_scoped_fallback`]: Self::get_scoped_fallback
    fn set_scoped_terminal(
        &self,
        name: &str,
        scopes: &ScopeSet,
        value: TaggedValue,
    ) -> Result<(), String> {
        // Root-only by construction: `set_with_scopes` recurses to the root
        // before falling back, and per-frame resolution has already rejected
        // every scoped binding on the chain by then. A non-root call would
        // run write semantics no test has ever exercised; make that loud.
        debug_assert!(
            self.parent.is_none(),
            "set_scoped_terminal called off the root for `{name}`"
        );
        if let Some(slot) = self.local_slot(name) {
            self.set_slot_value(slot, value);
            return Ok(());
        }
        if let Some((target_env, target_name)) = self.alias_target(name) {
            return match target_env {
                Some(env) => env.set(&target_name, value),
                None => self.set(&target_name, value),
            };
        }
        if let Some(i) = self.visible_scoped_index(name) {
            let mut table = self.scoped_bindings.borrow_mut();
            if let Some(binding) = table.get_mut(name).and_then(|bs| bs.get_mut(i)) {
                // Dead arm with a tripwire, exactly as in
                // `get_scoped_fallback`: the per-frame walk above already
                // rejected every scoped binding here, so a candidate showing
                // up means the rule changed under this fallback. Note the
                // caller would then trace the write as `byname`, which is one
                // more reason this must never fire silently.
                debug_assert!(
                    !crate::scope_resolve::is_candidate(&binding.scopes, scopes),
                    "set_scoped_terminal reached a binding of `{name}` that is a \
                     candidate for {scopes} — the per-frame walk should have \
                     written it"
                );
                if crate::scope_resolve::is_candidate(&binding.scopes, scopes) {
                    binding.tagged_value = value;
                    return Ok(());
                }
            }
            // Rejected for these scopes: not writable by spelling.
        }
        match &self.parent {
            Some(parent) => parent.set_scoped_terminal(name, scopes, value),
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
            return Ok(result);
        }

        // Every binding of this name in this environment and its parents,
        // in the order `resolve_scoped` wants them.
        let mut candidates: Vec<(ScopeSet, TaggedValue)> = Vec::new();

        fn collect_candidates(
            env: &Environment,
            name: &str,
            ref_scopes: &ScopeSet,
            candidates: &mut Vec<(ScopeSet, TaggedValue)>,
            debug: bool,
        ) {
            // Every candidate binding of the name here, latest first — the
            // order `resolve_scoped` documents. Candidacy is tested with the
            // rule's own `is_candidate`, so this is a filter and not a second
            // copy of the rule; a binding that fails it is shown neither to
            // the resolver nor to the check, so cloning its scope set would
            // be waste on a path the tree-walker takes per variable read.
            let scoped = env.scoped_bindings.borrow();
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
            drop(scoped);

            // Recurse to parent
            if let Some(parent) = &env.parent {
                collect_candidates(parent, name, ref_scopes, candidates, debug);
            }
        }

        collect_candidates(self, name, scopes, &mut candidates, debug);

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
        chosen?;

        if debug {
            match &result {
                Some(tv) => {
                    let v = crate::debug_format::format_tagged(*tv, &self.heap.borrow());
                    println!("[ENV]   Result (scoped or fallback): {}", v);
                }
                None => println!("[ENV]   No scoped match and the fallback found nothing"),
            }
        }

        Ok(result)
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
        if let Some(tv) = self.bindings.borrow().get(name) {
            return Some(tv);
        }
        if self.has_aliases.get()
            && let Some((target_env, target_name)) = self.alias_target(name)
        {
            return match target_env {
                Some(env) => env.get(&target_name),
                None => self.get(&target_name),
            };
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

    /// Does `name` have a *scoped* binding visible from `scopes`?
    ///
    /// The question [`get_with_scopes`] cannot answer, because it falls back to
    /// the plain bindings and returns a value either way. A caller that needs
    /// to know which of the two it got — `resolve_literal_bindings` in
    /// `patina-macros` does, since only a scoped binding's identity depends on
    /// the scopes it was reached with — asks here first.
    ///
    /// [`get_with_scopes`]: Self::get_with_scopes
    pub fn has_scoped_binding(&self, name: &str, scopes: &ScopeSet) -> bool {
        if scopes.is_empty() {
            return false;
        }
        self.scoped_bindings
            .borrow()
            .get(name)
            .is_some_and(|bindings| bindings.iter().any(|b| b.scopes.is_subset_of(scopes)))
            || self
                .parent
                .as_ref()
                .is_some_and(|p| p.has_scoped_binding(name, scopes))
    }

    /// Check if a binding exists
    #[allow(dead_code)]
    pub fn has(&self, name: &str) -> bool {
        self.bindings.borrow().slot_of(name).is_some()
            || self.parent.as_ref().is_some_and(|p| p.has(name))
    }

    /// Get all variable names defined in this environment and parent environments
    pub fn get_all_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .bindings
            .borrow()
            .names()
            .map(|k| k.to_string())
            .collect();
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

    /// Get all bindings in this environment only (not including parent)
    ///
    /// Returns a vector of (name, TaggedValue) pairs for all bindings defined locally.
    /// This is useful for library imports where we need to iterate over all exports.
    pub fn bindings(&self) -> Vec<(String, TaggedValue)> {
        let b = self.bindings.borrow();
        b.names()
            .enumerate()
            .map(|(i, k)| (k.to_string(), b.read_slot(i as u32)))
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
        for &tv in self.bindings.borrow().slots.iter() {
            f(tv);
        }
        for scoped in self.scoped_bindings.borrow().values() {
            for binding in scoped {
                f(binding.tagged_value);
            }
        }
    }

    /// Visit the environments this one's macro-expansion aliases point at.
    ///
    /// GC tracing hook. Values reachable only through an alias -- a library
    /// private referenced by an exported macro -- are live, but the alias edge
    /// is an `Rc<Environment>` in a side table rather than a `TaggedValue` in a
    /// slot, so `for_each_local_value` cannot see it.
    pub fn for_each_alias_target(&self, f: &mut dyn FnMut(&Rc<Environment>)) {
        for (env, _) in self.alias_bindings.borrow().values() {
            // A `None` target is this environment, which the caller is already
            // tracing.
            if let Some(env) = env {
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
