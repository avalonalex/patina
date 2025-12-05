//! PVREF-based template expansion (Version 2)
//!
//! This module implements template expansion for the PVREF-based macro system.
//! It takes compiled Template2 structures and MatchEnv trees from pattern matching,
//! expanding them into output expressions.
//!
//! Key improvements over the original expander:
//! - Uses PVREF encoding for O(1) variable lookup
//! - Uses MatchEnv tree navigation for nested ellipsis
//! - Supports double ellipsis (SRFI-149)
//! - Proper hygiene through Identifier renaming
//!
//! Inspired by Gauche's template expansion (macro.c:800+)
//! Reference: https://github.com/shirok/Gauche/blob/master/src/macro.c

use crate::macro_expander::template::{Identifier, Template};
use crate::macro_expander::utils::{
    is_macro_definition_form, list_to_vec as utils_list_to_vec, vec_to_list as utils_vec_to_list,
};
use patina_runtime::{MatchEnv, MatchValue, PVRef, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Error type for template expansion failures
#[derive(Debug, Clone, PartialEq)]
pub enum ExpandError {
    /// Undefined pattern variable referenced in template
    UndefinedVariable { pvref: String },

    /// Variable used at wrong ellipsis level
    LevelMismatch {
        pvref: String,
        template_level: usize,
        actual_level: usize,
    },

    /// Ellipsis iteration with inconsistent repetition counts
    InconsistentRepetition { expected: usize, actual: usize },

    /// Invalid template structure
    InvalidTemplate { message: String },
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpandError::UndefinedVariable { pvref } => {
                write!(
                    f,
                    "Template expansion failed: undefined pattern variable: {}\n\
                     Hint: This variable was not bound during pattern matching. \
                     Check that the variable appears in the macro pattern",
                    pvref
                )
            }
            ExpandError::LevelMismatch {
                pvref,
                template_level,
                actual_level,
            } => {
                write!(
                    f,
                    "Template expansion failed: ellipsis level mismatch\n\
                     Variable:       {}\n\
                     Template level: {}\n\
                     Actual level:   {}\n\
                     Hint: Variable must be used at the same ellipsis nesting depth where it was bound",
                    pvref, template_level, actual_level
                )
            }
            ExpandError::InconsistentRepetition { expected, actual } => {
                write!(
                    f,
                    "Template expansion failed: inconsistent ellipsis repetition\n\
                     Expected: {} iteration(s)\n\
                     Got:      {} iteration(s)\n\
                     Hint: All variables in the same ellipsis template must have the same repetition count",
                    expected, actual
                )
            }
            ExpandError::InvalidTemplate { message } => {
                write!(
                    f,
                    "Template expansion failed: invalid template structure\n\
                     Error: {}\n\
                     Hint: Check the macro template syntax",
                    message
                )
            }
        }
    }
}

impl std::error::Error for ExpandError {}

/// Template expander for PVREF-based macro system
///
/// This implements the template expansion phase of macro expansion.
/// It takes a compiled Template2 and a MatchEnv with pattern variable bindings,
/// returning the expanded output expression.
///
/// Based on Gauche's template expansion approach (macro.c:800+).
pub struct Expander {
    /// Expansion-time environment (for checking if identifiers are macro names)
    /// We use the full runtime Environment directly to avoid duplication
    expansion_env: std::rc::Rc<patina_runtime::Environment>,

    /// Macro scope for this expansion (Racket-style hygiene)
    /// Used to distinguish use-site vs introduced identifiers
    macro_scope: patina_runtime::ScopeId,
}

impl Expander {
    /// Create a new expander with runtime environment and macro scope
    ///
    /// # Arguments
    /// * `expansion_env` - Runtime environment at expansion time
    /// * `macro_scope` - Fresh scope for this macro expansion (for Racket-style hygiene)
    pub fn new(
        expansion_env: std::rc::Rc<patina_runtime::Environment>,
        macro_scope: patina_runtime::ScopeId,
    ) -> Self {
        Self {
            expansion_env,
            macro_scope,
        }
    }

    /// Expand a template with the given match environment
    ///
    /// This is the main entry point for template expansion.
    ///
    /// Inspired by Gauche's expand_template (macro.c:800+)
    pub fn expand(&self, template: &Template, env: &MatchEnv) -> Result<Value, ExpandError> {
        self.expand_impl(template, env, &[])
    }

