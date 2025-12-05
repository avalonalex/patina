//! Macro validation helpers
//!
//! This module provides early validation of macro definitions to catch common errors
//! before they cause runtime failures. Validations include:
//! - Variables in template that aren't in pattern
//! - Improper ellipsis nesting
//! - Invalid pattern/template structure
//! - Ellipsis level mismatches

use crate::macro_expander::utils::{
    collect_pattern_vars_with_levels, collect_template_vars_with_levels,
};
use crate::macro_expander::{Pattern, Template};
use patina_runtime::PVRef;
use std::collections::HashMap;

/// Validation error types
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    /// Template uses a variable that doesn't appear in the pattern
    UndefinedVariable { var_name: String, pvref: PVRef },

    /// Variable used at wrong ellipsis level in template
    LevelMismatch {
        var_name: String,
        pattern_level: usize,
        template_level: usize,
    },

    /// Ellipsis in template has no corresponding ellipsis in pattern
    UnmatchedEllipsis { level: u8 },

    /// Pattern contains invalid structure
    InvalidPattern { message: String },

    /// Template contains invalid structure
    InvalidTemplate { message: String },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::UndefinedVariable { var_name, pvref } => {
                write!(
                    f,
                    "Macro validation error: variable '{}' (PVRef {:?}) appears in template but not in pattern\n\
                     Hint: Ensure all template variables are bound in the pattern",
                    var_name, pvref
                )
            }
            ValidationError::LevelMismatch {
                var_name,
                pattern_level,
                template_level,
            } => {
                write!(
                    f,
                    "Macro validation error: variable '{}' level mismatch\n\
                     Pattern level:  {} (bound in {} ellips{})\n\
                     Template level: {} (used in {} ellips{})\n\
                     Hint: Variables can be used at the same or HIGHER ellipsis level (broadcast/repeat),\n\
                           but NOT at a lower level (cannot extract from ellipsis)",
                    var_name,
                    pattern_level,
                    pattern_level,
                    if *pattern_level == 1 { "is" } else { "es" },
                    template_level,
                    template_level,
                    if *template_level == 1 { "is" } else { "es" }
                )
            }
            ValidationError::UnmatchedEllipsis { level } => {
                write!(
                    f,
                    "Macro validation error: ellipsis at level {} in template has no corresponding pattern ellipsis\n\
                     Hint: Each ellipsis in the template must correspond to an ellipsis in the pattern",
                    level
                )
            }
            ValidationError::InvalidPattern { message } => {
                write!(
                    f,
                    "Macro validation error: invalid pattern structure\n\
                     Error: {}\n\
                     Hint: Check pattern syntax",
                    message
                )
            }
            ValidationError::InvalidTemplate { message } => {
                write!(
                    f,
                    "Macro validation error: invalid template structure\n\
                     Error: {}\n\
                     Hint: Check template syntax",
                    message
                )
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Validate a macro rule (pattern + template pair)
///
/// Returns Ok(()) if the rule is valid, or ValidationError if there are issues.
/// This should be called during macro compilation to catch errors early.
pub fn validate_rule(
    pattern: &Pattern,
    template: &Template,
    pvar_names: &HashMap<PVRef, std::rc::Rc<str>>,
) -> Result<(), ValidationError> {
    // Collect all variables from pattern with their levels
    let pattern_vars = collect_pattern_vars_with_levels(pattern);

    // Collect all variables from template with their levels
    let template_vars = collect_template_vars_with_levels(template);

    // Check that all template variables exist in pattern
    for (pvref, template_level) in &template_vars {
        if let Some(&pattern_level) = pattern_vars.get(pvref) {
            // Variable exists - check level consistency
            // R7RS allows using a variable at a HIGHER level (inside more ellipses)
            // than it was bound - it just gets repeated for each iteration.
            // But using at a LOWER level would be invalid (extracting from ellipsis).
            if *template_level < pattern_level {
                let var_name = pvar_names
                    .get(pvref)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("var#{}", pvref.index()));

                return Err(ValidationError::LevelMismatch {
                    var_name,
                    pattern_level,
                    template_level: *template_level,
                });
            }
            // template_level >= pattern_level is OK - variable is repeated
        } else {
            // Variable doesn't exist in pattern
            let var_name = pvar_names
                .get(pvref)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("var#{}", pvref.index()));

            return Err(ValidationError::UndefinedVariable {
                var_name,
                pvref: *pvref,
            });
        }
    }

    // Validate pattern structure
    validate_pattern_structure(pattern)?;

    // Validate template structure
    validate_template_structure(template)?;

    Ok(())
}

/// Validate pattern structure for common errors
fn validate_pattern_structure(pattern: &Pattern) -> Result<(), ValidationError> {
    match pattern {
        Pattern::List(patterns) => {
            // Check each subpattern
            for p in patterns {
                validate_pattern_structure(p)?;
            }

            // Check for multiple ellipses at same level (not allowed)
            let ellipsis_count = patterns.iter().filter(|p| p.is_ellipsis()).count();
            if ellipsis_count > 1 {
                return Err(ValidationError::InvalidPattern {
                    message: "Multiple ellipses in same list pattern not supported".to_string(),
                });
            }

            Ok(())
        }
        Pattern::Vector(patterns) => {
            for p in patterns {
                validate_pattern_structure(p)?;
            }
            Ok(())
        }
        Pattern::DottedList { patterns, tail } => {
            for p in patterns {
                validate_pattern_structure(p)?;
            }
            validate_pattern_structure(tail)?;
            Ok(())
        }
        Pattern::Ellipsis { subpattern, .. } => validate_pattern_structure(subpattern),
        Pattern::Var(_) | Pattern::Wildcard | Pattern::Literal(_) => Ok(()),
    }
}

