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
    /// `(bare name, renamed global)` for each macro-introduced top-level
    /// definition that was renamed. See `Renamed::global_aliases`.
    global_aliases: Vec<(Symbol, Symbol)>,
    /// Whether definitions seen right now become globals.
    ///
    /// True until a lambda body is entered, and false inside it — including
    /// inside any `begin` nested there, which is why this is a mode on the
    /// walk rather than an argument to one function: `begin` is reached
    /// through `rename_expr`, which would otherwise have to carry it too.
    at_top_level: bool,
}

impl RenameEnv {
    fn new() -> Self {
        Self {
            frames: vec![],
            counter: 0,
            global_aliases: Vec::new(),
            at_top_level: true,
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

    /// A name for a *local*, unique within this compilation unit.
    ///
    /// The counter is enough because a local never escapes the tree being
    /// renamed. A top-level definition does — it becomes a global — so it
    /// uses `scoped_global_name` instead.
    fn make_unique_name(&mut self, name: &Symbol) -> Symbol {
        let unique = format!("{}__#{}", name, self.counter);
        self.counter += 1;
        Rc::from(unique.as_str())
    }
}

/// A name for a macro-introduced *global*, unique across compilation units.
///
/// `alpha_rename` runs once per top-level form and its counter starts at zero
/// each time, so a counter-derived name is only unique within one form. A
/// global outlives the form, so two forms expanding the same macro both
/// minted `tmp__#0` and the second silently took the first's binding.
///
/// The scope set is the identity that makes these two definitions different
/// in the first place, and `ScopeId`s come from a process-global counter
/// (`ScopeId::fresh`), so naming the binding after it is unique by
/// construction rather than by arithmetic that has to be got right.
fn scoped_global_name(name: &Symbol, scopes: &ScopeSet) -> Symbol {
    use std::fmt::Write as _;
    let mut unique = format!("{name}__#");
    for (i, scope) in scopes.iter().enumerate() {
        if i > 0 {
            unique.push('.');
        }
        let _ = write!(unique, "{}", scope.0);
    }
    Rc::from(unique.as_str())
}

/// The output of alpha-renaming: the tree, and the environment aliases the
/// caller must install for it.
pub struct Renamed {
    pub expr: CoreExpr,
    /// `(bare name, renamed global)` pairs.
    ///
    /// A macro-introduced top-level definition is renamed, which makes it a
    /// global under a name no source code mentions. The definition-environment
    /// relinking still resolves its target by the *bare* name, so something
    /// has to answer for it — but a plain definition of that name would
    /// overwrite a user's own global of the same spelling, and would freeze a
    /// copy of the value besides.
    ///
    /// `Environment::define_alias` is neither: it is consulted only after real
    /// bindings, so a user's binding wins, and it forwards each access rather
    /// than copying, so a later `set!` is visible. Returned rather than
    /// installed here so this pass stays a pure transformation.
    pub global_aliases: Vec<(Symbol, Symbol)>,
}

/// Alpha-rename a CoreExpr tree for hygienic variable resolution.
pub fn alpha_rename(expr: &CoreExpr) -> Renamed {
    let mut env = RenameEnv::new();

    // Top-level definitions normally keep their names — they are globals, and
    // a later top-level form has to be able to find them. A *macro-introduced*
    // one is the exception: its scopes make it unreachable from anywhere but
    // the expansion that produced it, and that expansion is inside this same
    // form, so renaming it is both safe and necessary. Without a frame here,
    // a recursive macro's per-element temporaries all define the same global
    // and the last one wins.
    let bindings = collect_define_bindings(std::slice::from_ref(expr), &mut env, true);
    env.push_frame(bindings);
    let mut out = rename_body(std::slice::from_ref(expr), &mut env);
    env.pop_frame();

    // One expression in, one or two out: a lambda body's renamed definition
    // still gets its alias spliced, and the two then need a `Begin`.
    let expr = if out.len() == 1 {
        out.pop().expect("one element")
    } else {
        CoreExpr::new(CoreExprKind::Begin(out))
    };
    Renamed {
        expr,
        global_aliases: env.global_aliases,
    }
}

/// Bindings for the definitions `exprs` contributes, looking through `Begin`
/// at any depth.
///
/// The depth matters: `begin` splices, so a definition nested in one is a
/// definition of the enclosing body, and a macro that expands to several often
/// produces several levels of it — `(define-values (a b c) …)` expands to a
/// `begin` of `define`s, which a macro then wraps in a `begin` of its own. A
/// one-level walk found the outer group and missed every definition in it.
///
/// `Lambda` is deliberately not descended into: its body gets its own frame.
///
/// `only_scoped` selects the top-level rule — rename what a macro introduced
/// and leave source-written globals alone — from the lambda-body rule, where
/// every internal definition is a local and gets renamed.
///
/// A binding is visible to unscoped lookups only when it has no scopes of its
/// own, matching `build_bindings` for parameters: a macro-introduced name must
/// not answer a reference written in source.
fn collect_define_bindings(
    exprs: &[CoreExpr],
    env: &mut RenameEnv,
    only_scoped: bool,
) -> Vec<Binding> {
    let mut bindings = Vec::new();
    collect_define_bindings_into(exprs, only_scoped, &mut bindings);

    // Rename only what has to be renamed.
    //
    // A top-level definition becomes a *global*, so renaming one costs
    // something the lambda-body case does not pay: the original name has to be
    // aliased so the definition-environment relinking can still reach it, and
    // an alias is a second cell that a `set!` through the renamed name leaves
    // stale. Renaming is what disambiguates two expansions of one template, so
    // it is needed exactly when a name arrives more than once — and for the
    // single-definition case, which is every `jabberwocky`-shaped macro, not
    // renaming keeps one cell and the mutation stays visible.
    //
    // Lambda-body definitions are locals: they are renamed unconditionally,
    // and no alias is involved because nothing outside the body resolves them
    // by name.
    if only_scoped {
        // Top level: every macro-introduced definition is renamed, after its
        // own scopes rather than after a counter. An earlier version renamed
        // only a name that arrived twice, to keep the single-definition case
        // free of the alias below — but that made the *rule* depend on how
        // many times a macro happened to use a name, and it still collided
        // across forms, which is the failure the scope-derived name closes.
        for b in bindings.iter_mut() {
            b.unique_name = scoped_global_name(&b.name, &b.scopes);
        }
    } else {
        // Lambda body: locals, and a counter is unique enough for a name that
        // cannot leave this tree.
        for b in bindings.iter_mut() {
            b.unique_name = env.make_unique_name(&b.name);
        }
    }
    bindings
}

/// Rename a body or `Begin`, splicing a name-only alias after any definition
/// whose name was rewritten because it carried hygiene scopes.
///
/// The alias exists for the reason `Environment::define_scoped_definition`
/// documents: the definition-environment relinking resolves its target by
/// name, so a renamed-only binding is unreachable from a macro-generated
/// macro's template.
///
/// A top-level definition records an environment alias for the caller to
/// install (`Renamed::global_aliases`) rather than defining the bare name,
/// which would clobber a user's global and freeze a copy of the value. A
/// lambda-body definition is a local, so the bare name is simply bound
/// alongside — spliced flat rather than wrapped in a nested `Begin`, because
/// the passes downstream scan a `Begin`'s immediate children for
/// definitions.
fn rename_body(exprs: &[CoreExpr], env: &mut RenameEnv) -> Vec<CoreExpr> {
    let mut out = Vec::with_capacity(exprs.len());
    for expr in exprs {
        let renamed = rename_expr(expr, env);
        // Read the new name off the renamed node rather than resolving a
        // second time: the two answers have to agree, and this way they are
        // the same answer.
        let rename = match (&expr.kind, &renamed.kind) {
            (
                CoreExprKind::Define { name, scopes, .. },
                CoreExprKind::Define { name: new_name, .. },
            ) if !scopes.is_empty() && new_name != name => Some((name.clone(), new_name.clone())),
            _ => None,
        };
        if let Some((name, new_name)) = rename {
            if env.at_top_level {
                // A global: the bare name is answered by an environment alias
                // the caller installs, so a user's own global of that name
                // still wins and a `set!` is not frozen into a copy.
                env.global_aliases.push((name, new_name));
            } else {
                // A local: nothing outside this body resolves it, so the bare
                // name can simply be bound here too.
                out.push(renamed);
                out.push(CoreExpr::new(CoreExprKind::Define {
                    name,
                    scopes: ScopeSet::new(),
                    value: CoreExpr::rc(CoreExprKind::Var {
                        name: new_name,
                        scopes: ScopeSet::new(),
                    }),
                }));
                continue;
            }
        }
        out.push(renamed);
    }
    out
}

fn collect_define_bindings_into(
    exprs: &[CoreExpr],
    only_scoped: bool,
    bindings: &mut Vec<Binding>,
) {
    crate::compiler::for_each_define(exprs, &mut |name, scopes| {
        if only_scoped && scopes.is_empty() {
            return;
        }
        bindings.push(Binding {
            name: name.clone(),
            scopes: scopes.clone(),
            is_simple: scopes.is_empty(),
            // Filled in by the caller, which is where the two renaming rules
            // live.
            unique_name: name.clone(),
        });
    });
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

            // Add bindings for the body's internal defines. Each keeps the
            // scopes of the identifier it defines: they were forced to
            // `ScopeSet::new()` here, which made every expansion of one macro
            // template collapse onto a single local.
            bindings.extend(collect_define_bindings(body, env, false));

            let renamed_params = build_renamed_formals(params, &bindings);

            env.push_frame(bindings);
            let outer_top_level = std::mem::replace(&mut env.at_top_level, false);
            let renamed_body: Vec<CoreExpr> = rename_body(body, env);
            env.at_top_level = outer_top_level;
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

        CoreExprKind::Begin(exprs) => CoreExprKind::Begin(rename_body(exprs, env)),

        CoreExprKind::Define {
            name,
            scopes,
            value,
        } => {
            let renamed_name = env.resolve(name, scopes).unwrap_or_else(|| name.clone());
            CoreExprKind::Define {
                name: renamed_name,
                // Consumed: the pass exists so everything downstream can
                // resolve by name alone, as it does for `Var` above.
                scopes: ScopeSet::new(),
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
        let renamed = alpha_rename(&expr).expr;
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

        let renamed = alpha_rename(&app).expr;

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

        let renamed = alpha_rename(&outer_lambda).expr;

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