    /// Internal expansion implementation with indices
    ///
    /// # Arguments
    /// * `template` - The template to expand
    /// * `env` - The match environment with variable bindings
    /// * `indices` - Current ellipsis indices for navigation
    ///
    /// Based on Gauche's expand_rec (macro.c:820+)
    fn expand_impl(
        &self,
        template: &Template,
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        match template {
            Template::Literal(value) => {
                // Literal values are inserted as-is
                Ok(value.clone())
            }

            Template::Symbol(id) => {
                // Symbols are renamed for hygiene
                Ok(self.rename_identifier(id))
            }

            Template::Var(pvref) => {
                // Look up pattern variable in match environment
                // Use indices to navigate the tree
                match env.get(*pvref, indices) {
                    Some(value) => {
                        // IMPORTANT: Mark substituted symbols with macro scope for nested macro hygiene.
                        //
                        // When a macro template generates a `define-syntax`, pattern variable
                        // substitution can produce symbols that would be re-interpreted as
                        // pattern variables in the inner macro. For example:
                        //
                        //   (define-syntax foo
                        //     (syntax-rules ()
                        //       ((foo bar y)
                        //        (define-syntax bar
                        //          (syntax-rules ()
                        //            ((bar x) 'y))))))
                        //
                        // When (foo bar x) is called, 'y' substitutes to 'x'. Without marking,
                        // the inner macro sees ((bar x) 'x) and treats x in 'x as a pattern
                        // variable, wrongly returning the argument instead of the symbol 'x.
                        //
                        // By converting Symbol to Identifier with macro_scope, we mark it as
                        // "came from outer expansion". The inner macro compiler can then treat
                        // identifiers with scopes as literals rather than pattern variables.
                        Ok(self.mark_substituted_value(value))
                    }
                    None => Err(ExpandError::UndefinedVariable {
                        pvref: format!("{:?}", pvref),
                    }),
                }
            }

            Template::List(templates) => {
                // Expand list template: (t1 t2 t3)
                self.expand_list(templates, env, indices)
            }

            Template::Vector(templates) => {
                // Expand vector template: #(t1 t2 t3)
                self.expand_vector(templates, env, indices)
            }

            Template::DottedList { templates, tail } => {
                // Expand dotted list template: (t1 t2 . rest)
                self.expand_dotted_list(templates, tail, env, indices)
            }

            Template::Ellipsis {
                subtemplate,
                level,
                nesting,
                vars,
            } => {
                // Expand ellipsis template: (t ...)
                self.expand_ellipsis(subtemplate, *level, *nesting, vars, env, indices)
            }
        }
    }

    /// Expand a list template
    fn expand_list(
        &self,
        templates: &[Template],
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        let mut result = Vec::new();

        for template in templates {
            if template.is_ellipsis() {
                // Handle ellipsis specially - it expands to multiple elements
                if let Template::Ellipsis {
                    subtemplate,
                    level,
                    nesting,
                    vars,
                } = template
                {
                    let expanded =
                        self.expand_ellipsis(subtemplate, *level, *nesting, vars, env, indices)?;

                    // Expanded ellipsis should be a list - splice it in
                    match expanded {
                        Value::Null => {} // Empty list - nothing to add
                        Value::Pair(_) => {
                            // Convert list to vec and append all elements
                            let items = self.list_to_vec(&expanded)?;
                            result.extend(items);
                        }
                        _ => {
                            return Err(ExpandError::InvalidTemplate {
                                message: "Ellipsis must expand to list".to_string(),
                            });
                        }
                    }
                }
            } else {
                // Regular template - expands to single element
                let value = self.expand_impl(template, env, indices)?;
                result.push(value);
            }
        }

        // Convert vec back to Scheme list
        Ok(self.vec_to_list(result))
    }

    /// Expand a vector template
    fn expand_vector(
        &self,
        templates: &[Template],
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        let mut result = Vec::new();

        for template in templates {
            let value = self.expand_impl(template, env, indices)?;
            result.push(value);
        }

        Ok(Value::Vector(Rc::new(std::cell::RefCell::new(result))))
    }

    /// Expand a dotted list template: (t1 t2 . rest)
    fn expand_dotted_list(
        &self,
        templates: &[Template],
        tail: &Template,
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        // Expand the fixed part
        let mut items = Vec::new();
        for template in templates {
            let value = self.expand_impl(template, env, indices)?;
            items.push(value);
        }

        // Expand the tail
        let tail_value = self.expand_impl(tail, env, indices)?;

        // Build dotted list
        let mut result = tail_value;
        for item in items.into_iter().rev() {
            result = Value::Pair(Rc::new(RefCell::new((item, result))));
        }

        Ok(result)
    }

    /// Expand an ellipsis template
    ///
    /// This is the most complex case, handling iteration over pattern variable bindings.
    /// Based on Gauche's ellipsis expansion (macro.c:850+).
    ///
    /// # Parameters
    ///
    /// - `subtemplate`: The template to expand repeatedly
    /// - `level`: The ellipsis nesting level (1 for single `...`, 2 for inner of `... ...`)
    /// - `nesting`: How many consecutive ellipses (1 for `x ...`, 2 for `x ... ...`)
    /// - `vars`: Pattern variables in the subtemplate that drive iteration
    /// - `env`: The match environment containing bound values
    /// - `indices`: Current indices into nested branches for outer ellipsis levels
    ///
    /// # How Iteration Works
    ///
    /// The `indices` array tracks our position in nested branch structures. For example,
    /// with pattern `((x ...) ...)` matching `((1 2) (3 4 5))`:
    ///
    /// - `x` is bound to `Branch([Branch([1, 2]), Branch([3, 4, 5])])`
    /// - When expanding the outer `...` at iteration 0, `indices = [0]`
    /// - When expanding the inner `...` at iteration 1, `indices = [0, 1]` → gets `2`
    ///
    /// # Nesting Levels
    ///
    /// - `nesting = 1`: Standard ellipsis - iterate over branch and collect results
    /// - `nesting = 2`: Double ellipsis (SRFI-149) - expand and flatten one level
    /// - `nesting >= 3`: Not supported
    fn expand_ellipsis(
        &self,
        subtemplate: &Template,
        level: u8,
        nesting: u8,
        vars: &[PVRef],
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        if nesting == 1 {
            // Single ellipsis - standard iteration
            self.expand_single_ellipsis(subtemplate, level, vars, env, indices)
        } else if nesting == 2 {
            // Double ellipsis (SRFI-149)
            // Template: x ... ...
            // This expands x at each level, then flattens one level
            self.expand_double_ellipsis(subtemplate, level, vars, env, indices)
        } else {
            // Triple+ ellipsis not supported
            Err(ExpandError::InvalidTemplate {
                message: format!("Ellipsis nesting level {} not supported (max 2)", nesting),
            })
        }
    }

