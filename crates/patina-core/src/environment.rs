use crate::scope::ScopeSet;
use crate::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A binding with its associated scope set
/// Used for scope-based hygiene lookup
#[derive(Debug, Clone)]
pub struct ScopedBinding {
    pub scopes: ScopeSet,
    pub value: Value,
}

/// Environment for variable bindings
///
/// Uses Rc<RefCell<>> for shared mutable state (needed for set!)
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
    /// Simple name-based bindings (for built-ins and top-level)
    bindings: Rc<RefCell<HashMap<String, Value>>>,
    /// Scope-aware bindings (for scope sets hygiene)
    /// Each name can have multiple bindings with different scope sets
    scoped_bindings: Rc<RefCell<HashMap<String, Vec<ScopedBinding>>>>,
    parent: Option<Rc<Environment>>,
}

impl Environment {
    /// Create a new empty environment
    pub fn new() -> Self {
        Environment {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            scoped_bindings: Rc::new(RefCell::new(HashMap::new())),
            parent: None,
        }
    }

    /// Create a new environment with a parent
    #[allow(dead_code)]
    pub fn with_parent(parent: Rc<Environment>) -> Self {
        Environment {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            scoped_bindings: Rc::new(RefCell::new(HashMap::new())),
            parent: Some(parent),
        }
    }

    /// Define a new binding in this environment
    ///
    /// Use this for top-level defines, built-ins, and other simple bindings.
    pub fn define(&self, name: String, value: Value) {
        self.bindings.borrow_mut().insert(name, value);
    }

    /// Set an existing binding (searches parent environments)
    pub fn set(&self, name: &str, value: Value) -> Result<(), String> {
        if self.bindings.borrow().contains_key(name) {
            self.bindings.borrow_mut().insert(name.to_string(), value);
            Ok(())
        } else if let Some(parent) = &self.parent {
            parent.set(name, value)
        } else {
            Err(name.to_string())
        }
    }

    /// Get a value from the environment (searches parent environments)
    ///
    /// This is the simple name-based lookup for identifiers.
    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(value) = self.bindings.borrow().get(name) {
            Some(value.clone())
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
    pub fn define_with_scopes(&self, name: String, scopes: ScopeSet, value: Value) {
        use crate::macro_debug;

        if macro_debug::is_enabled() {
            println!(
                "[ENV] Defining '{}' with scopes {} = {}",
                name, scopes, value
            );
        }

        if scopes.is_empty() {
            // Empty scopes = treat as unmarked binding
            self.bindings.borrow_mut().insert(name, value);
        } else {
            // Non-empty scopes = add to scoped bindings
            let binding = ScopedBinding { scopes, value };
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
    pub fn set_with_scopes(
        &self,
        name: &str,
        scopes: &ScopeSet,
        value: Value,
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
                    binding.value = value;
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
    pub fn get_with_scopes(&self, name: &str, scopes: &ScopeSet) -> Option<Value> {
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
                match &result {
                    Some(v) => println!("[ENV]   Result: {}", v),
                    None => println!("[ENV]   Result: NOT FOUND"),
                }
            }
            return result;
        }

        // Collect all matching bindings from this environment and parents
        let mut candidates: Vec<(ScopeSet, Value)> = Vec::new();

        // Helper to collect matching bindings recursively
        fn collect_matches(
            env: &Environment,
            name: &str,
            ref_scopes: &ScopeSet,
            candidates: &mut Vec<(ScopeSet, Value)>,
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
                        candidates.push((binding.scopes.clone(), binding.value.clone()));
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
                match &result {
                    Some(v) => println!("[ENV]   Fallback result: {}", v),
                    None => println!("[ENV]   Fallback result: NOT FOUND"),
                }
            }
            return result;
        }

        if debug {
            println!("[ENV]   Found {} candidate(s):", candidates.len());
            for (ss, val) in &candidates {
                println!("[ENV]     {} -> {}", ss, val);
            }
        }

        // Find the most specific binding (largest scope set)
        let best = candidates
            .into_iter()
            .max_by_key(|(scope_set, _)| scope_set.len());

        if debug {
            match &best {
                Some((ss, val)) => {
                    println!("[ENV]   Best match (most specific): {} -> {}", ss, val);
                }
                None => {
                    println!("[ENV]   No best match found");
                }
            }
        }

        best.map(|(_, value)| value).or_else(|| self.get(name))
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
    /// Returns a vector of (name, value) pairs for all bindings defined locally.
    /// This is useful for library imports where we need to iterate over all exports.
    pub fn bindings(&self) -> Vec<(String, Value)> {
        self.bindings
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
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
        env.define("x".to_string(), Value::Integer(42));
        assert!(matches!(env.get("x"), Some(Value::Integer(42))));
    }

    #[test]
    fn test_parent_lookup() {
        let parent = Rc::new(Environment::new());
        parent.define("x".to_string(), Value::Integer(42));

        let child = Environment::with_parent(parent);
        assert!(matches!(child.get("x"), Some(Value::Integer(42))));
    }

    #[test]
    fn test_bindings() {
        let env = Environment::new();
        env.define("x".to_string(), Value::Integer(42));
        env.define("y".to_string(), Value::Boolean(true));
        env.define("z".to_string(), Value::Integer(100));

        let bindings = env.bindings();
        assert_eq!(bindings.len(), 3);

        // Check that all bindings are present
        let names: Vec<String> = bindings.iter().map(|(k, _)| k.clone()).collect();
        assert!(names.contains(&"x".to_string()));
        assert!(names.contains(&"y".to_string()));
        assert!(names.contains(&"z".to_string()));

        // Verify values
        for (name, value) in bindings {
            match name.as_str() {
                "x" => assert!(matches!(value, Value::Integer(42))),
                "y" => assert!(matches!(value, Value::Boolean(true))),
                "z" => assert!(matches!(value, Value::Integer(100))),
                _ => panic!("Unexpected binding: {}", name),
            }
        }
    }

    #[test]
    fn test_bindings_excludes_parent() {
        let parent = Rc::new(Environment::new());
        parent.define("x".to_string(), Value::Integer(42));
        parent.define("y".to_string(), Value::Integer(100));

        let child = Environment::with_parent(parent);
        child.define("z".to_string(), Value::Boolean(true));

        // bindings() should only return local bindings
        let bindings = child.bindings();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].0, "z");
        assert!(matches!(bindings[0].1, Value::Boolean(true)));
    }

