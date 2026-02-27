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

use crate::tagged_value::TaggedValue;

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
#[derive(Clone, Debug, Copy)]
pub enum MatchValue {
    /// Single value (level 0) - stores TaggedValue directly
    Leaf(TaggedValue),

    /// List of matched values (level > 0)
    ///
    /// For a pattern variable at level N, Branch contains N-1 layers of nesting.
    /// Each layer corresponds to one ellipsis level.
    ///
    /// Note: Branch is stored as a heap index pointing to a Vec<MatchValue>
    /// This allows MatchValue to remain Copy while supporting nested structures.
    Branch(u32), // Index into MatchEnv's branches storage
}

/// Storage for MatchValue branches, kept separate to allow MatchValue to be Copy
#[derive(Clone, Debug, Default)]
pub struct MatchBranches {
    branches: Vec<Vec<MatchValue>>,
}

impl MatchBranches {
    /// Create new empty branch storage
    pub fn new() -> Self {
        Self {
            branches: Vec::new(),
        }
    }

    /// Allocate a new branch, returning its index
    pub fn alloc(&mut self, children: Vec<MatchValue>) -> u32 {
        let index = self.branches.len() as u32;
        self.branches.push(children);
        index
    }

    /// Get a branch by index
    pub fn get(&self, index: u32) -> Option<&Vec<MatchValue>> {
        self.branches.get(index as usize)
    }
}

impl MatchValue {
    /// Create a leaf value from TaggedValue
    pub fn leaf(value: TaggedValue) -> Self {
        MatchValue::Leaf(value)
    }

    /// Create a branch with given children (requires branch storage)
    pub fn branch(children: Vec<MatchValue>, branches: &mut MatchBranches) -> Self {
        MatchValue::Branch(branches.alloc(children))
    }

    /// Create an empty branch (for zero-match ellipsis)
    pub fn empty_branch(branches: &mut MatchBranches) -> Self {
        MatchValue::Branch(branches.alloc(Vec::new()))
    }

    /// Check if this is a leaf
    pub fn is_leaf(&self) -> bool {
        matches!(self, MatchValue::Leaf(_))
    }

    /// Check if this is a branch
    pub fn is_branch(&self) -> bool {
        matches!(self, MatchValue::Branch(_))
    }

    /// Get the TaggedValue if this is a leaf
    pub fn as_leaf(&self) -> Option<TaggedValue> {
        match self {
            MatchValue::Leaf(v) => Some(*v),
            _ => None,
        }
    }

    /// Get the branch index if this is a branch
    pub fn branch_index(&self) -> Option<u32> {
        match self {
            MatchValue::Branch(idx) => Some(*idx),
            _ => None,
        }
    }

    /// Get as branch children (requires branch storage)
    pub fn as_branch<'a>(&self, branches: &'a MatchBranches) -> Option<&'a Vec<MatchValue>> {
        match self {
            MatchValue::Branch(idx) => branches.get(*idx),
            _ => None,
        }
    }
}

impl std::fmt::Display for MatchValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchValue::Leaf(v) => write!(f, "Leaf({:?})", v),
            MatchValue::Branch(idx) => write!(f, "Branch(#{})", idx),
        }
    }
}

/// Match environment - storage for pattern variable bindings during matching
///
/// Based on Gauche's MatchVar array and get_pvref_value (macro.c:730-750).
///
/// Uses a Vec indexed by PVREF (Pattern Variable Reference) index for O(1) lookups.
/// Each entry is a MatchValue tree representing the variable's bindings.
///
/// ## TaggedValue Storage
///
/// MatchEnv stores TaggedValue directly in MatchValue::Leaf variants.
/// Branch structures are stored separately in MatchBranches to keep MatchValue small (Copy).
pub struct MatchEnv {
    /// Array of match values, indexed by PVREF.index
    vars: Vec<MatchValue>,
    /// Storage for branch structures (nested ellipsis)
    branches: MatchBranches,
}

impl MatchEnv {
    /// Create a new match environment with space for `num_vars` variables
    pub fn new(num_vars: usize) -> Self {
        Self {
            vars: vec![MatchValue::Leaf(TaggedValue::NULL); num_vars],
            branches: MatchBranches::new(),
        }
    }

    /// Insert a TaggedValue for a pattern variable
    ///
    /// Used for level-0 variables (not in any ellipsis)
    pub fn insert(&mut self, pvref: PVRef, value: TaggedValue) {
        if pvref.index() < self.vars.len() {
            self.vars[pvref.index()] = MatchValue::Leaf(value);
        }
    }

    /// Insert a branch (list of values) for a pattern variable
    ///
    /// Used for level > 0 variables (inside ellipsis)
    pub fn insert_branch(&mut self, pvref: PVRef, values: Vec<MatchValue>) {
        if pvref.index() < self.vars.len() {
            self.vars[pvref.index()] = MatchValue::branch(values, &mut self.branches);
        }
    }

    /// Get reference to branches storage
    pub fn branches(&self) -> &MatchBranches {
        &self.branches
    }

