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

use smallvec::SmallVec;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Global counter for generating unique scope IDs
static SCOPE_COUNTER: AtomicUsize = AtomicUsize::new(1);

// Thread-local storage for scope origins (for debugging)
thread_local! {
    static SCOPE_ORIGINS: RefCell<HashMap<usize, &'static str>> = RefCell::new(HashMap::new());
}

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

    /// Create a fresh scope ID with origin tracking (for debugging)
    ///
    /// The origin string describes where/why this scope was created.
    /// Examples: "lambda", "let-syntax", "macro:my-or"
    pub fn fresh_with_origin(origin: &'static str) -> Self {
        let id = Self::fresh();
        SCOPE_ORIGINS.with(|origins| {
            origins.borrow_mut().insert(id.0, origin);
        });

        // Print if macro debug is enabled
        if crate::macro_debug::is_enabled() {
            println!("[SCOPE] Created {} for '{}'", id, origin);
        }

        id
    }

    /// Get the origin of a scope (if it was created with origin tracking)
    pub fn origin(&self) -> Option<&'static str> {
        SCOPE_ORIGINS.with(|origins| origins.borrow().get(&self.0).copied())
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
///
/// This implementation uses SmallVec to store up to 3 scopes inline (24 bytes),
/// avoiding heap allocation for the common case. Scopes are kept sorted
/// for efficient subset checking.
///
/// # Future Optimization Opportunities
///
/// TODO: Current size is 40 bytes. Could potentially reduce further:
/// - Use a bitset for small scope IDs (would need scope ID recycling)
/// - Use Rc-sharing for common scope sets (many identifiers share same scopes)
/// - Use u32 for ScopeId instead of usize (4B vs 8B per scope)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScopeSet {
    // Inline storage for up to 3 scopes (most identifiers have 0-3 scopes)
    // Kept sorted for efficient is_subset_of operation
    scopes: SmallVec<[ScopeId; 3]>,
}

impl ScopeSet {
    /// Create an empty scope set (for top-level identifiers)
    pub fn new() -> Self {
        ScopeSet {
            scopes: SmallVec::new(),
        }
    }

    /// Create a scope set with a single scope
    pub fn singleton(scope: ScopeId) -> Self {
        let mut scopes = SmallVec::new();
        scopes.push(scope);
        ScopeSet { scopes }
    }

    /// Add a scope to the set (returns new set, original unchanged)
    pub fn with_scope(&self, scope: ScopeId) -> Self {
        if self.contains(&scope) {
            return self.clone();
        }
        let mut scopes = self.scopes.clone();
        // Insert in sorted order
        let pos = scopes.binary_search(&scope).unwrap_or_else(|p| p);
        scopes.insert(pos, scope);
        ScopeSet { scopes }
    }

    /// Add a scope to this set (mutates in place)
    pub fn add_scope(&mut self, scope: ScopeId) {
        if !self.contains(&scope) {
            let pos = self.scopes.binary_search(&scope).unwrap_or_else(|p| p);
            self.scopes.insert(pos, scope);
        }
    }

    /// Remove a scope from the set (returns new set, original unchanged)
    pub fn without_scope(&self, scope: ScopeId) -> Self {
        let mut scopes = self.scopes.clone();
        if let Ok(pos) = scopes.binary_search(&scope) {
            scopes.remove(pos);
        }
        ScopeSet { scopes }
    }

    /// Flip (toggle) a scope in the set (returns new set, original unchanged)
    ///
    /// This is the key operation for macro hygiene (Racket-style):
    /// - If scope is present, removes it
    /// - If scope is absent, adds it
    ///
    /// Used during macro expansion:
    /// 1. Before expansion: flip macro scope on input (adds to all identifiers)
    /// 2. After expansion: flip macro scope on output
    ///    - Use-site identifiers (from input): had scope added, now removed
    ///    - Introduced identifiers (from template): didn't have scope, now added
    ///
    /// This distinguishes introduced vs use-site identifiers using only scopes.
    pub fn flip_scope(&self, scope: ScopeId) -> Self {
        let mut scopes = self.scopes.clone();
        match scopes.binary_search(&scope) {
            Ok(pos) => {
                scopes.remove(pos);
            }
            Err(pos) => {
                scopes.insert(pos, scope);
            }
        }
        ScopeSet { scopes }
    }

