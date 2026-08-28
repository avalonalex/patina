//! Pattern and template compiler for PVREF-based macro system
//!
//! This module compiles Scheme syntax-rules patterns and templates into
//! efficient PVREF (Pattern Variable Reference) based representations.
//!
//! Inspired by Gauche Scheme's pattern compilation (macro.c:400-683)
//! by Shiro Kawai.
//!
//! Key concepts:
//! - Two-phase design: compile pattern once, match many times
//! - PVREF encoding for O(1) variable lookup
//! - Precomputed metadata (num_following, vars) for optimization
//!
//! Reference: https://github.com/shirok/Gauche/blob/master/src/macro.c
//!
//! # Module Organization
//!
//! - `mod.rs` - Compiler struct, constructors, and `compile_macro()`
//! - `pattern.rs` - Pattern compilation logic
//! - `template.rs` - Template compilation logic
//! - `escape.rs` - Ellipsis escape handling
//! - `helpers.rs` - Utility methods
//! - `tests.rs` - Unit tests

mod escape;
mod helpers;
mod pattern;
mod template;
#[cfg(test)]
mod tests;

use super::IdentifierKey;
use super::utils::ELLIPSIS;
use crate::error::MacroError;
use patina_core::{SharedHeap, TaggedValue};
use patina_runtime::{Environment, LiteralBinding, PVRef, Pattern, ScopeSet, Template};
use std::collections::HashMap;
use std::rc::Rc;

// Re-export CompiledMacro and CompiledRule from patina-core (via patina-runtime)
pub use patina_runtime::{CompiledMacro, CompiledRule};

/// Pattern and template compiler
///
/// Compiles Scheme S-expressions into PVREF-based Pattern2/Template2.
///
/// Based on Gauche's compile_rules (macro.c:604-683).
pub struct Compiler {
    /// Literal identifiers with their binding information
    ///
    /// Each literal captures whether it was bound at macro definition time.
    /// This enables correct `bound-identifier=?` semantics during matching.
    pub(super) literals: Vec<LiteralBinding>,

    /// Literals exactly as written, for identity-based membership tests.
    ///
    /// R7RS 4.3.2 decides literal membership by identifier identity, so a
    /// pattern identifier is a literal only when it equals one of these in
    /// both name and scopes. See [`IdentifierKey`].
    pub(super) literal_keys: Vec<IdentifierKey>,

    /// Symbol used for ellipsis (usually "...")
    /// None means ellipsis is disabled (inside escape)
    pub(super) ellipsis: Option<Rc<str>>,

    /// Inside `(... template)`, the spelling the escape suspended — the one
    /// token in there that is not compiled as an ordinary template symbol.
    /// `None` outside an escape. Saved and restored by
    /// [`Compiler::compile_with_escaped_ellipsis`], like `ellipsis` itself.
    pub(super) escaped_ellipsis: Option<Rc<str>>,

    /// Whether that symbol was named by the macro (SRFI 46 / R7RS 4.3.2's
    /// `(syntax-rules <ellipsis> …)`) as something other than `...`.
    ///
    /// The two are identified differently and must not be conflated. A named
    /// ellipsis is a *declaration*: `:::` is the ellipsis inside this macro
    /// whatever `:::` may be bound to elsewhere. The default `...` is
    /// identified by its **binding** — R7RS 4.3.2 — so where `...` is bound
    /// as a variable it is an ordinary pattern variable instead. Looking the
    /// binding up for a declared ellipsis is one of the ways the backed-out
    /// attempt in #114 went wrong.
    pub(super) ellipsis_is_custom: bool,

    /// Lexical environment where the macro is being defined (for hygiene)
    ///
    /// Free variables in templates will capture this environment.
    /// This enables proper lexical scoping for macros following Gauche's approach.
    pub(super) env: Option<Rc<Environment>>,

    /// Scope set at macro definition time (for scope-based hygiene)
    ///
    /// Free variables will carry this scope set so they resolve to
    /// definition-time bindings, not use-site bindings.
    pub(super) definition_scopes: ScopeSet,

