//! Pattern compilation for syntax-rules
//!
//! This module handles compilation of Scheme syntax-rules patterns into
//! PVREF-based Pattern representations.

use super::Compiler;
use crate::error::MacroError;
use crate::macro_expander::pattern::Pattern;
use crate::macro_expander::utils::WILDCARD;
use patina_runtime::{PVRef, Value};

impl Compiler {
    /// Compile a top-level syntax-rules pattern (a rule pattern)
    ///
    /// R7RS Section 4.3.2: "The first element of each clause is the keyword, which
    /// is ignored." This means the first element of the pattern list is a placeholder
    /// for the macro name and should always be treated as a wildcard, regardless of
    /// whether it appears in the literals list.
    ///
    /// For example, in:
    ///   (syntax-rules (_) ((_ x) x))
    /// The first `_` in the pattern `(_ x)` is the macro keyword placeholder and should
    /// match the macro name, NOT be treated as a literal that only matches `_`.
    pub(super) fn compile_rule_pattern(
        &mut self,
        form: &Value,
        level: usize,
    ) -> Result<Pattern, MacroError> {
        match form {
            Value::Pair(_) => {
                let (items, tail) = self.collect_list_items(form)?;
                if items.is_empty() {
                    return Err(MacroError::InvalidSyntax(
                        "Pattern must have at least a macro keyword".to_string(),
                    ));
                }

                if let Some(tail_value) = tail {
                    // Dotted list pattern: (keyword p1 p2 . rest)
                    // First element is keyword (wildcard), rest are normal patterns
                    let mut patterns = vec![Pattern::Wildcard];
                    for item in items.iter().skip(1) {
                        patterns.push(self.compile_pattern(item, level)?);
                    }
                    let tail_pattern = Box::new(self.compile_pattern(&tail_value, level)?);
                    Ok(Pattern::DottedList {
                        patterns,
                        tail: tail_pattern,
                    })
                } else {
                    // Regular list pattern: (keyword p1 p2 ...)
                    // First element is keyword (wildcard), rest are compiled with ellipsis detection
                    self.compile_rule_list_pattern(&items, level)
                }
            }
            _ => Err(MacroError::InvalidSyntax(
                "Pattern must be a list starting with macro keyword".to_string(),
            )),
        }
    }

    /// Compile a rule list pattern where the first element is the macro keyword
    ///
    /// This is similar to compile_list_pattern but treats the first element as a wildcard.
    fn compile_rule_list_pattern(
        &mut self,
        items: &[Value],
        level: usize,
    ) -> Result<Pattern, MacroError> {
        let mut patterns = vec![Pattern::Wildcard]; // First element is always wildcard
        let mut i = 1; // Start from second element

        while i < items.len() {
            // Check for ellipsis
            if i + 1 < items.len() && self.is_ellipsis(&items[i + 1]) {
                // Found ellipsis pattern: (item ...)
                let num_following = items.len() - i - 2;
                let start_pvars = self.pvar_count;
                let subpattern = self.compile_pattern(&items[i], level + 1)?;
                let end_pvars = self.pvar_count;

                let mut vars = Vec::new();
                for idx in start_pvars..end_pvars {
                    vars.push(PVRef::new((level + 1) as u8, idx as u8));
                }

                self.max_level = self.max_level.max(level + 1);

                patterns.push(Pattern::Ellipsis {
                    subpattern: Box::new(subpattern),
                    level: (level + 1) as u8,
                    num_following,
                    vars,
                });

                i += 2; // Skip pattern and ellipsis
            } else {
                patterns.push(self.compile_pattern(&items[i], level)?);
                i += 1;
            }
        }

        Ok(Pattern::List(patterns))
    }