    /// Expand a single ellipsis template (nesting = 1)
    fn expand_single_ellipsis(
        &self,
        subtemplate: &Template,
        level: u8,
        vars: &[PVRef],
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        // Determine iteration count from the first variable
        // Note: For nested ellipsis patterns like ((item ...) ...), the variable
        // may be at a deeper level than this ellipsis. We use get_iteration_count_at_level
        // to get the count at THIS ellipsis level, not the variable's native level.
        let iteration_count = if let Some(&first_var) = vars.first() {
            self.get_iteration_count_at_level(env, first_var, indices, level as usize)?
        } else {
            // No variables - ellipsis matches empty
            0
        };

        // Validate that all variables have the same iteration count at this level
        for &var in vars.iter().skip(1) {
            let count = self.get_iteration_count_at_level(env, var, indices, level as usize)?;
            if count != iteration_count {
                return Err(ExpandError::InconsistentRepetition {
                    expected: iteration_count,
                    actual: count,
                });
            }
        }

        // Expand subtemplate for each iteration
        let mut result = Vec::new();
        for i in 0..iteration_count {
            // Build new indices array with this iteration index
            let mut new_indices = indices.to_vec();
            // Ensure indices is large enough
            while new_indices.len() <= level as usize {
                new_indices.push(0);
            }
            new_indices[level as usize] = i;

            let value = self.expand_impl(subtemplate, env, &new_indices)?;
            result.push(value);
        }

        // Convert result vec to Scheme list
        Ok(self.vec_to_list(result))
    }

    /// Expand a double ellipsis template (nesting = 2)
    ///
    /// Double ellipsis `x ... ...` means:
    /// 1. Expand x at the inner level (level) for each iteration
    /// 2. Then expand at the outer level (level-1) and flatten
    ///
    /// Example from SRFI-46:
    /// Pattern: ((a b ...) ...)
    /// Template: ((a b ... ...) ...)
    /// Input: ((1 2 3) (4 5))
    /// Output: ((1 2 3) (4 5))
    ///
    /// For the do macro:
    /// Expand a double ellipsis template (item ... ...)
    ///
    /// Double ellipsis (SRFI-149) expands nested repetitions and flattens the result.
    ///
    /// ## Algorithm
    ///
    /// 1. Get the outer iteration count (at level - 1)
    /// 2. For each outer iteration:
    ///    a. Build indices for the outer position
    ///    b. Get the inner iteration count
    ///    c. For each inner iteration, expand the subtemplate
    /// 3. Flatten all results into a single list
    ///
    /// ## Example
    ///
    /// Pattern: ((var init step ...) ...)
    /// Template: (loop step ... ...)
    /// Input: ((i 0 (+ i 1)) (j 10))
    /// step values: Branch([Branch([Leaf((+ i 1))]), Branch([])])
    /// Output: (loop (+ i 1) j)
    fn expand_double_ellipsis(
        &self,
        subtemplate: &Template,
        level: u8,
        vars: &[PVRef],
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        self.validate_double_ellipsis_level(level)?;

        let outer_count = self.get_outer_iteration_count(vars, env, indices, level)?;
        let max_var_level = self.get_max_var_level(vars);

        let all_results = self.expand_nested_iterations(
            subtemplate,
            env,
            indices,
            level,
            vars,
            outer_count,
            max_var_level,
        )?;

        Ok(self.vec_to_list(all_results))
    }

    /// Validate that level is appropriate for double ellipsis
    fn validate_double_ellipsis_level(&self, level: u8) -> Result<(), ExpandError> {
        if level == 0 {
            return Err(ExpandError::InvalidTemplate {
                message: "Double ellipsis requires level >= 1".to_string(),
            });
        }
        Ok(())
    }

    /// Get the iteration count at the outer level for double ellipsis
    fn get_outer_iteration_count(
        &self,
        vars: &[PVRef],
        env: &MatchEnv,
        indices: &[usize],
        level: u8,
    ) -> Result<usize, ExpandError> {
        match vars.first() {
            Some(&first_var) => {
                // We iterate at the ellipsis level (the variable's parent level)
                self.get_iteration_count_at_level(env, first_var, indices, (level - 1) as usize)
            }
            None => Ok(0),
        }
    }