    // Per-rule compilation context
    /// Map from pattern variable [`IdentifierKey`] to PVREF.
    ///
    /// Keyed by identity rather than name alone so that an identifier
    /// substituted from an outer expansion and an identifier introduced by that
    /// expansion's template never collide when they are spelled alike.
    pub(super) pvars: HashMap<IdentifierKey, PVRef>,

    /// Counter for assigning PVREF indices
    pub(super) pvar_count: usize,

    /// Maximum ellipsis level seen so far
    pub(super) max_level: usize,

    /// How many quasiquotes the template being compiled is inside of; a
    /// `(quote datum)` is only special at zero (`compile_template`).
    pub(super) quasiquote_depth: u32,

    /// Shared heap for converting Value literals to TaggedValue at compile time
    pub(super) heap: SharedHeap,
}

/// Whether an ellipsis spelling was *declared* as something other than `...`.
///
/// Derived from the spelling rather than from `Some`/`None`, because a caller
/// that passes the default spelling explicitly means the default. Keying on
/// `is_some()` made `Compiler::new(_, Some("...".into()), _)` — which is how a
/// dozen of this module's own unit tests build a compiler — count as a
/// declaration, and so silently disabled the R7RS 4.3.2 binding rule in
/// exactly the tests meant to cover it.
fn is_declared_ellipsis(ellipsis: &Option<Rc<str>>) -> bool {
    ellipsis.as_deref().is_some_and(|e| e != ELLIPSIS)
}

impl Compiler {
    /// Resolve literal bindings at macro definition time
    ///
    /// For each literal name, check if it's bound in the environment or in
    /// `shadowed_names` (which contains lambda parameters that aren't bound yet
    /// but will be at runtime). If bound, capture the scopes of that binding.
    /// This enables correct `bound-identifier=?` semantics during pattern matching.
    fn resolve_literal_bindings(
        literal_keys: &[IdentifierKey],
        env: Option<&Rc<Environment>>,
        definition_scopes: &ScopeSet,
        shadowed_names: &std::collections::HashSet<Rc<str>>,
    ) -> Vec<LiteralBinding> {
        // Deliberately name-scoped: this resolves where a literal was *bound*
        // in the definition environment, which is a different question from the
        // identity test in `is_literal_form`. Two literals spelled alike always
        // resolve to the same binding, which is why the matcher's shadow lookup
        // (`is_literal_shadowed_tagged`) can find one by name.
        literal_keys
            .iter()
            .map(|key| {
                let name = &key.name;
                // Check if this literal is "bound" - either in the environment
                // OR in shadowed_names (e.g., lambda parameters not yet evaluated)
                let binding_scope = if shadowed_names.contains(name) {
                    // The literal is in shadowed_names - this means it's a lambda parameter
                    // that will be bound when the lambda is called. Treat it as bound
                    // with the current definition scopes.
                    // This is the key fix for "binding before macro definition" - the
                    // lambda parameter IS bound from the macro's perspective.
                    Some(definition_scopes.clone())
                } else if let Some(env) = env {
                    // Which *kind* of binding, not just whether there is one.
                    // `get_with_scopes` cannot say: it falls back to the plain
                    // bindings and returns a value either way, so asking it
                    // labelled every global binding as scoped.
                    if env.has_scoped_binding(name, definition_scopes) {
                        // The literal is bound with scopes - capture the definition scopes
                        Some(definition_scopes.clone())
                    } else if env.get(name).is_some() {
                        // Bound in the plain, unscoped bindings — a global or a
                        // library binding. Its identity has nothing to do with
                        // where the macro happened to be *defined*, so record
                        // no scopes rather than the definition's.
                        //
                        // Claiming `definition_scopes` here made any macro
                        // defined inside a scope unable to match a global
                        // literal written at the use site, because the matcher
                        // then compared a non-empty definition scope set
                        // against an unscoped input and called them different
                        // bindings:
                        //
                        //   (let ()
                        //     (define-syntax m
                        //       (syntax-rules (car) ((_ car) 'matched)
                        //                           ((_ x) 'not-matched)))
                        //     (m car))
                        //   ;; chibi, Gauche => matched; was not-matched
                        //
                        // Pre-existing and unrelated to what the literal is;
                        // `car` above is an ordinary procedure. It surfaced
                        // when syntactic keywords became bindings, because
                        // `(syntax-rules ::: (...))` then had a literal that
                        // resolved for the first time.
                        Some(ScopeSet::new())
                    } else {
                        // Not bound - literal is free at definition time
                        None
                    }
                } else {
                    // No environment - treat as unbound
                    None
                };

                LiteralBinding {
                    name: name.clone(),
                    binding_scope,
                }
            })
            .collect()
    }

