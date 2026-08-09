// DesugarError is large, but boxing it would add complexity for minimal benefit
#![allow(clippy::result_large_err)]

//! Desugarer: Transform surface syntax (TaggedValue) to core IR (CoreExpr)
//!
//! This module converts Scheme's surface syntax into a minimal core IR.
//! It handles the **6 core forms** that cannot be expressed as macros:
//! - `quote`, `lambda`, `if`, `set!`, `define`, `begin`
//!
//! # Architecture
//!
//! ```text
//! Parser → TaggedValue → Macro Expander → TaggedValue → Desugarer → CoreExpr → Evaluator
//! ```
//!
//! # Design Decision: Core Forms Only
//!
//! **This desugarer intentionally handles ONLY core forms, not derived forms.**
//!
//! ## Why Not Desugar Derived Forms (let, cond, and, or, etc.)?
//!
//! Derived forms like `let`, `cond`, `and`, `or` are **already handled by macros**
//! in `lib/scheme/base-extras.scm`. The macro expander transforms them BEFORE
//! the desugarer runs:
//!
//! ```text
//! (let ((x 1)) x)
//!   → [Macro Expander] → ((lambda (x) x) 1)
//!   → [Desugarer] → CoreExpr::App { func: Lambda {...}, ... }
//! ```
//!
//! **The desugarer never sees "let"** - it's already been transformed by macros!
//!
//! Implementing `desugar_let`, `desugar_cond`, etc. would be:
//! - **Dead code** - Never called in normal operation (macros expand first)
//! - **Redundant** - Same logic as macro definitions
//! - **Misleading** - Suggests desugarer is "macro-independent" when it's not
//!
//! ## What If We Need Macro-Independent Desugaring Later?
//!
//! If we ever need to desugar derived forms without macro expansion
//! (e.g., for testing, bootstrapping, or alternative pipelines), we can
//! add them back. The implementations are straightforward:
//! - `let` → lambda application
//! - `cond` → nested `if`
//! - `and`/`or` → short-circuit `if`
//!
//! But until we have a concrete use case, we keep the desugarer simple
//! and focused on its actual job: translating core forms to IR.
//!
//! ## Core Forms vs Derived Forms
//!
//! | Form | Type | Handled By | Notes |
//! |------|------|------------|-------|
//! | `quote` | Core | Desugarer | Cannot be macro |
//! | `lambda` | Core | Desugarer | Cannot be macro |
//! | `if` | Core | Desugarer | Cannot be macro |
//! | `set!` | Core | Desugarer | Cannot be macro |
//! | `define` | Core | Desugarer | Cannot be macro |
//! | `begin` | Core | Desugarer | Cannot be macro |
//! | `let`, `let*`, `letrec` | Derived | Macros | Expand before desugarer |
//! | `cond`, `case`, `when`, `unless` | Derived | Macros | Expand before desugarer |
//! | `and`, `or` | Derived | Macros | Expand before desugarer |
//! | `do` | Derived | Macros | Expand before desugarer |
//!
//! ## See Also
//! - `lib/scheme/base-extras.scm` - Macro definitions for derived forms
//! - `PRD/phase1/CORE_IR_MIGRATION.md` - Full architecture design

mod error;
mod utils;

pub use error::{DesugarError, Result};

use crate::source_map::SourceMap;
use patina_core::error::SourceLocation;
use patina_core::{SharedHeap, TaggedValue};
use patina_ir::{CoreExpr, CoreExprKind};
use patina_runtime::{Environment, ScopeId, ScopeSet};
use std::cell::RefCell;
use std::rc::Rc;

/// Walk a freshly-expanded pair tree and stamp each unrecorded pair with the call-site source.
///
/// Pairs from the original user source are already recorded by the parser; this only
/// stamps new template-created pairs. Bounded by depth to avoid runaway recursion.
fn stamp_expansion_source(
    tv: TaggedValue,
    source: &SourceLocation,
    source_map: &Rc<RefCell<SourceMap>>,
    heap: &SharedHeap,
    depth: u32,
) {
    const MAX_DEPTH: u32 = 64;
    if depth > MAX_DEPTH || !tv.is_pair() {
        return;
    }

    {
        let mut sm = source_map.borrow_mut();
        if sm.get(tv).is_none() {
            sm.record(tv, source.clone());
        }
    }

    // Extract car/cdr without holding SourceMap borrow
    let pair = heap.borrow().try_pair(tv);
    if let Some((car, cdr)) = pair {
        stamp_expansion_source(car, source, source_map, heap, depth + 1);
        stamp_expansion_source(cdr, source, source_map, heap, depth + 1);
    }
}

/// Desugarer converts Value (surface syntax) to CoreExpr (core IR)
///
/// **Macro-Aware Design**: The desugarer can optionally take an environment
/// to enable macro expansion during desugaring. This allows the desugarer to:
/// 1. Check if a form is a macro
/// 2. Expand the macro
/// 3. Recursively desugar the expanded result
///
/// This approach means we don't need to pre-expand all macros before desugaring.
/// The desugarer handles macro expansion selectively, only when encountering
/// macro calls during the desugaring process.
///
/// **Scope-Based Hygiene**: The desugarer tracks a current scope set that
/// accumulates as we enter binding forms (lambda, let-syntax, etc.). This
/// enables scope-based hygiene lookup where identifiers carry scope information.
///
/// **Name Shadowing**: The desugarer also tracks which names are shadowed by
/// local bindings (lambda parameters). These names should not be treated as
/// macro calls, even if a macro with that name exists in the environment.
/// This handles cases like `(let ((let odd?)) (let 8))` where the inner `let`
/// should call the variable, not expand the macro.
/// Monotonic counter for the unique names given to definition-environment
/// aliases. Never reused, so an alias cannot collide with a user binding or
/// with an alias from another expansion.
fn next_alias_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

pub struct Desugarer {
    /// Optional environment for macro lookup
    /// If None, macros cannot be expanded (will cause desugar errors)
    env: Option<Rc<Environment>>,

    /// Current scope set for scope-based hygiene
    /// Accumulates scopes as we enter binding forms
    current_scopes: ScopeSet,

    /// Names that are shadowed by local bindings (lambda parameters)
    /// These should not be treated as macro calls
    shadowed_names: std::collections::HashSet<Rc<str>>,

    /// Optional source map for looking up source positions of parsed forms
    source_map: Option<Rc<RefCell<SourceMap>>>,

    /// Virtual filesystem for `include` / `include-ci` forms.
    /// Defaults to `NativeFs` when not explicitly set.
    fs: std::sync::Arc<dyn patina_core::FileSystem>,
}

impl Desugarer {
    /// Create a new desugarer without macro expansion support
    ///
    /// This is used when the input is already fully macro-expanded.
    pub fn new() -> Self {
        Self {
            env: None,
            current_scopes: ScopeSet::new(),
            shadowed_names: std::collections::HashSet::new(),
            source_map: None,
            fs: std::sync::Arc::new(patina_core::NativeFs),
        }
    }

    /// Create a new desugarer with macro expansion support
    ///
    /// This allows the desugarer to expand macros as it encounters them.
    /// The environment is used to look up macro definitions.
    ///
    /// The desugarer compiles `define-syntax` immediately during desugaring
    /// and installs macros in the environment, returning `CoreExpr::Literal(Unspecified)`.
    pub fn with_env(env: Rc<Environment>) -> Self {
        Self {
            env: Some(env),
            current_scopes: ScopeSet::new(),
            shadowed_names: std::collections::HashSet::new(),
            source_map: None,
            fs: std::sync::Arc::new(patina_core::NativeFs),
        }
    }

    /// Create a new desugarer with environment and source map
    ///
    /// The source map is used to look up source positions recorded by the parser.
    /// These positions are attached to the resulting CoreExpr nodes.
    pub fn with_env_and_source_map(
        env: Rc<Environment>,
        source_map: Rc<RefCell<SourceMap>>,
    ) -> Self {
        Self {
            env: Some(env),
            current_scopes: ScopeSet::new(),
            shadowed_names: std::collections::HashSet::new(),
            source_map: Some(source_map),
            fs: std::sync::Arc::new(patina_core::NativeFs),
        }
    }

    /// Set the virtual filesystem for `include` handling.
    pub fn with_fs(mut self, fs: std::sync::Arc<dyn patina_core::FileSystem>) -> Self {
        self.fs = fs;
        self
    }

    /// Create a new desugarer with environment and specific scope set
    ///
    /// Used when creating child desugarers that inherit scope context.
    pub fn with_env_and_scopes(env: Rc<Environment>, scopes: ScopeSet) -> Self {
        Self {
            env: Some(env),
            current_scopes: scopes,
            shadowed_names: std::collections::HashSet::new(),
            source_map: None,
            fs: std::sync::Arc::new(patina_core::NativeFs),
        }
    }

    /// Get the current scope set
    pub fn current_scopes(&self) -> &ScopeSet {
        &self.current_scopes
    }

    /// Create a child desugarer with an additional scope
    ///
    /// Used when entering a binding form (lambda, let-syntax, etc.)
    #[allow(dead_code)]
    fn with_fresh_scope(&self) -> (Self, ScopeId) {
        let scope = ScopeId::fresh();
        let new_scopes = self.current_scopes.with_scope(scope);
        let desugarer = Self {
            env: self.env.clone(),
            current_scopes: new_scopes,
            shadowed_names: self.shadowed_names.clone(),
            source_map: self.source_map.clone(),
            fs: self.fs.clone(),
        };
        (desugarer, scope)
    }