/// Validate template structure for common errors
fn validate_template_structure(template: &Template) -> Result<(), ValidationError> {
    match template {
        Template::List(templates) => {
            for t in templates {
                validate_template_structure(t)?;
            }
            Ok(())
        }
        Template::Vector(templates) => {
            for t in templates {
                validate_template_structure(t)?;
            }
            Ok(())
        }
        Template::DottedList { templates, tail } => {
            for t in templates {
                validate_template_structure(t)?;
            }
            validate_template_structure(tail)?;
            Ok(())
        }
        Template::Ellipsis {
            subtemplate,
            nesting,
            ..
        } => {
            // Validate nesting level
            if *nesting == 0 {
                return Err(ValidationError::InvalidTemplate {
                    message: "Ellipsis nesting level cannot be 0".to_string(),
                });
            }
            if *nesting > 2 {
                return Err(ValidationError::InvalidTemplate {
                    message: format!("Ellipsis nesting level {} not supported (max 2)", nesting),
                });
            }

            validate_template_structure(subtemplate)
        }
        Template::Var(_) | Template::Literal(_) | Template::Symbol(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_rule() {
        // Pattern: (x y)
        // Template: (list x y)
        let x = PVRef::new(0, 0);
        let y = PVRef::new(0, 1);

        let pattern = Pattern::List(vec![Pattern::Var(x), Pattern::Var(y)]);

        let template = Template::List(vec![
            Template::Symbol(crate::macro_expander::Identifier::new("list")),
            Template::Var(x),
            Template::Var(y),
        ]);

        let mut pvar_names = HashMap::new();
        pvar_names.insert(x, std::rc::Rc::from("x"));
        pvar_names.insert(y, std::rc::Rc::from("y"));

        let result = validate_rule(&pattern, &template, &pvar_names);
        assert!(result.is_ok());
    }

    #[test]
    fn test_undefined_variable() {
        // Pattern: (x)
        // Template: (list x y)  <- y is undefined!
        let x = PVRef::new(0, 0);
        let y = PVRef::new(0, 1);

        let pattern = Pattern::List(vec![Pattern::Var(x)]);

        let template = Template::List(vec![
            Template::Symbol(crate::macro_expander::Identifier::new("list")),
            Template::Var(x),
            Template::Var(y), // Not in pattern!
        ]);

        let mut pvar_names = HashMap::new();
        pvar_names.insert(x, std::rc::Rc::from("x"));
        pvar_names.insert(y, std::rc::Rc::from("y"));

        let result = validate_rule(&pattern, &template, &pvar_names);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::UndefinedVariable { .. }
        ));
    }

    #[test]
    fn test_level_mismatch() {
        // Pattern: (x ...)  <- x at level 1
        // Template: (list x)  <- x used at level 0! (extracting from ellipsis - INVALID)
        let x = PVRef::new(1, 0);

        let pattern = Pattern::List(vec![Pattern::Ellipsis {
            subpattern: Box::new(Pattern::Var(x)),
            level: 1,
            num_following: 0,
            vars: vec![x],
        }]);

        let template = Template::List(vec![
            Template::Symbol(crate::macro_expander::Identifier::new("list")),
            Template::Var(x), // Used at level 0, but defined at level 1 - INVALID!
        ]);

        let mut pvar_names = HashMap::new();
        pvar_names.insert(x, std::rc::Rc::from("x"));

        let result = validate_rule(&pattern, &template, &pvar_names);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::LevelMismatch { .. }
        ));
    }

    #[test]
    fn test_level_broadcast_valid() {
        // Pattern: (broadcast x (y ...))  <- x at level 0, y at level 1
        // Template: ((list x y) ...)  <- x used at level 1 (broadcast) - VALID!
        // This is valid R7RS - x gets repeated for each y
        let x = PVRef::new(0, 0);
        let y = PVRef::new(1, 1);

        let pattern = Pattern::List(vec![
            Pattern::Var(x),
            Pattern::List(vec![Pattern::Ellipsis {
                subpattern: Box::new(Pattern::Var(y)),
                level: 1,
                num_following: 0,
                vars: vec![y],
            }]),
        ]);

        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::List(vec![
                Template::Symbol(crate::macro_expander::Identifier::new("list")),
                Template::Var(x), // Level 0 var used in level 1 context - VALID (broadcast)
                Template::Var(y),
            ])),
            level: 1,
            nesting: 1,
            vars: vec![y],
        }]);

        let mut pvar_names = HashMap::new();
        pvar_names.insert(x, std::rc::Rc::from("x"));
        pvar_names.insert(y, std::rc::Rc::from("y"));

        let result = validate_rule(&pattern, &template, &pvar_names);
        assert!(result.is_ok(), "Broadcast pattern should be valid");
    }

    #[test]
    fn test_triple_ellipsis_rejected() {
        // Template with triple ellipsis should be rejected
        let x = PVRef::new(3, 0);

        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::Var(x)),
            level: 1,
            nesting: 3, // Triple!
            vars: vec![x],
        }]);

        let result = validate_template_structure(&template);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ValidationError::InvalidTemplate { .. }
        ));
    }
}