    /// Create a new compiler
    ///
    /// # Arguments
    /// - `literals`: List of literal identifier names
    /// - `ellipsis`: Symbol to use for ellipsis (typically "...")
    pub fn new(literals: Vec<IdentifierKey>, ellipsis: Option<Rc<str>>, heap: SharedHeap) -> Self {
        let empty_shadowed = std::collections::HashSet::new();
        let literal_bindings =
            Self::resolve_literal_bindings(&literals, None, &ScopeSet::new(), &empty_shadowed);
        Self {
            literals: literal_bindings,
            literal_keys: literals,
            ellipsis_is_custom: is_declared_ellipsis(&ellipsis),
            ellipsis: ellipsis.or_else(|| Some(ELLIPSIS.into())),
            escaped_ellipsis: None,
            env: None,
            definition_scopes: ScopeSet::new(),
            pvars: HashMap::new(),
            pvar_count: 0,
            max_level: 0,
            quasiquote_depth: 0,
            heap,
        }
    }

    /// Create a new compiler with environment capture (for hygiene)
    ///
    /// # Arguments
    /// - `literals`: List of literal identifier names
    /// - `ellipsis`: Symbol to use for ellipsis (typically "...")
    /// - `env`: Lexical environment where the macro is being defined
    pub fn with_env(
        literals: Vec<IdentifierKey>,
        ellipsis: Option<Rc<str>>,
        env: Rc<Environment>,
        heap: SharedHeap,
    ) -> Self {
        let definition_scopes = ScopeSet::new();
        let empty_shadowed = std::collections::HashSet::new();
        let literal_bindings = Self::resolve_literal_bindings(
            &literals,
            Some(&env),
            &definition_scopes,
            &empty_shadowed,
        );
        Self {
            literals: literal_bindings,
            literal_keys: literals,
            ellipsis_is_custom: is_declared_ellipsis(&ellipsis),
            ellipsis: ellipsis.or_else(|| Some(ELLIPSIS.into())),
            escaped_ellipsis: None,
            env: Some(env),
            definition_scopes,
            pvars: HashMap::new(),
            pvar_count: 0,
            max_level: 0,
            quasiquote_depth: 0,
            heap,
        }
    }

    /// Create a new compiler with environment and scope set (for scope-based hygiene)
    ///
    /// # Arguments
    /// - `literals`: List of literal identifier names
    /// - `ellipsis`: Symbol to use for ellipsis (typically "...")
    /// - `env`: Lexical environment where the macro is being defined
    /// - `scopes`: Scope set at macro definition time
    ///
    /// Free variables in templates will carry the scope set so they resolve to
    /// definition-time bindings, not use-site bindings.
    pub fn with_env_and_scopes(
        literals: Vec<IdentifierKey>,
        ellipsis: Option<Rc<str>>,
        env: Rc<Environment>,
        scopes: ScopeSet,
        heap: SharedHeap,
    ) -> Self {
        let empty_shadowed = std::collections::HashSet::new();
        let literal_bindings =
            Self::resolve_literal_bindings(&literals, Some(&env), &scopes, &empty_shadowed);
        Self {
            literals: literal_bindings,
            literal_keys: literals,
            ellipsis_is_custom: is_declared_ellipsis(&ellipsis),
            ellipsis: ellipsis.or_else(|| Some(ELLIPSIS.into())),
            escaped_ellipsis: None,
            env: Some(env),
            definition_scopes: scopes,
            pvars: HashMap::new(),
            pvar_count: 0,
            max_level: 0,
            quasiquote_depth: 0,
            heap,
        }
    }

