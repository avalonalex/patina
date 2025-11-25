//! Scope sets for macro hygiene
//!
//! This module implements the scope sets approach to macro hygiene, based on
//! Matthew Flatt's "Binding as Sets of Scopes" (POPL 2016).
//!
//! Key concepts:
//! - **ScopeId**: A unique identifier for each binding form (lambda, let, let-syntax, etc.)
//! - **ScopeSet**: The set of scopes an identifier is "inside of"
//! - **Lookup rule**: A binding matches a reference if `binding.scopes ⊆ reference.scopes`
//!
//! This approach solves hygiene without renaming - the same name can have different
//! bindings distinguished by their scope sets.
//!
//! ## Example
//!
//! ```scheme
//! (let ((x 'outer))           ; creates scope S1, binds x with scopes {S1}
//!   (let-syntax ((m ...))     ; creates scope S2
//!     (let ((x 'inner))       ; creates scope S3, binds x with scopes {S1, S2, S3}
//!       (m))))                ; m's free var x has scopes {S1, S2}
//! ```
//!
//! When looking up `x` with scopes {S1, S2}:
//! - Outer x: {S1} ⊆ {S1, S2} ✓ (matches!)
//! - Inner x: {S1, S2, S3} ⊈ {S1, S2} ✗ (doesn't match - S3 not in reference)
//!
//! Result: correctly finds outer x, achieving hygiene.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Global counter for generating unique scope IDs
static SCOPE_COUNTER: AtomicUsize = AtomicUsize::new(1);

/// A unique identifier for a lexical scope (binding form)
///
/// Each binding form (lambda, let, let-syntax, etc.) creates a fresh ScopeId.
/// Scope 0 is reserved for the "top-level" or "empty" scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeId(pub usize);

impl ScopeId {
    /// Create a fresh, unique scope ID
    pub fn fresh() -> Self {
        ScopeId(SCOPE_COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// The empty/top-level scope (scope 0)
    pub const TOP_LEVEL: ScopeId = ScopeId(0);

    /// Get the raw scope ID value
    pub fn as_usize(&self) -> usize {
        self.0
    }
}

impl std::fmt::Display for ScopeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "S{}", self.0)
    }
}

/// A set of scopes that an identifier carries
///
/// Identifiers accumulate scopes as they pass through binding forms.
/// An identifier introduced at top-level has an empty scope set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScopeSet {
    scopes: HashSet<ScopeId>,
}

impl ScopeSet {
    /// Create an empty scope set (for top-level identifiers)
    pub fn new() -> Self {
        ScopeSet {
            scopes: HashSet::new(),
        }
    }

    /// Create a scope set with a single scope
    pub fn singleton(scope: ScopeId) -> Self {
        let mut scopes = HashSet::new();
        scopes.insert(scope);
        ScopeSet { scopes }
    }

    /// Add a scope to the set (returns new set, original unchanged)
    pub fn with_scope(&self, scope: ScopeId) -> Self {
        let mut scopes = self.scopes.clone();
        scopes.insert(scope);
        ScopeSet { scopes }
    }

    /// Add a scope to this set (mutates in place)
    pub fn add_scope(&mut self, scope: ScopeId) {
        self.scopes.insert(scope);
    }

    /// Remove a scope from the set (returns new set, original unchanged)
    /// Used during macro expansion when "flipping" scopes
    pub fn without_scope(&self, scope: ScopeId) -> Self {
        let mut scopes = self.scopes.clone();
        scopes.remove(&scope);
        ScopeSet { scopes }
    }

    /// Check if this scope set is a subset of another
    ///
    /// This is the key operation for hygiene: a binding matches a reference
    /// if `binding.scopes ⊆ reference.scopes`
    pub fn is_subset_of(&self, other: &ScopeSet) -> bool {
        self.scopes.is_subset(&other.scopes)
    }

    /// Check if this scope set is a proper subset of another
    pub fn is_proper_subset_of(&self, other: &ScopeSet) -> bool {
        self.scopes.is_subset(&other.scopes) && self.scopes.len() < other.scopes.len()
    }

    /// Check if the scope set is empty
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }

    /// Get the number of scopes
    pub fn len(&self) -> usize {
        self.scopes.len()
    }

    /// Check if a specific scope is in the set
    pub fn contains(&self, scope: &ScopeId) -> bool {
        self.scopes.contains(scope)
    }

    /// Iterate over the scopes
    pub fn iter(&self) -> impl Iterator<Item = &ScopeId> {
        self.scopes.iter()
    }

    /// Convert to a sorted vector (for consistent display/comparison)
    pub fn to_sorted_vec(&self) -> Vec<ScopeId> {
        let mut vec: Vec<_> = self.scopes.iter().copied().collect();
        vec.sort();
        vec
    }
}

