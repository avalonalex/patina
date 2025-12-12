//! Template compilation for syntax-rules
//!
//! This module handles compilation of Scheme syntax-rules templates into
//! PVREF-based Template representations.

use super::Compiler;
use crate::error::MacroError;
use crate::macro_expander::template::{Identifier, Template};
use crate::macro_expander::utils::collect_template_vars_at_level;
use patina_runtime::{PVRef, ScopeSet, Value};

impl Compiler {
    /// Compile a template at the given ellipsis level
    ///
    /// Based on Gauche's template compilation (macro.c:400+).
    pub fn compile_template(&mut self, form: &Value, level: usize) -> Result<Template, MacroError> {
        // Handle all identifier types (Symbol, ScopedIdentifier, WrappedIdentifier)
        // This is needed for nested macro definitions where identifiers may be wrapped.
        if let Some(s) = Self::extract_symbol_name(form) {
            // IMPORTANT: Check for scopes BEFORE checking pattern variables.
            // Identifiers with non-empty scopes came from outer macro expansion
            // and should be treated as literals, not as pattern variables.
            // This is the template-side counterpart to the pattern check in compile_pattern.
            if Self::is_substituted_from_outer_macro(form) {
                // Identifier with scopes = came from outer expansion = literal value
                // Keep it as a literal so it's inserted verbatim into the output
                return Ok(Template::Literal(form.clone()));
            }

            // Check if it's a pattern variable (only for identifiers WITHOUT scopes)
            if let Some(pvref) = self.pvars.get(s) {
                // Verify level is valid
                if pvref.level() > level {
                    return Err(MacroError::InvalidSyntax(format!(
                        "Pattern variable {} at level {} used at level {}",
                        s,
                        pvref.level(),
                        level
                    )));
                }
                return Ok(Template::Var(*pvref));
            }

            // Not a pattern variable - apply hygiene handling
            // Scope-based hygiene approach (Racket-style):
            // ALL non-pattern-variable identifiers in templates are tagged with definition scopes.
            // The scopes represent the lexical context at macro definition time.
            // At expansion time, the identifier will carry these scopes.
            // At lookup time, we find the binding with matching scope subset.
            //
            // This differs from the old marks-and-ribs approach which only tagged
            // identifiers that were bound at definition time ("free variables").
            // In scope-based hygiene, the scopes ARE the mechanism for hygiene -
            // we don't need to distinguish free vs introduced at compile time.

            if !self.definition_scopes.is_empty() {
                // Scope-based hygiene: tag with definition scopes
                return Ok(Template::Symbol(Identifier::with_scopes(
                    s.clone(),
                    self.definition_scopes.clone(),
                )));
            } else {
                // Fall back to marks-and-ribs hygiene (no scopes available)
                // Check if it's bound in the captured environment at compile time
                let should_capture = self.env.as_ref().is_some_and(|env| env.get(s).is_some());

                if should_capture {
                    // Free variable - use empty scopes (definition-time scopes)
                    return Ok(Template::Symbol(Identifier::with_scopes(
                        s.clone(),
                        ScopeSet::new(),
                    )));
                } else {
                    // Introduced identifier - will get expansion scope via flip-scope
                    return Ok(Template::Symbol(Identifier::new(s.clone())));
                }
            }
        }

        match form {
            // List - check for ellipsis and ellipsis escape
            Value::Pair(_) => {
                let (items, tail) = self.collect_list_items(form)?;

                // Check for quote form: (quote datum)
                // R7RS requires pattern variables to be expanded inside quotes,
                // but literal data without pattern variables should stay literal.
                // For example: (syntax-rules () ((m x) '(x))) should expand (m 5) to '(5)
                // But: (syntax-rules () ((m x) 'ok)) should expand (m 5) to 'ok (not renamed)
                // Note: Check for identifier types to support nested macros
                if items.len() == 2
                    && Self::extract_symbol_name(&items[0])
                        .map(|s| s.as_ref() == "quote")
                        .unwrap_or(false)
                    && tail.is_none()
                {
                    // Check for ellipsis escape inside quote: '(... template)
                    // R7RS: (... template) produces template with ... treated literally
                    // but pattern variables are still substituted.
                    // So '(... ...) should produce '...
                    // And '(... (x ...)) where x is a pvar should produce '(val ...)
                    if let Value::Pair(_) = &items[1]
                        && let Ok((inner_items, None)) = self.collect_list_items(&items[1])
                        && inner_items.len() == 2
                        && self.is_ellipsis(&inner_items[0])
                    {
                        // Ellipsis escape inside quote: '(... template) → '(compiled_template_with_no_ellipsis)
                        // Compile the inner template with ellipsis disabled, using quote compilation
                        // so that symbols become Literal values, not ScopedIdentifiers
                        let inner_template =
                            self.compile_quote_template_escaped(&inner_items[1], level)?;

                        // Wrap in a quote template
                        let quote_symbol = Template::Literal(Value::Symbol("quote".into()));
                        return Ok(Template::List(vec![quote_symbol, inner_template]));
                    }

                    // Check if the quoted datum contains pattern variables
                    if self.contains_pattern_vars(&items[1]) {
                        // Has pattern variables - compile normally so they expand
                        // Fall through to normal list compilation
                    } else {
                        // No pattern variables - treat as literal (no hygiene renaming)
                        return Ok(Template::Literal(form.clone()));
                    }
                }

                // Check for ellipsis escape: (... template)
                if items.len() == 2 && self.is_ellipsis(&items[0]) {
                    // Ellipsis escape - compile inner template with ellipsis disabled
                    return self.compile_with_escaped_ellipsis(&items[1], level);
                }

                if let Some(tail_value) = tail {
                    // Dotted list template
                    self.compile_dotted_template(&items, &tail_value, level)
                } else {
                    // Regular list template
                    self.compile_list_template(&items, level)
                }
            }

            Value::Null => Ok(Template::List(vec![])),

            // Vector
            Value::Vector(v) => {
                let items = v.borrow();
                let mut templates = Vec::new();
                for item in items.iter() {
                    templates.push(self.compile_template(item, level)?);
                }
                Ok(Template::Vector(templates))
            }

            // Literal value
            other => Ok(Template::Literal(other.clone())),
        }
    }