    /// Get the maximum variable level from a list of PVRefs
    fn get_max_var_level(&self, vars: &[PVRef]) -> usize {
        vars.iter().map(|v| v.level()).max().unwrap_or(0)
    }

    /// Expand all nested iterations for double ellipsis
    ///
    /// Iterates through outer and inner levels, collecting all expanded values.
    #[allow(clippy::too_many_arguments)]
    fn expand_nested_iterations(
        &self,
        subtemplate: &Template,
        env: &MatchEnv,
        indices: &[usize],
        level: u8,
        vars: &[PVRef],
        outer_count: usize,
        max_var_level: usize,
    ) -> Result<Vec<Value>, ExpandError> {
        let mut all_results = Vec::new();

        for outer_idx in 0..outer_count {
            let outer_indices = self.build_outer_indices(indices, level, outer_idx, max_var_level);
            let inner_results =
                self.expand_inner_iterations(subtemplate, env, &outer_indices, vars, level)?;
            all_results.extend(inner_results);
        }

        Ok(all_results)
    }

    /// Build indices for an outer iteration
    fn build_outer_indices(
        &self,
        base_indices: &[usize],
        level: u8,
        outer_idx: usize,
        max_var_level: usize,
    ) -> Vec<usize> {
        let mut indices = base_indices.to_vec();
        // Ensure indices is long enough for the variable's level
        while indices.len() <= max_var_level {
            indices.push(0);
        }
        // Set the index at the ellipsis level (outer iteration)
        indices[level as usize] = outer_idx;
        indices
    }

    /// Expand all inner iterations for a given outer index
    fn expand_inner_iterations(
        &self,
        subtemplate: &Template,
        env: &MatchEnv,
        outer_indices: &[usize],
        vars: &[PVRef],
        level: u8,
    ) -> Result<Vec<Value>, ExpandError> {
        let inner_count = self.get_inner_iteration_count(vars, env, outer_indices)?;
        let var_level = self.get_inner_var_level(vars, level);
        let max_var_level = self.get_max_var_level(vars);

        let mut results = Vec::new();
        for inner_idx in 0..inner_count {
            let inner_indices =
                self.build_inner_indices(outer_indices, inner_idx, var_level, max_var_level);
            let value = self.expand_impl(subtemplate, env, &inner_indices)?;
            results.push(value);
        }

        Ok(results)
    }

    /// Get the iteration count for inner iterations
    fn get_inner_iteration_count(
        &self,
        vars: &[PVRef],
        env: &MatchEnv,
        outer_indices: &[usize],
    ) -> Result<usize, ExpandError> {
        match vars.first() {
            Some(&first_var) => self.get_iteration_count(env, first_var, outer_indices),
            None => Ok(0),
        }
    }

    /// Get the variable level for inner iteration indexing
    fn get_inner_var_level(&self, vars: &[PVRef], level: u8) -> usize {
        vars.first()
            .map(|v| v.level())
            .unwrap_or((level + 1) as usize)
    }

    /// Build indices for an inner iteration
    fn build_inner_indices(
        &self,
        outer_indices: &[usize],
        inner_idx: usize,
        var_level: usize,
        max_var_level: usize,
    ) -> Vec<usize> {
        let mut indices = outer_indices.to_vec();
        // Ensure indices has enough space for the variable's level
        while indices.len() <= max_var_level {
            indices.push(0);
        }
        // Set the index at the variable's level (inner iteration)
        indices[var_level] = inner_idx;
        indices
    }

    /// Get iteration count at a specific level
    fn get_iteration_count_at_level(
        &self,
        env: &MatchEnv,
        pvref: PVRef,
        indices: &[usize],
        target_level: usize,
    ) -> Result<usize, ExpandError> {
        let match_value = env
            .get_raw(pvref)
            .ok_or_else(|| ExpandError::UndefinedVariable {
                pvref: format!("{:?}", pvref),
            })?;

        // Navigate to target level
        let mut current = match_value;
        for (depth, &idx) in indices.iter().enumerate().skip(1) {
            if depth > target_level {
                break;
            }
            match current {
                MatchValue::Branch(items) => {
                    current = items
                        .get(idx)
                        .ok_or_else(|| ExpandError::UndefinedVariable {
                            pvref: format!("{:?}", pvref),
                        })?;
                }
                MatchValue::Leaf(_) => {
                    return Err(ExpandError::LevelMismatch {
                        pvref: format!("{:?}", pvref),
                        template_level: target_level,
                        actual_level: depth - 1,
                    });
                }
            }
        }

        // Current should be a Branch at target_level
        match current {
            MatchValue::Branch(items) => Ok(items.len()),
            MatchValue::Leaf(_) => Err(ExpandError::LevelMismatch {
                pvref: format!("{:?}", pvref),
                template_level: target_level,
                actual_level: 0,
            }),
        }
    }