    /// Create a child desugarer with additional shadowed names
    ///
    /// Used when entering a lambda body where parameters shadow outer bindings.
    /// Names in `new_shadows` will not be treated as macro calls even if a
    /// macro with that name exists in the environment.
    fn with_shadowed_names(
        &self,
        new_shadows: impl IntoIterator<Item = Rc<str>>,
        new_scopes: ScopeSet,
    ) -> Self {
        let mut shadowed = self.shadowed_names.clone();
        shadowed.extend(new_shadows);
        Self {
            env: self.env.clone(),
            current_scopes: new_scopes,
            shadowed_names: shadowed,
            source_map: self.source_map.clone(),
            fs: self.fs.clone(),
        }
    }

    /// Check if a name is shadowed by a local binding
    fn is_shadowed(&self, name: &str) -> bool {
        self.shadowed_names.contains(name)
    }

    /// Create a child desugarer with a new environment (for let-syntax bodies)
    ///
    /// This inherits the current shadowed_names and uses the new environment and scopes.
    fn with_new_env(&self, env: Rc<Environment>, scopes: ScopeSet) -> Self {
        Self {
            env: Some(env),
            current_scopes: scopes,
            shadowed_names: self.shadowed_names.clone(),
            source_map: self.source_map.clone(),
            fs: self.fs.clone(),
        }
    }

    /// Rewrite free identifiers in a macro expansion that only the macro's
    /// definition environment can resolve.
    ///
    /// A `syntax-rules` template may name a helper that is private to the
    /// library defining the macro. The expansion carries only the bare name, so
    /// at the use site it compiles to a global load that is not there. For each
    /// such name this installs a uniquely-named alias in the use-site
    /// environment pointing back at the definition environment, and rewrites
    /// the reference to use it.
    ///
    /// Deliberately conservative: a name the use site *can* already resolve is
    /// left untouched. Preferring the definition binding in that case would be
    /// more faithful to R7RS, but it would also change the meaning of macros
    /// that resolve correctly today, so it is left for a separate change.
    fn link_definition_env_refs(
        &self,
        expanded: patina_core::TaggedValue,
        definition_env: Option<&std::rc::Rc<patina_runtime::Environment>>,
        template_symbols: &std::collections::HashSet<std::rc::Rc<str>>,
        shared_heap: &patina_core::SharedHeap,
    ) -> patina_core::TaggedValue {
        let (Some(def_env), Some(use_env)) = (definition_env, self.env.clone()) else {
            return expanded;
        };
        // Same environment: nothing can be missing.
        if def_env.env_id() == use_env.env_id() || template_symbols.is_empty() {
            return expanded;
        }
        self.link_refs_impl(expanded, def_env, &use_env, template_symbols, shared_heap)
    }

    fn link_refs_impl(
        &self,
        tv: patina_core::TaggedValue,
        def_env: &std::rc::Rc<patina_runtime::Environment>,
        use_env: &std::rc::Rc<patina_runtime::Environment>,
        template_symbols: &std::collections::HashSet<std::rc::Rc<str>>,
        shared_heap: &patina_core::SharedHeap,
    ) -> patina_core::TaggedValue {
        if tv.is_pair() {
            let (car, cdr) = shared_heap.borrow().get_pair(tv);
            let new_car = self.link_refs_impl(car, def_env, use_env, template_symbols, shared_heap);
            let new_cdr = self.link_refs_impl(cdr, def_env, use_env, template_symbols, shared_heap);
            if new_car == car && new_cdr == cdr {
                return tv;
            }
            return shared_heap.borrow_mut().alloc_pair(new_car, new_cdr);
        }

        let name = {
            let heap = shared_heap.borrow();
            heap.get_symbol_name(tv)
                .map(std::rc::Rc::from)
                .or_else(|| utils::get_identifier_info(tv, &heap).map(|(n, _)| n))
        };
        let Some(name) = name else { return tv };

        // Only names the template itself mentions are eligible: anything else
        // arrived by pattern-variable substitution and belongs to the caller.
        if !template_symbols.contains(&name) {
            return tv;
        }
        // Resolvable at the use site, or not known to the definition env: leave it.
        if use_env.get(&name).is_some() || def_env.get(&name).is_none() {
            return tv;
        }

        let alias = format!("{}.{}", name, next_alias_id());
        use_env.define_alias(alias.clone(), def_env.clone(), name);
        shared_heap.borrow_mut().intern_symbol(&alias)
    }
    /// Look up the source location for a TaggedValue in the source map
    fn lookup_source(&self, tv: TaggedValue) -> Option<SourceLocation> {
        self.source_map
            .as_ref()
            .and_then(|sm| sm.borrow().get(tv).cloned())
    }

    /// Desugar a TaggedValue (surface syntax) to CoreExpr (core IR)
    ///
    /// This is the primary entry point for desugaring.
    ///
    /// # Arguments
    /// * `tagged` - The TaggedValue to desugar (from parser output)
    /// * `shared_heap` - The shared heap containing the TaggedValue's data
    ///
    /// # Heap Borrow Management
    /// This method takes a SharedHeap (Rc<RefCell<Heap>>) instead of &Heap so it
    /// can manage borrows internally. This allows releasing the immutable borrow
    /// before operations that need mutable access (like macro expansion).
    pub fn desugar_tagged(
        &self,
        tagged: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        // Immediate values - no heap access needed
        if tagged.is_fixnum() {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }
        if tagged.is_char() {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }
        if tagged == TaggedValue::TRUE || tagged == TaggedValue::FALSE {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }
        if tagged == TaggedValue::NULL {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }
        if tagged == TaggedValue::UNSPECIFIED {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }
        if tagged == TaggedValue::EOF {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }

        // For heap-dependent operations, borrow the heap
        let heap = shared_heap.borrow();

        // Symbol - variable reference without scopes
        if let Some(name) = heap.get_symbol_name(tagged) {
            return Ok(CoreExpr::new(CoreExprKind::Var {
                name: Rc::from(name),
                scopes: ScopeSet::new(),
            }));
        }

        // Identifier - variable reference with scopes (for hygiene)
        if let Some((name, scopes)) = utils::get_identifier_info(tagged, &heap) {
            return Ok(CoreExpr::new(CoreExprKind::Var { name, scopes }));
        }

        // Native heap types - self-evaluating literals
        // String - native heap strings now support mutation directly
        if tagged.is_string() {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }
        if tagged.is_vector() {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }
        // Numeric types stored natively on heap
        if heap.is_complex(tagged) {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }
        if heap.is_real(tagged) {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }
        if heap.is_bigint(tagged) {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }
        if heap.is_rational(tagged) {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }
        if heap.is_bytevector(tagged) {
            return Ok(CoreExpr::new(CoreExprKind::Literal(tagged)));
        }

        // Pair - special form or application
        if tagged.is_pair() {
            // Drop the borrow before calling desugar_list_tagged
            // (it will manage its own borrows)
            drop(heap);
            let source = self.lookup_source(tagged);
            let mut expr = self.desugar_list_tagged(tagged, shared_heap)?;
            // Attach source location from the source map if available
            // and the desugared result doesn't already have one
            if expr.source.is_none() {
                expr.source = source;
            }
            return Ok(expr);
        }

        // All valid AST types should be handled above
        drop(heap);
        Err(DesugarError::InvalidSyntax(
            "Cannot desugar unknown tagged value type".to_string(),
        ))
    }

