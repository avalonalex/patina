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
}

/// Where a macro-expansion alias points: the environment holding the real
/// binding, and the name it has there.
type AliasTarget = (Rc<Environment>, Rc<str>);

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
    scoped_bindings: Rc<RefCell<FxHashMap<String, Vec<ScopedBinding>>>>,
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
            scoped_bindings: Rc::new(RefCell::new(FxHashMap::default())),
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
            scoped_bindings: Rc::new(RefCell::new(FxHashMap::default())),
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
        let proc = Rc::new(crate::procedure::Procedure::Primitive {
            name,
            arity,
            qualified_name,
            registry_index: std::cell::Cell::new(None),
        });
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
            return target_env.set(&target_name, value);
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
            return target_env.get(&target_name);
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
    /// `alias` is expected to be a name expansion generated and therefore
    /// unique, so this cannot shadow anything the program itself wrote.
    pub fn define_alias(&self, alias: String, target_env: Rc<Environment>, target_name: Rc<str>) {
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

        if scopes.is_empty() {
            // Empty scopes = treat as unmarked binding
            self.bindings.borrow_mut().insert(name, value);
        } else {
            // Non-empty scopes = add to scoped bindings
            let binding = ScopedBinding {
                scopes,
                tagged_value: value,
            };
            self.scoped_bindings
                .borrow_mut()
                .entry(name)
                .or_default()
                .push(binding);
        }
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

        // Collect all matching bindings from this environment and parents
        // Store TaggedValue internally for efficiency
        let mut candidates: Vec<(ScopeSet, TaggedValue)> = Vec::new();

        // Helper to collect matching bindings recursively
        fn collect_matches(
            env: &Environment,
            name: &str,
            ref_scopes: &ScopeSet,
            candidates: &mut Vec<(ScopeSet, TaggedValue)>,
            debug: bool,
        ) {
            // Check scoped bindings in this environment
            let scoped = env.scoped_bindings.borrow();
            if let Some(bindings) = scoped.get(name) {
                for binding in bindings {
                    // binding.scopes ⊆ reference.scopes
                    let is_subset = binding.scopes.is_subset_of(ref_scopes);
                    if debug {
                        println!(
                            "[ENV]   Checking binding {} ⊆ {} : {}",
                            binding.scopes,
                            ref_scopes,
                            if is_subset { "YES" } else { "NO" }
                        );
                    }
                    if is_subset {
                        candidates.push((binding.scopes.clone(), binding.tagged_value));
                    }
                }
            }
            drop(scoped);

            // Recurse to parent
            if let Some(parent) = &env.parent {
                collect_matches(parent, name, ref_scopes, candidates, debug);
            }
        }

        collect_matches(self, name, scopes, &mut candidates, debug);

        if candidates.is_empty() {
            // No scoped binding found, fall back to unmarked bindings
            if debug {
                println!("[ENV]   No scoped candidates, falling back to simple lookup");
            }
            let result = self.get(name);
            if debug {
                match result {
                    Some(tv) => {
                        let v = crate::debug_format::format_tagged(tv, &self.heap.borrow());
                        println!("[ENV]   Fallback result: {}", v);
                    }
                    None => println!("[ENV]   Fallback result: NOT FOUND"),
                }
            }
            return result;
        }

        if debug {
            println!("[ENV]   Found {} candidate(s):", candidates.len());
            for (ss, tv) in &candidates {
                let v = crate::debug_format::format_tagged(*tv, &self.heap.borrow());
                println!("[ENV]     {} -> {}", ss, v);
            }
        }

        // Find the most specific binding (largest scope set)
        // When scope sets have the same size, prefer the earlier candidate (closer binding)
        // since collect_matches adds child environment bindings before parent environment bindings.
        // We use a stable comparison that prefers earlier elements on ties.
        let mut best: Option<(ScopeSet, TaggedValue)> = None;
        for (scope_set, tv) in candidates {
            match &best {
                None => best = Some((scope_set, tv)),
                Some((best_scopes, _)) => {
                    // Prefer strictly larger scope set, or keep existing on tie
                    if scope_set.len() > best_scopes.len() {
                        best = Some((scope_set, tv));
                    }
                    // On tie (same length), keep the earlier candidate (child binding)
                }
            }
        }

        if debug {
            match &best {
                Some((ss, tv)) => {
                    let v = crate::debug_format::format_tagged(*tv, &self.heap.borrow());
                    println!("[ENV]   Best match (most specific): {} -> {}", ss, v);
                }
                None => {
                    println!("[ENV]   No best match found");
                }
            }
        }

        best.map(|(_, tv)| tv).or_else(|| self.get(name))
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
            f(env);
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