    /// Get the iteration count for a variable at the current ellipsis level
    ///
    /// This looks at the variable's bindings and determines how many times
    /// we need to iterate at this level.
    fn get_iteration_count(
        &self,
        env: &MatchEnv,
        pvref: PVRef,
        indices: &[usize],
    ) -> Result<usize, ExpandError> {
        // Get the raw match value
        let match_value = env
            .get_raw(pvref)
            .ok_or_else(|| ExpandError::UndefinedVariable {
                pvref: format!("{:?}", pvref),
            })?;

        // Navigate to the appropriate level using indices
        // For a level-N variable, we need to navigate N-1 times using indices[1] through indices[N-1]
        // Example: level-2 variable with indices=[_, 0] navigates once using indices[1]=0
        let mut current = match_value;
        for level_idx in 1..pvref.level() {
            if level_idx >= indices.len() {
                return Err(ExpandError::UndefinedVariable {
                    pvref: format!("{:?}", pvref),
                });
            }
            let idx = indices[level_idx];
            match current {
                MatchValue::Branch(items) => {
                    current = items
                        .get(idx)
                        .ok_or_else(|| ExpandError::UndefinedVariable {
                            pvref: format!("{:?}", pvref),
                        })?;
                }
                MatchValue::Leaf(_) => {
                    return Err(ExpandError::LevelMismatch {
                        pvref: format!("{:?}", pvref),
                        template_level: pvref.level(),
                        actual_level: 0,
                    });
                }
            }
        }

        // Now current should be at the level just above where we're iterating
        // It should be a Branch
        match current {
            MatchValue::Branch(items) => Ok(items.len()),
            MatchValue::Leaf(_) => Err(ExpandError::LevelMismatch {
                pvref: format!("{:?}", pvref),
                template_level: pvref.level(),
                actual_level: 0,
            }),
        }
    }

    /// Rename an identifier for hygiene using Racket-style scope sets
    ///
    /// All identifiers become `Identifier` with appropriate scopes:
    /// - Free variables: use definition scopes (for binding resolution)
    /// - Introduced identifiers: empty scopes (macro_scope will be added by flip on output)
    /// - Special forms/keywords: also get empty scopes (macro_scope added by flip)
    ///
    /// The actual hygiene discrimination happens via flip-scope:
    /// 1. Before expansion: flip macro_scope on INPUT (adds to use-site identifiers)
    /// 2. Template symbols get their definition_scopes here
    /// 3. After expansion: flip macro_scope on OUTPUT
    ///    - Use-site (from pattern vars): macro_scope removed (was added, then flipped off)
    ///    - Introduced (from template): macro_scope added (wasn't there, then flipped on)
    ///
    /// NOTE: Special forms like `let`, `if` are NOT special-cased anymore.
    /// They are treated like introduced identifiers and will get the macro_scope.
    /// This allows the desugarer to distinguish macro-introduced keywords from
    /// user variables with the same name. For example, in:
    ///   (let ((let odd?)) (my-or (let 8)))
    /// The macro's `let` (from template) will have macro_scope, while the user's
    /// `(let 8)` (from input) will NOT have macro_scope, so they can be distinguished.
    fn rename_identifier(&self, id: &Identifier) -> Value {
        let name = id.name();

        // Get scopes for the identifier
        // NOTE: Special forms are NOT special-cased here - they get scopes like any other identifier
        let scopes = if let Some(def_scopes) = id.definition_scopes() {
            // FREE VARIABLE - use definition-time scopes
            if patina_runtime::macro_debug::is_enabled() {
                println!(
                    "[SCOPE-SETS] Free variable '{}' with scopes {}",
                    name, def_scopes
                );
            }
            def_scopes.clone()
        } else {
            // INTRODUCED IDENTIFIER (including keywords like `let`, `if`)
            // Empty scopes for now - the macro_scope will be added when we flip on the output
            if patina_runtime::macro_debug::is_enabled() {
                println!(
                    "[SCOPE-SETS] Introduced '{}' (will get macro scope {} on output flip)",
                    name, self.macro_scope
                );
            }
            patina_runtime::ScopeSet::new()
        };

        // Return Identifier with scopes (Racket-style hygiene)
        Value::Identifier(Box::new(patina_runtime::IdentifierData {
            name: name.clone(),
            scopes,
        }))
    }

    /// Mark a substituted value from a pattern variable with the macro scope.
    ///
    /// This is crucial for nested macro hygiene. When a macro generates another
    /// `define-syntax`, symbols substituted from pattern variables need to be
    /// distinguishable from fresh pattern variables in the inner macro.
    ///
    /// IMPORTANT: We do NOT recurse into `syntax-rules` or `define-syntax` forms.
    /// These forms define their own macro context and their identifiers should
    /// not be marked with the current macro scope. They will be compiled later
    /// when the define-syntax is processed, with their own hygiene context.
    fn mark_substituted_value(&self, value: Value) -> Value {
        use patina_runtime::Value;

        // Check if this is a syntax-rules or define-syntax form - don't mark inside
        if self.is_macro_definition(&value) {
            return value;
        }

        match value {
            // Convert Symbol to Identifier with macro_scope
            // This marks it as "came from outer macro expansion"
            Value::Symbol(name) => {
                let mut scopes = patina_runtime::ScopeSet::new();
                scopes = scopes.with_scope(self.macro_scope);
                Value::Identifier(Box::new(patina_runtime::IdentifierData { name, scopes }))
            }

            // Identifiers already have scopes - add macro_scope to them
            Value::Identifier(id) => {
                let new_scopes = id.scopes.with_scope(self.macro_scope);
                Value::Identifier(Box::new(patina_runtime::IdentifierData {
                    name: id.name.clone(),
                    scopes: new_scopes,
                }))
            }

            // Recursively mark pairs (but is_macro_definition check above handles special forms)
            Value::Pair(pair) => {
                let borrowed = pair.borrow();
                let new_car = self.mark_substituted_value(borrowed.0.clone());
                let new_cdr = self.mark_substituted_value(borrowed.1.clone());
                Value::Pair(Rc::new(RefCell::new((new_car, new_cdr))))
            }

            // Recursively mark vectors
            Value::Vector(vec) => {
                let new_elements: Vec<_> = vec
                    .borrow()
                    .iter()
                    .map(|elem| self.mark_substituted_value(elem.clone()))
                    .collect();
                Value::Vector(Rc::new(RefCell::new(new_elements)))
            }

            // Other values pass through unchanged
            _ => value,
        }
    }

