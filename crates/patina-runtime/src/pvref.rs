//! Pattern Variable Reference (PVREF) for macro system
//!
//! This module implements PVREF (Pattern Variable Reference) encoding and
//! match value storage for the macro system's pattern matching and template expansion.
//!
//! Inspired by Gauche Scheme's macro.c implementation by Shiro Kawai.
//! Key concepts:
//! - PVREF encoding: (level, index) for pattern variables
//! - Tree-based MatchValue storage for nested ellipsis
//!
//! Reference: https://github.com/shirok/Gauche/blob/master/src/macro.c
//! - PVREF definition: macro.c:297-300, macroP.h:133-139
//! - MatchVar structure: macro.c:709-726
//! - Tree navigation: macro.c:730-750

use crate::Value;

/// Pattern Variable Reference (PVREF) - compact encoding of pattern variable location
///
/// Inspired by Gauche's PVREF (Pattern Variable Reference) encoding (macro.c:297-300, macroP.h:133-139).
///
/// A PVREF encodes both the ellipsis nesting level and the variable's unique index:
/// - **level**: Ellipsis nesting depth (0 = not in ellipsis, 1 = in one `...`, 2 = nested, etc.)
/// - **index**: Unique identifier for this variable within the pattern (0, 1, 2, ...)
///
/// # Example
///
/// ```text
/// Pattern: (foo (bar x ...) y ...)
///
/// Variables:
///   x   -> PVRef { level: 1, index: 0 }
///   y   -> PVRef { level: 1, index: 1 }
///   bar -> PVRef { level: 0, index: 2 }
///   foo -> PVRef { level: 0, index: 3 }
/// ```
///
/// This encoding allows O(1) array indexing instead of HashMap lookups,
/// and makes it easy to detect variable level mismatches at compile time.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PVRef {
    /// Ellipsis nesting depth
    /// - 0 = not in any ellipsis
    /// - 1 = inside one `...`
    /// - 2 = inside nested `... ...`
    /// - etc.
    level: u8,

    /// Unique index for this variable within the pattern
    /// Used for array indexing in MatchEnv
    index: u8,
}

impl PVRef {
    /// Create a new PVREF
    ///
    /// # Arguments
    /// - `level`: Ellipsis nesting depth (0-255)
    /// - `index`: Variable index within pattern (0-255)
    pub fn new(level: u8, index: u8) -> Self {
        Self { level, index }
    }

    /// Get the ellipsis nesting level
    pub fn level(&self) -> usize {
        self.level as usize
    }

    /// Get the variable index
    pub fn index(&self) -> usize {
        self.index as usize
    }

    /// Pack PVREF (Pattern Variable Reference) into a u16 for compact storage
    ///
    /// Layout: [level:8][index:8]
    ///
    /// This matches Gauche's approach of packing PVREF into a single word
    /// (macro.c:138-139), though Gauche uses more bits since they pack into
    /// a pointer-sized word.
    pub fn pack(self) -> u16 {
        ((self.level as u16) << 8) | (self.index as u16)
    }

    /// Unpack PVREF (Pattern Variable Reference) from a u16
    pub fn unpack(packed: u16) -> Self {
        Self {
            level: (packed >> 8) as u8,
            index: (packed & 0xFF) as u8,
        }
    }
}

impl std::fmt::Display for PVRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PVRef(level={}, index={})", self.level, self.index)
    }
}

/// Match value storage - tree structure for nested ellipsis
///
/// Based on Gauche's MatchVar structure (macro.c:709-726).
///
/// The tree structure naturally represents ellipsis nesting levels:
/// - `Leaf`: Single matched value (level 0)
/// - `Branch`: List of matched values (level > 0)
///
/// # Example from Gauche docs (macro.c:689-707)
///
/// ```text
/// Pattern: (a (b (c d ...) ...) ...)
/// Variables: a=level0, b=level1, c=level2, d=level3
///
/// Matched form: (1 (2 (3 4 5) (6)) (7 (8 9) (10 11 12)))
///
/// Bindings (tree structure):
/// a => Leaf(1)
/// b => Branch([Leaf(2), Leaf(7)])
/// c => Branch([Branch([Leaf(3), Leaf(6)]), Branch([Leaf(8), Leaf(10)])])
/// d => Branch([Branch([Branch([Leaf(4), Leaf(5)]), Branch([])]),
///              Branch([Branch([Leaf(9)]), Branch([Leaf(11), Leaf(12)])])])
/// ```
#[derive(Clone, Debug)]
pub enum MatchValue {
    /// Single value (level 0)
    Leaf(Value),

