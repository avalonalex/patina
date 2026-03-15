//! Alpha-renaming pass for macro hygiene.
//!
//! Runs before the 5-pass compiler pipeline. Resolves variable references
//! using scope-set information (from macro expansion) and renames variables
//! so that each unique binding has a unique name. This allows the rest of
//! the compiler pipeline (which uses name-only resolution) to correctly
//! handle hygienic macros.
//!
//! Mirrors the tree-walker's dual-storage environment:
//! - Non-macro params → visible to both simple (`env.get`) and scoped lookups
//! - Macro params (with explicit scopes) → visible only to scoped lookups
//!
//! Resolution:
//! - `Var { scopes: {} }` → simple lookup: innermost non-macro binding
//! - `Var { scopes: {S...} }` → scoped lookup: most specific `binding.scopes ⊆ ref_scopes`
//!   with fallback to simple bindings

use patina_core::core_expr::{CoreExpr, CoreExprKind, Formals, ScopedParam, Symbol};
use patina_core::scope::{ScopeId, ScopeSet};
use std::rc::Rc;

/// A binding in the rename environment.
#[derive(Debug, Clone)]
struct Binding {
    /// Original name
    name: Symbol,
    /// Scope set of the binding site
    scopes: ScopeSet,
    /// Whether this binding is visible to simple (non-scoped) lookups.
    /// True for non-macro params, false for macro-introduced params.
    is_simple: bool,
    /// Unique renamed name
    unique_name: Symbol,
}

/// Rename environment: stack of binding frames.
struct RenameEnv {
    frames: Vec<Vec<Binding>>,
    counter: u32,
}

impl RenameEnv {
    fn new() -> Self {
        Self {
            frames: vec![],
            counter: 0,
        }
    }

    fn push_frame(&mut self, bindings: Vec<Binding>) {
        self.frames.push(bindings);
    }

    fn pop_frame(&mut self) {
        self.frames.pop();
    }

    /// Resolve a variable reference.
    ///
    /// - Empty ref_scopes → simple lookup: innermost `is_simple` binding
    /// - Non-empty ref_scopes → scoped lookup: most specific subset match,
    ///   falling back to simple bindings
    fn resolve(&self, name: &str, ref_scopes: &ScopeSet) -> Option<Symbol> {
        if ref_scopes.is_empty() {
            // Simple lookup: find innermost binding visible to simple lookups
            for frame in self.frames.iter().rev() {
                for binding in frame.iter().rev() {
                    if binding.name.as_ref() == name && binding.is_simple {
                        return Some(binding.unique_name.clone());
                    }
                }
            }
            return None;
        }

        // Scoped lookup: find most specific binding where binding.scopes ⊆ ref_scopes
        let mut best: Option<&Binding> = None;

        for frame in self.frames.iter().rev() {
            for binding in frame.iter().rev() {
                if binding.name.as_ref() != name {
                    continue;
                }

                if !binding.scopes.is_subset_of(ref_scopes) {
                    continue;
                }

                match &best {
                    None => best = Some(binding),
                    Some(current_best) => {
                        if binding.scopes.len() > current_best.scopes.len() {
                            best = Some(binding);
                        }
                    }
                }
            }
        }

        best.map(|b| b.unique_name.clone())
    }

    fn make_unique_name(&mut self, name: &Symbol) -> Symbol {
        let unique = format!("{}__#{}", name, self.counter);
        self.counter += 1;
        Rc::from(unique.as_str())
    }
}

/// Alpha-rename a CoreExpr tree for hygienic variable resolution.
pub fn alpha_rename(expr: &CoreExpr) -> CoreExpr {
    let mut env = RenameEnv::new();
    rename_expr(expr, &mut env)
}

