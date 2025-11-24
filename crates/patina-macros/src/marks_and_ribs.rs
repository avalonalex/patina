//! Marks-and-ribs hygiene system
//!
//! This module implements the marks-and-ribs algorithm for hygienic macro expansion,
//! as described in the Chez Scheme implementation and various academic papers.
//!
//! ## References
//!
//! 1. **Chez Scheme implementation** (primary reference)
//!    - File: `csug/syntax.ss` and `s/syntax.ss` in Chez Scheme source
//!    - Dybvig, Kent. "The Development of Chez Scheme"
//!    - Chez Scheme Version 9 User's Guide, Chapter 11: Syntactic Extension
//!
//! 2. **Academic papers:**
//!    - Dybvig, Friedman, and Haynes (1992). "Expansion-passing style: A general
//!      macro mechanism." Lisp and Symbolic Computation.
//!    - Dybvig, Hieb, and Bruggeman (1993). "Syntactic abstraction in Scheme."
//!      Lisp and Symbolic Computation.
//!    - Flatt (2016). "Binding as sets of scopes." POPL 2016.
//!      (This describes Racket's scope sets, which evolved from marks-and-ribs)
//!
//! 3. **Online resources:**
//!    - https://www.cs.indiana.edu/~dyb/pubs/LaSC-5-4-pp295-326.pdf
//!      (Original marks paper)
//!
//! ## Algorithm Overview
//!
//! The marks-and-ribs algorithm solves macro hygiene through **phase tracking**:
//!
//! ### Core Concepts
//!
//! 1. **Marks** - Track expansion phases
//!    - Each macro expansion adds a fresh mark to identifiers
//!    - Marks distinguish identifiers introduced at different expansion times
//!    - Format: List of integers, e.g., `[3, 1]` means "introduced in expansion 3,
//!      inside expansion 1"
//!
//! 2. **Ribs** - Track renamings within a phase
//!    - Map pattern variables to their captured values
//!    - Scoped to a single macro expansion
//!    - Think of ribs as "local environment" for pattern variables
//!
//! 3. **Wrapped Identifiers** - Identifiers with hygiene metadata
//!    - `(name, marks)` where marks is a list of expansion phases
//!    - Two identifiers are "same" if they have the same name AND same marks
//!
//! ### How It Works
//!
//! **During expansion:**
//! ```text
//! 1. Allocate fresh mark M for this expansion
//! 2. Add mark M to all identifiers in the INPUT form
//! 3. Match pattern against marked input
//! 4. Expand template, substituting pattern variables
//! 5. Add mark M to all identifiers in OUTPUT (except pattern variables)
//! ```
//!
//! **During lookup:**
//! ```text
//! 1. Find identifier with (name, marks)
//! 2. Search environment for matching binding
//! 3. Two identifiers match if they have same name AND same marks
//! ```
//!
//! ### Example
//!
//! Consider this macro that introduces `temp`:
//!
//! ```scheme
//! (define-syntax swap!
//!   (syntax-rules ()
//!     ((swap! a b)
//!      (let ((temp a))
//!        (set! a b)
//!        (set! b temp)))))
//! ```
//!
//! When we expand `(swap! x y)`:
//!
//! 1. **Allocate mark 1** for this expansion
//!
//! 2. **Mark the input:**
//!    - Input: `(swap! x y)`
//!    - Marked: `(swap!₁ x₁ y₁)` (all get mark 1)
//!
//! 3. **Match pattern:** `(swap! a b)` against `(swap!₁ x₁ y₁)`
//!    - `a` binds to `x₁`
//!    - `b` binds to `y₁`
//!
//! 4. **Expand template:** `(let ((temp a)) (set! a b) (set! b temp))`
//!    - Substitute: `a` → `x₁`, `b` → `y₁`
//!    - Result: `(let ((temp x₁)) (set! x₁ y₁) (set! y₁ temp))`
//!
//! 5. **Mark the output:**
//!    - Template identifiers get mark 1: `let₁`, `temp₁`, `set!₁`
//!    - Pattern variables keep their marks: `x₁`, `y₁`
//!    - Final: `(let₁ ((temp₁ x₁)) (set!₁ x₁ y₁) (set!₁ y₁ temp₁))`
//!
//! 6. **Result:**
//!    - User's `x` and `y` have mark `[1]`
//!    - Macro's `temp` has mark `[1]`
//!    - If user also has a `temp`, it would have mark `[]` (no marks)
//!    - These are DIFFERENT identifiers! No capture.
//!
//! ### Why This Works
//!
//! - Identifiers from different sources have different marks
//! - Marks persist through nested expansions
//! - Name lookup is marks-aware: `temp₁` ≠ `temp` ≠ `temp₂`
//! - No need to rename or gensym - marks provide discrimination
//!
//! ### Nested Macros
//!
//! When macros call macros, marks stack:
//!
//! ```scheme
//! (define-syntax outer
//!   (syntax-rules ()
//!     ((outer x) (inner x))))
//!
//! (define-syntax inner
//!   (syntax-rules ()
//!     ((inner y) (let ((temp y)) temp))))
//!
//! (outer foo)
//! ```
//!
//! Expansion trace:
//! 1. Expand `(outer foo)` with mark 1 → `(inner₁ foo₁)`
//! 2. Expand `(inner₁ foo₁)` with mark 2 → `(let₂ ((temp₂ foo₁)) temp₂)`
//!
//! Note: `foo₁` has marks `[1]`, `temp₂` has marks `[2, 1]`
//!
//! ## Implementation Notes
//!
//! This implementation differs slightly from Chez Scheme:
//!
//! - **Simplified ribs:** We don't track full substitution ribs yet, just marks
//! - **No syntax objects:** Chez uses full syntax objects; we wrap Value enum
//! - **Eager marking:** We mark during expansion rather than lazy marking
//! - **Integration:** Designed to work with existing patina-macros infrastructure
//!
//! Future improvements (when needed for R7RS compliance):
//! - Add full rib tracking for complex pattern variable substitutions
//! - Implement syntax objects for better source location tracking
//! - Add phase tracking for module system (macro-time vs runtime)