    /// Compile a pattern at the given ellipsis level
    ///
    /// Based on Gauche's compile_rule1 (macro.c:400+).
    ///
    /// # Arguments
    /// - `form`: S-expression representing the pattern
    /// - `level`: Current ellipsis nesting level (0 = not in ellipsis)
    pub fn compile_pattern(&mut self, form: &Value, level: usize) -> Result<Pattern, MacroError> {
        // Handle all identifier types (Symbol, ScopedIdentifier, WrappedIdentifier)
        // This is needed for nested macro definitions where identifiers may be wrapped.
        if let Some(s) = Self::extract_symbol_name(form) {
            // R7RS: Check literals FIRST (including _ if it's in the literals list)
            // This check is done by NAME only - an identifier is a literal if its name
            // appears in the literals list, regardless of scopes.
            if self.is_literal(s) {
                // Literal identifier (including _ if explicitly listed)
                // Use the original form to preserve identifier type
                return Ok(Pattern::Literal(form.clone()));
            }

            // Check if this is a SUBSTITUTED identifier (empty scopes) from outer expansion.
            // These should be treated as literals even if not in the explicit literals list.
            // This enables nested macro hygiene: when an outer macro generates a define-syntax,
            // substituted symbols shouldn't become pattern variables in the inner macro.
            //
            // Example: (define-syntax foo (syntax-rules () ((foo bar y)
            //            (define-syntax bar (syntax-rules () ((bar x) 'y))))))
            // When (foo mybar x) is called, 'y' substitutes to 'x'. The 'x' in the inner
            // pattern `'x` should be a literal (returns symbol 'x), not a pattern variable.
            if Self::is_substituted_from_outer_macro(form) {
                // Identifier with empty scopes = substituted from outer expansion = literal
                return Ok(Pattern::Literal(form.clone()));
            }

            if s.as_ref() == WILDCARD {
                // Underscore is wildcard only if NOT in literals
                Ok(Pattern::Wildcard)
            } else {
                // Pattern variable - assign PVREF
                let pvref = self.add_pvar(s.clone(), level)?;
                Ok(Pattern::Var(pvref))
            }
        } else {
            match form {
                // List - check for ellipsis and dotted tails
                Value::Pair(_) => {
                    let (items, tail) = self.collect_list_items(form)?;
                    if let Some(tail_value) = tail {
                        // Dotted list pattern
                        self.compile_dotted_pattern(&items, &tail_value, level)
                    } else {
                        // Regular list pattern
                        self.compile_list_pattern(&items, level)
                    }
                }

                Value::Null => Ok(Pattern::List(vec![])),

                // Vector
                Value::Vector(v) => {
                    let items = v.borrow();
                    let mut patterns = Vec::new();
                    for item in items.iter() {
                        patterns.push(self.compile_pattern(item, level)?);
                    }
                    Ok(Pattern::Vector(patterns))
                }

                // Literal value (boolean, number, string, character, etc.)
                other => Ok(Pattern::Literal(other.clone())),
            }
        }
    }

    /// Compile a list pattern, detecting ellipsis
    ///
    /// This is where the magic happens - we detect ellipsis patterns
    /// and precompute the num_following optimization.
    fn compile_list_pattern(
        &mut self,
        items: &[Value],
        level: usize,
    ) -> Result<Pattern, MacroError> {
        let patterns = self.compile_pattern_items(items, level)?;
        Ok(Pattern::List(patterns))
    }

    /// Compile a dotted list pattern: (a b . rest)
    /// Also handles patterns with ellipsis: (a b ... . rest)
    fn compile_dotted_pattern(
        &mut self,
        items: &[Value],
        tail: &Value,
        level: usize,
    ) -> Result<Pattern, MacroError> {
        let patterns = self.compile_pattern_items(items, level)?;
        let tail_pattern = Box::new(self.compile_pattern(tail, level)?);

        Ok(Pattern::DottedList {
            patterns,
            tail: tail_pattern,
        })
    }

    /// Compile a sequence of pattern items, handling ellipsis patterns
    ///
    /// This is shared between list and dotted list pattern compilation.
    /// It handles the detection of ellipsis patterns and the num_following optimization.
    fn compile_pattern_items(
        &mut self,
        items: &[Value],
        level: usize,
    ) -> Result<Vec<Pattern>, MacroError> {
        let mut patterns = Vec::new();
        let mut i = 0;

        while i < items.len() {
            if self.is_followed_by_ellipsis(items, i) {
                // Found ellipsis pattern: (item ...)
                let num_following = items.len() - i - 2;
                let ellipsis_pattern =
                    self.compile_ellipsis_pattern(&items[i], level, num_following)?;
                patterns.push(ellipsis_pattern);
                i += 2; // Skip pattern and ellipsis
            } else {
                patterns.push(self.compile_pattern(&items[i], level)?);
                i += 1;
            }
        }

        Ok(patterns)
    }

    /// Compile an ellipsis pattern (item ...)
    ///
    /// This compiles the subpattern at an increased level and tracks which
    /// pattern variables are introduced. Uses Gauche's num_following optimization.
    fn compile_ellipsis_pattern(
        &mut self,
        item: &Value,
        level: usize,
        num_following: usize,
    ) -> Result<Pattern, MacroError> {
        // Track which pattern variables are introduced in subpattern
        let start_pvars = self.pvar_count;

        // Compile the subpattern at increased level
        let subpattern = self.compile_pattern(item, level + 1)?;

        // Collect PVREFs for variables introduced in this subpattern
        let vars = self.collect_new_pvars(start_pvars, level + 1);

        // Update max level
        self.max_level = self.max_level.max(level + 1);

        Ok(Pattern::Ellipsis {
            subpattern: Box::new(subpattern),
            level: (level + 1) as u8,
            num_following,
            vars,
        })
    }

    /// Collect PVREFs for pattern variables introduced since start_pvars
    fn collect_new_pvars(&self, start_pvars: usize, level: usize) -> Vec<PVRef> {
        (start_pvars..self.pvar_count)
            .map(|idx| PVRef::new(level as u8, idx as u8))
            .collect()
    }
}