fn rename_expr(expr: &CoreExpr, env: &mut RenameEnv) -> CoreExpr {
    let kind = match &expr.kind {
        CoreExprKind::Literal(v) => CoreExprKind::Literal(*v),
        CoreExprKind::Quote(v) => CoreExprKind::Quote(*v),
        CoreExprKind::Quasiquote(v) => CoreExprKind::Quasiquote(*v),

        CoreExprKind::Var { name, scopes } => {
            if let Some(unique_name) = env.resolve(name, scopes) {
                CoreExprKind::Var {
                    name: unique_name,
                    scopes: ScopeSet::new(),
                }
            } else {
                CoreExprKind::Var {
                    name: name.clone(),
                    scopes: ScopeSet::new(),
                }
            }
        }

        CoreExprKind::Set { var, scopes, value } => {
            let renamed_value = rename_expr(value, env);
            if let Some(unique_name) = env.resolve(var, scopes) {
                CoreExprKind::Set {
                    var: unique_name,
                    scopes: ScopeSet::new(),
                    value: Rc::new(renamed_value),
                }
            } else {
                CoreExprKind::Set {
                    var: var.clone(),
                    scopes: ScopeSet::new(),
                    value: Rc::new(renamed_value),
                }
            }
        }

        CoreExprKind::Lambda {
            params,
            body,
            binding_scope,
        } => {
            let mut bindings = build_bindings(params, *binding_scope, env);

            // Add bindings for internal defines (treated as simple, local bindings).
            for expr in body.iter() {
                match &expr.kind {
                    CoreExprKind::Define { name, .. } => {
                        let unique_name = env.make_unique_name(name);
                        bindings.push(Binding {
                            name: name.clone(),
                            scopes: ScopeSet::new(),
                            is_simple: true,
                            unique_name,
                        });
                    }
                    CoreExprKind::Begin(exprs) => {
                        for e in exprs {
                            if let CoreExprKind::Define { name, .. } = &e.kind {
                                let unique_name = env.make_unique_name(name);
                                bindings.push(Binding {
                                    name: name.clone(),
                                    scopes: ScopeSet::new(),
                                    is_simple: true,
                                    unique_name,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }

            let renamed_params = build_renamed_formals(params, &bindings);

            env.push_frame(bindings);
            let renamed_body: Vec<CoreExpr> = body.iter().map(|e| rename_expr(e, env)).collect();
            env.pop_frame();

            CoreExprKind::Lambda {
                params: renamed_params,
                body: renamed_body,
                binding_scope: *binding_scope,
            }
        }

        CoreExprKind::If { test, then, else_ } => CoreExprKind::If {
            test: Rc::new(rename_expr(test, env)),
            then: Rc::new(rename_expr(then, env)),
            else_: Rc::new(rename_expr(else_, env)),
        },

        CoreExprKind::Begin(exprs) => {
            CoreExprKind::Begin(exprs.iter().map(|e| rename_expr(e, env)).collect())
        }

        CoreExprKind::Define { name, value } => {
            let renamed_name = env
                .resolve(name, &ScopeSet::new())
                .unwrap_or_else(|| name.clone());
            CoreExprKind::Define {
                name: renamed_name,
                value: Rc::new(rename_expr(value, env)),
            }
        }

        CoreExprKind::App { func, args } => CoreExprKind::App {
            func: Rc::new(rename_expr(func, env)),
            args: args.iter().map(|a| rename_expr(a, env)).collect(),
        },

        CoreExprKind::Apply { func, args } => CoreExprKind::Apply {
            func: Rc::new(rename_expr(func, env)),
            args: args.iter().map(|a| rename_expr(a, env)).collect(),
        },

        CoreExprKind::Expand { expr: inner } => CoreExprKind::Expand {
            expr: Rc::new(rename_expr(inner, env)),
        },

        CoreExprKind::Import { .. } => expr.kind.clone(),
    };

    CoreExpr {
        kind,
        source: expr.source.clone(),
    }
}

/// Build bindings for lambda parameters.
///
/// Mirrors the tree-walker's parameter binding logic:
/// - Non-macro params (empty scopes) → `is_simple = true`, scopes from `binding_scope`
///   (visible to both simple and scoped lookups)
/// - Macro params (non-empty scopes) → `is_simple = false`, use param's own scopes
///   (visible only to scoped lookups)
fn build_bindings(
    formals: &Formals,
    binding_scope: Option<ScopeId>,
    env: &mut RenameEnv,
) -> Vec<Binding> {
    let params: Vec<&ScopedParam> = match formals {
        Formals::Fixed(ps) => ps.iter().collect(),
        Formals::Variadic(p) => vec![p],
        Formals::Mixed { fixed, rest } => {
            let mut ps: Vec<&ScopedParam> = fixed.iter().collect();
            ps.push(rest);
            ps
        }
    };

    params
        .into_iter()
        .map(|p| {
            let unique_name = env.make_unique_name(&p.name);

            if !p.scopes.is_empty() {
                // Macro-introduced param: only visible to scoped lookups
                Binding {
                    name: p.name.clone(),
                    scopes: p.scopes.clone(),
                    is_simple: false,
                    unique_name,
                }
            } else if let Some(scope) = binding_scope {
                // Non-macro param with binding_scope: visible to both
                Binding {
                    name: p.name.clone(),
                    scopes: ScopeSet::singleton(scope),
                    is_simple: true,
                    unique_name,
                }
            } else {
                // No scope info (rare): simple binding only
                Binding {
                    name: p.name.clone(),
                    scopes: ScopeSet::new(),
                    is_simple: true,
                    unique_name,
                }
            }
        })
        .collect()
}

/// Build renamed formals from the bindings.
fn build_renamed_formals(formals: &Formals, bindings: &[Binding]) -> Formals {
    match formals {
        Formals::Fixed(params) => Formals::Fixed(
            params
                .iter()
                .zip(bindings.iter())
                .map(|(_, b)| ScopedParam {
                    name: b.unique_name.clone(),
                    scopes: ScopeSet::new(),
                })
                .collect(),
        ),
        Formals::Variadic(_) => Formals::Variadic(ScopedParam {
            name: bindings[0].unique_name.clone(),
            scopes: ScopeSet::new(),
        }),
        Formals::Mixed { fixed, .. } => {
            let renamed_fixed: Vec<ScopedParam> = fixed
                .iter()
                .zip(bindings.iter())
                .map(|(_, b)| ScopedParam {
                    name: b.unique_name.clone(),
                    scopes: ScopeSet::new(),
                })
                .collect();
            let rest_binding = &bindings[fixed.len()];
            Formals::Mixed {
                fixed: renamed_fixed,
                rest: ScopedParam {
                    name: rest_binding.unique_name.clone(),
                    scopes: ScopeSet::new(),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_core::tagged_value::TaggedValue;

    fn var(name: &str) -> CoreExpr {
        CoreExpr::new(CoreExprKind::Var {
            name: Rc::from(name),
            scopes: ScopeSet::new(),
        })
    }

    fn var_scoped(name: &str, scopes: ScopeSet) -> CoreExpr {
        CoreExpr::new(CoreExprKind::Var {
            name: Rc::from(name),
            scopes,
        })
    }

    fn lambda_with_scope(
        params: Vec<(&str, ScopeSet)>,
        body: Vec<CoreExpr>,
        binding_scope: Option<ScopeId>,
    ) -> CoreExpr {
        CoreExpr::new(CoreExprKind::Lambda {
            params: Formals::Fixed(
                params
                    .into_iter()
                    .map(|(n, s)| ScopedParam {
                        name: Rc::from(n),
                        scopes: s,
                    })
                    .collect(),
            ),
            body,
            binding_scope,
        })
    }

    #[test]
    fn no_scopes_resolves_innermost() {
        let expr = lambda_with_scope(vec![("x", ScopeSet::new())], vec![var("x")], None);
        let renamed = alpha_rename(&expr);
        match &renamed.kind {
            CoreExprKind::Lambda { body, params, .. } => {
                let Formals::Fixed(ps) = params else {
                    panic!("expected Fixed formals")
                };
                let param_name = ps[0].name.as_ref();
                match &body[0].kind {
                    CoreExprKind::Var { name, .. } => assert_eq!(name.as_ref(), param_name),
                    _ => panic!("expected Var in body"),
                }
            }
            _ => panic!("expected Lambda"),
        }
    }

    #[test]
    fn hygiene_with_binding_scope() {
        // outer lambda: x, binding_scope=S1
        // inner lambda: x, binding_scope=S3
        // var ref: x with scopes {S1, S2}
        // → should resolve to outer x

        let s1 = ScopeId(100);
        let s2 = ScopeId(101);
        let s3 = ScopeId(102);

        let ref_scopes = ScopeSet::from_iter([s1, s2]);

        let inner_lambda = lambda_with_scope(
            vec![("x", ScopeSet::new())],
            vec![var_scoped("x", ref_scopes)],
            Some(s3),
        );

        let outer_lambda = lambda_with_scope(
            vec![("x", ScopeSet::new())],
            vec![CoreExpr::new(CoreExprKind::App {
                func: Rc::new(inner_lambda),
                args: vec![CoreExpr::new(CoreExprKind::Quote(TaggedValue::fixnum(2)))],
            })],
            Some(s1),
        );

        let app = CoreExpr::new(CoreExprKind::App {
            func: Rc::new(outer_lambda),
            args: vec![CoreExpr::new(CoreExprKind::Quote(TaggedValue::fixnum(1)))],
        });

        let renamed = alpha_rename(&app);

        fn find_param(expr: &CoreExpr, depth: usize) -> Option<String> {
            match &expr.kind {
                CoreExprKind::App { func, .. } => find_param(func, depth),
                CoreExprKind::Lambda { params, body, .. } => {
                    let Formals::Fixed(ps) = params else {
                        return None;
                    };
                    if depth == 0 {
                        Some(ps[0].name.to_string())
                    } else {
                        for b in body {
                            if let Some(name) = find_param(b, depth - 1) {
                                return Some(name);
                            }
                        }
                        None
                    }
                }
                _ => None,
            }
        }

        fn find_body_var(expr: &CoreExpr) -> Option<String> {
            match &expr.kind {
                CoreExprKind::App { func, .. } => find_body_var(func),
                CoreExprKind::Lambda { body, .. } => {
                    for b in body {
                        if let Some(name) = find_body_var(b) {
                            return Some(name);
                        }
                    }
                    None
                }
                CoreExprKind::Var { name, .. } => Some(name.to_string()),
                _ => None,
            }
        }

        let outer_param = find_param(&renamed, 0).unwrap();
        let inner_param = find_param(&renamed, 1).unwrap();
        let body_var = find_body_var(&renamed).unwrap();

        assert_ne!(outer_param, inner_param);
        assert_eq!(body_var, outer_param);
    }

    #[test]
    fn macro_param_not_visible_to_simple_lookup() {
        // Simulates my-or: macro introduces temp (with scopes), user has temp (no scopes)
        // User's temp ref (empty scopes) should find user's temp, not macro's

        let s1 = ScopeId(200); // user let's binding_scope
        let s2 = ScopeId(201); // macro scope
        let s3 = ScopeId(202); // macro let's binding_scope

        // User lambda: (lambda (temp) ...)  with binding_scope=S1
        // Inside it, macro lambda: (lambda (temp{S2,S3}) ...) with binding_scope=None
        // Reference: temp with empty scopes

        let macro_temp_scopes = ScopeSet::from_iter([s2, s3]);

        let inner_lambda = lambda_with_scope(
            vec![("temp", macro_temp_scopes)],
            vec![var("temp")], // user's temp ref (empty scopes)
            None,
        );

        let outer_lambda = lambda_with_scope(
            vec![("temp", ScopeSet::new())],
            vec![CoreExpr::new(CoreExprKind::App {
                func: Rc::new(inner_lambda),
                args: vec![var("temp")], // also user's temp
            })],
            Some(s1),
        );

        let renamed = alpha_rename(&outer_lambda);

        // The body var inside inner lambda should resolve to outer's temp
        fn find_inner_body_var(expr: &CoreExpr) -> Option<String> {
            match &expr.kind {
                CoreExprKind::Lambda { body, .. } => {
                    for b in body {
                        if let CoreExprKind::App { func, .. } = &b.kind {
                            return find_inner_body_var(func);
                        }
                    }
                    // If we're at innermost lambda, return body var
                    for b in body {
                        if let CoreExprKind::Var { name, .. } = &b.kind {
                            return Some(name.to_string());
                        }
                    }
                    None
                }
                _ => None,
            }
        }

        let outer_param = match &renamed.kind {
            CoreExprKind::Lambda { params, .. } => {
                let Formals::Fixed(ps) = params else {
                    panic!("expected Fixed formals")
                };
                ps[0].name.to_string()
            }
            _ => panic!("expected Lambda"),
        };

        let inner_var = find_inner_body_var(&renamed).unwrap();
        assert_eq!(
            inner_var, outer_param,
            "empty-scoped ref should resolve to non-macro binding, not macro-introduced one"
        );
    }
}
