//! Pattern compilation for syntax-rules
//!
//! This module handles compilation of Scheme syntax-rules patterns into
//! PVREF-based Pattern representations.

use super::Compiler;
use crate::error::MacroError;
use crate::macro_expander::Pattern;
use crate::macro_expander::utils::WILDCARD;
use patina_core::TaggedValue;
use patina_runtime::PVRef;

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
        form: TaggedValue,
        level: usize,
    ) -> Result<Pattern, MacroError> {
        if form.is_pair() {
            let (items, tail) = self.collect_list_items(form)?;
            if items.is_empty() {
                return Err(MacroError::InvalidSyntax(
                    "Pattern must have at least a macro keyword".to_string(),
                ));
            }

            if let Some(tail_value) = tail {
                // Dotted list pattern: (keyword p1 p2 . rest)
                // First element is keyword (wildcard), rest are normal patterns
                // The items before the dot may carry ellipses -- `(kw x ... . r)`
                // is a valid R7RS rule pattern -- so they go through the same
                // ellipsis-aware compiler the proper-list rule path uses.
                let mut patterns = vec![Pattern::Wildcard];
                patterns.extend(self.compile_pattern_items(&items[1..], level)?);
                let tail_pattern = Box::new(self.compile_pattern(tail_value, level)?);
                Ok(Pattern::DottedList {
                    patterns,
                    tail: tail_pattern,
                })
            } else {
                // Regular list pattern: (keyword p1 p2 ...)
                // First element is keyword (wildcard), rest are compiled with ellipsis detection
                self.compile_rule_list_pattern(&items, level)
            }
        } else {
            Err(MacroError::InvalidSyntax(
                "Pattern must be a list starting with macro keyword".to_string(),
            ))
        }
    }

    /// Compile a rule list pattern where the first element is the macro keyword
    ///
    /// This is similar to compile_list_pattern but treats the first element as a wildcard.
    fn compile_rule_list_pattern(
        &mut self,
        items: &[TaggedValue],
        level: usize,
    ) -> Result<Pattern, MacroError> {
        let mut patterns = vec![Pattern::Wildcard]; // First element is always wildcard
        let mut i = 1; // Start from second element

        while i < items.len() {
            // Check for ellipsis
            if i + 1 < items.len() && self.is_ellipsis(items[i + 1]) {
                // Found ellipsis pattern: (item ...)
                let num_following = items.len() - i - 2;
                let start_pvars = self.pvar_count;
                let subpattern = self.compile_pattern(items[i], level + 1)?;
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
                patterns.push(self.compile_pattern(items[i], level)?);
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
    /// - `form`: TaggedValue representing the pattern
    /// - `level`: Current ellipsis nesting level (0 = not in ellipsis)
    pub fn compile_pattern(
        &mut self,
        form: TaggedValue,
        level: usize,
    ) -> Result<Pattern, MacroError> {
        // Handle all identifier types (Symbol, Identifier)
        if let Some(s) = self.extract_symbol_name(form) {
            // R7RS: Check literals FIRST (including _ if it's in the literals list)
            if self.is_literal(&s) {
                return Ok(self.make_literal_pattern(form));
            }

            // Check if this is a SUBSTITUTED identifier (empty scopes) from outer expansion.
            if self.is_substituted_from_outer_macro(form) {
                return Ok(self.make_literal_pattern(form));
            }

            if s.as_ref() == WILDCARD {
                // Underscore is wildcard only if NOT in literals
                Ok(Pattern::Wildcard)
            } else {
                // Pattern variable - assign PVREF
                let pvref = self.add_pvar(s, level)?;
                Ok(Pattern::Var(pvref))
            }
        } else {
            // Check for pair
            let is_pair = form.is_pair();
            if is_pair {
                let (items, tail) = self.collect_list_items(form)?;
                if let Some(tail_value) = tail {
                    // Dotted list pattern
                    self.compile_dotted_pattern(&items, tail_value, level)
                } else {
                    // Regular list pattern
                    self.compile_list_pattern(&items, level)
                }
            } else if form == TaggedValue::NULL {
                Ok(Pattern::List(vec![]))
            } else if form.is_vector() {
                // Vector
                let heap = self.heap.borrow();
                let len = heap.vector_len(form);
                let elements: Vec<TaggedValue> =
                    (0..len).map(|i| heap.vector_ref(form, i)).collect();
                drop(heap);
                // Vector patterns carry ellipses just like list ones --
                // `#(x ...)` is valid -- so this shares the ellipsis-aware
                // item compiler.
                let patterns = self.compile_pattern_items(&elements, level)?;
                Ok(Pattern::Vector(patterns))
            } else {
                // Literal value (boolean, number, string, character, etc.)
                Ok(self.make_literal_pattern(form))
            }
        }
    }

    /// Compile a list pattern, detecting ellipsis
    fn compile_list_pattern(
        &mut self,
        items: &[TaggedValue],
        level: usize,
    ) -> Result<Pattern, MacroError> {
        let patterns = self.compile_pattern_items(items, level)?;
        Ok(Pattern::List(patterns))
    }

    /// Compile a dotted list pattern: (a b . rest)
    /// Also handles patterns with ellipsis: (a b ... . rest)
    fn compile_dotted_pattern(
        &mut self,
        items: &[TaggedValue],
        tail: TaggedValue,
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
    fn compile_pattern_items(
        &mut self,
        items: &[TaggedValue],
        level: usize,
    ) -> Result<Vec<Pattern>, MacroError> {
        let mut patterns = Vec::new();
        let mut i = 0;

        while i < items.len() {
            if self.is_followed_by_ellipsis(items, i) {
                // Found ellipsis pattern: (item ...)
                let num_following = items.len() - i - 2;
                let ellipsis_pattern =
                    self.compile_ellipsis_pattern(items[i], level, num_following)?;
                patterns.push(ellipsis_pattern);
                i += 2; // Skip pattern and ellipsis
            } else {
                patterns.push(self.compile_pattern(items[i], level)?);
                i += 1;
            }
        }

        Ok(patterns)
    }

    /// Compile an ellipsis pattern (item ...)
    fn compile_ellipsis_pattern(
        &mut self,
        item: TaggedValue,
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
