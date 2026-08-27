use crate::heap::SharedHeap;
use crate::scope::ScopeSet;
use crate::tagged_value::TaggedValue;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
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
    map: FxHashMap<String, Vec<ScopedBinding>>,
    /// Whether any binding here is visible to a name-only lookup.
    any_visible_by_name: bool,
}

impl std::ops::Deref for ScopedTable {
    type Target = FxHashMap<String, Vec<ScopedBinding>>;
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
type AliasBindings = FxHashMap<String, AliasTarget>;

/// Simple (non-scoped) binding storage: name → slot index into `slots`.
///
/// Slots are append-only: a binding's slot never moves or disappears once
/// created, and redefining a name overwrites its slot in place. The VM's
/// per-site global caches rest on this invariant (see `GlobalCacheEntry`
/// in patina-vm), so every mutation must go through these methods.
#[derive(Debug, Default)]
struct Bindings {
    map: FxHashMap<String, u32>,
    slots: Vec<TaggedValue>,
}

impl Bindings {
    fn slot_of(&self, name: &str) -> Option<u32> {
        self.map.get(name).copied()
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
    fn insert(&mut self, name: String, value: TaggedValue) {
        match self.map.entry(name) {
            std::collections::hash_map::Entry::Occupied(e) => {
                let slot = *e.get();
                self.slots[slot as usize] = value;
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(self.slots.len() as u32);
                self.slots.push(value);
            }
        }
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
/// 3. Fall back to simple bindings if no scoped binding matches
#[derive(Debug, Clone)]
pub struct Environment {
    /// Shared heap for TaggedValue interpretation
    heap: SharedHeap,
    /// Simple name-based bindings (for built-ins and top-level)
    /// Stores TaggedValue internally for memory efficiency
    bindings: Rc<RefCell<Bindings>>,
    /// Process-unique id for this environment's simple bindings, minted at
    /// construction and shared by clones (which alias the same bindings).
    /// Never reused, unlike an address — see `GlobalCacheEntry` in patina-vm
    /// for the cache soundness argument, and `gc_identity` for the
    /// address-based identity the GC uses to dedup *live* environments.
    env_id: u64,
    /// Scope-aware bindings (for scope sets hygiene)
    /// Each name can have multiple bindings with different scope sets
    scoped_bindings: Rc<RefCell<ScopedTable>>,
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
    alias_bindings: Rc<RefCell<AliasBindings>>,
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
            bindings: Rc::new(RefCell::new(Bindings::default())),
            env_id: fresh_env_id(),
            scoped_bindings: Rc::new(RefCell::new(ScopedTable::default())),
            alias_bindings: Rc::new(RefCell::new(FxHashMap::default())),
            parent: None,
        }
    }

    /// Create a new environment with a parent (shares the parent's heap)
    pub fn with_parent(parent: Rc<Environment>) -> Self {
        Environment {
            heap: parent.heap.clone(),
            bindings: Rc::new(RefCell::new(Bindings::default())),
            env_id: fresh_env_id(),
            scoped_bindings: Rc::new(RefCell::new(ScopedTable::default())),
            alias_bindings: Rc::new(RefCell::new(FxHashMap::default())),
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
    pub fn define(&self, name: String, value: TaggedValue) {
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
        if let Some(tv) = self.bindings.borrow().get(name) {
            return Some(tv);
        }
        // Follow a macro-expansion alias into the environment the macro was
        // defined in. Checked after real bindings so a local definition always
        // wins, and before the parent so the alias is not shadowed by an
        // unrelated outer binding of the same (unique) name.
        if let Some((target_env, target_name)) = self.alias_target(name) {
            return match target_env {
                Some(env) => env.get(&target_name),
                None => self.get(&target_name),
            };
        }
        // A macro-introduced definition lives under its scopes; this is the
        // name-only view of it. Checked after real bindings and aliases, so a
        // binding written in source always wins.
        if let Some(i) = self.visible_scoped_index(name) {
            return self.scoped_bindings.borrow()[name]
                .get(i)
                .map(|b| b.tagged_value);
        }
        self.parent.as_ref().and_then(|p| p.get(name))
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
    pub fn define_alias(&self, alias: String, target_env: Rc<Environment>, target_name: Rc<str>) {
        // A target that is this environment is stored as `None`: see
        // `AliasTarget`.
        let target_env = (target_env.env_id() != self.env_id()).then_some(target_env);
        self.alias_bindings
            .borrow_mut()
            .insert(alias, (target_env, target_name));
    }

    /// Define a binding with a scope set (for scope-based hygiene)
    ///
    /// Use this when creating bindings from binding forms (lambda, let, etc.)
    /// where you want to track the lexical scope for hygiene.
    /// This is the primary API - accepts TaggedValue directly.
    pub fn define_with_scopes(&self, name: String, scopes: ScopeSet, value: TaggedValue) {
        use crate::macro_debug;

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
        name: String,
        scopes: ScopeSet,
        value: TaggedValue,
        visible_by_name: bool,
    ) {
        if scopes.is_empty() {
            self.define(name, value);
            return;
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
    /// It is still **one** binding. An earlier version stored the value under
    /// the bare name as well, and a `set!` through the scoped path then left
    /// the two disagreeing — the freeze `alias_bindings` names as the reason
    /// its own indirection exists.
    pub fn define_scoped_definition(&self, name: String, scopes: ScopeSet, value: TaggedValue) {
        self.insert_scoped(name, scopes, value, true);
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

        // Check scoped bindings - find exact scope set match
        let mut scoped = self.scoped_bindings.borrow_mut();
        if let Some(bindings) = scoped.get_mut(name) {
            for binding in bindings.iter_mut() {
                if &binding.scopes == scopes {
                    binding.tagged_value = value;
                    return Ok(());
                }
            }
        }
        drop(scoped);
        // Check parent
        if let Some(parent) = &self.parent {
            parent.set_with_scopes(name, scopes, value)
        } else {
            // Fall back to unmarked binding
            self.set(name, value)
        }
    }

    /// Get a value with scope sets (for hygienic lookup)
    ///
    /// This is the key lookup algorithm for scope-based hygiene:
    /// 1. Collect all bindings for this name where `binding.scopes ⊆ reference.scopes`
    /// 2. Return the binding with the largest scope set (most specific match)
    /// 3. Fall back to unmarked bindings if no scoped binding matches
    ///
    /// The "most specific" rule ensures that inner bindings shadow outer ones
    /// when their scopes are a subset of the reference's scopes.
    /// This is the primary API - returns TaggedValue directly.
    pub fn get_with_scopes(&self, name: &str, scopes: &ScopeSet) -> Option<TaggedValue> {
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
            return result;
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
            // Every binding of the name here, latest first — the order
            // `resolve_scoped` documents. Filtering is the rule's job, not
            // this walk's; collecting all of them is also what lets the
            // ambiguity check see the candidates that lost.
            let scoped = env.scoped_bindings.borrow();
            if let Some(bindings) = scoped.get(name) {
                for binding in bindings.iter().rev() {
                    if debug {
                        println!(
                            "[ENV]   Candidate {} ⊆ {} : {}",
                            binding.scopes,
                            ref_scopes,
                            if binding.scopes.is_subset_of(ref_scopes) {
                                "YES"
                            } else {
                                "NO"
                            }
                        );
                    }
                    candidates.push((binding.scopes.clone(), binding.tagged_value));
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
        let result = crate::scope_resolve::resolve_scoped(name, scopes, &candidates);

        if debug {
            match &result {
                Some(tv) => {
                    let v = crate::debug_format::format_tagged(*tv, &self.heap.borrow());
                    println!("[ENV]   Best match (most specific): {}", v);
                }
                None => println!("[ENV]   No scoped match; falling back to simple lookup"),
            }
        }

        result.or_else(|| self.get(name))
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
        let mut names: Vec<String> = self.bindings.borrow().map.keys().cloned().collect();
        // Include names from scoped bindings
        for name in self.scoped_bindings.borrow().keys() {
            if !names.contains(name) {
                names.push(name.clone());
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
        b.map
            .iter()
            .map(|(k, &i)| (k.clone(), b.read_slot(i)))
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
    /// Keyed on the shared bindings map rather than the `Environment` struct:
    /// `Environment` is `Clone`, so several structs (and several
    /// `Rc<Environment>`s) can alias the same bindings.
    pub fn gc_identity(&self) -> usize {
        Rc::as_ptr(&self.bindings) as usize
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
    pub fn for_each_alias_target(&self, f: &mut dyn FnMut(&Environment)) {
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
    fn test_env_ids_unique_and_shared_by_clones() {
        let a = Environment::new();
        let b = Environment::new();
        assert_ne!(a.env_id(), 0);
        assert_ne!(a.env_id(), b.env_id());
        // A clone aliases the same bindings and must carry the same id.
        assert_eq!(a.clone().env_id(), a.env_id());
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
        let result = env.get_with_scopes("x", &scopes);
        assert_eq!(result, Some(TaggedValue::fixnum(42)));

        // Lookup with superset scopes should also find it (binding.scopes ⊆ ref.scopes)
        let s2 = ScopeId(2);
        let larger_scopes = ScopeSet::from_iter([s1, s2]);
        let result = env.get_with_scopes("x", &larger_scopes);
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
        let result_tv = env.get_with_scopes("x", &macro_x_scopes).unwrap();
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
            env.get_with_scopes("x", &lookup_scopes),
            Some(TaggedValue::fixnum(2))
        );

        // Lookup with just {S1} should find the less specific binding
        let lookup_scopes_s1 = ScopeSet::singleton(s1);
        assert_eq!(
            env.get_with_scopes("x", &lookup_scopes_s1),
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
        let result_tv = env.get_with_scopes("cons", &scopes).unwrap();
        let name = env
            .heap()
            .borrow()
            .get_symbol_name(result_tv)
            .map(|s| s.to_string());
        assert_eq!(name.as_deref(), Some("primitive-cons"));
    }
}
