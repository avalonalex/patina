use crate::heap::SharedHeap;
use crate::scope::ScopeSet;
use crate::tagged_value::TaggedValue;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A binding with its associated scope set
/// Used for scope-based hygiene lookup
#[derive(Debug, Clone)]
pub struct ScopedBinding {
    pub scopes: ScopeSet,
    /// Internal storage uses TaggedValue for efficiency
    pub(crate) tagged_value: TaggedValue,
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
    bindings: Rc<RefCell<HashMap<String, TaggedValue>>>,
    /// Scope-aware bindings (for scope sets hygiene)
    /// Each name can have multiple bindings with different scope sets
    scoped_bindings: Rc<RefCell<HashMap<String, Vec<ScopedBinding>>>>,
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
            bindings: Rc::new(RefCell::new(HashMap::new())),
            scoped_bindings: Rc::new(RefCell::new(HashMap::new())),
            parent: None,
        }
    }

    /// Create a new environment with a parent (shares the parent's heap)
    pub fn with_parent(parent: Rc<Environment>) -> Self {
        Environment {
            heap: parent.heap.clone(),
            bindings: Rc::new(RefCell::new(HashMap::new())),
            scoped_bindings: Rc::new(RefCell::new(HashMap::new())),
            parent: Some(parent),
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
        });
        let tv = self.heap.borrow_mut().alloc_procedure(proc);
        self.define(name.to_string(), tv);
    }

    /// Set an existing binding (searches parent environments)
    /// This is the primary API - accepts TaggedValue directly.
    pub fn set(&self, name: &str, value: TaggedValue) -> Result<(), String> {
        if self.bindings.borrow().contains_key(name) {
            self.bindings.borrow_mut().insert(name.to_string(), value);
            Ok(())
        } else if let Some(parent) = &self.parent {
            parent.set(name, value)
        } else {
            Err(name.to_string())
        }
    }

    /// Get a TaggedValue from the environment (searches parent environments)
    ///
    /// This is the primary API - returns TaggedValue directly.
    /// For the simple name-based lookup for identifiers.
    pub fn get(&self, name: &str) -> Option<TaggedValue> {
        if let Some(tv) = self.bindings.borrow().get(name).copied() {
            Some(tv)
        } else if let Some(parent) = &self.parent {
            parent.get(name)
        } else {
            None
        }
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

    /// Check if a binding exists
    #[allow(dead_code)]
    pub fn has(&self, name: &str) -> bool {
        self.bindings.borrow().contains_key(name)
            || self.parent.as_ref().is_some_and(|p| p.has(name))
    }

    /// Get all variable names defined in this environment and parent environments
    pub fn get_all_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.bindings.borrow().keys().cloned().collect();
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
        self.bindings
            .borrow()
            .iter()
            .map(|(k, tv)| (k.clone(), *tv))
            .collect()
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