    /// Desugar a list (TaggedValue pair) - special form or application
    fn desugar_list_tagged(&self, list: TaggedValue, shared_heap: &SharedHeap) -> Result<CoreExpr> {
        // Step 1: Get car/cdr (try_pair takes &self)
        let (car, cdr) = {
            let heap = shared_heap.borrow();
            heap.try_pair(list).ok_or_else(|| {
                DesugarError::InvalidSyntax("Expected a pair in desugar_list_tagged".to_string())
            })?
        };
        // Borrow released

        // Step 2: Extract operator name (immutable access only)
        let (name, is_macro_introduced) = {
            let heap = shared_heap.borrow();
            if let Some(s) = heap.get_symbol_name(car) {
                (Some(Rc::from(s)), false)
            } else if let Some((id_name, id_scopes)) = utils::get_identifier_info(car, &heap) {
                (Some(id_name), !id_scopes.is_empty())
            } else {
                (None, false)
            }
        };
        // Immutable borrow released

        // Determine if this name is shadowed by a local binding
        let is_shadowed = name
            .as_ref()
            .map(|n| !is_macro_introduced && self.is_shadowed(n))
            .unwrap_or(false);

        // Step 3: Check for macro (no heap borrow - env.get may access heap internally)
        let macro_to_expand = if let (Some(env), Some(sym)) = (&self.env, &name) {
            if !is_shadowed {
                if let Some(tv) = env.get(sym) {
                    let heap_ref = env.heap().borrow();
                    heap_ref.get_macro(tv).cloned()
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Handle macro expansion
        // Uses expand_macro_with_shadowed_tagged which:
        // - Accepts TaggedValue input directly
        // - Returns TaggedValue output directly
        if let Some(compiled_macro) = macro_to_expand {
            // Save call-site source location before expansion
            let call_site_source = self.lookup_source(list);

            let expanded_tagged = patina_macros::expand_macro_with_shadowed_tagged(
                &compiled_macro,
                list, // Pass TaggedValue directly
                shared_heap,
                &self.shadowed_names,
                &self.current_scopes,
            )
            .map_err(|e| DesugarError::InvalidSyntax(format!("Macro expansion failed: {}", e)))?;

            // Referential transparency: a template's free identifiers denote what
            // they were bound to where the macro was *defined*. Link any that the
            // use site cannot resolve back to the definition environment before
            // desugaring, otherwise they become bare global loads here and fail.
            let expanded_tagged = self.link_definition_env_refs(
                expanded_tagged,
                compiled_macro.definition_env.as_ref(),
                &compiled_macro.template_symbols,
                shared_heap,
            );

            // Phase 4: stamp expanded pairs + record macro expansion chain
            if let (Some(src), Some(sm)) = (&call_site_source, &self.source_map) {
                stamp_expansion_source(expanded_tagged, src, sm, shared_heap, 0);
                sm.borrow_mut()
                    .record_expansion(src, compiled_macro.name.to_string());
            }

            // Result is already TaggedValue - continue desugaring
            let mut expr = self.desugar_tagged(expanded_tagged, shared_heap)?;
            // Use call-site source as fallback if the expanded form has no source
            if expr.source.is_none() {
                expr.source = call_site_source;
            }
            return Ok(expr);
        }

        // Check if it's a core special form
        let is_shadowed = name
            .as_ref()
            .map(|n| !is_macro_introduced && self.is_shadowed(n))
            .unwrap_or(false);

        if let Some(sym) = &name
            && !is_shadowed
        {
            match sym.as_ref() {
                "quote" => return self.desugar_quote_tagged(cdr, shared_heap),
                "quasiquote" => return self.desugar_quasiquote_tagged(cdr, shared_heap),
                "lambda" => return self.desugar_lambda_tagged(cdr, shared_heap),
                "if" => return self.desugar_if_tagged(cdr, shared_heap),
                "set!" => return self.desugar_set_tagged(cdr, shared_heap),
                "define" => return self.desugar_define_tagged(cdr, shared_heap),
                "define-syntax" => return self.desugar_define_syntax_tagged(cdr, shared_heap),
                "import" => return self.desugar_import_tagged(cdr, shared_heap),
                "begin" => return self.desugar_begin_tagged(cdr, shared_heap),
                "apply" => return self.desugar_apply_tagged(list, cdr, shared_heap),
                "expand" => return self.desugar_expand_tagged(cdr, shared_heap),
                "let-syntax" => return self.desugar_let_syntax_tagged(cdr, shared_heap),
                "letrec-syntax" => return self.desugar_letrec_syntax_tagged(cdr, shared_heap),
                "cond-expand" => return self.desugar_cond_expand_tagged(cdr, shared_heap),
                "syntax-error" => return self.desugar_syntax_error_tagged(cdr, shared_heap),
                "include" => return self.desugar_include_tagged(cdr, shared_heap, false),
                "include-ci" => return self.desugar_include_tagged(cdr, shared_heap, true),
                _ => {}
            }
        }

        // Regular application
        self.desugar_app_tagged(list, shared_heap)
    }

    /// Desugar lambda using TaggedValue
    fn desugar_lambda_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec_tagged(args, shared_heap)?;

        if args_vec.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "lambda requires formals and body".to_string(),
            ));
        }

        let formals_tv = args_vec[0];
        let body_tvs: Vec<_> = args_vec[1..].to_vec();

        if body_tvs.is_empty() {
            return Err(DesugarError::EmptyBody("lambda".to_string()));
        }

        let params = utils::convert_formals_tagged(formals_tv, shared_heap)?;

        // Check if any parameter has scopes from macro expansion
        let has_macro_scopes = match &params {
            patina_ir::Formals::Fixed(ps) => ps.iter().any(|p| !p.scopes.is_empty()),
            patina_ir::Formals::Variadic(p) => !p.scopes.is_empty(),
            patina_ir::Formals::Mixed { fixed, rest } => {
                fixed.iter().any(|p| !p.scopes.is_empty()) || !rest.scopes.is_empty()
            }
        };

        // Create a fresh scope for this lambda's bindings
        // This is used for:
        // 1. Non-macro parameters (they have no scopes, need fresh scope)
        // 2. let-syntax inside the lambda (to capture lexical context)
        let binding_scope = ScopeId::fresh();
        let body_scopes = self.current_scopes.with_scope(binding_scope);

        // Extract parameter names for shadowing
        let param_names = utils::formals_to_names(&params);

        // Desugar body with:
        // 1. The new scope set (for hygiene)
        // 2. Parameter names added to shadowed_names (so they don't trigger macro expansion)
        let body_desugarer = self.with_shadowed_names(param_names, body_scopes.clone());

        // Desugar body expressions with internal define-syntax handling
        let body =
            self.desugar_body_tagged(&body_desugarer, &body_tvs, shared_heap, &body_scopes)?;

        // If parameters have macro scopes, set binding_scope to None
        // since we'll use the parameter-specific scopes at runtime.
        // Otherwise, use the fresh scope for all parameters.
        let binding_scope = if has_macro_scopes {
            None // Parameters carry their own scopes
        } else {
            Some(binding_scope) // Use fresh scope for all
        };