    /// Check if this scope set is a subset of another
    ///
    /// This is the key operation for hygiene: a binding matches a reference
    /// if `binding.scopes ⊆ reference.scopes`
    ///
    /// Since both sets are sorted, we can use a merge-like algorithm in O(n+m).
    pub fn is_subset_of(&self, other: &ScopeSet) -> bool {
        // Empty set is subset of everything
        if self.scopes.is_empty() {
            return true;
        }
        // Non-empty can't be subset of empty
        if other.scopes.is_empty() {
            return false;
        }

        // Merge-style comparison on sorted arrays
        let mut self_iter = self.scopes.iter();
        let mut other_iter = other.scopes.iter();
        let mut self_curr = self_iter.next();
        let mut other_curr = other_iter.next();

        while let (Some(s), Some(o)) = (self_curr, other_curr) {
            match s.cmp(o) {
                std::cmp::Ordering::Equal => {
                    // Found match, advance both
                    self_curr = self_iter.next();
                    other_curr = other_iter.next();
                }
                std::cmp::Ordering::Less => {
                    // self has element not in other -> not a subset
                    return false;
                }
                std::cmp::Ordering::Greater => {
                    // other has element self doesn't need, advance other
                    other_curr = other_iter.next();
                }
            }
        }

        // If we exhausted self, it's a subset. If self has remaining, it's not.
        self_curr.is_none()
    }

    /// Check if this scope set is a proper subset of another
    pub fn is_proper_subset_of(&self, other: &ScopeSet) -> bool {
        self.is_subset_of(other) && self.scopes.len() < other.scopes.len()
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
        self.scopes.binary_search(scope).is_ok()
    }

    /// Iterate over the scopes
    pub fn iter(&self) -> impl Iterator<Item = &ScopeId> {
        self.scopes.iter()
    }

    /// Convert to a sorted vector (for consistent display/comparison)
    /// Note: already sorted internally, so this is just a copy
    pub fn to_sorted_vec(&self) -> Vec<ScopeId> {
        self.scopes.to_vec()
    }

    /// Format the scope set with origin information for debugging
    ///
    /// Example output: `{S1(lambda), S2(let-syntax), S3(macro:my-or)}`
    pub fn format_with_origins(&self) -> String {
        use std::fmt::Write;
        let mut buf = String::new();
        buf.push('{');
        for (i, scope) in self.scopes.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            write!(buf, "{}", scope).unwrap();
            if let Some(origin) = scope.origin() {
                write!(buf, "({})", origin).unwrap();
            }
        }
        buf.push('}');
        buf
    }
}

impl std::fmt::Display for ScopeSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{")?;
        for (i, scope) in self.scopes.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", scope)?;
        }
        write!(f, "}}")
    }
}

impl From<HashSet<ScopeId>> for ScopeSet {
    fn from(set: HashSet<ScopeId>) -> Self {
        let mut scopes: SmallVec<[ScopeId; 3]> = set.into_iter().collect();
        scopes.sort();
        ScopeSet { scopes }
    }
}

impl std::iter::FromIterator<ScopeId> for ScopeSet {
    fn from_iter<I: IntoIterator<Item = ScopeId>>(iter: I) -> Self {
        let mut scopes: SmallVec<[ScopeId; 3]> = iter.into_iter().collect();
        scopes.sort();
        // Remove duplicates (stable sort + dedup)
        scopes.dedup();
        ScopeSet { scopes }
    }
}

impl From<ScopeSet> for HashSet<ScopeId> {
    fn from(scope_set: ScopeSet) -> Self {
        scope_set.scopes.into_iter().collect()
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
        // Should be sorted: {S1, S2}
        assert_eq!(display, "{S1, S2}");
    }