    /// List of matched values (level > 0)
    ///
    /// For a pattern variable at level N, Branch contains N-1 layers of nesting.
    /// Each layer corresponds to one ellipsis level.
    Branch(Vec<MatchValue>),
}

impl MatchValue {
    /// Create a leaf value
    pub fn leaf(value: Value) -> Self {
        MatchValue::Leaf(value)
    }

    /// Create a branch with given children
    pub fn branch(children: Vec<MatchValue>) -> Self {
        MatchValue::Branch(children)
    }

    /// Create an empty branch (for zero-match ellipsis)
    pub fn empty_branch() -> Self {
        MatchValue::Branch(Vec::new())
    }

    /// Check if this is a leaf
    pub fn is_leaf(&self) -> bool {
        matches!(self, MatchValue::Leaf(_))
    }

    /// Check if this is a branch
    pub fn is_branch(&self) -> bool {
        matches!(self, MatchValue::Branch(_))
    }

    /// Get as leaf value if this is a leaf
    pub fn as_leaf(&self) -> Option<&Value> {
        match self {
            MatchValue::Leaf(v) => Some(v),
            _ => None,
        }
    }

    /// Get as branch if this is a branch
    pub fn as_branch(&self) -> Option<&Vec<MatchValue>> {
        match self {
            MatchValue::Branch(children) => Some(children),
            _ => None,
        }
    }
}

impl std::fmt::Display for MatchValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchValue::Leaf(v) => write!(f, "Leaf({})", v),
            MatchValue::Branch(children) => {
                write!(f, "Branch([")?;
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", child)?;
                }
                write!(f, "])")
            }
        }
    }
}

/// Match environment - storage for pattern variable bindings during matching
///
/// Based on Gauche's MatchVar array and get_pvref_value (macro.c:730-750).
///
/// Uses a Vec indexed by PVREF (Pattern Variable Reference) index for O(1) lookups.
/// Each entry is a MatchValue tree representing the variable's bindings.
pub struct MatchEnv {
    /// Array of match values, indexed by PVREF.index
    vars: Vec<MatchValue>,
}

impl MatchEnv {
    /// Create a new match environment with space for `num_vars` variables
    pub fn new(num_vars: usize) -> Self {
        Self {
            vars: vec![MatchValue::Leaf(Value::Null); num_vars],
        }
    }

    /// Insert a single value for a pattern variable
    ///
    /// Used for level-0 variables (not in any ellipsis)
    pub fn insert(&mut self, pvref: PVRef, value: Value) {
        if pvref.index() < self.vars.len() {
            self.vars[pvref.index()] = MatchValue::Leaf(value);
        }
    }

    /// Insert a branch (list of values) for a pattern variable
    ///
    /// Used for level > 0 variables (inside ellipsis)
    pub fn insert_branch(&mut self, pvref: PVRef, values: Vec<MatchValue>) {
        if pvref.index() < self.vars.len() {
            self.vars[pvref.index()] = MatchValue::Branch(values);
        }
    }

    /// Get value at given PVREF (Pattern Variable Reference) location by navigating the match tree
    ///
    /// Based on Gauche's get_pvref_value (macro.c:730-750).
    ///
    /// # Arguments
    /// - `pvref`: The Pattern Variable Reference
    /// - `indices`: Array of indices for each ellipsis level
    ///   - `indices[0]` is unused
    ///   - `indices[1..=level]` are used to navigate the tree
    ///
    /// # Returns
    /// - `Some(value)` if the value exists at the given indices
    /// - `None` if the indices are exhausted (iteration complete)
    ///
    /// # Example
    ///
    /// ```text
    /// Pattern: ((a b ...) ...)
    /// Matched: ((1 2 3) (4 5))
    ///
    /// get(a_pvref, &[_, 0]) => Some(1)
    /// get(a_pvref, &[_, 1]) => Some(4)
    /// get(b_pvref, &[_, 0, 0]) => Some(2)
    /// get(b_pvref, &[_, 0, 1]) => Some(3)
    /// get(b_pvref, &[_, 1, 0]) => Some(5)
    /// get(b_pvref, &[_, 1, 1]) => None (exhausted)
    /// ```
    pub fn get(&self, pvref: PVRef, indices: &[usize]) -> Option<Value> {
        if pvref.index() >= self.vars.len() {
            return None;
        }

        let mut val = &self.vars[pvref.index()];

        // Navigate tree using indices
        // The indices array is 1-indexed: indices[0] is unused, indices[level] is for level
        for level in 1..=pvref.level() {
            match val {
                MatchValue::Branch(items) => {
                    if level >= indices.len() || indices[level] >= items.len() {
                        return None; // Index out of bounds
                    }
                    val = &items[indices[level]];
                }
                MatchValue::Leaf(_) => return None, // Type mismatch - shouldn't happen
            }
        }

        // At the final level, extract the leaf value
        match val {
            MatchValue::Leaf(expr) => Some(expr.clone()),
            MatchValue::Branch(_) => None, // Expected leaf, got branch - shouldn't happen
        }
    }