    #[test]
    fn test_scoped_bindings_basic() {
        use crate::scope::ScopeId;

        let env = Environment::new();
        let s1 = ScopeId(1);
        let scopes = ScopeSet::singleton(s1);

        env.define_with_scopes("x".to_string(), scopes.clone(), Value::Integer(42));

        // Lookup with matching scopes should find the binding
        assert!(matches!(
            env.get_with_scopes("x", &scopes),
            Some(Value::Integer(42))
        ));

        // Lookup with superset scopes should also find it (binding.scopes ⊆ ref.scopes)
        let s2 = ScopeId(2);
        let larger_scopes = ScopeSet::from_iter([s1, s2]);
        assert!(matches!(
            env.get_with_scopes("x", &larger_scopes),
            Some(Value::Integer(42))
        ));
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

        // Outer x binding: introduced at S1
        let outer_x_scopes = ScopeSet::singleton(s1);
        env.define_with_scopes(
            "x".to_string(),
            outer_x_scopes.clone(),
            Value::Symbol("outer".into()),
        );

        // Inner x binding: inside S1, S2, S3
        let inner_x_scopes = ScopeSet::from_iter([s1, s2, s3]);
        env.define_with_scopes(
            "x".to_string(),
            inner_x_scopes.clone(),
            Value::Symbol("inner".into()),
        );

        // Free var x in macro m: inside S1, S2 (captured when macro was defined)
        let macro_x_scopes = ScopeSet::from_iter([s1, s2]);

        // Lookup should find outer x, NOT inner x
        // Because:
        // - outer_x_scopes {S1} ⊆ macro_x_scopes {S1, S2} ✓
        // - inner_x_scopes {S1, S2, S3} ⊆ macro_x_scopes {S1, S2} ✗ (S3 not in ref)
        let result = env.get_with_scopes("x", &macro_x_scopes);
        assert!(matches!(result, Some(Value::Symbol(s)) if &*s == "outer"));
    }

    #[test]
    fn test_scoped_bindings_most_specific() {
        use crate::scope::ScopeId;

        let env = Environment::new();
        let s1 = ScopeId(1);
        let s2 = ScopeId(2);

        // Less specific binding
        env.define_with_scopes("x".to_string(), ScopeSet::singleton(s1), Value::Integer(1));

        // More specific binding
        env.define_with_scopes(
            "x".to_string(),
            ScopeSet::from_iter([s1, s2]),
            Value::Integer(2),
        );

        // Lookup with {S1, S2} should find the more specific binding
        let lookup_scopes = ScopeSet::from_iter([s1, s2]);
        let result = env.get_with_scopes("x", &lookup_scopes);
        assert!(matches!(result, Some(Value::Integer(2))));

        // Lookup with just {S1} should find the less specific binding
        // (since {S1, S2} ⊈ {S1})
        let lookup_scopes_s1 = ScopeSet::singleton(s1);
        let result = env.get_with_scopes("x", &lookup_scopes_s1);
        assert!(matches!(result, Some(Value::Integer(1))));
    }

    #[test]
    fn test_scoped_bindings_fallback_to_unmarked() {
        use crate::scope::ScopeId;

        let env = Environment::new();

        // Define an unmarked binding (e.g., a primitive)
        env.define("cons".to_string(), Value::Symbol("primitive-cons".into()));

        // Lookup with scopes should fall back to unmarked binding
        let s1 = ScopeId(1);
        let scopes = ScopeSet::singleton(s1);
        let result = env.get_with_scopes("cons", &scopes);
        assert!(matches!(result, Some(Value::Symbol(s)) if &*s == "primitive-cons"));
    }
}