    #[test]
    fn test_flip_scope() {
        let s1 = ScopeId(1);
        let s2 = ScopeId(2);

        // Start with empty set
        let empty = ScopeSet::new();
        assert!(!empty.contains(&s1));

        // Flip adds when absent
        let with_s1 = empty.flip_scope(s1);
        assert!(with_s1.contains(&s1));

        // Flip removes when present
        let back_to_empty = with_s1.flip_scope(s1);
        assert!(!back_to_empty.contains(&s1));

        // Flip twice returns to original
        let set = ScopeSet::from_iter([s1, s2]);
        let flipped_once = set.flip_scope(s1);
        let flipped_twice = flipped_once.flip_scope(s1);
        assert_eq!(set, flipped_twice);
    }

    #[test]
    fn test_flip_scope_macro_hygiene() {
        // Simulate Racket's macro expansion hygiene:
        // 1. Create macro scope
        // 2. Flip on input (adds scope to use-site identifiers)
        // 3. After expansion, flip on output
        //    - Use-site: scope gets removed (was added, then flipped off)
        //    - Introduced: scope gets added (wasn't there, then flipped on)

        let macro_scope = ScopeId(100);

        // Use-site identifier (from macro call input)
        let use_site = ScopeSet::from_iter([ScopeId(1), ScopeId(2)]);

        // Step 1: Flip on input (adds macro_scope)
        let use_site_after_input_flip = use_site.flip_scope(macro_scope);
        assert!(use_site_after_input_flip.contains(&macro_scope));

        // Step 2: After expansion, flip on output (removes macro_scope)
        let use_site_final = use_site_after_input_flip.flip_scope(macro_scope);
        assert!(!use_site_final.contains(&macro_scope));
        assert_eq!(use_site, use_site_final); // Back to original!

        // Introduced identifier (from macro template, no macro_scope initially)
        let introduced = ScopeSet::from_iter([ScopeId(1)]);
        assert!(!introduced.contains(&macro_scope));

        // Introduced identifiers don't go through input flip, only output flip
        // Step 2: Flip on output (adds macro_scope)
        let introduced_final = introduced.flip_scope(macro_scope);
        assert!(introduced_final.contains(&macro_scope));

        // Now use_site_final and introduced_final have different scopes
        // use_site_final: {S1, S2} (no macro_scope)
        // introduced_final: {S1, S100} (has macro_scope)
        assert!(!use_site_final.contains(&macro_scope));
        assert!(introduced_final.contains(&macro_scope));
    }

    #[test]
    fn test_sorted_invariant() {
        // Test that scopes are always kept sorted
        let s1 = ScopeId(1);
        let s2 = ScopeId(2);
        let s3 = ScopeId(3);

        // From iterator (potentially unsorted input)
        let set = ScopeSet::from_iter([s3, s1, s2]);
        let vec = set.to_sorted_vec();
        assert_eq!(vec, vec![s1, s2, s3]);

        // Adding in non-sorted order
        let set2 = ScopeSet::new().with_scope(s3).with_scope(s1).with_scope(s2);
        let vec2 = set2.to_sorted_vec();
        assert_eq!(vec2, vec![s1, s2, s3]);
    }

    #[test]
    fn test_with_scope_idempotent() {
        let s1 = ScopeId(1);
        let set = ScopeSet::singleton(s1);

        // Adding same scope again should be idempotent
        let set2 = set.with_scope(s1);
        assert_eq!(set, set2);
        assert_eq!(set2.len(), 1);
    }

    #[test]
    fn test_from_hashset() {
        let mut hs = HashSet::new();
        hs.insert(ScopeId(3));
        hs.insert(ScopeId(1));
        hs.insert(ScopeId(2));

        let ss = ScopeSet::from(hs);
        assert_eq!(ss.len(), 3);
        assert_eq!(ss.to_sorted_vec(), vec![ScopeId(1), ScopeId(2), ScopeId(3)]);
    }
}