    /// Get the raw MatchValue for a variable (for debugging)
    pub fn get_raw(&self, pvref: PVRef) -> Option<&MatchValue> {
        self.vars.get(pvref.index())
    }

    /// Get the number of variables in this environment
    pub fn num_vars(&self) -> usize {
        self.vars.len()
    }
}

impl std::fmt::Debug for MatchEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatchEnv")
            .field("num_vars", &self.vars.len())
            .field("vars", &self.vars)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to assert Option<Value> equality using Debug repr
    fn assert_value_eq(actual: Option<Value>, expected: Option<Value>) {
        assert_eq!(format!("{:?}", actual), format!("{:?}", expected));
    }

    #[test]
    fn test_pvref_creation() {
        // Test PVREF (Pattern Variable Reference) creation
        let pvref = PVRef::new(2, 5);
        assert_eq!(pvref.level(), 2);
        assert_eq!(pvref.index(), 5);
    }

    #[test]
    fn test_pvref_pack_unpack() {
        // Test PVREF (Pattern Variable Reference) packing/unpacking
        let pvref = PVRef::new(3, 42);
        let packed = pvref.pack();
        let unpacked = PVRef::unpack(packed);
        assert_eq!(pvref, unpacked);
    }

    #[test]
    fn test_pvref_display() {
        // Test PVREF (Pattern Variable Reference) display format
        let pvref = PVRef::new(1, 2);
        assert_eq!(format!("{}", pvref), "PVRef(level=1, index=2)");
    }

    #[test]
    fn test_match_value_leaf() {
        let val = MatchValue::leaf(Value::Integer(42));
        assert!(val.is_leaf());
        assert!(!val.is_branch());
        match val.as_leaf() {
            Some(v) => assert_eq!(format!("{}", v), "42"),
            None => panic!("Expected leaf value"),
        }
    }

    #[test]
    fn test_match_value_branch() {
        let val = MatchValue::branch(vec![
            MatchValue::leaf(Value::Integer(1)),
            MatchValue::leaf(Value::Integer(2)),
        ]);
        assert!(!val.is_leaf());
        assert!(val.is_branch());
        assert_eq!(val.as_branch().unwrap().len(), 2);
    }

    #[test]
    fn test_match_env_simple() {
        let mut env = MatchEnv::new(3);

        let pvref = PVRef::new(0, 0);
        env.insert(pvref, Value::Integer(42));

        let retrieved = env.get(pvref, &[0]);
        assert_value_eq(retrieved, Some(Value::Integer(42)));
    }

    #[test]
    fn test_match_env_single_ellipsis() {
        // Pattern: (x ...)
        // Matched: (1 2 3)
        let mut env = MatchEnv::new(1);

        let pvref = PVRef::new(1, 0); // x at level 1
        env.insert_branch(
            pvref,
            vec![
                MatchValue::leaf(Value::Integer(1)),
                MatchValue::leaf(Value::Integer(2)),
                MatchValue::leaf(Value::Integer(3)),
            ],
        );

        // Access using indices
        assert_value_eq(env.get(pvref, &[0, 0]), Some(Value::Integer(1)));
        assert_value_eq(env.get(pvref, &[0, 1]), Some(Value::Integer(2)));
        assert_value_eq(env.get(pvref, &[0, 2]), Some(Value::Integer(3)));
        assert_value_eq(env.get(pvref, &[0, 3]), None); // Exhausted
    }

    #[test]
    fn test_match_env_nested_ellipsis() {
        // Pattern: ((a b ...) ...)
        // Matched: ((1 2 3) (4 5))
        //
        // a at level 1: [1, 4]
        // b at level 2: [[2, 3], [5]]
        let mut env = MatchEnv::new(2);

        // a at level 1
        let a_pvref = PVRef::new(1, 0);
        env.insert_branch(
            a_pvref,
            vec![
                MatchValue::leaf(Value::Integer(1)),
                MatchValue::leaf(Value::Integer(4)),
            ],
        );

        // b at level 2
        let b_pvref = PVRef::new(2, 1);
        env.insert_branch(
            b_pvref,
            vec![
                MatchValue::branch(vec![
                    MatchValue::leaf(Value::Integer(2)),
                    MatchValue::leaf(Value::Integer(3)),
                ]),
                MatchValue::branch(vec![MatchValue::leaf(Value::Integer(5))]),
            ],
        );

        // Test 'a' access
        assert_value_eq(env.get(a_pvref, &[0, 0]), Some(Value::Integer(1)));
        assert_value_eq(env.get(a_pvref, &[0, 1]), Some(Value::Integer(4)));
        assert_value_eq(env.get(a_pvref, &[0, 2]), None); // Exhausted

        // Test 'b' access - need 2 indices for level 2
        assert_value_eq(env.get(b_pvref, &[0, 0, 0]), Some(Value::Integer(2)));
        assert_value_eq(env.get(b_pvref, &[0, 0, 1]), Some(Value::Integer(3)));
        assert_value_eq(env.get(b_pvref, &[0, 0, 2]), None); // Inner exhausted
        assert_value_eq(env.get(b_pvref, &[0, 1, 0]), Some(Value::Integer(5)));
        assert_value_eq(env.get(b_pvref, &[0, 1, 1]), None); // Inner exhausted
        assert_value_eq(env.get(b_pvref, &[0, 2, 0]), None); // Outer exhausted
    }

    #[test]
    fn test_match_env_gauche_example() {
        // Example from Gauche docs (macro.c:689-707)
        // Pattern: (a (b (c d ...) ...) ...)
        // Variables: a=level0, b=level1, c=level2, d=level3
        // Matched: (1 (2 (3 4 5) (6)) (7 (8 9) (10 11 12)))

        let mut env = MatchEnv::new(4);

        // a => 1 (level 0)
        let a_pvref = PVRef::new(0, 0);
        env.insert(a_pvref, Value::Integer(1));

        // b => (2 7) (level 1)
        let b_pvref = PVRef::new(1, 1);
        env.insert_branch(
            b_pvref,
            vec![
                MatchValue::leaf(Value::Integer(2)),
                MatchValue::leaf(Value::Integer(7)),
            ],
        );

        // c => ((3 6) (8 10)) (level 2)
        let c_pvref = PVRef::new(2, 2);
        env.insert_branch(
            c_pvref,
            vec![
                MatchValue::branch(vec![
                    MatchValue::leaf(Value::Integer(3)),
                    MatchValue::leaf(Value::Integer(6)),
                ]),
                MatchValue::branch(vec![
                    MatchValue::leaf(Value::Integer(8)),
                    MatchValue::leaf(Value::Integer(10)),
                ]),
            ],
        );

        // d => (((4 5) ()) ((9) (11 12))) (level 3)
        let d_pvref = PVRef::new(3, 3);
        env.insert_branch(
            d_pvref,
            vec![
                MatchValue::branch(vec![
                    MatchValue::branch(vec![
                        MatchValue::leaf(Value::Integer(4)),
                        MatchValue::leaf(Value::Integer(5)),
                    ]),
                    MatchValue::branch(vec![]),
                ]),
                MatchValue::branch(vec![
                    MatchValue::branch(vec![MatchValue::leaf(Value::Integer(9))]),
                    MatchValue::branch(vec![
                        MatchValue::leaf(Value::Integer(11)),
                        MatchValue::leaf(Value::Integer(12)),
                    ]),
                ]),
            ],
        );

        // Test 'a' (level 0 - no indices needed)
        assert_value_eq(env.get(a_pvref, &[0]), Some(Value::Integer(1)));

        // Test 'b' (level 1)
        assert_value_eq(env.get(b_pvref, &[0, 0]), Some(Value::Integer(2)));
        assert_value_eq(env.get(b_pvref, &[0, 1]), Some(Value::Integer(7)));

        // Test 'c' (level 2)
        assert_value_eq(env.get(c_pvref, &[0, 0, 0]), Some(Value::Integer(3)));
        assert_value_eq(env.get(c_pvref, &[0, 0, 1]), Some(Value::Integer(6)));
        assert_value_eq(env.get(c_pvref, &[0, 1, 0]), Some(Value::Integer(8)));
        assert_value_eq(env.get(c_pvref, &[0, 1, 1]), Some(Value::Integer(10)));

        // Test 'd' (level 3)
        assert_value_eq(env.get(d_pvref, &[0, 0, 0, 0]), Some(Value::Integer(4)));
        assert_value_eq(env.get(d_pvref, &[0, 0, 0, 1]), Some(Value::Integer(5)));
        assert_value_eq(env.get(d_pvref, &[0, 0, 1, 0]), None); // Empty
        assert_value_eq(env.get(d_pvref, &[0, 1, 0, 0]), Some(Value::Integer(9)));
        assert_value_eq(env.get(d_pvref, &[0, 1, 1, 0]), Some(Value::Integer(11)));
        assert_value_eq(env.get(d_pvref, &[0, 1, 1, 1]), Some(Value::Integer(12)));
    }
}