impl std::fmt::Display for ScopeSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sorted = self.to_sorted_vec();
        write!(f, "{{")?;
        for (i, scope) in sorted.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", scope)?;
        }
        write!(f, "}}")
    }
}

impl From<HashSet<ScopeId>> for ScopeSet {
    fn from(scopes: HashSet<ScopeId>) -> Self {
        ScopeSet { scopes }
    }
}

impl std::iter::FromIterator<ScopeId> for ScopeSet {
    fn from_iter<I: IntoIterator<Item = ScopeId>>(iter: I) -> Self {
        ScopeSet {
            scopes: iter.into_iter().collect(),
        }
    }
}

impl From<ScopeSet> for HashSet<ScopeId> {
    fn from(scope_set: ScopeSet) -> Self {
        scope_set.scopes
    }
}

/// Reset the scope counter (for testing only)
#[cfg(test)]
pub fn reset_scope_counter() {
    SCOPE_COUNTER.store(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fresh_scope_ids() {
        reset_scope_counter();
        let s1 = ScopeId::fresh();
        let s2 = ScopeId::fresh();
        let s3 = ScopeId::fresh();

        assert_eq!(s1.0, 1);
        assert_eq!(s2.0, 2);
        assert_eq!(s3.0, 3);
        assert_ne!(s1, s2);
        assert_ne!(s2, s3);
    }

    #[test]
    fn test_scope_set_subset() {
        let s1 = ScopeId(1);
        let s2 = ScopeId(2);
        let s3 = ScopeId(3);

        let set_1 = ScopeSet::singleton(s1);
        let set_12 = ScopeSet::from_iter([s1, s2]);
        let set_123 = ScopeSet::from_iter([s1, s2, s3]);

        // {S1} ⊆ {S1, S2} - true
        assert!(set_1.is_subset_of(&set_12));

        // {S1} ⊆ {S1, S2, S3} - true
        assert!(set_1.is_subset_of(&set_123));

        // {S1, S2, S3} ⊆ {S1, S2} - false (S3 not in right side)
        assert!(!set_123.is_subset_of(&set_12));

        // {S1, S2} ⊆ {S1, S2} - true (equal sets)
        assert!(set_12.is_subset_of(&set_12));
    }

    #[test]
    fn test_hygiene_example() {
        // Simulating:
        // (let ((x 'outer))           ; S1
        //   (let-syntax ((m ...))     ; S2
        //     (let ((x 'inner))       ; S3
        //       (m))))

        let s1 = ScopeId(1);
        let s2 = ScopeId(2);
        let s3 = ScopeId(3);

        // Outer x binding: introduced at S1
        let outer_x_scopes = ScopeSet::singleton(s1);

        // Inner x binding: inside S1, S2, S3
        let inner_x_scopes = ScopeSet::from_iter([s1, s2, s3]);

        // Free var x in macro m: inside S1, S2 (captured when macro was defined)
        let free_x_scopes = ScopeSet::from_iter([s1, s2]);

        // Lookup: find binding where binding.scopes ⊆ reference.scopes
        // Outer x: {S1} ⊆ {S1, S2} - YES
        assert!(outer_x_scopes.is_subset_of(&free_x_scopes));

        // Inner x: {S1, S2, S3} ⊆ {S1, S2} - NO (S3 not in reference)
        assert!(!inner_x_scopes.is_subset_of(&free_x_scopes));

        // So the lookup correctly finds outer x!
    }

    #[test]
    fn test_scope_set_operations() {
        let s1 = ScopeId(1);
        let s2 = ScopeId(2);

        let empty = ScopeSet::new();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let with_s1 = empty.with_scope(s1);
        assert!(!with_s1.is_empty());
        assert_eq!(with_s1.len(), 1);
        assert!(with_s1.contains(&s1));

        let with_s1_s2 = with_s1.with_scope(s2);
        assert_eq!(with_s1_s2.len(), 2);
        assert!(with_s1_s2.contains(&s1));
        assert!(with_s1_s2.contains(&s2));

        let back_to_s1 = with_s1_s2.without_scope(s2);
        assert_eq!(back_to_s1.len(), 1);
        assert!(back_to_s1.contains(&s1));
        assert!(!back_to_s1.contains(&s2));
    }

    #[test]
    fn test_display() {
        let s1 = ScopeId(1);
        let s2 = ScopeId(2);

        assert_eq!(format!("{}", s1), "S1");
        assert_eq!(format!("{}", ScopeSet::new()), "{}");
        assert_eq!(format!("{}", ScopeSet::singleton(s1)), "{S1}");

        let set = ScopeSet::from_iter([s1, s2]);
        let display = format!("{}", set);
        // Order might vary, but should contain both
        assert!(display.contains("S1"));
        assert!(display.contains("S2"));
    }
}