    /// Create a new compiler with environment, scope set, and shadowed names
    ///
    /// This is the most complete constructor that captures all binding information
    /// for correct `bound-identifier=?` semantics.
    ///
    /// # Arguments
    /// - `literals`: List of literal identifier names
    /// - `ellipsis`: Symbol to use for ellipsis (typically "...")
    /// - `env`: Lexical environment where the macro is being defined
    /// - `scopes`: Scope set at macro definition time
    /// - `shadowed_names`: Names that are shadowed by local bindings (e.g., lambda parameters)
    ///
    /// The `shadowed_names` allows the compiler to treat lambda parameters as "bound"
    /// even though they're not yet in the environment. This is essential for correct
    /// literal matching when a literal refers to an enclosing lambda parameter.
    pub fn with_env_scopes_and_shadowed(
        literals: Vec<IdentifierKey>,
        ellipsis: Option<Rc<str>>,
        env: Rc<Environment>,
        scopes: ScopeSet,
        shadowed_names: &std::collections::HashSet<Rc<str>>,
        heap: SharedHeap,
    ) -> Self {
        let literal_bindings =
            Self::resolve_literal_bindings(&literals, Some(&env), &scopes, shadowed_names);
        Self {
            literals: literal_bindings,
            literal_keys: literals,
            ellipsis_is_custom: is_declared_ellipsis(&ellipsis),
            ellipsis: ellipsis.or_else(|| Some(ELLIPSIS.into())),
            escaped_ellipsis: None,
            env: Some(env),
            definition_scopes: scopes,
            pvars: HashMap::new(),
            pvar_count: 0,
            max_level: 0,
            quasiquote_depth: 0,
            heap,
        }
    }

    /// Create a literal pattern from a TaggedValue
    pub(super) fn make_literal_pattern(&self, form: TaggedValue) -> Pattern {
        Pattern::Literal(form)
    }

    /// Create a literal template from a TaggedValue
    pub(super) fn make_literal_template(&self, form: TaggedValue) -> Template {
        Template::Literal(form)
    }

    /// Compile a complete macro definition
    ///
    /// # Arguments
    /// - `name`: Macro name
    /// - `rules`: List of (pattern, template) pairs as TaggedValues
    ///
    /// # Returns
    /// Compiled macro with all rules in PVREF form
    pub fn compile_macro(
        &mut self,
        name: Rc<str>,
        rules: Vec<(TaggedValue, TaggedValue)>,
    ) -> Result<CompiledMacro, MacroError> {
        let mut compiled_rules = Vec::new();
        let mut max_pvars = 0;

        for (pat_form, tmpl_form) in rules {
            // Reset per-rule context
            self.pvars.clear();
            self.pvar_count = 0;
            self.max_level = 0;

            // R7RS: The first element of each pattern is the macro keyword placeholder
            // and should be ignored (treated as wildcard). This is true even if the
            // symbol appears in the literals list (e.g., when _ is in literals).
            let pattern = self.compile_rule_pattern(pat_form, 0)?;
            let template = self.compile_template(tmpl_form, 0)?;

            // Build reverse mapping: PVREF -> name (for debug output)
            let pvar_names: HashMap<PVRef, Rc<str>> = self
                .pvars
                .iter()
                .map(|(key, pvref)| (*pvref, key.name.clone()))
                .collect();

            // Validate the rule before adding it. The macro name is NOT
            // embedded here: the desugarer funnel wraps every compile error
            // with it, and naming at both layers printed it twice.
            if let Err(e) = super::validator::validate_rule(&pattern, &template, &pvar_names) {
                return Err(MacroError::InvalidSyntax(format!(
                    "validation failed: {}",
                    e
                )));
            }

            compiled_rules.push(CompiledRule {
                pattern,
                template,
                num_pvars: self.pvar_count,
                max_level: self.max_level,
                pvar_names,
            });

            max_pvars = max_pvars.max(self.pvar_count);
        }

        let template_symbols = CompiledMacro::collect_template_symbols(&compiled_rules);
        Ok(CompiledMacro {
            name,
            literals: self.literals.clone(),
            template_symbols,
            rules: compiled_rules,
            max_pvars,
            definition_scopes: self.definition_scopes.clone(),
            heap: self.heap.clone(),
            // Carry the definition environment so a template's free identifiers
            // can be resolved where the macro was written rather than where it
            // is used. Previously `env` was consulted only as a yes/no predicate
            // and then dropped.
            definition_env: self.env.clone(),
        })
    }
}
