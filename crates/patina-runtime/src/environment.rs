use crate::scope::ScopeSet;
use crate::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Type alias for mark lists used in hygiene
pub type MarkList = Vec<usize>;

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
/// The environment supports multiple hygiene approaches:
///
/// ### Marks-and-ribs (Chez Scheme style)
/// - `bindings`: Simple name → value mapping for unmarked bindings (built-ins, top-level defines)
/// - `marked_bindings`: (name, marks) → value mapping for macro-introduced bindings
///
/// ### Scope sets (Racket style) - Preferred for complex hygiene
/// - `scoped_bindings`: name → Vec<ScopedBinding> mapping
/// - Each binding has an associated scope set
/// - Lookup finds binding where `binding.scopes ⊆ reference.scopes`
/// - Most specific (largest) matching scope set wins
///
/// Lookup order for `get_with_scopes(name, scopes)`:
/// 1. Collect all bindings where `binding.scopes ⊆ scopes`
/// 2. Return the binding with the largest scope set (most specific)
/// 3. Fall back to unmarked bindings if no scoped binding matches
#[derive(Debug, Clone)]
pub struct Environment {
    /// Simple name-based bindings (unmarked, for built-ins and top-level)
    bindings: Rc<RefCell<HashMap<String, Value>>>,
    /// Mark-aware bindings (for macro-introduced identifiers)
    /// Key is (name, marks) tuple
    marked_bindings: Rc<RefCell<HashMap<(String, MarkList), Value>>>,
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
            marked_bindings: Rc::new(RefCell::new(HashMap::new())),
            scoped_bindings: Rc::new(RefCell::new(HashMap::new())),
            parent: None,
        }
    }

    /// Create a new environment with a parent
    #[allow(dead_code)]
    pub fn with_parent(parent: Rc<Environment>) -> Self {
        Environment {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            marked_bindings: Rc::new(RefCell::new(HashMap::new())),
            scoped_bindings: Rc::new(RefCell::new(HashMap::new())),
            parent: Some(parent),
        }
    }

    /// Define a new binding in this environment (unmarked)
    ///
    /// Use this for top-level defines, built-ins, and other non-macro bindings.
    pub fn define(&self, name: String, value: Value) {
        self.bindings.borrow_mut().insert(name, value);
    }

    /// Define a new binding with marks (for macro-introduced identifiers)
    ///
    /// Use this when creating bindings from macro-expanded code where
    /// the identifier has hygiene marks.
    pub fn define_with_marks(&self, name: String, marks: MarkList, value: Value) {
        if marks.is_empty() {
            // Empty marks = unmarked binding
            self.bindings.borrow_mut().insert(name, value);
        } else {
            // Non-empty marks = marked binding
            self.marked_bindings
                .borrow_mut()
                .insert((name, marks), value);
        }
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

    /// Set an existing binding with marks (searches parent environments)
    pub fn set_with_marks(&self, name: &str, marks: &MarkList, value: Value) -> Result<(), String> {
        if marks.is_empty() {
            // Empty marks - use simple lookup
            return self.set(name, value);
        }

        // Check marked bindings first
        let key = (name.to_string(), marks.clone());
        if self.marked_bindings.borrow().contains_key(&key) {
            self.marked_bindings.borrow_mut().insert(key, value);
            return Ok(());
        }

        // Check parent
        if let Some(parent) = &self.parent {
            parent.set_with_marks(name, marks, value)
        } else {
            // Fall back to unmarked binding
            self.set(name, value)
        }
    }

    /// Get a value from the environment (searches parent environments)
    ///
    /// This is the simple name-based lookup for unmarked identifiers.
    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(value) = self.bindings.borrow().get(name) {
            Some(value.clone())
        } else if let Some(parent) = &self.parent {
            parent.get(name)
        } else {
            None
        }
    }

    /// Get a value with marks (for hygienic lookup)
    ///
    /// Lookup order:
    /// 1. If marks is empty, use simple lookup
    /// 2. Search marked_bindings for exact (name, marks) match
    /// 3. Fall back to unmarked bindings (built-ins, global defines)
    pub fn get_with_marks(&self, name: &str, marks: &MarkList) -> Option<Value> {
        if marks.is_empty() {
            // Empty marks = simple lookup (free variables, primitives)
            return self.get(name);
        }

        // Try marked bindings first (exact match)
        let key = (name.to_string(), marks.clone());
        if let Some(value) = self.marked_bindings.borrow().get(&key) {
            return Some(value.clone());
        }

        // Try parent's marked bindings
        if let Some(parent) = &self.parent
            && let Some(value) = parent.get_with_marks(name, marks)
        {
            return Some(value);
        }

        // Fall back to unmarked bindings (primitives, global defines)
        // This allows macro-introduced identifiers to access built-ins
        self.get(name)
    }

    /// Define a binding with a scope set (for scope-based hygiene)
    ///
    /// Use this when creating bindings from binding forms (lambda, let, etc.)
    /// where you want to track the lexical scope for hygiene.
    pub fn define_with_scopes(&self, name: String, scopes: ScopeSet, value: Value) {
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
        if scopes.is_empty() {
            // Empty scopes = simple lookup (top-level identifiers)
            return self.get(name);
        }

        // Collect all matching bindings from this environment and parents
        let mut candidates: Vec<(ScopeSet, Value)> = Vec::new();

        // Helper to collect matching bindings recursively
        fn collect_matches(
            env: &Environment,
            name: &str,
            ref_scopes: &ScopeSet,
            candidates: &mut Vec<(ScopeSet, Value)>,
        ) {
            // Check scoped bindings in this environment
            let scoped = env.scoped_bindings.borrow();
            if let Some(bindings) = scoped.get(name) {
                for binding in bindings {
                    // binding.scopes ⊆ reference.scopes
                    if binding.scopes.is_subset_of(ref_scopes) {
                        candidates.push((binding.scopes.clone(), binding.value.clone()));
                    }
                }
            }
            drop(scoped);

            // Recurse to parent
            if let Some(parent) = &env.parent {
                collect_matches(parent, name, ref_scopes, candidates);
            }
        }

        collect_matches(self, name, scopes, &mut candidates);

        if candidates.is_empty() {
            // No scoped binding found, fall back to unmarked bindings
            return self.get(name);
        }

        // Find the most specific binding (largest scope set)
        let best = candidates
            .into_iter()
            .max_by_key(|(scope_set, _)| scope_set.len())
            .map(|(_, value)| value);

        best.or_else(|| self.get(name))
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
        // Include names from marked bindings
        for (name, _marks) in self.marked_bindings.borrow().keys() {
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