    /// Check if a value is a form whose contents should not be marked.
    /// This includes:
    /// - Macro definitions (syntax-rules, define-syntax, etc.) - they have their own context
    /// - Quote forms - quoted data should remain as-is without scope marking
    fn is_macro_definition(&self, value: &Value) -> bool {
        is_macro_definition_form(value)
    }

    /// Check if a name is bound to a macro in the expansion environment
    #[allow(dead_code)]
    fn is_macro(&self, name: &Rc<str>) -> bool {
        matches!(self.expansion_env.get(name), Some(Value::Macro(_)))
    }

    /// Convert a Scheme list to a Vec
    fn list_to_vec(&self, value: &Value) -> Result<Vec<Value>, ExpandError> {
        utils_list_to_vec(value).map_err(|msg| ExpandError::InvalidTemplate { message: msg })
    }

    /// Convert a Vec to a Scheme list
    fn vec_to_list(&self, values: Vec<Value>) -> Value {
        utils_vec_to_list(values)
    }
}

impl Default for Expander {
    fn default() -> Self {
        // Create an empty runtime environment for tests
        use patina_runtime::Environment;
        Self::new(
            std::rc::Rc::new(Environment::new()),
            patina_runtime::ScopeId::fresh(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macro_expander::template::Identifier;
    use crate::macro_expander::utils::vec_to_list;

    // Use vec_to_list from utils as make_list alias for test readability
    fn make_list(values: Vec<Value>) -> Value {
        vec_to_list(values)
    }

    #[test]
    fn test_expand_literal() {
        let expander = Expander::default();
        let template = Template::Literal(Value::Integer(42));
        let env = MatchEnv::new(0);

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());
        assert_eq!(
            format!("{:?}", result.unwrap()),
            format!("{:?}", Value::Integer(42))
        );
    }

    #[test]
    fn test_expand_symbol() {
        let expander = Expander::default();
        let template = Template::Symbol(Identifier::new("if"));
        let env = MatchEnv::new(0);

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());
        // After hygiene implementation, symbols become Identifiers with scope sets
        if let Value::Identifier(id) = result.unwrap() {
            assert_eq!(&*id.name, "if");
        } else {
            panic!("Expected Identifier (hygiene-wrapped symbol)");
        }
    }

    #[test]
    fn test_expand_var() {
        let expander = Expander::default();
        let pvref = PVRef::new(0, 0);
        let template = Template::Var(pvref);

        let mut env = MatchEnv::new(1);
        env.insert(pvref, Value::Integer(42));

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());
        assert_eq!(
            format!("{:?}", result.unwrap()),
            format!("{:?}", Value::Integer(42))
        );
    }

    #[test]
    fn test_expand_simple_list() {
        let expander = Expander::default();
        let x = PVRef::new(0, 0);
        let y = PVRef::new(0, 1);

        // Template: (if x y)
        let template = Template::List(vec![
            Template::Symbol(Identifier::new("if")),
            Template::Var(x),
            Template::Var(y),
        ]);

        let mut env = MatchEnv::new(2);
        env.insert(x, Value::Integer(1));
        env.insert(y, Value::Integer(2));

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());

