//! Helper methods for the pattern/template compiler
//!
//! This module contains utility functions used throughout pattern and template
//! compilation, including symbol extraction, literal checking, and list traversal.

use super::super::IdentifierKey;
use super::Compiler;
use crate::error::MacroError;
use patina_core::TaggedValue;
use patina_runtime::PVRef;
use std::rc::Rc;

impl Compiler {
    /// Add a pattern variable and assign it a PVREF
    ///
    /// # Arguments
    /// - `key`: The variable's identity — name plus the scopes it was written with
    /// - `level`: Ellipsis nesting level
    ///
    /// # Returns
    /// The assigned PVREF
    pub(super) fn add_pvar(
        &mut self,
        key: IdentifierKey,
        level: usize,
    ) -> Result<PVRef, MacroError> {
        if self.pvars.contains_key(&key) {
            return Err(MacroError::InvalidSyntax(format!(
                "Duplicate pattern variable: {}",
                key.name
            )));
        }

        if level > 255 {
            return Err(MacroError::InvalidSyntax(
                "Ellipsis nesting too deep (max 255 levels)".to_string(),
            ));
        }

        if self.pvar_count >= 255 {
            return Err(MacroError::InvalidSyntax(
                "Too many pattern variables (max 255)".to_string(),
            ));
        }

        let pvref = PVRef::new(level as u8, self.pvar_count as u8);
        self.pvars.insert(key, pvref);
        self.pvar_count += 1;

        Ok(pvref)
    }

    /// The identity of an identifier-or-symbol form, or `None` for anything else.
    pub(super) fn identifier_key(&self, form: TaggedValue) -> Option<IdentifierKey> {
        IdentifierKey::from_heap(form, &self.heap.borrow())
    }

    /// Whether an identifier is one of this macro's literals.
    ///
    /// Membership is by identity, so `key` must equal a literal in **both**
    /// name and scopes — see [`IdentifierKey`] for why the name alone is not
    /// enough.
    pub(super) fn is_literal_key(&self, key: &IdentifierKey) -> bool {
        self.literal_keys.contains(key)
    }

    /// The PVREF this identifier is bound to, if it is one of this rule's
    /// pattern variables.
    pub(super) fn lookup_pvar(&self, form: TaggedValue) -> Option<PVRef> {
        let key = self.identifier_key(form)?;
        self.pvars.get(&key).copied()
    }

    /// Whether the default `...` names something other than the ellipsis here.
    ///
    /// R7RS 4.3.2 identifies the default ellipsis by its **binding**, so in a
    /// scope that binds `...` — `(let ((... 'dots)) …)` — a `syntax-rules`
    /// written there has no ellipsis and `(_ a b ...)` is a three-variable
    /// pattern. chibi, Larceny, Kawa and Sagittarius agree.
    ///
    /// The question is asked per token, not per macro, and that is the whole
    /// difficulty. A macro defined where `...` *is* the ellipsis can generate
    /// one used where it is not — `(... ...)` escapes an ellipsis into a
    /// generated `syntax-rules` — and that token must keep working. So the
    /// scopes consulted are the token's own where it has any, which is the
    /// case for an identifier an outer expansion introduced, and this macro's
    /// definition scopes otherwise, which is the case for a `...` written in
    /// source and for one substituted through a pattern variable (those
    /// arrive bare). Deciding once for a whole macro was #111 and lost the
    /// introduced ellipsis; deciding from the enclosing bindings' scope sets
    /// was #114 and could not tell a token's own scopes from unioned ones.
    ///
    /// A declared ellipsis (SRFI 46) is exempt: it is a declaration, not a
    /// reference, so no binding is consulted.
    fn default_ellipsis_is_bound_away(&self, form: TaggedValue) -> bool {
        if self.ellipsis_is_custom {
            return false;
        }
        // Without an environment there is no binding to consult, so the
        // spelling keeps its default meaning. Only `Compiler::new` builds a
        // compiler that way and only tests call it; every path that compiles a
        // real macro carries the definition environment. If that stops being
        // true this is a silent divergence, not an error, which is why it is
        // spelled out rather than left to read as a deliberate exemption.
        let Some(env) = self.env.as_ref() else {
            return false;
        };
        // Resolve the scopes before touching the environment: `get_with_scopes`
        // borrows the heap itself when macro debugging is on.
        let scopes = {
            let heap = self.heap.borrow();
            match heap.get_identifier_data_any(form) {
                // An identifier: introduced by an expansion, and its own scopes
                // are the context it came from — empty meaning top level, where
                // `...` is the ellipsis. `get_identifier_data_any` answers None
                // for a plain symbol, which is exactly the source-written case.
                Some((_, token_scopes)) => token_scopes,
                None => self.definition_scopes.clone(),
            }
        };
        let Some(tv) = env.get_with_scopes(super::super::utils::ELLIPSIS, &scopes) else {
            return false;
        };
        let heap = self.heap.borrow();
        heap.get_core_syntax(tv) != Some(patina_core::CoreForm::Ellipsis)
    }