    /// Get mutable reference to branches storage
    pub fn branches_mut(&mut self) -> &mut MatchBranches {
        &mut self.branches
    }

    /// Get TaggedValue at given PVREF location by navigating the match tree
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
    /// - `Some(TaggedValue)` if the value exists at the given indices
    /// - `None` if the indices are exhausted (iteration complete)
    pub fn get(&self, pvref: PVRef, indices: &[usize]) -> Option<TaggedValue> {
        if pvref.index() >= self.vars.len() {
            return None;
        }

        let mut val = self.vars[pvref.index()];

        // Navigate tree using indices
        // The indices array is 1-indexed: indices[0] is unused, indices[level] is for level
        for level in 1..=pvref.level() {
            match val {
                MatchValue::Branch(branch_idx) => {
                    let items = self.branches.get(branch_idx)?;
                    if level >= indices.len() || indices[level] >= items.len() {
                        return None; // Index out of bounds
                    }
                    val = items[indices[level]];
                }
                MatchValue::Leaf(_) => return None, // Type mismatch - shouldn't happen
            }
        }

        // At the final level, extract the leaf value
        match val {
            MatchValue::Leaf(tv) => Some(tv),
            MatchValue::Branch(_) => None, // Expected leaf, got branch - shouldn't happen
        }
    }

    /// Get the raw MatchValue for a variable (for debugging)
    pub fn get_raw(&self, pvref: PVRef) -> Option<MatchValue> {
        self.vars.get(pvref.index()).copied()
    }

    /// Get the number of variables in this environment
    pub fn num_vars(&self) -> usize {
        self.vars.len()
    }

    /// Get all match values (for iteration)
    pub fn all_values(&self) -> &[MatchValue] {
        &self.vars
    }

    /// Deep copy a MatchValue from another MatchEnv into this one
    ///
    /// This method recursively copies branch structures, properly reindexing
    /// branch references to point to this env's branches storage.
    ///
    /// This is necessary because MatchValue::Branch(idx) stores an index into
    /// the source env's branches, which needs to be translated when copying
    /// to a different env.
    ///
    /// # Arguments
    /// * `src_value` - The MatchValue to copy
    /// * `src_env` - The source MatchEnv that owns the branch data
    ///
    /// # Returns
    /// A new MatchValue with branch indices pointing to this env's branches
    pub fn copy_match_value_from(
        &mut self,
        src_value: MatchValue,
        src_env: &MatchEnv,
    ) -> MatchValue {
        match src_value {
            MatchValue::Leaf(tv) => MatchValue::Leaf(tv),
            MatchValue::Branch(src_idx) => {
                // Get the branch contents from source
                if let Some(src_children) = src_env.branches.get(src_idx) {
                    // Recursively copy children, reindexing any nested branches
                    let copied_children: Vec<MatchValue> = src_children
                        .iter()
                        .map(|child| self.copy_match_value_from(*child, src_env))
                        .collect();
                    // Allocate new branch in this env
                    MatchValue::branch(copied_children, &mut self.branches)
                } else {
                    // Invalid branch index - return empty branch
                    MatchValue::empty_branch(&mut self.branches)
                }
            }
        }
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

    /// Helper to create a fixnum TaggedValue for tests
    fn fixnum(n: i64) -> TaggedValue {
        TaggedValue::fixnum(n)
    }

    /// Helper to assert TaggedValue equality
    fn assert_tv_eq(actual: Option<TaggedValue>, expected: Option<TaggedValue>) {
        assert_eq!(actual, expected);
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
        let val = MatchValue::leaf(fixnum(42));
        assert!(val.is_leaf());
        assert!(!val.is_branch());
        assert_eq!(val.as_leaf(), Some(fixnum(42)));
    }

    #[test]
    fn test_match_value_branch() {
        let mut branches = MatchBranches::new();
        let val = MatchValue::branch(
            vec![MatchValue::leaf(fixnum(1)), MatchValue::leaf(fixnum(2))],
            &mut branches,
        );
        assert!(!val.is_leaf());
        assert!(val.is_branch());
        assert_eq!(val.as_branch(&branches).unwrap().len(), 2);
    }

    #[test]
    fn test_match_env_simple() {
        let mut env = MatchEnv::new(3);

        let pvref = PVRef::new(0, 0);
        env.insert(pvref, fixnum(42));

        let retrieved = env.get(pvref, &[0]);
        assert_tv_eq(retrieved, Some(fixnum(42)));
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
                MatchValue::leaf(fixnum(1)),
                MatchValue::leaf(fixnum(2)),
                MatchValue::leaf(fixnum(3)),
            ],
        );