    /// Compile a list template, detecting ellipsis and double ellipsis
    pub(super) fn compile_list_template(
        &mut self,
        items: &[Value],
        level: usize,
    ) -> Result<Template, MacroError> {
        let templates = self.compile_template_items(items, level)?;
        Ok(Template::List(templates))
    }

    /// Compile a dotted list template: (a b . rest)
    pub(super) fn compile_dotted_template(
        &mut self,
        items: &[Value],
        tail: &Value,
        level: usize,
    ) -> Result<Template, MacroError> {
        // Dotted templates don't support ellipsis in the items before the dot
        // (that would be unusual syntax), so we compile each item directly
        let mut templates = Vec::new();
        for item in items {
            templates.push(self.compile_template(item, level)?);
        }

        let tail_template = Box::new(self.compile_template(tail, level)?);

        Ok(Template::DottedList {
            templates,
            tail: tail_template,
        })
    }

    /// Compile a sequence of template items, handling ellipsis and double ellipsis
    ///
    /// This handles the SRFI-149 double ellipsis extension where `x ... ...`
    /// means nested iteration.
    pub(super) fn compile_template_items(
        &mut self,
        items: &[Value],
        level: usize,
    ) -> Result<Vec<Template>, MacroError> {
        let mut templates = Vec::new();
        let mut i = 0;

        while i < items.len() {
            if self.is_followed_by_ellipsis(items, i) {
                // Found ellipsis in template - check for consecutive ellipses
                let (ellipsis_template, skip_count) =
                    self.compile_ellipsis_template(&items[i], &items[i + 1..], level)?;
                templates.push(ellipsis_template);
                i += 1 + skip_count; // Skip item and ellipses
            } else {
                templates.push(self.compile_template(&items[i], level)?);
                i += 1;
            }
        }

        Ok(templates)
    }

    /// Compile an ellipsis template (item ... or item ... ...)
    ///
    /// Returns the compiled template and the number of ellipses consumed.
    /// Supports SRFI-149 double ellipsis for nested iteration.
    pub(super) fn compile_ellipsis_template(
        &mut self,
        item: &Value,
        rest: &[Value],
        level: usize,
    ) -> Result<(Template, usize), MacroError> {
        // Count consecutive ellipses for double ellipsis support (SRFI-149)
        let nesting = self.count_consecutive_ellipses(rest);

        // Compile the base template at deepest level
        let subtemplate = self.compile_template(item, level + nesting as usize)?;

        // Collect variables that should iterate
        let vars = collect_template_vars_at_level(&subtemplate, level + 1);

        if vars.is_empty() {
            return Err(MacroError::InvalidSyntax(
                "Ellipsis in template contains no pattern variables".to_string(),
            ));
        }

        // Verify variables are at appropriate levels for nesting
        self.verify_ellipsis_nesting(&vars, level, nesting as usize)?;

        let template = Template::Ellipsis {
            subtemplate: Box::new(subtemplate),
            level: (level + 1) as u8,
            nesting,
            vars,
        };

        Ok((template, nesting as usize))
    }

    /// Count consecutive ellipses starting from the beginning of items
    pub(super) fn count_consecutive_ellipses(&self, items: &[Value]) -> u8 {
        let mut count = 0u8;
        for item in items {
            if self.is_ellipsis(item) {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Verify that template variables are at valid levels for ellipsis nesting
    ///
    /// For an ellipsis at a given level, we need at least one variable that will
    /// provide iteration. This can be:
    /// 1. A variable at exactly the right level (standard case)
    /// 2. A variable at a DEEPER level (pass-through case for nested ellipsis)
    ///
    /// For example, with pattern `((item ...) ...)` and template `(list (list item ...) ...)`:
    /// - The outer `...` is at level 1
    /// - `item` is at level 2
    /// - This is valid: the outer ellipsis iterates over the groups, and `item` provides
    ///   the iteration count indirectly through its nested structure
    pub(super) fn verify_ellipsis_nesting(
        &self,
        vars: &[PVRef],
        base_level: usize,
        _nesting: usize,
    ) -> Result<(), MacroError> {
        // At least one variable must be at a level higher than base_level.
        // This includes variables at deeper levels (pass-through for nested ellipsis).
        let has_valid_var = vars.iter().any(|pv| pv.level() > base_level);

        if !has_valid_var {
            return Err(MacroError::InvalidSyntax(format!(
                "Invalid ellipsis nesting: no variables above level {} to drive iteration",
                base_level
            )));
        }

        Ok(())
    }
}