use patina_runtime::Value;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Global counter for generating unique marks
///
/// Each macro expansion gets a fresh mark to distinguish identifiers
/// introduced at different expansion times.
///
/// In Chez Scheme, this is called the "expansion counter" or "mark counter".
static MARK_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A mark represents a single expansion phase
///
/// Marks are integers that uniquely identify when an identifier was introduced.
/// They're generated monotonically and never reused.
///
/// ## Reference
/// In Chez Scheme's `syntax.ss`, marks are represented as integers.
/// See: `make-wrap` and `new-mark` in Chez source.
pub type Mark = usize;

/// List of marks attached to an identifier
///
/// Marks form a "trace" of expansions that introduced or touched this identifier.
/// The list is ordered from most recent (head) to oldest (tail).
///
/// ## Examples
/// - `[]` - User-written identifier (no expansions)
/// - `[3]` - Introduced in expansion 3
/// - `[5, 3, 1]` - Introduced in expansion 1, passed through 3, then through 5
///
/// ## Reference
/// Chez Scheme calls this a "wrap" - a list of marks and substitution ribs.
/// We're using a simplified version with just marks for now.
pub type MarkList = Vec<Mark>;

/// A wrapped identifier with hygiene metadata
///
/// This represents an identifier after macro expansion, carrying information
/// about which expansion phases introduced it.
///
/// ## Structure
/// - `name`: The symbolic name (e.g., "temp", "x")
/// - `marks`: List of expansion phases that touched this identifier
///
/// ## Equivalence
/// Two identifiers are "the same" (for binding purposes) if they have:
/// 1. The same name
/// 2. The same marks (in the same order)
///
/// ## Reference
/// In Chez Scheme, this is part of the "syntax object" structure.
/// See: `make-syntax` in Chez's syntax.ss
///
/// ## Example
/// ```text
/// // User writes: x
/// WrappedIdentifier { name: "x", marks: [] }
///
/// // After expanding (let ((temp x)) temp):
/// WrappedIdentifier { name: "temp", marks: [1] }  // macro's temp
/// WrappedIdentifier { name: "x", marks: [1] }      // user's x (marked)
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WrappedIdentifier {
    /// The symbolic name of the identifier
    pub name: Rc<str>,

    /// List of marks tracking expansion history
    ///
    /// Ordered from most recent (index 0) to oldest.
    /// Empty list means this identifier came from source code (not macro-generated).
    pub marks: MarkList,
}

impl WrappedIdentifier {
    /// Create a new wrapped identifier with no marks (user-written)
    ///
    /// ## Reference
    /// Corresponds to Chez's `datum->syntax` for top-level forms.
    pub fn new(name: Rc<str>) -> Self {
        Self {
            name,
            marks: Vec::new(),
        }
    }

    /// Create a wrapped identifier with specific marks
    pub fn with_marks(name: Rc<str>, marks: MarkList) -> Self {
        Self { name, marks }
    }

    /// Add a mark to this identifier
    ///
    /// Marks are added to the front (most recent first).
    ///
    /// ## Reference
    /// Corresponds to Chez's `add-mark` operation in syntax.ss
    pub fn add_mark(&mut self, mark: Mark) {
        self.marks.insert(0, mark);
    }