        // Should produce (if 1 2) where 'if' is an Identifier (for hygiene)
        let expected = make_list(vec![
            Value::identifier("if"),
            Value::Integer(1),
            Value::Integer(2),
        ]);
        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }

    #[test]
    fn test_expand_ellipsis_simple() {
        let expander = Expander::default();
        let x = PVRef::new(1, 0);

        // Template: (x ...)
        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::Var(x)),
            level: 1,
            nesting: 1,
            vars: vec![x],
        }]);

        let mut env = MatchEnv::new(1);
        // x has 3 values: 1, 2, 3
        env.insert_branch(
            x,
            vec![
                MatchValue::Leaf(Value::Integer(1)),
                MatchValue::Leaf(Value::Integer(2)),
                MatchValue::Leaf(Value::Integer(3)),
            ],
        );

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());

        // Should produce (1 2 3)
        let expected = make_list(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }

    #[test]
    fn test_expand_ellipsis_with_constant() {
        let expander = Expander::default();
        let x = PVRef::new(1, 0);

        // Template: ((+ x 1) ...)
        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::List(vec![
                Template::Symbol(Identifier::new("+")),
                Template::Var(x),
                Template::Literal(Value::Integer(1)),
            ])),
            level: 1,
            nesting: 1,
            vars: vec![x],
        }]);

        let mut env = MatchEnv::new(1);
        // x has 2 values: 10, 20
        env.insert_branch(
            x,
            vec![
                MatchValue::Leaf(Value::Integer(10)),
                MatchValue::Leaf(Value::Integer(20)),
            ],
        );

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());

        // Should produce ((+ 10 1) (+ 20 1)) where + is an Identifier (hygiene)
        let elem1 = make_list(vec![
            Value::identifier("+"),
            Value::Integer(10),
            Value::Integer(1),
        ]);
        let elem2 = make_list(vec![
            Value::identifier("+"),
            Value::Integer(20),
            Value::Integer(1),
        ]);
        let expected = make_list(vec![elem1, elem2]);
        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }

    #[test]
    fn test_expand_ellipsis_with_following() {
        let expander = Expander::default();
        let x = PVRef::new(1, 0);
        let y = PVRef::new(0, 1);

        // Template: (begin x ... y)
        let template = Template::List(vec![
            Template::Symbol(Identifier::new("begin")),
            Template::Ellipsis {
                subtemplate: Box::new(Template::Var(x)),
                level: 1,
                nesting: 1,
                vars: vec![x],
            },
            Template::Var(y),
        ]);

        let mut env = MatchEnv::new(2);
        // x has 2 values: 1, 2
        env.insert_branch(
            x,
            vec![
                MatchValue::Leaf(Value::Integer(1)),
                MatchValue::Leaf(Value::Integer(2)),
            ],
        );
        // y is a single value: 99
        env.insert(y, Value::Integer(99));

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());

        // Should produce (begin 1 2 99) where 'begin' is an Identifier (for hygiene)
        let expected = make_list(vec![
            Value::identifier("begin"),
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(99),
        ]);
        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }

    #[test]
    fn test_expand_double_ellipsis() {
        // This tests the do macro use case with BOTH bindings having steps:
        // Pattern: ((var init step ...) ...)
        // Template: (loop step ... ...)
        // Input: ((i 0 (+ i 1)) (j 10 (- j 1)))
        // Expected: (loop (+ i 1) (- j 1))

        use patina_runtime::Environment;
        let macro_scope = patina_runtime::ScopeId::fresh();
        let expander = Expander::new(std::rc::Rc::new(Environment::new()), macro_scope);
        let step = PVRef::new(2, 0); // level 2 because it's in nested ellipsis

        // Template: (loop step ... ...)
        let template = Template::List(vec![
            Template::Symbol(Identifier::new("loop")),
            Template::Ellipsis {
                subtemplate: Box::new(Template::Var(step)),
                level: 1, // ellipsis level is 1 for double ellipsis with level-2 variables
                nesting: 2,
                vars: vec![step],
            },
        ]);

        let mut env = MatchEnv::new(1);

        // step is a doubly-nested structure:
        // Branch([
        //   Branch([Leaf((+ i 1))]),    // First binding has 1 step
        //   Branch([Leaf((- j 1))])     // Second binding has 1 step
        // ])
        // NOTE: Input values are symbols, but mark_substituted_value converts them to
        // identifiers with macro_scope when they flow through pattern variables
        let step_i = make_list(vec![
            Value::symbol("+"),
            Value::symbol("i"),
            Value::Integer(1),
        ]);
        let step_j = make_list(vec![
            Value::symbol("-"),
            Value::symbol("j"),
            Value::Integer(1),
        ]);

        env.insert_branch(
            step,
            vec![
                MatchValue::Branch(vec![MatchValue::Leaf(step_i.clone())]), // i binding: 1 step
                MatchValue::Branch(vec![MatchValue::Leaf(step_j.clone())]), // j binding: 1 step
            ],
        );

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());
        let result = result.unwrap();

        // Verify structure: (loop <step1> <step2>)
        // 'loop' is introduced by template
        // step_i and step_j come from input but get macro_scope added by mark_substituted_value
        let result_str = format!("{}", result);
        assert!(
            result_str.contains("loop"),
            "Result should contain 'loop': {}",
            result_str
        );
        assert!(
            result_str.contains("+") && result_str.contains("i"),
            "Result should contain (+ i 1): {}",
            result_str
        );
        assert!(
            result_str.contains("-") && result_str.contains("j"),
            "Result should contain (- j 1): {}",
            result_str
        );
    }

    #[test]
    fn test_expand_double_ellipsis_empty_inner() {
        // Test case where inner ellipsis has 0 elements for some iterations
        // Pattern: ((a b ...) ...)
        // Template: (result b ... ...)
        // Input: ((x 1 2) (y))
        // Expected: (result 1 2) - empty inner branch contributes nothing

        let expander = Expander::default();
        let b = PVRef::new(2, 0);

        let template = Template::List(vec![
            Template::Symbol(Identifier::new("result")),
            Template::Ellipsis {
                subtemplate: Box::new(Template::Var(b)),
                level: 1, // ellipsis level is 1 for double ellipsis with level-2 variables
                nesting: 2,
                vars: vec![b],
            },
        ]);

        let mut env = MatchEnv::new(1);
        env.insert_branch(
            b,
            vec![
                MatchValue::Branch(vec![
                    MatchValue::Leaf(Value::Integer(1)),
                    MatchValue::Leaf(Value::Integer(2)),
                ]), // First group: 2 elements
                MatchValue::Branch(vec![]), // Second group: 0 elements
            ],
        );

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());

        // Should produce (result 1 2) - second group contributes nothing
        // 'result' is introduced by template (so becomes Identifier)
        let expected = make_list(vec![
            Value::identifier("result"),
            Value::Integer(1),
            Value::Integer(2),
        ]);

        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }

    // === Error Condition Tests ===

    #[test]
    fn test_error_undefined_variable() {
        // Template references a variable that's not in the environment
        let expander = Expander::default();
        let x = PVRef::new(0, 5); // Out of bounds!
        let template = Template::Var(x);

        let env = MatchEnv::new(1); // Only has space for 1 var (index 0)

        let result = expander.expand(&template, &env);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ExpandError::UndefinedVariable { .. }),
            "Expected UndefinedVariable error"
        );
    }

    #[test]
    fn test_error_inconsistent_repetition() {
        // Two variables in same ellipsis with different repetition counts
        let expander = Expander::default();
        let x = PVRef::new(1, 0);
        let y = PVRef::new(1, 1);

        // Template: ((x y) ...)
        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::List(vec![Template::Var(x), Template::Var(y)])),
            level: 1,
            nesting: 1,
            vars: vec![x, y],
        }]);

        let mut env = MatchEnv::new(2);
        // x has 3 values
        env.insert_branch(
            x,
            vec![
                MatchValue::Leaf(Value::Integer(1)),
                MatchValue::Leaf(Value::Integer(2)),
                MatchValue::Leaf(Value::Integer(3)),
            ],
        );
        // y has only 2 values - INCONSISTENT!
        env.insert_branch(
            y,
            vec![
                MatchValue::Leaf(Value::Integer(10)),
                MatchValue::Leaf(Value::Integer(20)),
            ],
        );

        let result = expander.expand(&template, &env);
        assert!(result.is_err());
        assert!(
            matches!(
                result.unwrap_err(),
                ExpandError::InconsistentRepetition { .. }
            ),
            "Expected InconsistentRepetition error"
        );
    }

    #[test]
    fn test_error_triple_ellipsis_not_supported() {
        // Triple ellipsis (nesting = 3) should be rejected
        let expander = Expander::default();
        let x = PVRef::new(3, 0);

        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::Var(x)),
            level: 1,
            nesting: 3, // Triple ellipsis!
            vars: vec![x],
        }]);

        let env = MatchEnv::new(1);

        let result = expander.expand(&template, &env);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ExpandError::InvalidTemplate { .. }),
            "Expected InvalidTemplate error for triple ellipsis"
        );
    }

    #[test]
    fn test_error_double_ellipsis_level_zero() {
        // Double ellipsis with level 0 is invalid
        let expander = Expander::default();
        let x = PVRef::new(1, 0);

        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::Var(x)),
            level: 0, // Level 0 with double ellipsis is invalid!
            nesting: 2,
            vars: vec![x],
        }]);

        let env = MatchEnv::new(1);

        let result = expander.expand(&template, &env);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ExpandError::InvalidTemplate { .. }),
            "Expected InvalidTemplate error for level-0 double ellipsis"
        );
    }

    #[test]
    fn test_error_ellipsis_expands_to_non_list() {
        // This tests that ellipsis expansion produces a list
        // Actually, this is hard to trigger with the current implementation
        // because ellipsis always produces lists. Keeping this as a placeholder
        // in case we add validation later.
    }

    #[test]
    fn test_error_level_mismatch_leaf_instead_of_branch() {
        // Try to expand an ellipsis when the variable is a Leaf instead of Branch
        let expander = Expander::default();
        let x = PVRef::new(1, 0);

        // Template: (x ...)
        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::Var(x)),
            level: 1,
            nesting: 1,
            vars: vec![x],
        }]);

        let mut env = MatchEnv::new(1);
        // Insert x as a Leaf instead of Branch - level mismatch!
        env.insert(x, Value::Integer(42));

        let result = expander.expand(&template, &env);
        assert!(result.is_err());
        // This will likely be UndefinedVariable or LevelMismatch
        assert!(
            matches!(
                result.unwrap_err(),
                ExpandError::LevelMismatch { .. } | ExpandError::UndefinedVariable { .. }
            ),
            "Expected LevelMismatch or UndefinedVariable error"
        );
    }

    #[test]
    fn test_error_undefined_in_nested_context() {
        // Variable undefined in nested ellipsis context (out of bounds)
        let expander = Expander::default();
        let x = PVRef::new(1, 5); // Out of bounds!

        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::Var(x)),
            level: 1,
            nesting: 1,
            vars: vec![x],
        }]);

        // Create env with limited space
        let env = MatchEnv::new(1); // Only has space for index 0

        let result = expander.expand(&template, &env);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ExpandError::UndefinedVariable { .. }),
            "Expected UndefinedVariable error"
        );
    }
}