        Ok(CoreExpr::new(CoreExprKind::Lambda {
            params,
            body,
            binding_scope,
        }))
    }

    /// Desugar a body that may contain internal define-syntax forms (TaggedValue version)
    ///
    /// Matches the behavior of `desugar_body_with_internal_defines`: when encountering
    /// `define-syntax`, creates a child environment to scope the macro locally instead
    /// of polluting the global environment.
    fn desugar_body_tagged(
        &self,
        initial_desugarer: &Desugarer,
        body_tvs: &[TaggedValue],
        shared_heap: &SharedHeap,
        body_scopes: &ScopeSet,
    ) -> Result<Vec<CoreExpr>> {
        let env = match &initial_desugarer.env {
            Some(e) => e.clone(),
            None => {
                return body_tvs
                    .iter()
                    .map(|tv| initial_desugarer.desugar_tagged(*tv, shared_heap))
                    .collect();
            }
        };

        let mut body_exprs = Vec::new();
        let mut current_env = env.clone();
        let mut current_desugarer = initial_desugarer.with_new_env(env, body_scopes.clone());

        for tv in body_tvs {
            // Check if this is a define-syntax form BEFORE desugaring
            let define_syntax_info = self.try_parse_define_syntax_tagged(*tv, shared_heap);

            if let Some((macro_name, transformer_tv)) = define_syntax_info {
                // Compile the macro immediately
                let compiled_macro = self.compile_syntax_rules_tagged(
                    transformer_tv,
                    shared_heap,
                    macro_name.clone(),
                    &current_env,
                    body_scopes,
                )?;

                // Create a new child environment with the macro binding
                let new_env = Rc::new(Environment::with_parent(current_env.clone()));
                let tv = new_env
                    .heap()
                    .borrow_mut()
                    .alloc_macro(Rc::new(compiled_macro));
                new_env.define(macro_name.to_string(), tv);

                current_env = new_env.clone();
                current_desugarer = current_desugarer.with_new_env(new_env, body_scopes.clone());
            } else {
                let desugared = current_desugarer.desugar_tagged(*tv, shared_heap)?;

                // Filter out Literal(Unspecified) from macro definitions
                if !matches!(&desugared.kind, CoreExprKind::Literal(v) if *v == TaggedValue::UNSPECIFIED)
                {
                    body_exprs.push(desugared);
                }
            }
        }

        if body_exprs.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "Body must contain at least one expression (not just define-syntax)".to_string(),
            ));
        }

        Ok(body_exprs)
    }

    /// Try to parse a TaggedValue as a define-syntax form
    ///
    /// Returns (macro_name, transformer_tv) if the TaggedValue is a
    /// (define-syntax name transformer) form. Works directly with TaggedValue.
    fn try_parse_define_syntax_tagged(
        &self,
        tagged: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Option<(Rc<str>, TaggedValue)> {
        // Must be a pair
        if !tagged.is_pair() {
            return None;
        }

        let heap = shared_heap.borrow();
        let (car, cdr) = heap.get_pair(tagged);

        // Check car is "define-syntax"
        let is_define_syntax = if let Some(s) = heap.get_symbol_name(car) {
            s == "define-syntax"
        } else if let Some((name, _)) = utils::get_identifier_info(car, &heap) {
            name.as_ref() == "define-syntax"
        } else {
            false
        };
        if !is_define_syntax {
            return None;
        }

        // Parse (name transformer) from cdr
        if !cdr.is_pair() {
            return None;
        }
        let (name_tv, rest) = heap.get_pair(cdr);
        if !rest.is_pair() {
            return None;
        }
        let (transformer_tv, tail) = heap.get_pair(rest);
        if tail != TaggedValue::NULL {
            return None;
        }

        // Extract macro name
        let macro_name = if let Some(s) = heap.get_symbol_name(name_tv) {
            Rc::from(s)
        } else if let Some((id_name, _)) = utils::get_identifier_info(name_tv, &heap) {
            id_name
        } else {
            return None;
        };

        Some((macro_name, transformer_tv))
    }

    /// Desugar if using TaggedValue
    fn desugar_if_tagged(&self, args: TaggedValue, shared_heap: &SharedHeap) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec_tagged(args, shared_heap)?;

        match args_vec.len() {
            2 => {
                let test = self.desugar_tagged(args_vec[0], shared_heap)?;
                let then = self.desugar_tagged(args_vec[1], shared_heap)?;
                Ok(CoreExpr::new(CoreExprKind::If {
                    test: Rc::new(test),
                    then: Rc::new(then),
                    else_: CoreExpr::rc(CoreExprKind::Literal(TaggedValue::UNSPECIFIED)),
                }))
            }
            3 => {
                let test = self.desugar_tagged(args_vec[0], shared_heap)?;
                let then = self.desugar_tagged(args_vec[1], shared_heap)?;
                let else_ = self.desugar_tagged(args_vec[2], shared_heap)?;
                Ok(CoreExpr::new(CoreExprKind::If {
                    test: Rc::new(test),
                    then: Rc::new(then),
                    else_: Rc::new(else_),
                }))
            }
            _ => Err(DesugarError::WrongArgCount {
                form: "if".to_string(),
                expected: "2 or 3".to_string(),
                got: args_vec.len(),
            }),
        }
    }

    /// Desugar set! using TaggedValue
    fn desugar_set_tagged(&self, args: TaggedValue, shared_heap: &SharedHeap) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec_tagged(args, shared_heap)?;

        if args_vec.len() != 2 {
            return Err(DesugarError::WrongArgCount {
                form: "set!".to_string(),
                expected: "2".to_string(),
                got: args_vec.len(),
            });
        }

        let var_tv = args_vec[0];
        let (name, scopes) = {
            let heap = shared_heap.borrow();
            if let Some(s) = heap.get_symbol_name(var_tv) {
                (Rc::from(s), ScopeSet::new())
            } else if let Some((id_name, id_scopes)) = utils::get_identifier_info(var_tv, &heap) {
                (id_name, id_scopes)
            } else {
                return Err(DesugarError::InvalidSyntax(
                    "set! requires a symbol as first argument".to_string(),
                ));
            }
        };

        let value = self.desugar_tagged(args_vec[1], shared_heap)?;

        Ok(CoreExpr::new(CoreExprKind::Set {
            var: name,
            scopes,
            value: Rc::new(value),
        }))
    }

    /// Desugar define using TaggedValue
    fn desugar_define_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec_tagged(args, shared_heap)?;

        if args_vec.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "define requires at least a name".to_string(),
            ));
        }

        let first = args_vec[0];

        // Check if it's function shorthand: (define (name params...) body...)
        if first.is_pair() {
            let (name, formals_tv) = utils::parse_define_function_tagged(first, shared_heap)?;
            let body_tvs: Vec<_> = args_vec[1..].to_vec();

            if body_tvs.is_empty() {
                return Err(DesugarError::EmptyBody("define".to_string()));
            }

            let params = utils::convert_formals_tagged(formals_tv, shared_heap)?;

            // Create body desugarer with shadowed names
            let param_names = utils::formals_to_names(&params);
            let body_scopes = self.current_scopes.clone();
            let body_desugarer = self.with_shadowed_names(param_names, body_scopes);

            // Create a fresh binding scope for this lambda
            let binding_scope = ScopeId::fresh();

            let body: Vec<CoreExpr> = body_tvs
                .iter()
                .map(|tv| body_desugarer.desugar_tagged(*tv, shared_heap))
                .collect::<Result<Vec<_>>>()?;

            return Ok(CoreExpr::new(CoreExprKind::Define {
                name,
                value: CoreExpr::rc(CoreExprKind::Lambda {
                    params,
                    body,
                    binding_scope: Some(binding_scope),
                }),
            }));
        }

        // Simple variable define: (define name value)
        let name = {
            let heap = shared_heap.borrow();
            if let Some(s) = heap.get_symbol_name(first) {
                Rc::from(s)
            } else if let Some((id_name, _)) = utils::get_identifier_info(first, &heap) {
                id_name
            } else {
                return Err(DesugarError::InvalidSyntax(
                    "define requires a symbol as first argument".to_string(),
                ));
            }
        };

        if args_vec.len() != 2 {
            return Err(DesugarError::WrongArgCount {
                form: "define".to_string(),
                expected: "2".to_string(),
                got: args_vec.len(),
            });
        }

        let value_tv = args_vec[1];
        let value = self.desugar_tagged(value_tv, shared_heap)?;

        Ok(CoreExpr::new(CoreExprKind::Define {
            name,
            value: Rc::new(value),
        }))
    }

    /// Desugar begin using TaggedValue
    fn desugar_begin_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        let exprs = utils::list_to_vec_tagged(args, shared_heap)?;

        if exprs.is_empty() {
            // (begin) with no body is valid in Scheme, returns unspecified
            return Ok(CoreExpr::new(CoreExprKind::Literal(
                TaggedValue::UNSPECIFIED,
            )));
        }

        let body: Vec<CoreExpr> = exprs
            .iter()
            .map(|tv| self.desugar_tagged(*tv, shared_heap))
            .collect::<Result<Vec<_>>>()?;

        Ok(CoreExpr::new(CoreExprKind::Begin(body)))
    }

    /// Desugar apply using TaggedValue
    fn desugar_apply_tagged(
        &self,
        list: TaggedValue,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec_tagged(args, shared_heap)?;

        if args_vec.len() < 2 {
            // Don't reject at compile time — fall through to regular
            // procedure call so the runtime raises the arity error.
            // This allows (guard ...) and (test-error ...) to catch it.
            return self.desugar_app_tagged(list, shared_heap);
        }

        let func = self.desugar_tagged(args_vec[0], shared_heap)?;
        let operands: Vec<CoreExpr> = args_vec[1..]
            .iter()
            .map(|tv| self.desugar_tagged(*tv, shared_heap))
            .collect::<Result<Vec<_>>>()?;

        Ok(CoreExpr::new(CoreExprKind::Apply {
            func: Rc::new(func),
            args: operands,
        }))
    }

    /// Desugar application using TaggedValue
    fn desugar_app_tagged(&self, list: TaggedValue, shared_heap: &SharedHeap) -> Result<CoreExpr> {
        let exprs = utils::list_to_vec_tagged(list, shared_heap)?;

        if exprs.is_empty() {
            return Err(DesugarError::InvalidSyntax("Empty application".to_string()));
        }

        let func = self.desugar_tagged(exprs[0], shared_heap)?;
        let operands: Vec<CoreExpr> = exprs[1..]
            .iter()
            .map(|tv| self.desugar_tagged(*tv, shared_heap))
            .collect::<Result<Vec<_>>>()?;

        Ok(CoreExpr::new(CoreExprKind::App {
            func: Rc::new(func),
            args: operands,
        }))
    }

    /// Desugar quote using TaggedValue: (quote datum) → Quote(datum)
    ///
    /// Strips identifiers to symbols in quoted data (hygiene cleanup).
    fn desugar_quote_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec_tagged(args, shared_heap)?;
        if args_vec.len() != 1 {
            return Err(DesugarError::WrongArgCount {
                form: "quote".to_string(),
                expected: "1".to_string(),
                got: args_vec.len(),
            });
        }
        let datum = utils::strip_identifiers_tagged(args_vec[0], shared_heap);
        Ok(CoreExpr::new(CoreExprKind::Quote(datum)))
    }

    /// Desugar quasiquote using TaggedValue: (quasiquote template) → Quasiquote(template)
    fn desugar_quasiquote_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec_tagged(args, shared_heap)?;
        if args_vec.len() != 1 {
            return Err(DesugarError::WrongArgCount {
                form: "quasiquote".to_string(),
                expected: "1".to_string(),
                got: args_vec.len(),
            });
        }
        Ok(CoreExpr::new(CoreExprKind::Quasiquote(args_vec[0])))
    }

    /// Desugar define-syntax using TaggedValue
    ///
    /// Extracts name and transformer from TaggedValue args. Passes the transformer
    /// directly as TaggedValue to `compile_syntax_rules_tagged`.
    fn desugar_define_syntax_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec_tagged(args, shared_heap)?;

        if args_vec.len() != 2 {
            return Err(DesugarError::InvalidSyntax(
                "define-syntax requires (define-syntax name transformer)".to_string(),
            ));
        }

        // Extract name from TaggedValue
        let name = {
            let heap = shared_heap.borrow();
            if let Some(s) = heap.get_symbol_name(args_vec[0]) {
                Rc::from(s)
            } else if let Some((id_name, _)) = utils::get_identifier_info(args_vec[0], &heap) {
                id_name
            } else {
                return Err(DesugarError::InvalidSyntax(
                    "define-syntax requires (define-syntax name transformer)".to_string(),
                ));
            }
        };

        // Compile macro immediately and install in environment
        let env = self.env.as_ref().ok_or_else(|| {
            DesugarError::InvalidSyntax(
                "define-syntax requires environment (use Desugarer::with_env)".into(),
            )
        })?;

        let compiled_macro = self.compile_syntax_rules_tagged(
            args_vec[1],
            shared_heap,
            name.clone(),
            env,
            &self.current_scopes,
        )?;

        // Install in environment
        let tv = env.heap().borrow_mut().alloc_macro(Rc::new(compiled_macro));
        env.define(name.to_string(), tv);

        Ok(CoreExpr::new(CoreExprKind::Literal(
            TaggedValue::UNSPECIFIED,
        )))
    }

    /// Desugar import using TaggedValue: (import import-set ...) → Import { import_sets }
    fn desugar_import_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        let import_sets = utils::list_to_vec_tagged(args, shared_heap)?;

        if import_sets.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "import requires at least one import set".to_string(),
            ));
        }

        Ok(CoreExpr::new(CoreExprKind::Import { import_sets }))
    }

    /// Desugar expand using TaggedValue: (expand expr) → Expand { expr }
    fn desugar_expand_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec_tagged(args, shared_heap)?;
        if args_vec.len() != 1 {
            return Err(DesugarError::WrongArgCount {
                form: "expand".to_string(),
                expected: "1".to_string(),
                got: args_vec.len(),
            });
        }

        Ok(CoreExpr::new(CoreExprKind::Expand {
            expr: Rc::new(self.desugar_tagged(args_vec[0], shared_heap)?),
        }))
    }

    /// Desugar let-syntax using TaggedValue
    fn desugar_let_syntax_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        self.desugar_let_syntax_impl_tagged(args, shared_heap, false)
    }

    /// Desugar letrec-syntax using TaggedValue
    fn desugar_letrec_syntax_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        self.desugar_let_syntax_impl_tagged(args, shared_heap, true)
    }

    /// Common implementation for let-syntax and letrec-syntax using TaggedValue
    fn desugar_let_syntax_impl_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
        is_letrec: bool,
    ) -> Result<CoreExpr> {
        let env = self.env.as_ref().ok_or_else(|| {
            DesugarError::InvalidSyntax(
                "let-syntax requires macro environment (desugarer must be created with with_env)"
                    .to_string(),
            )
        })?;

        let let_syntax_scope = ScopeId::fresh();
        let definition_scopes = self.current_scopes.with_scope(let_syntax_scope);

        let args_vec = utils::list_to_vec_tagged(args, shared_heap)?;

        let form_name = if is_letrec {
            "letrec-syntax"
        } else {
            "let-syntax"
        };

        if args_vec.len() < 2 {
            return Err(DesugarError::InvalidSyntax(format!(
                "{} requires bindings and at least one body expression",
                form_name
            )));
        }

        // Parse bindings: ((name transformer) ...)
        let bindings_list = utils::list_to_vec_tagged(args_vec[0], shared_heap)?;

        // Determine compilation environment
        let compile_env = if is_letrec {
            Rc::new(Environment::with_parent(env.clone()))
        } else {
            env.clone()
        };

        // Compile each macro binding — pass transformer TaggedValue directly
        let mut macro_bindings = Vec::new();
        for binding_tv in bindings_list {
            let binding_vec = utils::list_to_vec_tagged(binding_tv, shared_heap)?;
            if binding_vec.len() != 2 {
                return Err(DesugarError::InvalidSyntax(
                    "Each let-syntax binding must be (name transformer)".to_string(),
                ));
            }

            let name = {
                let heap = shared_heap.borrow();
                if let Some(s) = heap.get_symbol_name(binding_vec[0]) {
                    Rc::from(s)
                } else if let Some((id_name, _)) = utils::get_identifier_info(binding_vec[0], &heap)
                {
                    id_name
                } else {
                    return Err(DesugarError::InvalidSyntax(
                        "Macro name must be a symbol".to_string(),
                    ));
                }
            };

            let compiled_macro = self.compile_syntax_rules_tagged(
                binding_vec[1],
                shared_heap,
                name.clone(),
                &compile_env,
                &definition_scopes,
            )?;

            macro_bindings.push((name, compiled_macro));
        }

        // Create environment with macro bindings
        let body_env = Rc::new(Environment::with_parent(env.clone()));
        for (name, compiled_macro) in macro_bindings {
            let tv = body_env
                .heap()
                .borrow_mut()
                .alloc_macro(Rc::new(compiled_macro));
            body_env.define(name.to_string(), tv);
        }

        let body_desugarer = self.with_new_env(body_env, definition_scopes.clone());

        // Get body TaggedValues
        let body_tvs: Vec<TaggedValue> = args_vec[1..].to_vec();

        // Check if body contains internal defines
        let has_internal_defines = body_tvs
            .iter()
            .any(|tv| self.is_regular_define_tagged(*tv, shared_heap));

        if has_internal_defines {
            let desugared_body = self.desugar_body_tagged(
                &body_desugarer,
                &body_tvs,
                shared_heap,
                &definition_scopes,
            )?;

            Ok(CoreExpr::new(CoreExprKind::App {
                func: CoreExpr::rc(CoreExprKind::Lambda {
                    params: patina_ir::Formals::Fixed(vec![]),
                    body: desugared_body,
                    binding_scope: Some(ScopeId::fresh()),
                }),
                args: vec![],
            }))
        } else {
            let desugared_body = self.desugar_body_tagged(
                &body_desugarer,
                &body_tvs,
                shared_heap,
                &definition_scopes,
            )?;

            if desugared_body.len() == 1 {
                Ok(desugared_body.into_iter().next().unwrap())
            } else {
                Ok(CoreExpr::new(CoreExprKind::Begin(desugared_body)))
            }
        }
    }

    /// Check if a TaggedValue is a regular `define` form (not `define-syntax`)
    fn is_regular_define_tagged(&self, tv: TaggedValue, shared_heap: &SharedHeap) -> bool {
        if !tv.is_pair() {
            return false;
        }

        let heap = shared_heap.borrow();
        if let Some((car, _)) = heap.try_pair(tv) {
            if let Some(s) = heap.get_symbol_name(car) {
                return s == "define";
            }
            if let Some((name, _)) = heap.get_identifier_data_any(car) {
                return name.as_ref() == "define";
            }
        }
        false
    }

    /// Desugar cond-expand using TaggedValue: (cond-expand clause ...)
    fn desugar_cond_expand_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        use crate::cond_expand::evaluate_feature_requirement_tagged;
        use patina_runtime::features::FeatureRegistry;

        let clauses = utils::list_to_vec_tagged(args, shared_heap)?;

        if clauses.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "cond-expand requires at least one clause".to_string(),
            ));
        }

        let features = FeatureRegistry::new();

        let can_load_library = |_lib_name: &[String]| false;

        for (i, &clause_tv) in clauses.iter().enumerate() {
            let clause_list = utils::list_to_vec_tagged(clause_tv, shared_heap)?;

            if clause_list.is_empty() {
                return Err(DesugarError::InvalidSyntax(
                    "cond-expand clause cannot be empty".to_string(),
                ));
            }

            let requirement_tv = clause_list[0];
            let body_tvs = &clause_list[1..];

            // Check for 'else' clause
            let is_else = {
                let heap = shared_heap.borrow();
                if let Some(s) = heap.get_symbol_name(requirement_tv) {
                    s == "else"
                } else if let Some((name, _)) = utils::get_identifier_info(requirement_tv, &heap) {
                    name.as_ref() == "else"
                } else {
                    false
                }
            };

            if is_else {
                if i != clauses.len() - 1 {
                    return Err(DesugarError::InvalidSyntax(
                        "cond-expand: else clause must be last".to_string(),
                    ));
                }
                return self.desugar_cond_expand_body_tagged(body_tvs, shared_heap);
            }

            // Evaluate feature requirement directly from TaggedValue
            let matches = evaluate_feature_requirement_tagged(
                requirement_tv,
                shared_heap,
                &features,
                &can_load_library,
            )
            .map_err(|e| {
                DesugarError::InvalidSyntax(format!(
                    "cond-expand: invalid feature requirement: {}",
                    e
                ))
            })?;

            if matches {
                return self.desugar_cond_expand_body_tagged(body_tvs, shared_heap);
            }
        }

        Err(DesugarError::InvalidSyntax(
            "cond-expand: no matching clause".to_string(),
        ))
    }

    /// Desugar the body of a cond-expand clause (TaggedValue version)
    fn desugar_cond_expand_body_tagged(
        &self,
        body: &[TaggedValue],
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        if body.is_empty() {
            return Ok(CoreExpr::new(CoreExprKind::Literal(
                TaggedValue::UNSPECIFIED,
            )));
        }

        let desugared: Vec<CoreExpr> = body
            .iter()
            .map(|tv| self.desugar_tagged(*tv, shared_heap))
            .collect::<Result<_>>()?;

        if desugared.len() == 1 {
            Ok(desugared.into_iter().next().unwrap())
        } else {
            Ok(CoreExpr::new(CoreExprKind::Begin(desugared)))
        }
    }

    // =========================================================================
    // syntax-error
    // =========================================================================

    /// (syntax-error message args ...) — signal a compile-time error
    ///
    /// R7RS Section 4.3.1: "It is an error" at macro expansion time.
    /// The first argument must be a string literal (the message); any remaining
    /// arguments are irritants displayed alongside it.
    fn desugar_syntax_error_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        let parts = utils::list_to_vec_tagged(args, shared_heap)?;

        if parts.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "syntax-error requires at least a message argument".to_string(),
            ));
        }

        // Extract message string
        let message = {
            let heap = shared_heap.borrow();
            heap.get_string_contents(parts[0])
                .unwrap_or_else(|| patina_core::debug_format::format_tagged(parts[0], &heap))
        };

        // Format irritants
        if parts.len() > 1 {
            let irritants: Vec<String> = {
                let heap = shared_heap.borrow();
                parts[1..]
                    .iter()
                    .map(|tv| patina_core::debug_format::format_tagged(*tv, &heap))
                    .collect()
            };
            Err(DesugarError::InvalidSyntax(format!(
                "syntax-error: {} {}",
                message,
                irritants.join(" ")
            )))
        } else {
            Err(DesugarError::InvalidSyntax(format!(
                "syntax-error: {}",
                message
            )))
        }
    }

    // =========================================================================
    // include / include-ci
    // =========================================================================

    /// (include filename ...) or (include-ci filename ...)
    ///
    /// R7RS Section 4.1.7: Reads the contents of each file as if by repeated
    /// applications of `read`, and effectively replaces the include expression
    /// with a begin expression containing the read expressions.
    /// include-ci reads as if each file began with `#!fold-case`.
    fn desugar_include_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
        case_insensitive: bool,
    ) -> Result<CoreExpr> {
        let filenames = utils::list_to_vec_tagged(args, shared_heap)?;

        if filenames.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "include requires at least one filename".to_string(),
            ));
        }

        // Determine the base directory for resolving relative paths.
        // We look at the source map to find the current file being compiled.
        let base_dir = self.resolve_include_base_dir();

        let mut all_exprs: Vec<CoreExpr> = Vec::new();

        for &filename_tv in &filenames {
            // Extract the filename string
            let filename = {
                let heap = shared_heap.borrow();
                heap.get_string_contents(filename_tv).ok_or_else(|| {
                    DesugarError::InvalidSyntax(format!(
                        "include: expected string filename, got {}",
                        heap.type_name(filename_tv)
                    ))
                })?
            };

            // Resolve the path
            let path = if let Some(ref base) = base_dir {
                let p = base.join(&filename);
                if self.fs.file_exists(&p) {
                    p
                } else {
                    // Fall back to current directory
                    std::path::PathBuf::from(&filename)
                }
            } else {
                std::path::PathBuf::from(&filename)
            };

            // Read the file
            let content = self.fs.read_to_string(&path).map_err(|e| {
                DesugarError::InvalidSyntax(format!(
                    "include: cannot read '{}': {}",
                    path.display(),
                    e
                ))
            })?;

            // Parse the file contents
            let mut parser = if case_insensitive {
                crate::Parser::new_case_insensitive_with_heap(&content, shared_heap.clone())
            } else {
                crate::Parser::new_with_heap(&content, shared_heap.clone())
            }
            .map_err(|e| {
                DesugarError::InvalidSyntax(format!(
                    "include: parse error in '{}': {}",
                    path.display(),
                    e
                ))
            })?;

            let parsed_exprs = parser.parse_all().map_err(|e| {
                DesugarError::InvalidSyntax(format!(
                    "include: parse error in '{}': {}",
                    path.display(),
                    e
                ))
            })?;

            // Desugar each expression from the included file
            for expr_tv in parsed_exprs {
                all_exprs.push(self.desugar_tagged(expr_tv, shared_heap)?);
            }
        }

        if all_exprs.is_empty() {
            Ok(CoreExpr::new(CoreExprKind::Literal(
                TaggedValue::UNSPECIFIED,
            )))
        } else if all_exprs.len() == 1 {
            Ok(all_exprs.into_iter().next().unwrap())
        } else {
            Ok(CoreExpr::new(CoreExprKind::Begin(all_exprs)))
        }
    }

    /// Determine the base directory for resolving include paths.
    ///
    /// Looks at source locations in the source map to find a file path,
    /// then uses its parent directory. Falls back to the current working
    /// directory if no file path is available (e.g., REPL or eval).
    fn resolve_include_base_dir(&self) -> Option<std::path::PathBuf> {
        if let Some(ref sm) = self.source_map {
            let sm_ref = sm.borrow();
            // Look for any source location that has a real file path
            for loc in sm_ref.iter_locations() {
                let s = loc.source.as_ref();
                if !s.starts_with('<') && !s.is_empty() {
                    let p = std::path::Path::new(s);
                    if let Some(parent) = p.parent() {
                        return Some(parent.to_path_buf());
                    }
                }
            }
        }
        // Fall back to current working directory
        std::env::current_dir().ok()
    }

    // =========================================================================
    // Shared helpers
    // =========================================================================

    /// Compile a syntax-rules transformer from TaggedValue with scope-based hygiene
    fn compile_syntax_rules_tagged(
        &self,
        transformer_tv: TaggedValue,
        shared_heap: &SharedHeap,
        name: Rc<str>,
        env: &Rc<Environment>,
        scopes: &ScopeSet,
    ) -> Result<patina_macros::CompiledMacro> {
        use patina_macros::Compiler;

        let list = utils::list_to_vec_tagged(transformer_tv, shared_heap)?;

        if list.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "Expected syntax-rules".to_string(),
            ));
        }

        // Check that first element is "syntax-rules"
        let is_syntax_rules = {
            let heap = shared_heap.borrow();
            if let Some(s) = heap.get_symbol_name(list[0]) {
                s == "syntax-rules"
            } else if let Some((name, _)) = utils::get_identifier_info(list[0], &heap) {
                name.as_ref() == "syntax-rules"
            } else {
                false
            }
        };
        if !is_syntax_rules {
            return Err(DesugarError::InvalidSyntax(
                "Expected syntax-rules".to_string(),
            ));
        }

        if list.len() < 2 {
            return Err(DesugarError::InvalidSyntax(
                "syntax-rules requires literals and rules".to_string(),
            ));
        }

        // Check for custom ellipsis: (syntax-rules my-ellipsis (lits...) rules...)
        let (custom_ellipsis, literals_index) = {
            let heap = shared_heap.borrow();
            let second = list[1];
            if second == TaggedValue::NULL || second.is_pair() {
                // It's a list (literals list) — no custom ellipsis
                (None, 1)
            } else if let Some(s) = heap.get_symbol_name(second) {
                (Some(Rc::from(s)), 2)
            } else if let Some((id_name, _)) = utils::get_identifier_info(second, &heap) {
                (Some(id_name), 2)
            } else {
                return Err(DesugarError::InvalidSyntax(
                    "syntax-rules: expected literals list or ellipsis identifier".to_string(),
                ));
            }
        };

        if custom_ellipsis.is_some() && list.len() < 3 {
            return Err(DesugarError::InvalidSyntax(
                "syntax-rules with custom ellipsis requires literals and rules".to_string(),
            ));
        }

        let literals = self.parse_literals_list_tagged(list[literals_index], shared_heap)?;

        // Parse rules as (pattern, template) pairs, converting to Value at the boundary
        let rules_start = literals_index + 1;
        let rules = self.parse_macro_rules_tagged(&list[rules_start..], shared_heap)?;

        let mut compiler = Compiler::with_env_scopes_and_shadowed(
            literals,
            custom_ellipsis,
            env.clone(),
            scopes.clone(),
            &self.shadowed_names,
            env.heap().clone(),
        );
        compiler
            .compile_macro(name, rules)
            .map_err(|e| DesugarError::InvalidSyntax(format!("Failed to compile macro: {}", e)))
    }

    /// Parse the literals list from TaggedValue: (lit1 lit2 ...)
    fn parse_literals_list_tagged(
        &self,
        literals_tv: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<Vec<Rc<str>>> {
        let items = utils::list_to_vec_tagged(literals_tv, shared_heap)?;
        let heap = shared_heap.borrow();
        let mut literals = Vec::new();
        for item in items {
            if let Some(s) = heap.get_symbol_name(item) {
                literals.push(Rc::from(s));
            } else if let Some((name, _)) = utils::get_identifier_info(item, &heap) {
                literals.push(name);
            } else {
                return Err(DesugarError::InvalidSyntax(
                    "syntax-rules literals must be symbols".to_string(),
                ));
            }
        }
        Ok(literals)
    }

    /// Parse macro rules from a slice of TaggedValue
    ///
    /// Each rule is a TaggedValue list `(pattern template)`. Returns the
    /// pattern/template pairs directly as TaggedValue.
    fn parse_macro_rules_tagged(
        &self,
        rules_tvs: &[TaggedValue],
        shared_heap: &SharedHeap,
    ) -> Result<Vec<(TaggedValue, TaggedValue)>> {
        if rules_tvs.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "syntax-rules must have at least one rule".to_string(),
            ));
        }

        let mut rules = Vec::new();
        for &rule_tv in rules_tvs {
            let rule_list = utils::list_to_vec_tagged(rule_tv, shared_heap)?;

            if rule_list.len() != 2 {
                return Err(DesugarError::InvalidSyntax(
                    "Each syntax-rules rule must have exactly 2 elements (pattern template)"
                        .to_string(),
                ));
            }

            rules.push((rule_list[0], rule_list[1]));
        }

        Ok(rules)
    }
}