        // Access using indices
        assert_tv_eq(env.get(pvref, &[0, 0]), Some(fixnum(1)));
        assert_tv_eq(env.get(pvref, &[0, 1]), Some(fixnum(2)));
        assert_tv_eq(env.get(pvref, &[0, 2]), Some(fixnum(3)));
        assert_tv_eq(env.get(pvref, &[0, 3]), None); // Exhausted
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
            vec![MatchValue::leaf(fixnum(1)), MatchValue::leaf(fixnum(4))],
        );

        // b at level 2 - need to build nested branches
        let b_pvref = PVRef::new(2, 1);
        let inner1 = MatchValue::branch(
            vec![MatchValue::leaf(fixnum(2)), MatchValue::leaf(fixnum(3))],
            env.branches_mut(),
        );
        let inner2 = MatchValue::branch(vec![MatchValue::leaf(fixnum(5))], env.branches_mut());
        env.insert_branch(b_pvref, vec![inner1, inner2]);

        // Test 'a' access
        assert_tv_eq(env.get(a_pvref, &[0, 0]), Some(fixnum(1)));
        assert_tv_eq(env.get(a_pvref, &[0, 1]), Some(fixnum(4)));
        assert_tv_eq(env.get(a_pvref, &[0, 2]), None); // Exhausted

        // Test 'b' access - need 2 indices for level 2
        assert_tv_eq(env.get(b_pvref, &[0, 0, 0]), Some(fixnum(2)));
        assert_tv_eq(env.get(b_pvref, &[0, 0, 1]), Some(fixnum(3)));
        assert_tv_eq(env.get(b_pvref, &[0, 0, 2]), None); // Inner exhausted
        assert_tv_eq(env.get(b_pvref, &[0, 1, 0]), Some(fixnum(5)));
        assert_tv_eq(env.get(b_pvref, &[0, 1, 1]), None); // Inner exhausted
        assert_tv_eq(env.get(b_pvref, &[0, 2, 0]), None); // Outer exhausted
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
        env.insert(a_pvref, fixnum(1));

        // b => (2 7) (level 1)
        let b_pvref = PVRef::new(1, 1);
        env.insert_branch(
            b_pvref,
            vec![MatchValue::leaf(fixnum(2)), MatchValue::leaf(fixnum(7))],
        );

        // c => ((3 6) (8 10)) (level 2)
        let c_pvref = PVRef::new(2, 2);
        let c_inner1 = MatchValue::branch(
            vec![MatchValue::leaf(fixnum(3)), MatchValue::leaf(fixnum(6))],
            env.branches_mut(),
        );
        let c_inner2 = MatchValue::branch(
            vec![MatchValue::leaf(fixnum(8)), MatchValue::leaf(fixnum(10))],
            env.branches_mut(),
        );
        env.insert_branch(c_pvref, vec![c_inner1, c_inner2]);

        // d => (((4 5) ()) ((9) (11 12))) (level 3)
        let d_pvref = PVRef::new(3, 3);
        // Build innermost branches first
        let d_inner_45 = MatchValue::branch(
            vec![MatchValue::leaf(fixnum(4)), MatchValue::leaf(fixnum(5))],
            env.branches_mut(),
        );
        let d_inner_empty = MatchValue::branch(vec![], env.branches_mut());
        let d_inner_9 = MatchValue::branch(vec![MatchValue::leaf(fixnum(9))], env.branches_mut());
        let d_inner_1112 = MatchValue::branch(
            vec![MatchValue::leaf(fixnum(11)), MatchValue::leaf(fixnum(12))],
            env.branches_mut(),
        );
        // Build middle branches
        let d_mid1 = MatchValue::branch(vec![d_inner_45, d_inner_empty], env.branches_mut());
        let d_mid2 = MatchValue::branch(vec![d_inner_9, d_inner_1112], env.branches_mut());
        env.insert_branch(d_pvref, vec![d_mid1, d_mid2]);

        // Test 'a' (level 0 - no indices needed)
        assert_tv_eq(env.get(a_pvref, &[0]), Some(fixnum(1)));

        // Test 'b' (level 1)
        assert_tv_eq(env.get(b_pvref, &[0, 0]), Some(fixnum(2)));
        assert_tv_eq(env.get(b_pvref, &[0, 1]), Some(fixnum(7)));

        // Test 'c' (level 2)
        assert_tv_eq(env.get(c_pvref, &[0, 0, 0]), Some(fixnum(3)));
        assert_tv_eq(env.get(c_pvref, &[0, 0, 1]), Some(fixnum(6)));
        assert_tv_eq(env.get(c_pvref, &[0, 1, 0]), Some(fixnum(8)));
        assert_tv_eq(env.get(c_pvref, &[0, 1, 1]), Some(fixnum(10)));

        // Test 'd' (level 3)
        assert_tv_eq(env.get(d_pvref, &[0, 0, 0, 0]), Some(fixnum(4)));
        assert_tv_eq(env.get(d_pvref, &[0, 0, 0, 1]), Some(fixnum(5)));
        assert_tv_eq(env.get(d_pvref, &[0, 0, 1, 0]), None); // Empty
        assert_tv_eq(env.get(d_pvref, &[0, 1, 0, 0]), Some(fixnum(9)));
        assert_tv_eq(env.get(d_pvref, &[0, 1, 1, 0]), Some(fixnum(11)));
        assert_tv_eq(env.get(d_pvref, &[0, 1, 1, 1]), Some(fixnum(12)));
    }
}
