//! Template compilation for syntax-rules
//!
//! This module handles compilation of Scheme syntax-rules templates into
//! PVREF-based Template representations.

use super::Compiler;
use crate::error::MacroError;
use crate::macro_expander::utils::collect_template_vars_at_level;
use crate::macro_expander::{Identifier, Template};
use patina_core::TaggedValue;
use patina_runtime::{PVRef, ScopeSet};

impl Compiler {
    /// Compile a template at the given ellipsis level
    ///
    /// Based on Gauche's template compilation (macro.c:400+).
    pub fn compile_template(
        &mut self,
        form: TaggedValue,
        level: usize,
    ) -> Result<Template, MacroError> {
        // Handle all identifier types (Symbol, Identifier). Every leaf of every
        // template reaches here, so the identity is read once and reused.
        if let Some(key) = self.identifier_key(form) {
            let s = key.name.clone();

            // Check if it's a pattern variable.
            //
            // This runs BEFORE the substituted-identifier case below. An
            // identifier substituted by an outer expansion can legitimately be
            // this rule's own pattern variable -- the pattern compiler
            // classifies it by the literals list alone -- and a rule's own
            // pattern variable always wins over its spelling.
            if let Some(&pvref) = self.pvars.get(&key) {
                // Verify level is valid
                if pvref.level() > level {
                    return Err(MacroError::InvalidSyntax(format!(
                        "Pattern variable {} at level {} used at level {}",
                        s,
                        pvref.level(),
                        level
                    )));
                }
                return Ok(Template::Var(pvref));
            }

            // An identifier substituted by an outer expansion, and not this
            // rule's pattern variable, keeps the identity the outer macro gave
            // it. Emitting it verbatim preserves that; re-tagging it with this
            // macro's definition scopes below would rebind it to the wrong
            // context.
            if self.is_substituted_from_outer_macro(form) {
                return Ok(self.make_literal_template(form));
            }

            // Not a pattern variable - apply hygiene handling
            if !self.definition_scopes.is_empty() {
                // Scope-based hygiene: tag with definition scopes, KEEPING any
                // scopes the identifier already carries. Replacing them
                // collapsed identity: when a macro-generated macro's template
                // captures identifiers introduced by *different* expansions of
                // the same outer rule — chibi-match's per-variable `p-ls`
                // temporaries, spelled alike and distinguished only by their
                // expansion scopes — stamping all of them with this macro's
                // one definition scope set made them indistinguishable, so
                // match-letrec bound two variables to one temporary. The union
                // preserves the distinction and still contains everything the
                // definition-scope resolution looks for.
                let mut scopes = self.definition_scopes.clone();
                for scope in key.scopes.iter() {
                    scopes.add_scope(*scope);
                }
                return Ok(Template::Symbol(Identifier::with_scopes(s, scopes)));
            } else {
                // Fall back to marks-and-ribs hygiene (no scopes available)
                let should_capture = self.env.as_ref().is_some_and(|env| env.get(&s).is_some());

                if should_capture {
                    return Ok(Template::Symbol(Identifier::with_scopes(
                        s,
                        ScopeSet::new(),
                    )));
                } else {
                    return Ok(Template::Symbol(Identifier::new(s)));
                }
            }
        }

        // Check for pair
        let is_pair = form.is_pair();
        if is_pair {
            let (items, tail) = self.collect_list_items(form)?;

            // Check for quote form: (quote datum)
            if items.len() == 2
                && self
                    .extract_symbol_name(items[0])
                    .map(|s| s.as_ref() == "quote")
                    .unwrap_or(false)
                && tail.is_none()
            {
                // Check for ellipsis escape inside quote: '(... template)
                let is_inner_pair = items[1].is_pair();
                if is_inner_pair {
                    let inner_result = self.collect_list_items(items[1]);
                    if let Ok((inner_items, None)) = &inner_result
                        && inner_items.len() == 2
                        && self.is_ellipsis(inner_items[0])
                    {
                        // Ellipsis escape inside quote
                        let inner_template =
                            self.compile_quote_template_escaped(inner_items[1], level)?;

                        let quote_sym = self.heap.borrow_mut().intern_symbol("quote");
                        let quote_symbol = self.make_literal_template(quote_sym);
                        return Ok(Template::List(vec![quote_symbol, inner_template]));
                    }
                }

                // Check if the quoted datum contains pattern variables
                if self.contains_pattern_vars(items[1]) {
                    // Has pattern variables - compile normally so they expand
                    // Fall through to normal list compilation
                } else {
                    // No pattern variables - treat as literal (no hygiene renaming)
                    return Ok(self.make_literal_template(form));
                }
            }

            // Check for ellipsis escape: (... template)
            if items.len() == 2 && self.is_ellipsis(items[0]) {
                // Ellipsis escape - compile inner template with ellipsis disabled
                return self.compile_with_escaped_ellipsis(items[1], level);
            }

            if let Some(tail_value) = tail {
                // Dotted list template
                self.compile_dotted_template(&items, tail_value, level)
            } else {
                // Regular list template
                self.compile_list_template(&items, level)
            }
        } else if form == TaggedValue::NULL {
            Ok(Template::List(vec![]))
        } else if form.is_vector() {
            // Vector
            let heap = self.heap.borrow();
            let len = heap.vector_len(form);
            let elements: Vec<TaggedValue> = (0..len).map(|i| heap.vector_ref(form, i)).collect();
            drop(heap);
            // Vector templates carry ellipses just like list ones -- `#(x ...)`
            // is valid -- so this shares the ellipsis-aware item compiler.
            let templates = self.compile_template_items(&elements, level)?;
            Ok(Template::Vector(templates))
        } else {
            // Literal value
            Ok(self.make_literal_template(form))
        }
    }

