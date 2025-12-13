//! Ellipsis expansion for templates
//!
//! This module handles the complex ellipsis expansion logic, including
//! single ellipsis (`x ...`) and double ellipsis (`x ... ...`) from SRFI-149.

use super::Expander;
use super::error::ExpandError;
use crate::macro_expander::Template;
use patina_runtime::{MatchEnv, MatchValue, PVRef, Value};

/// Context for ellipsis expansion operations
///
/// This struct consolidates the parameters needed for ellipsis expansion,
/// reducing the number of arguments passed to helper functions.
pub(super) struct EllipsisContext<'a> {
    /// The template to expand repeatedly
    pub subtemplate: &'a Template,
    /// The match environment containing bound values
    pub env: &'a MatchEnv,
    /// Current indices into nested branches for outer ellipsis levels
    pub indices: &'a [usize],
    /// Pattern variables in the subtemplate that drive iteration
    pub vars: &'a [PVRef],
    /// The ellipsis nesting level (1 for single `...`, 2 for inner of `... ...`)
    pub level: u8,
    /// Whether we're inside a (quote ...) form
    pub inside_quote: bool,
}

impl Expander {
    /// Expand an ellipsis template
    ///
    /// This is the most complex case, handling iteration over pattern variable bindings.
    /// Based on Gauche's ellipsis expansion (macro.c:850+).
    ///
    /// # Parameters
    ///
    /// - `ctx`: Context containing subtemplate, environment, indices, vars, level, and quote flag
    /// - `nesting`: How many consecutive ellipses (1 for `x ...`, 2 for `x ... ...`)
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
    pub(super) fn expand_ellipsis(
        &self,
        ctx: &EllipsisContext<'_>,
        nesting: u8,
    ) -> Result<Value, ExpandError> {
        if nesting == 1 {
            // Single ellipsis - standard iteration
            self.expand_single_ellipsis(ctx)
        } else if nesting == 2 {
            // Double ellipsis (SRFI-149)
            // Template: x ... ...
            // This expands x at each level, then flattens one level
            self.expand_double_ellipsis(ctx)
        } else {
            // Triple+ ellipsis not supported
            Err(ExpandError::InvalidTemplate {
                message: format!("Ellipsis nesting level {} not supported (max 2)", nesting),
            })
        }
    }

    /// Expand a single ellipsis template (nesting = 1)
    fn expand_single_ellipsis(&self, ctx: &EllipsisContext<'_>) -> Result<Value, ExpandError> {
        // Determine iteration count from the first variable
        // Note: For nested ellipsis patterns like ((item ...) ...), the variable
        // may be at a deeper level than this ellipsis. We use get_iteration_count_at_level
        // to get the count at THIS ellipsis level, not the variable's native level.
        let iteration_count = if let Some(&first_var) = ctx.vars.first() {
            self.get_iteration_count_at_level(ctx.env, first_var, ctx.indices, ctx.level as usize)?
        } else {
            // No variables - ellipsis matches empty
            0
        };

        // Validate that all variables have the same iteration count at this level
        for &var in ctx.vars.iter().skip(1) {
            let count =
                self.get_iteration_count_at_level(ctx.env, var, ctx.indices, ctx.level as usize)?;
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
            let mut new_indices = ctx.indices.to_vec();
            // Ensure indices is large enough
            while new_indices.len() <= ctx.level as usize {
                new_indices.push(0);
            }
            new_indices[ctx.level as usize] = i;

            let value =
                self.expand_impl(ctx.subtemplate, ctx.env, &new_indices, ctx.inside_quote)?;
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
    fn expand_double_ellipsis(&self, ctx: &EllipsisContext<'_>) -> Result<Value, ExpandError> {
        self.validate_double_ellipsis_level(ctx.level)?;

        let outer_count =
            self.get_outer_iteration_count(ctx.vars, ctx.env, ctx.indices, ctx.level)?;
        let max_var_level = self.get_max_var_level(ctx.vars);

        let all_results = self.expand_nested_iterations(ctx, outer_count, max_var_level)?;

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
    fn expand_nested_iterations(
        &self,
        ctx: &EllipsisContext<'_>,
        outer_count: usize,
        max_var_level: usize,
    ) -> Result<Vec<Value>, ExpandError> {
        let mut all_results = Vec::new();

        for outer_idx in 0..outer_count {
            let outer_indices =
                self.build_outer_indices(ctx.indices, ctx.level, outer_idx, max_var_level);
            let inner_results = self.expand_inner_iterations(ctx, &outer_indices)?;
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
        ctx: &EllipsisContext<'_>,
        outer_indices: &[usize],
    ) -> Result<Vec<Value>, ExpandError> {
        let inner_count = self.get_inner_iteration_count(ctx.vars, ctx.env, outer_indices)?;
        let var_level = self.get_inner_var_level(ctx.vars, ctx.level);
        let max_var_level = self.get_max_var_level(ctx.vars);

        let mut results = Vec::new();
        for inner_idx in 0..inner_count {
            let inner_indices =
                self.build_inner_indices(outer_indices, inner_idx, var_level, max_var_level);
            let value =
                self.expand_impl(ctx.subtemplate, ctx.env, &inner_indices, ctx.inside_quote)?;
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
    pub(super) fn get_iteration_count_at_level(
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
    pub(super) fn get_iteration_count(
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
}
