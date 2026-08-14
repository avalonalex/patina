//! Identifier identity for `syntax-rules` compilation.

use patina_core::{Heap, TaggedValue};
use patina_runtime::ScopeSet;
use std::rc::Rc;

/// An identifier's identity: its name together with the scopes it carries.
///
/// R7RS 4.3.2 classifies a pattern identifier by identity, not by spelling, so
/// the compiler compares both halves everywhere it asks "are these the same
/// identifier?" — literals-list membership and pattern-variable binding alike.
///
/// The two spellings that matter come from a macro that writes another macro:
///
/// ```scheme
/// (let-syntax ((m (syntax-rules ()
///                   ((m x)
///                    (let-syntax ((n (syntax-rules (k)      ; k INTRODUCED  -> non-empty scopes
///                                      ((n x) 'a)           ; x SUBSTITUTED -> empty scopes
///                                      ((n y) 'b))))
///                      (n z))))))
///   (m k))
/// ```
///
/// Both spell `k`, but the literal is introduced by the template while the
/// pattern identifier is substituted from the use site. They are different
/// identifiers, so `x` is a pattern variable and the answer is `'a`. Comparing
/// names alone would call `x` a literal and give `'b`.
///
/// A bare symbol carries no scopes and normalizes to an empty scope set, so
/// ordinary macros written in source compare exactly as they always have.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IdentifierKey {
    /// The identifier name
    pub name: Rc<str>,
    /// Scopes the identifier carried where it was written
    pub scopes: ScopeSet,
}

impl IdentifierKey {
    /// Create a key from a name and the scopes it was written with
    pub fn new(name: Rc<str>, scopes: ScopeSet) -> Self {
        Self { name, scopes }
    }

    /// Read an identifier's identity off the heap.
    ///
    /// Returns `None` for a value that is neither a symbol nor an identifier.
    /// This is the single place the "bare symbol means empty scopes"
    /// normalization lives, and it takes one heap borrow rather than fetching
    /// the name and the scopes separately.
    pub fn from_heap(form: TaggedValue, heap: &Heap) -> Option<Self> {
        if let Some((name, scopes)) = heap.get_identifier_data(form) {
            return Some(Self::new(name.clone(), scopes.clone()));
        }
        heap.get_symbol_name(form)
            .map(|name| Self::new(Rc::from(name), ScopeSet::new()))
    }
}

impl From<&str> for IdentifierKey {
    fn from(name: &str) -> Self {
        Self::new(Rc::from(name), ScopeSet::new())
    }
}