    /// Check if a TaggedValue is the ellipsis symbol
    ///
    /// Recognizes both plain Symbol and Identifier (with marks/scopes)
    /// to support nested macro definitions where ellipsis may be wrapped.
    ///
    /// R7RS 4.3.2: "Literals have priority over ellipsis."
    /// If the ellipsis symbol is also in the literals list, return false.
    pub(super) fn is_ellipsis(&self, form: TaggedValue) -> bool {
        match &self.ellipsis {
            None => false, // Ellipsis disabled
            Some(ellipsis_sym) => {
                let is_ellipsis_match = {
                    let heap = self.heap.borrow();
                    if let Some(s) = heap.get_symbol_name(form) {
                        s == ellipsis_sym.as_ref()
                    } else if let Some((name, _)) = heap.get_identifier_data_any(form) {
                        &name == ellipsis_sym
                    } else {
                        false
                    }
                };
                // Literals have priority over ellipsis (SRFI-46, R7RS 4.3.2).
                // Only reached for a token actually spelled like the ellipsis,
                // so the extra identity read is rare.
                if is_ellipsis_match
                    && self
                        .identifier_key(form)
                        .is_some_and(|key| self.is_literal_key(&key))
                {
                    return false;
                }
                if is_ellipsis_match && self.default_ellipsis_is_bound_away(form) {
                    return false;
                }
                is_ellipsis_match
            }
        }
    }

    /// Extract symbol name from any identifier type (TaggedValue)
    ///
    /// Returns Some(name) for Symbol or Identifier.
    /// Returns None for other value types.
    pub(super) fn extract_symbol_name(&self, form: TaggedValue) -> Option<Rc<str>> {
        let heap = self.heap.borrow();
        if let Some(s) = heap.get_symbol_name(form) {
            Some(Rc::from(s))
        } else if let Some((name, _)) = heap.get_identifier_data_any(form) {
            Some(name)
        } else {
            None
        }
    }

    /// Check if a TaggedValue is a SUBSTITUTED identifier from outer macro expansion.
    ///
    /// After the flip-scope algorithm in macro expansion:
    /// - Substituted values (from pattern variables): end up with EMPTY scopes
    ///   (they had the macro_scope before flip, which got removed)
    /// - Introduced identifiers (from template): end up with NON-EMPTY scopes
    ///   (they had empty scopes before flip, macro_scope got added)
    ///
    /// Only the template compiler uses this now, to decide how to emit an
    /// identifier that is *not* this rule's pattern variable: a substituted one
    /// already carries the identity the outer macro gave it and is emitted
    /// verbatim, while any other identifier gets this macro's definition scopes.
    ///
    /// Pattern compilation does not consult it. Classification there is by the
    /// literals list, and pattern-variable identity is (name, scopes) -- which
    /// distinguishes substituted from introduced identifiers without a
    /// provenance test.
    pub(super) fn is_substituted_from_outer_macro(&self, form: TaggedValue) -> bool {
        let heap = self.heap.borrow();
        // Any identifier at all, whatever scopes it carries.
        //
        // Being an identifier rather than a plain symbol is what says the token
        // arrived from somewhere else: the reader produces symbols, and only an
        // expansion produces identifiers. So this is the whole "not written
        // here" test, and the scopes it carries are the context it came from,
        // whether that is a macro defined in some inner scope (a non-empty set)
        // or one defined at top level (an empty one).
        //
        // It used to hold only for the empty case, which quietly made the
        // top-level macro the *only* one whose introduced text kept its
        // identity. A macro defined inside a scope had its text re-tagged with
        // the definition scopes of whatever macro it was generating, and once a
        // local variable became a real binding that re-tagging was enough to
        // capture it: `def-mid`'s template `if`, expanded inside
        // `(let ((if 'shadowed)) …)`, resolved to the *variable*.
        heap.get_identifier_data_any(form).is_some()
    }

    /// Collect items from a list TaggedValue
    ///
    /// Returns (items, tail) where tail is Some(value) for improper lists
    pub(super) fn collect_list_items(
        &self,
        expr: TaggedValue,
    ) -> Result<(Vec<TaggedValue>, Option<TaggedValue>), MacroError> {
        let mut items = Vec::new();
        let mut current = expr;

        loop {
            if current == TaggedValue::NULL {
                return Ok((items, None));
            }

            // Fast path: native pair
            if current.is_pair() {
                let heap = self.heap.borrow();
                let (car, cdr) = heap.get_pair(current);
                items.push(car);
                current = cdr;
                continue;
            }

            // Check if it's any pair (including boxed)
            let is_pair = current.is_pair();
            if is_pair {
                let heap = self.heap.borrow();
                if let Some((car, cdr)) = heap.try_pair(current) {
                    items.push(car);
                    current = cdr;
                    continue;
                }
            }

            // Improper list: (a b . c)
            return Ok((items, Some(current)));
        }
    }

    /// Check if the item at the given index is followed by an ellipsis
    pub(super) fn is_followed_by_ellipsis(&self, items: &[TaggedValue], index: usize) -> bool {
        index + 1 < items.len() && self.is_ellipsis(items[index + 1])
    }

    /// Check if a TaggedValue contains any pattern variables
    /// Used to determine if a quoted expression needs template expansion
    pub(super) fn contains_pattern_vars(&self, value: TaggedValue) -> bool {
        // Handle all identifier types (Symbol, Identifier).
        //
        // A substituted identifier is not skipped: the pattern compiler can
        // bind one as a pattern variable, and a quoted template that mentions
        // it does need expansion. The lookup is by identity, so a substituted
        // identifier does not collide with an introduced pattern variable of
        // the same name.
        if let Some(key) = self.identifier_key(value) {
            return self.pvars.contains_key(&key);
        }

        // Check pairs
        if value.is_pair() {
            let items = match self.collect_list_items(value) {
                Ok((items, _)) => items,
                Err(_) => return false,
            };
            return items.iter().any(|item| self.contains_pattern_vars(*item));
        }

        // Check vectors
        if value.is_vector() {
            let heap = self.heap.borrow();
            let len = heap.vector_len(value);
            for i in 0..len {
                if self.contains_pattern_vars(heap.vector_ref(value, i)) {
                    return true;
                }
            }
            return false;
        }

        false
    }
}