    /// Create a new identifier with an additional mark
    pub fn with_added_mark(&self, mark: Mark) -> Self {
        let mut new_marks = self.marks.clone();
        new_marks.insert(0, mark);
        Self {
            name: self.name.clone(),
            marks: new_marks,
        }
    }

    /// Check if two identifiers are "the same" for binding purposes
    ///
    /// Two identifiers match if they have the same name AND same marks.
    /// This is used during variable lookup to ensure hygiene.
    ///
    /// ## Reference
    /// Corresponds to Chez's `bound-identifier=?` predicate.
    pub fn binds_same_as(&self, other: &WrappedIdentifier) -> bool {
        self.name == other.name && self.marks == other.marks
    }

    /// Get the base name without marks (for display/debugging)
    pub fn base_name(&self) -> &str {
        &self.name
    }

    /// Format with marks for debugging
    ///
    /// Examples:
    /// - `x` (no marks)
    /// - `x@1` (one mark)
    /// - `temp@2@1` (multiple marks)
    pub fn debug_name(&self) -> String {
        if self.marks.is_empty() {
            self.name.to_string()
        } else {
            let marks_str = self
                .marks
                .iter()
                .map(|m| format!("@{}", m))
                .collect::<String>();
            format!("{}{}", self.name, marks_str)
        }
    }
}

/// A rib represents a local substitution environment within an expansion
///
/// Ribs map pattern variables to their captured values during macro expansion.
/// They are scoped to a single macro expansion and discarded after expansion.
///
/// ## Reference
/// In Chez Scheme's syntax.ss, ribs are part of the "wrap" structure along with marks.
/// See: `make-resolved-id` and `extend-rib` in Chez source.
///
/// ## Current Implementation
/// This is a simplified version. Full Chez ribs also track:
/// - Substitution mappings (pattern var → syntax object)
/// - Import/export information for modules
/// - Label mappings for identifiers
///
/// We may extend this structure when implementing the module system.
#[derive(Debug, Clone)]
pub struct Rib {
    /// Map from pattern variable names to their mark lists
    ///
    /// When we match a pattern like `(mac x)`, we create a rib:
    /// `{"x" → marks_of_x}`
    ///
    /// This allows us to preserve the marks of pattern variables
    /// when substituting them into the template.
    bindings: HashMap<Rc<str>, MarkList>,
}

impl Rib {
    /// Create an empty rib
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }

    /// Add a pattern variable binding to this rib
    ///
    /// ## Arguments
    /// - `name`: Pattern variable name
    /// - `marks`: The marks from the actual identifier that matched
    ///
    /// ## Reference
    /// Corresponds to Chez's `extend-rib` operation.
    pub fn bind(&mut self, name: Rc<str>, marks: MarkList) {
        self.bindings.insert(name, marks);
    }

    /// Look up a pattern variable's marks in this rib
    ///
    /// Returns `None` if this identifier is not a pattern variable.
    pub fn lookup(&self, name: &str) -> Option<&MarkList> {
        self.bindings.get(name)
    }

    /// Check if an identifier is bound in this rib
    pub fn contains(&self, name: &str) -> bool {
        self.bindings.contains_key(name)
    }
}

impl Default for Rib {
    fn default() -> Self {
        Self::new()
    }
}

/// The marks-and-ribs hygiene system
///
/// This implements the marks-and-ribs algorithm from Chez Scheme for hygienic
/// macro expansion.
///
/// ## State
/// Currently stateless - marks are generated globally. In a more sophisticated
/// implementation, we might track:
/// - Current expansion depth
/// - Active ribs stack
/// - Syntax object cache
///
/// ## Reference
/// See Chez Scheme's `sc-expand` function in syntax.ss for the full expansion
/// algorithm that uses marks and ribs.
pub struct MarksAndRibsHygiene {
    // Currently stateless - could add per-instance state if needed
}

impl MarksAndRibsHygiene {
    /// Create a new marks-and-ribs hygiene system
    pub fn new() -> Self {
        Self {}
    }

