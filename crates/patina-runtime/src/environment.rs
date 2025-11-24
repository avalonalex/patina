use crate::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Type alias for mark lists used in hygiene
pub type MarkList = Vec<usize>;

/// Environment for variable bindings
///
/// Uses Rc<RefCell<>> for shared mutable state (needed for set!)
///
/// ## Hygiene Support
///
/// The environment supports marks-and-ribs hygiene through two binding stores:
/// - `bindings`: Simple name → value mapping for unmarked bindings (built-ins, top-level defines)
/// - `marked_bindings`: (name, marks) → value mapping for macro-introduced bindings
///
/// Lookup order for `get_with_marks(name, marks)`:
/// 1. If marks is empty, use simple lookup (backwards compatible)
/// 2. Otherwise, search marked_bindings for exact (name, marks) match
/// 3. Fall back to unmarked bindings (built-ins, global defines)
#[derive(Debug, Clone)]
pub struct Environment {
    /// Simple name-based bindings (unmarked, for built-ins and top-level)
    bindings: Rc<RefCell<HashMap<String, Value>>>,
    /// Mark-aware bindings (for macro-introduced identifiers)
    /// Key is (name, marks) tuple
    marked_bindings: Rc<RefCell<HashMap<(String, MarkList), Value>>>,
    parent: Option<Rc<Environment>>,
}

impl Environment {
    /// Create a new empty environment
    pub fn new() -> Self {
        Environment {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            marked_bindings: Rc::new(RefCell::new(HashMap::new())),
            parent: None,
        }
    }

    /// Create a new environment with a parent
    #[allow(dead_code)]
    pub fn with_parent(parent: Rc<Environment>) -> Self {
        Environment {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            marked_bindings: Rc::new(RefCell::new(HashMap::new())),
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
}