    /// Compile a list template, detecting ellipsis and double ellipsis
    pub(super) fn compile_list_template(
        &mut self,
        items: &[TaggedValue],
        level: usize,
    ) -> Result<Template, MacroError> {
        let templates = self.compile_template_items(items, level)?;
        Ok(Template::List(templates))
    }

    /// Compile a dotted list template: (a b . rest)
    pub(super) fn compile_dotted_template(
        &mut self,
        items: &[TaggedValue],
        tail: TaggedValue,
        level: usize,
    ) -> Result<Template, MacroError> {
        // The items before the dot can themselves carry ellipses: `(x ... . tail)`
        // is a valid R7RS template (R7RS 4.3.2), so this has to go through the
        // same ellipsis-aware path as the proper-list case. Compiling each item
        // directly at `level` instead would report any ellipsis variable as
        // "level 1 used at level 0".
        let templates = self.compile_template_items(items, level)?;

        let tail_template = Box::new(self.compile_template(tail, level)?);

        Ok(Template::DottedList {
            templates,
            tail: tail_template,
        })
    }

    /// Compile a sequence of template items, handling ellipsis and double ellipsis
    pub(super) fn compile_template_items(
        &mut self,
        items: &[TaggedValue],
        level: usize,
    ) -> Result<Vec<Template>, MacroError> {
        let mut templates = Vec::new();
        let mut i = 0;

        while i < items.len() {
            if self.is_followed_by_ellipsis(items, i) {
                // Found ellipsis in template - check for consecutive ellipses
                let (ellipsis_template, skip_count) =
                    self.compile_ellipsis_template(items[i], &items[i + 1..], level)?;
                templates.push(ellipsis_template);
                i += 1 + skip_count; // Skip item and ellipses
            } else {
                templates.push(self.compile_template(items[i], level)?);
                i += 1;
            }
        }

        Ok(templates)
    }

    /// Compile an ellipsis template (item ... or item ... ...)
    ///
    /// Returns the compiled template and the number of ellipses consumed.
    pub(super) fn compile_ellipsis_template(
        &mut self,
        item: TaggedValue,
        rest: &[TaggedValue],
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
    pub(super) fn count_consecutive_ellipses(&self, items: &[TaggedValue]) -> u8 {
        let mut count = 0u8;
        for item in items {
            if self.is_ellipsis(*item) {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Verify that template variables are at valid levels for ellipsis nesting
    pub(super) fn verify_ellipsis_nesting(
        &self,
        vars: &[PVRef],
        base_level: usize,
        _nesting: usize,
    ) -> Result<(), MacroError> {
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
