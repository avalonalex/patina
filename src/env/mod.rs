use crate::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Environment for variable bindings
/// Uses Rc<RefCell<>> for shared mutable state (needed for set!)
#[derive(Debug, Clone)]
pub struct Environment {
    bindings: Rc<RefCell<HashMap<String, Value>>>,
    parent: Option<Rc<Environment>>,
}

impl Environment {
    /// Create a new empty environment
    pub fn new() -> Self {
        Environment {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            parent: None,
        }
    }

    /// Create a new environment with a parent
    #[allow(dead_code)]
    pub fn with_parent(parent: Rc<Environment>) -> Self {
        Environment {
            bindings: Rc::new(RefCell::new(HashMap::new())),
            parent: Some(parent),
        }
    }

    /// Define a new binding in this environment
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
    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(value) = self.bindings.borrow().get(name) {
            Some(value.clone())
        } else if let Some(parent) = &self.parent {
            parent.get(name)
        } else {
            None
        }
    }

    /// Check if a binding exists
    #[allow(dead_code)]
    pub fn has(&self, name: &str) -> bool {
        self.bindings.borrow().contains_key(name)
            || self.parent.as_ref().is_some_and(|p| p.has(name))
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
}