impl Default for Desugarer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_core::Heap;
    use patina_ir::Formals;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Create a fresh SharedHeap for tests
    fn test_heap() -> SharedHeap {
        Rc::new(RefCell::new(Heap::new()))
    }

    /// Build a proper list of TaggedValues on the heap
    fn make_list(heap: &SharedHeap, items: &[TaggedValue]) -> TaggedValue {
        items.iter().rev().fold(TaggedValue::NULL, |acc, tv| {
            heap.borrow_mut().alloc_pair(*tv, acc)
        })
    }

    /// Intern a symbol on the heap
    fn sym(heap: &SharedHeap, name: &str) -> TaggedValue {
        heap.borrow_mut().intern_symbol(name)
    }

    // =========================================================================
    // Self-evaluating literals
    // =========================================================================

    #[test]
    fn test_desugar_integer() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        let tagged = TaggedValue::fixnum(42);
        let result = desugarer.desugar_tagged(tagged, &heap).unwrap();
        if let CoreExprKind::Literal(v) = result.kind {
            assert!(v.is_fixnum() && v.as_fixnum_unchecked() == 42);
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_boolean() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        let result = desugarer.desugar_tagged(TaggedValue::TRUE, &heap).unwrap();
        if let CoreExprKind::Literal(v) = result.kind {
            assert_eq!(v, TaggedValue::TRUE);
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_string() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        let tagged = heap.borrow_mut().alloc_string("hello".to_string());
        let result = desugarer.desugar_tagged(tagged, &heap).unwrap();
        if let CoreExprKind::Literal(v) = result.kind {
            assert!(!v.is_immediate());
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_character() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        let tagged = TaggedValue::character('a');
        let result = desugarer.desugar_tagged(tagged, &heap).unwrap();
        if let CoreExprKind::Literal(v) = result.kind {
            assert!(v.is_char() && v.as_char_unchecked() == 'a');
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    // =========================================================================
    // Variables
    // =========================================================================

    #[test]
    fn test_desugar_variable() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        let x = sym(&heap, "x");
        let result = desugarer.desugar_tagged(x, &heap).unwrap();
        if let CoreExprKind::Var { name, scopes } = result.kind {
            assert_eq!(name.as_ref(), "x");
            assert!(scopes.is_empty());
        } else {
            panic!("Expected Var, got {:?}", result);
        }
    }

    // =========================================================================
    // Core Form: quote
    // =========================================================================

    #[test]
    fn test_desugar_quote_symbol() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        let list = make_list(&heap, &[sym(&heap, "quote"), sym(&heap, "x")]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Quote(val) = result.kind {
            assert!(!val.is_immediate());
        } else {
            panic!("Expected Quote, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_quote_list() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        let inner = make_list(
            &heap,
            &[
                TaggedValue::fixnum(1),
                TaggedValue::fixnum(2),
                TaggedValue::fixnum(3),
            ],
        );
        let list = make_list(&heap, &[sym(&heap, "quote"), inner]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        assert!(matches!(&result.kind, CoreExprKind::Quote(_)));
    }

    // =========================================================================
    // Core Form: lambda
    // =========================================================================

    #[test]
    fn test_desugar_lambda_fixed_params() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (lambda (x y) (+ x y))
        let params = make_list(&heap, &[sym(&heap, "x"), sym(&heap, "y")]);
        let body = make_list(&heap, &[sym(&heap, "+"), sym(&heap, "x"), sym(&heap, "y")]);
        let list = make_list(&heap, &[sym(&heap, "lambda"), params, body]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Lambda { params, body, .. } = result.kind {
            assert!(matches!(params, Formals::Fixed(_)));
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected Lambda, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_lambda_variadic() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (lambda args (car args))
        let body = make_list(&heap, &[sym(&heap, "car"), sym(&heap, "args")]);
        let list = make_list(&heap, &[sym(&heap, "lambda"), sym(&heap, "args"), body]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Lambda { params, body, .. } = result.kind {
            assert!(matches!(params, Formals::Variadic(_)));
            assert_eq!(body.len(), 1);
        } else {
            panic!("Expected Lambda, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_lambda_rest_params() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (lambda (x y . rest) x) — improper list formals
        let rest = sym(&heap, "rest");
        let y = sym(&heap, "y");
        let y_rest = heap.borrow_mut().alloc_pair(y, rest);
        let x = sym(&heap, "x");
        let formals = heap.borrow_mut().alloc_pair(x, y_rest);
        let list = make_list(&heap, &[sym(&heap, "lambda"), formals, sym(&heap, "x")]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Lambda { params, .. } = result.kind {
            assert!(matches!(params, Formals::Mixed { .. }));
        } else {
            panic!("Expected Lambda, got {:?}", result);
        }
    }

    // =========================================================================
    // Core Form: if
    // =========================================================================

    #[test]
    fn test_desugar_if_three_args() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (if #t 1 2)
        let list = make_list(
            &heap,
            &[
                sym(&heap, "if"),
                TaggedValue::TRUE,
                TaggedValue::fixnum(1),
                TaggedValue::fixnum(2),
            ],
        );
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::If { test, then, else_ } = result.kind {
            assert!(matches!(&test.kind, CoreExprKind::Literal(v) if *v == TaggedValue::TRUE));
            assert!(
                matches!(&then.kind, CoreExprKind::Literal(v) if v.is_fixnum() && v.as_fixnum_unchecked() == 1)
            );
            assert!(
                matches!(&else_.kind, CoreExprKind::Literal(v) if v.is_fixnum() && v.as_fixnum_unchecked() == 2)
            );
        } else {
            panic!("Expected If, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_if_two_args() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (if #t 1)
        let list = make_list(
            &heap,
            &[sym(&heap, "if"), TaggedValue::TRUE, TaggedValue::fixnum(1)],
        );
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::If { test, then, else_ } = result.kind {
            assert!(matches!(&test.kind, CoreExprKind::Literal(v) if *v == TaggedValue::TRUE));
            assert!(
                matches!(&then.kind, CoreExprKind::Literal(v) if v.is_fixnum() && v.as_fixnum_unchecked() == 1)
            );
            assert!(
                matches!(&else_.kind, CoreExprKind::Literal(v) if *v == TaggedValue::UNSPECIFIED)
            );
        } else {
            panic!("Expected If, got {:?}", result);
        }
    }

    // =========================================================================
    // Core Form: set!
    // =========================================================================

    #[test]
    fn test_desugar_set() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (set! x 42)
        let list = make_list(
            &heap,
            &[sym(&heap, "set!"), sym(&heap, "x"), TaggedValue::fixnum(42)],
        );
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Set { var, scopes, value } = result.kind {
            assert_eq!(var.as_ref(), "x");
            assert!(scopes.is_empty());
            assert!(
                matches!(&value.kind, CoreExprKind::Literal(v) if v.is_fixnum() && v.as_fixnum_unchecked() == 42)
            );
        } else {
            panic!("Expected Set, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_set_non_symbol_error() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (set! 123 42) - invalid
        let list = make_list(
            &heap,
            &[
                sym(&heap, "set!"),
                TaggedValue::fixnum(123),
                TaggedValue::fixnum(42),
            ],
        );
        let result = desugarer.desugar_tagged(list, &heap);
        assert!(result.is_err());
    }

    // =========================================================================
    // Core Form: define
    // =========================================================================

    #[test]
    fn test_desugar_define_variable() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (define x 42)
        let list = make_list(
            &heap,
            &[
                sym(&heap, "define"),
                sym(&heap, "x"),
                TaggedValue::fixnum(42),
            ],
        );
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Define { name, value } = result.kind {
            assert_eq!(name.as_ref(), "x");
            assert!(
                matches!(&value.kind, CoreExprKind::Literal(v) if v.is_fixnum() && v.as_fixnum_unchecked() == 42)
            );
        } else {
            panic!("Expected Define, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_define_function() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (define (add x y) (+ x y))
        let name_params = make_list(
            &heap,
            &[sym(&heap, "add"), sym(&heap, "x"), sym(&heap, "y")],
        );
        let body = make_list(&heap, &[sym(&heap, "+"), sym(&heap, "x"), sym(&heap, "y")]);
        let list = make_list(&heap, &[sym(&heap, "define"), name_params, body]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Define { name, value } = result.kind {
            assert_eq!(name.as_ref(), "add");
            assert!(matches!(&value.kind, CoreExprKind::Lambda { .. }));
        } else {
            panic!("Expected Define, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_define_variadic() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (define (f . args) args) — improper list
        let f = sym(&heap, "f");
        let args_sym = sym(&heap, "args");
        let name_params = heap.borrow_mut().alloc_pair(f, args_sym);
        let list = make_list(
            &heap,
            &[sym(&heap, "define"), name_params, sym(&heap, "args")],
        );
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Define { name, value } = result.kind {
            assert_eq!(name.as_ref(), "f");
            if let CoreExprKind::Lambda { params, .. } = &value.kind {
                assert!(matches!(params, Formals::Variadic(_)));
            } else {
                panic!("Expected Lambda, got {:?}", value);
            }
        } else {
            panic!("Expected Define, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_define_mixed_variadic() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (define (f x y . rest) rest) — improper list with fixed + rest
        let y = sym(&heap, "y");
        let rest = sym(&heap, "rest");
        let y_rest = heap.borrow_mut().alloc_pair(y, rest);
        let x = sym(&heap, "x");
        let x_y_rest = heap.borrow_mut().alloc_pair(x, y_rest);
        let f = sym(&heap, "f");
        let name_params = heap.borrow_mut().alloc_pair(f, x_y_rest);
        let list = make_list(
            &heap,
            &[sym(&heap, "define"), name_params, sym(&heap, "rest")],
        );
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Define { name, value } = result.kind {
            assert_eq!(name.as_ref(), "f");
            if let CoreExprKind::Lambda { params, .. } = &value.kind {
                if let Formals::Mixed { fixed, rest } = params {
                    assert_eq!(fixed.len(), 2);
                    assert_eq!(fixed[0].name.as_ref(), "x");
                    assert_eq!(fixed[1].name.as_ref(), "y");
                    assert_eq!(rest.name.as_ref(), "rest");
                } else {
                    panic!("Expected mixed formals, got {:?}", params);
                }
            } else {
                panic!("Expected Lambda, got {:?}", value);
            }
        } else {
            panic!("Expected Define, got {:?}", result);
        }
    }

    // =========================================================================
    // Core Form: begin
    // =========================================================================

    #[test]
    fn test_desugar_begin_single_expr() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (begin 42)
        let list = make_list(&heap, &[sym(&heap, "begin"), TaggedValue::fixnum(42)]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Begin(exprs) = result.kind {
            assert_eq!(exprs.len(), 1);
            assert!(
                matches!(&exprs[0].kind, CoreExprKind::Literal(v) if v.is_fixnum() && v.as_fixnum_unchecked() == 42)
            );
        } else {
            panic!("Expected Begin, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_begin_multiple_exprs() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (begin 1 2 3)
        let list = make_list(
            &heap,
            &[
                sym(&heap, "begin"),
                TaggedValue::fixnum(1),
                TaggedValue::fixnum(2),
                TaggedValue::fixnum(3),
            ],
        );
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Begin(exprs) = result.kind {
            assert_eq!(exprs.len(), 3);
        } else {
            panic!("Expected Begin, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_begin_empty() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (begin) → #<unspecified>
        let list = make_list(&heap, &[sym(&heap, "begin")]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Literal(v) = result.kind {
            assert_eq!(v, TaggedValue::UNSPECIFIED);
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    // =========================================================================
    // Application
    // =========================================================================

    #[test]
    fn test_desugar_application() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (+ 1 2)
        let list = make_list(
            &heap,
            &[
                sym(&heap, "+"),
                TaggedValue::fixnum(1),
                TaggedValue::fixnum(2),
            ],
        );
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::App { func, args } = result.kind {
            assert!(matches!(&func.kind, CoreExprKind::Var { .. }));
            assert_eq!(args.len(), 2);
        } else {
            panic!("Expected App, got {:?}", result);
        }
    }

    #[test]
    fn test_desugar_lambda_application() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // ((lambda (x) x) 42)
        let params = make_list(&heap, &[sym(&heap, "x")]);
        let lambda = make_list(&heap, &[sym(&heap, "lambda"), params, sym(&heap, "x")]);
        let list = make_list(&heap, &[lambda, TaggedValue::fixnum(42)]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::App { func, args } = result.kind {
            assert!(matches!(&func.kind, CoreExprKind::Lambda { .. }));
            assert_eq!(args.len(), 1);
        } else {
            panic!("Expected App, got {:?}", result);
        }
    }

    // =========================================================================
    // Error cases
    // =========================================================================

    #[test]
    fn test_desugar_empty_list_literal() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // () as a literal is fine (Null)
        let result = desugarer.desugar_tagged(TaggedValue::NULL, &heap);
        assert!(result.is_ok());
    }

    // =========================================================================
    // cond-expand
    // =========================================================================

    #[test]
    fn test_cond_expand_r7rs_feature() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (cond-expand (r7rs 42))
        let clause = make_list(&heap, &[sym(&heap, "r7rs"), TaggedValue::fixnum(42)]);
        let list = make_list(&heap, &[sym(&heap, "cond-expand"), clause]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Literal(v) = result.kind {
            assert!(v.is_fixnum() && v.as_fixnum_unchecked() == 42);
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_patina_feature() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (cond-expand (patina (quote patina-impl)))
        let quoted = make_list(&heap, &[sym(&heap, "quote"), sym(&heap, "patina-impl")]);
        let clause = make_list(&heap, &[sym(&heap, "patina"), quoted]);
        let list = make_list(&heap, &[sym(&heap, "cond-expand"), clause]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        assert!(matches!(&result.kind, CoreExprKind::Quote(_)));
    }

    #[test]
    fn test_cond_expand_else_clause() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (cond-expand (nonexistent 1) (else 99))
        let c1 = make_list(&heap, &[sym(&heap, "nonexistent"), TaggedValue::fixnum(1)]);
        let c2 = make_list(&heap, &[sym(&heap, "else"), TaggedValue::fixnum(99)]);
        let list = make_list(&heap, &[sym(&heap, "cond-expand"), c1, c2]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Literal(v) = result.kind {
            assert!(v.is_fixnum() && v.as_fixnum_unchecked() == 99);
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_no_match_error() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (cond-expand (nonexistent 1))
        let clause = make_list(&heap, &[sym(&heap, "nonexistent"), TaggedValue::fixnum(1)]);
        let list = make_list(&heap, &[sym(&heap, "cond-expand"), clause]);
        let result = desugarer.desugar_tagged(list, &heap);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("no matching clause"));
    }

    #[test]
    fn test_cond_expand_and_requirement() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (cond-expand ((and r7rs patina) 100))
        let req = make_list(
            &heap,
            &[sym(&heap, "and"), sym(&heap, "r7rs"), sym(&heap, "patina")],
        );
        let clause = make_list(&heap, &[req, TaggedValue::fixnum(100)]);
        let list = make_list(&heap, &[sym(&heap, "cond-expand"), clause]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Literal(v) = result.kind {
            assert!(v.is_fixnum() && v.as_fixnum_unchecked() == 100);
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_or_requirement() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (cond-expand ((or nonexistent r7rs) 200))
        let req = make_list(
            &heap,
            &[
                sym(&heap, "or"),
                sym(&heap, "nonexistent"),
                sym(&heap, "r7rs"),
            ],
        );
        let clause = make_list(&heap, &[req, TaggedValue::fixnum(200)]);
        let list = make_list(&heap, &[sym(&heap, "cond-expand"), clause]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Literal(v) = result.kind {
            assert!(v.is_fixnum() && v.as_fixnum_unchecked() == 200);
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_not_requirement() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (cond-expand ((not nonexistent) 300))
        let req = make_list(&heap, &[sym(&heap, "not"), sym(&heap, "nonexistent")]);
        let clause = make_list(&heap, &[req, TaggedValue::fixnum(300)]);
        let list = make_list(&heap, &[sym(&heap, "cond-expand"), clause]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Literal(v) = result.kind {
            assert!(v.is_fixnum() && v.as_fixnum_unchecked() == 300);
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_multiple_expressions() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (cond-expand (r7rs 1 2 3))
        let clause = make_list(
            &heap,
            &[
                sym(&heap, "r7rs"),
                TaggedValue::fixnum(1),
                TaggedValue::fixnum(2),
                TaggedValue::fixnum(3),
            ],
        );
        let list = make_list(&heap, &[sym(&heap, "cond-expand"), clause]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Begin(exprs) = result.kind {
            assert_eq!(exprs.len(), 3);
        } else {
            panic!("Expected Begin, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_first_match_wins() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (cond-expand (r7rs 1) (patina 2) (else 3))
        let c1 = make_list(&heap, &[sym(&heap, "r7rs"), TaggedValue::fixnum(1)]);
        let c2 = make_list(&heap, &[sym(&heap, "patina"), TaggedValue::fixnum(2)]);
        let c3 = make_list(&heap, &[sym(&heap, "else"), TaggedValue::fixnum(3)]);
        let list = make_list(&heap, &[sym(&heap, "cond-expand"), c1, c2, c3]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Literal(v) = result.kind {
            assert!(v.is_fixnum() && v.as_fixnum_unchecked() == 1);
        } else {
            panic!("Expected Literal, got {:?}", result);
        }
    }

    #[test]
    fn test_cond_expand_else_not_last_error() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (cond-expand (else 1) (r7rs 2))
        let c1 = make_list(&heap, &[sym(&heap, "else"), TaggedValue::fixnum(1)]);
        let c2 = make_list(&heap, &[sym(&heap, "r7rs"), TaggedValue::fixnum(2)]);
        let list = make_list(&heap, &[sym(&heap, "cond-expand"), c1, c2]);
        let result = desugarer.desugar_tagged(list, &heap);
        assert!(result.is_err());
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(err_msg.contains("else clause must be last"));
    }

    #[test]
    fn test_cond_expand_empty_body() {
        let heap = test_heap();
        let desugarer = Desugarer::new();
        // (cond-expand (r7rs))
        let clause = make_list(&heap, &[sym(&heap, "r7rs")]);
        let list = make_list(&heap, &[sym(&heap, "cond-expand"), clause]);
        let result = desugarer.desugar_tagged(list, &heap).unwrap();
        if let CoreExprKind::Literal(v) = result.kind {
            assert_eq!(v, TaggedValue::UNSPECIFIED);
        } else {
            panic!("Expected Literal(Unspecified), got {:?}", result);
        }
    }
}