    /// Generate a fresh mark for a new expansion
    ///
    /// Each macro expansion should call this once to get a unique mark.
    ///
    /// ## Reference
    /// Corresponds to Chez's `new-mark` in syntax.ss
    pub fn fresh_mark() -> Mark {
        MARK_COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    /// Add a mark to all identifiers in an expression
    ///
    /// This is called on both the input (before matching) and output (after expansion)
    /// of a macro.
    ///
    /// ## Arguments
    /// - `expr`: The expression to mark
    /// - `mark`: The mark to add
    /// - `pattern_vars`: Set of pattern variables (these should NOT be marked in output)
    ///
    /// ## Reference
    /// Corresponds to Chez's `add-mark` operation applied recursively.
    /// See: `add-subst` in syntax.ss for similar recursive marking.
    pub fn add_mark_to_expr(expr: &Value, mark: Mark, pattern_vars: &HashSet<Rc<str>>) -> Value {
        match expr {
            // Mark symbols (convert to WrappedIdentifier)
            Value::Symbol(name) => {
                // Don't mark pattern variables in template expansion
                if pattern_vars.contains(name) {
                    expr.clone()
                } else {
                    // Convert to wrapped identifier with this mark
                    Value::WrappedIdentifier {
                        name: name.clone(),
                        marks: vec![mark],
                    }
                }
            }

            // Add mark to existing wrapped identifiers
            Value::WrappedIdentifier { name, marks } => {
                // Don't mark pattern variables
                if pattern_vars.contains(name) {
                    expr.clone()
                } else {
                    // Add new mark to the front (most recent first)
                    let mut new_marks = vec![mark];
                    new_marks.extend_from_slice(marks);
                    Value::WrappedIdentifier {
                        name: name.clone(),
                        marks: new_marks,
                    }
                }
            }

            // Recursively mark pairs (lists)
            Value::Pair(pair) => {
                let borrowed = pair.borrow();
                let marked_car = Self::add_mark_to_expr(&borrowed.0, mark, pattern_vars);
                let marked_cdr = Self::add_mark_to_expr(&borrowed.1, mark, pattern_vars);
                Value::Pair(Rc::new(std::cell::RefCell::new((marked_car, marked_cdr))))
            }

            // Recursively mark vectors
            Value::Vector(vec) => {
                let marked_items: Vec<Value> = vec
                    .borrow()
                    .iter()
                    .map(|item| Self::add_mark_to_expr(item, mark, pattern_vars))
                    .collect();
                Value::Vector(Rc::new(std::cell::RefCell::new(marked_items)))
            }

            // Don't mark quoted data
            // In full implementation, we'd check if this is inside a quote form
            // For now, we'll just pass through non-identifier values
            _ => expr.clone(),
        }
    }
}

impl Default for MarksAndRibsHygiene {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrapped_identifier_creation() {
        let id = WrappedIdentifier::new(Rc::from("test"));
        assert_eq!(id.name.as_ref(), "test");
        assert!(id.marks.is_empty());
        assert_eq!(id.debug_name(), "test");
    }

    #[test]
    fn test_wrapped_identifier_with_marks() {
        let id = WrappedIdentifier::with_marks(Rc::from("temp"), vec![3, 1]);
        assert_eq!(id.name.as_ref(), "temp");
        assert_eq!(id.marks, vec![3, 1]);
        assert_eq!(id.debug_name(), "temp@3@1");
    }

    #[test]
    fn test_add_mark() {
        let mut id = WrappedIdentifier::new(Rc::from("x"));
        id.add_mark(1);
        assert_eq!(id.marks, vec![1]);
        id.add_mark(2);
        assert_eq!(id.marks, vec![2, 1]); // Most recent first
    }

    #[test]
    fn test_binds_same_as() {
        let id1 = WrappedIdentifier::with_marks(Rc::from("x"), vec![1]);
        let id2 = WrappedIdentifier::with_marks(Rc::from("x"), vec![1]);
        let id3 = WrappedIdentifier::with_marks(Rc::from("x"), vec![2]);
        let id4 = WrappedIdentifier::with_marks(Rc::from("y"), vec![1]);

        // Same name, same marks → binds same
        assert!(id1.binds_same_as(&id2));

        // Same name, different marks → different
        assert!(!id1.binds_same_as(&id3));

        // Different name, same marks → different
        assert!(!id1.binds_same_as(&id4));
    }

    #[test]
    fn test_rib_operations() {
        let mut rib = Rib::new();

        // Initially empty
        assert!(!rib.contains("x"));

        // Add binding
        rib.bind(Rc::from("x"), vec![1]);
        assert!(rib.contains("x"));
        assert_eq!(rib.lookup("x"), Some(&vec![1]));

        // Lookup non-existent
        assert_eq!(rib.lookup("y"), None);
    }

    #[test]
    fn test_fresh_mark_uniqueness() {
        let mark1 = MarksAndRibsHygiene::fresh_mark();
        let mark2 = MarksAndRibsHygiene::fresh_mark();
        let mark3 = MarksAndRibsHygiene::fresh_mark();

        // All marks should be unique
        assert_ne!(mark1, mark2);
        assert_ne!(mark2, mark3);
        assert_ne!(mark1, mark3);

        // Marks should be monotonically increasing
        assert!(mark2 > mark1);
        assert!(mark3 > mark2);
    }

    // Note: More comprehensive tests are in the integration test suite
    // that verifies macro expansion with marks-and-ribs hygiene.
}
