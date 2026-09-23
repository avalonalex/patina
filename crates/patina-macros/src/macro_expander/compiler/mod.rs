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
use patina_core::walk::OpenNodes;
use patina_core::{SharedHeap, TaggedValue};
use patina_runtime::{Environment, PVRef, Pattern, ScopeSet, Template};
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

    /// Identifiers an enclosing expansion put into the templates compiled so
    /// far, which become [`CompiledMacro::inherited_identifiers`]. Across all
    /// rules, unlike the per-rule context above.
    pub(super) inherited_identifiers: HashMap<Rc<str>, Vec<ScopeSet>>,

    /// Shared heap for converting Value literals to TaggedValue at compile time
    pub(super) heap: SharedHeap,

    /// The pairs and vectors of the pattern or template being compiled that
    /// the compiler is inside — so that one met again inside itself, which a
    /// datum label can write, is refused rather than compiled until the stack
    /// overflows (#459).
    pub(super) open: OpenNodes,
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
    /// Create a new compiler
    ///
    /// # Arguments
    /// - `literals`: List of literal identifier names
    /// - `ellipsis`: Symbol to use for ellipsis (typically "...")
    pub fn new(literals: Vec<IdentifierKey>, ellipsis: Option<Rc<str>>, heap: SharedHeap) -> Self {
        Self {
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
            inherited_identifiers: HashMap::new(),
            heap,
            open: OpenNodes::default(),
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
        Self {
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
            inherited_identifiers: HashMap::new(),
            heap,
            open: OpenNodes::default(),
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
        Self {
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
            inherited_identifiers: HashMap::new(),
            heap,
            open: OpenNodes::default(),
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
        // Cleared here as well as taken at the end: a rule that fails to
        // compile returns early, and what its templates had recorded by then
        // would otherwise become the next macro's.
        self.inherited_identifiers.clear();

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
            template_symbols,
            inherited_identifiers: std::mem::take(&mut self.inherited_identifiers),
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
