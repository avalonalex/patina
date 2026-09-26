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
mod quasiquote;
mod utils;

pub use error::{DesugarError, Result};

use crate::source_map::SourceMap;
use patina_core::error::SourceLocation;
use patina_core::walk::OpenNodes;
use patina_core::{CoreForm, SharedHeap, TaggedValue};
use patina_ir::{CoreExpr, CoreExprKind};
use patina_macros::IdentifierKey;
use patina_macros::macro_expander::utils::list_to_vec_with_tail_tagged;
use patina_runtime::{Environment, ScopeId, ScopeSet};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Walk a freshly-expanded pair tree and stamp each unrecorded pair with the call-site source.
///
/// Pairs from the original user source are already recorded by the parser; this only
/// stamps new template-created pairs. Bounded by depth to avoid runaway recursion.
/// Whether a desugared body contributes any definition, looking through
/// `Begin`.
///
/// `begin` splices, so a definition inside one is a definition of the body
/// however deep it sits — and that is not an exotic shape here but the usual
/// one: `define-values` and `define-record-type` both expand to a `begin` of
/// definitions. Testing only the top level of the body saw `(define-values (a
/// b) …)` inside a `let-syntax` as no definition at all and let both names
/// escape into the enclosing body.
///
/// `patina-vm`'s `body_defines::for_each_define` answers the same question
/// over the same type and cannot be shared: it lives downstream of this crate.
fn body_binds_definitions(exprs: &[CoreExpr]) -> bool {
    exprs.iter().any(|e| match &e.kind {
        CoreExprKind::Define { .. } => true,
        CoreExprKind::Begin(inner) => body_binds_definitions(inner),
        _ => false,
    })
}

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

/// The aliases one expansion's relinking installs, keyed by spelling, and
/// the scope that says which occurrences they apply to.
struct Renames<'a> {
    aliases: &'a HashMap<Rc<str>, Aliases>,
    /// The expansion's own scope — see `MacroExpansion::scope`.
    expansion_scope: ScopeId,
    /// The pairs and vectors the rewrite is inside (`rewrite_refs`).
    open: RefCell<OpenNodes>,
}

/// The aliases made for one spelling.
enum Aliases {
    /// The macro mentions the spelling under one identity, so every
    /// occurrence the expansion introduced is that mention. All a written
    /// macro has, and the usual case for a generated one.
    Sole(TaggedValue),
    /// The macro mentions it under several, which only a generated macro can
    /// (`CompiledMacro::inherited_identifiers`), and they need not mean one
    /// binding. Each identity that was aliased, as the scopes the mention
    /// carries in the template — an occurrence in the expansion carries those
    /// and the expansion's scope — with its own alias.
    PerIdentity(Vec<(ScopeSet, TaggedValue)>),
}

/// Monotonic counter for the unique names given to definition-environment
/// aliases. Never reused, so an alias cannot collide with a user binding or
/// with an alias from another expansion.
fn next_alias_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The name to give an alias for `name`. Paired with [`alias_base`], which
/// is the only reader of the shape: keeping the two together is what stops
/// a change here from silently making relinked heads unrecognisable there.
fn alias_name(name: &str) -> String {
    format!("{name}.{}", next_alias_id())
}

/// The name an alias was made from, if `spelling` is one.
fn alias_base(spelling: &str) -> Option<&str> {
    let (base, id) = spelling.rsplit_once('.')?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some(base)
}

/// Is this spelling `quote`, or an alias made from it?
///
/// The cheap half of the head test: answered from a `&str` the caller
/// already has, so the environment lookup in
/// [`Desugarer::head_resolves_to_quote`] is reached only for a head that
/// could be `quote` at all.
fn is_quote_spelling(name: &str) -> bool {
    name == "quote" || alias_base(name) == Some("quote")
}

/// The spelling a head must have for `(apply f args)` to be lowered to
/// `CoreExpr::Apply`, and the name the `apply` primitive is registered under
/// — the filter in front of, and the answer to, the binding check that decides
/// the lowering (`Desugarer::head_is_the_apply_primitive`, #443).
const APPLY: &str = "apply";

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
/// **Name Shadowing**: a local binding is recorded in the environment at the
/// scopes of the body that binds it, so shadowing is resolution: in
/// `(let ((let odd?)) (let 8))` the inner `let` resolves to the variable, not
/// the macro.
/// What a head symbol, variable reference or `set!` target names, when it
/// names syntax rather than a value.
///
/// Returned by [`Desugarer::resolve_syntax`], which is the single place that
/// decides. Head position acts on both variants, the other two refuse both.
enum SyntaxRef {
    CoreSyntax(CoreForm),
    Macro(Rc<patina_core::CompiledMacro>),
}

impl SyntaxRef {
    /// How to name this in a diagnostic.
    fn describe(&self) -> &'static str {
        match self {
            SyntaxRef::CoreSyntax(_) => "a syntactic keyword",
            SyntaxRef::Macro(_) => "a macro",
        }
    }
}

/// What a name means to a macro's environment, where the root here imports
/// that very location: the location, and the root's alias for it.
type ImportOf = Option<(patina_core::BindingLocation, Rc<str>)>;

/// What early binding (#438, `Desugarer::early_bound`) has to remember across
/// one top-level form. Shared, not cloned, with the child desugarers made for
/// nested scopes, and emptied when `desugar_tagged` returns.
#[derive(Default)]
struct EarlyBinding {
    /// The scope of each expansion, in this form, of a macro defined in
    /// another program or library, with the environment it was defined in.
    foreign: RefCell<FxHashMap<ScopeId, Rc<Environment>>>,
    /// Alias → the spelling it was written with, for each reference bound
    /// early in this form.
    bound: RefCell<FxHashMap<Rc<str>, Rc<str>>>,
    /// Per macro environment, then name: the location the name means there
    /// and the root's alias for it, or `None` where it is not an import to
    /// bind. Two levels so that a hit is looked up by `&str`, without minting
    /// a key — this is asked once per reference a library macro makes, and
    /// with the default hasher and a cloned key it was most of what early
    /// binding cost.
    imports: RefCell<FxHashMap<u64, FxHashMap<Rc<str>, ImportOf>>>,
    /// The definitions this form's expansions introduced: name, and scopes.
    introduced: RefCell<Vec<(Rc<str>, ScopeSet)>>,
}

/// What `Desugarer::settle_early_bindings` reads of a finished form: the
/// definitions it introduced, and alias → spelling for what it bound early.
type FinishedForm = (Vec<(Rc<str>, ScopeSet)>, FxHashMap<Rc<str>, Rc<str>>);

impl EarlyBinding {
    /// Whether no form is in progress — what `desugar_tagged` finds on entry.
    fn is_idle(&self) -> bool {
        self.foreign.borrow().is_empty()
            && self.bound.borrow().is_empty()
            && self.imports.borrow().is_empty()
            && self.introduced.borrow().is_empty()
    }

    /// Forget the form, handing back what settling it needs.
    fn finish_form(&self) -> FinishedForm {
        self.foreign.borrow_mut().clear();
        self.imports.borrow_mut().clear();
        (
            std::mem::take(&mut *self.introduced.borrow_mut()),
            std::mem::take(&mut *self.bound.borrow_mut()),
        )
    }
}

pub struct Desugarer {
    /// The environment head symbols resolve in.
    ///
    /// Not optional: since core syntactic keywords became bindings and the
    /// spelling fallback was deleted, a desugarer without an environment could
    /// not desugar `(quote x)` — it would compile a call to an unbound
    /// `quote`. `Desugarer::new()` supplies one holding exactly the keywords.
    env: Rc<Environment>,

    /// Current scope set for scope-based hygiene
    /// Accumulates scopes as we enter binding forms
    current_scopes: ScopeSet,

    /// Optional source map for looking up source positions of parsed forms
    source_map: Option<Rc<RefCell<SourceMap>>>,

    /// Virtual filesystem for `include` / `include-ci` forms.
    /// Defaults to `NativeFs` when not explicitly set.
    fs: std::sync::Arc<dyn patina_core::FileSystem>,

    /// Directories a relative `include` path is resolved against, innermost
    /// last: the directory of the file being desugared, then of each file an
    /// `include` has opened on the way here. Shared (not cloned) with the
    /// child desugarers made for nested scopes, so an `include` inside a
    /// `let-syntax` body pushes and pops the same stack. Empty for a program
    /// that has no file (the REPL, `eval`), where the cwd is what is left.
    include_dirs: Rc<RefCell<Vec<std::path::PathBuf>>>,

    /// See [`EarlyBinding`].
    early: Rc<EarlyBinding>,

    /// The forms being desugared, outermost first — so that one met again
    /// inside itself is refused (`desugar_open_form`). Shared with the child
    /// desugarers made for nested scopes, like `early`, since a form's
    /// elements are desugared by them.
    open_forms: Rc<RefCell<OpenNodes>>,
}

impl Desugarer {
    /// Create a desugarer whose environment holds the syntactic keywords and
    /// nothing else.
    ///
    /// The desugarer's equivalent of `(null-environment)`: core forms desugar,
    /// and every other name is an ordinary variable reference. Before stage 2
    /// this constructor carried no environment at all and relied on keywords
    /// being recognized by spelling; with the fallback gone it has to bind
    /// them, which also means `define-syntax` now works here instead of
    /// erroring.
    pub fn new() -> Self {
        let env = Rc::new(Environment::new());
        patina_runtime::stdlib::seed_core_syntax(&env);
        Self::with_env(env)
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
            env,
            current_scopes: ScopeSet::new(),
            source_map: None,
            fs: std::sync::Arc::new(patina_core::NativeFs),
            include_dirs: Rc::new(RefCell::new(Vec::new())),
            early: Rc::default(),
            open_forms: Rc::default(),
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
            env,
            current_scopes: ScopeSet::new(),
            source_map: Some(source_map),
            fs: std::sync::Arc::new(patina_core::NativeFs),
            include_dirs: Rc::new(RefCell::new(Vec::new())),
            early: Rc::default(),
            open_forms: Rc::default(),
        }
    }

    /// Set the virtual filesystem for `include` handling.
    pub fn with_fs(mut self, fs: std::sync::Arc<dyn patina_core::FileSystem>) -> Self {
        self.fs = fs;
        self
    }

    /// Name the directory of the file whose forms this desugarer will see,
    /// so a relative `include` inside them resolves beside that file. The
    /// library loaders call this with the `.sld`'s directory; without it the
    /// only candidate is the current working directory.
    pub fn with_include_base(self, dir: std::path::PathBuf) -> Self {
        self.include_dirs.borrow_mut().push(dir);
        self
    }

    /// [`Self::with_include_base`] for a library's source path, when there is
    /// one — the shape every library loader has in hand. One helper so the
    /// three loaders (VM backend, VM runtime, tree-walker) cannot drift.
    pub fn with_include_base_of(self, source: Option<&std::path::Path>) -> Self {
        match source.and_then(|p| p.parent()) {
            Some(dir) if !dir.as_os_str().is_empty() => self.with_include_base(dir.to_path_buf()),
            _ => self,
        }
    }

    /// Create a new desugarer with environment and specific scope set
    ///
    /// Used when creating child desugarers that inherit scope context.
    pub fn with_env_and_scopes(env: Rc<Environment>, scopes: ScopeSet) -> Self {
        Self {
            env,
            current_scopes: scopes,
            source_map: None,
            fs: std::sync::Arc::new(patina_core::NativeFs),
            include_dirs: Rc::new(RefCell::new(Vec::new())),
            early: Rc::default(),
            open_forms: Rc::default(),
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
            source_map: self.source_map.clone(),
            fs: self.fs.clone(),
            include_dirs: self.include_dirs.clone(),
            early: Rc::clone(&self.early),
            open_forms: Rc::clone(&self.open_forms),
        };
        (desugarer, scope)
    }

    /// Put `binding_scope` on this body's references to the names `binders`
    /// binds. A reference a template made to a name that same template binds
    /// carries the template's scopes, never this form's, so without this the
    /// binding installed below is unreachable from it.
    fn scope_body(
        binders: &[(Rc<str>, ScopeSet)],
        body_tvs: &[TaggedValue],
        binding_scope: ScopeId,
        shared_heap: &SharedHeap,
    ) -> Vec<TaggedValue> {
        let names: HashSet<Rc<str>> = binders.iter().map(|(name, _)| name.clone()).collect();
        if names.is_empty() {
            return body_tvs.to_vec();
        }
        body_tvs
            .iter()
            .map(|&tv| {
                patina_macros::add_scope_to_bound_names(tv, &names, binding_scope, shared_heap)
            })
            .collect()
    }

    fn enter_binding_form(
        &self,
        binders: impl IntoIterator<Item = (Rc<str>, ScopeSet)>,
        binding_scope: ScopeId,
    ) -> Self {
        let binders: Vec<(Rc<str>, ScopeSet)> = binders.into_iter().collect();
        let new_scopes = self.current_scopes.with_scope(binding_scope);

        // A local binding is a binding. Recording it in the environment at the
        // body's scope set is what lets `resolve_syntax` answer by ordinary
        // set-of-scopes resolution — innermost wins, because the innermost
        // binder's scope set is the largest one the reference contains — with
        // no separate shadowing rule to keep in step. The value is a marker:
        // nothing reads it, and all that matters is that it is neither core
        // syntax nor a macro, so a name bound here answers "not syntax".
        // An empty scope set would not make a narrow binding, it would make a
        // global one: `insert_scoped` treats "no scopes" as "not scoped" and
        // falls through to `define`. Every binding form must therefore hand a
        // fresh scope down with its names; asserting it here is what turns a
        // future omission into a test failure instead of a captured keyword.
        debug_assert!(
            binders.is_empty() || !new_scopes.is_empty(),
            "local bindings need a scope of their own: {:?}",
            binders.iter().map(|(name, _)| name).collect::<Vec<_>>()
        );
        // A child environment even when this body binds no names, because the
        // body is still a body: an internal `define-syntax` installs itself in
        // `self.env`, so reusing the parent is what let `(define (f) (define-
        // syntax m …) …)` leak `m` into the enclosing environment. Keeping the
        // parent when the formals happened to be empty, and not otherwise, made
        // that depend on an unrelated property of the lambda.
        let env = if new_scopes.is_empty() {
            self.env.clone()
        } else {
            let child = Rc::new(Environment::with_parent(self.env.clone()));
            for (name, as_written) in &binders {
                // Each binder at the scopes it was *written* with plus this
                // form's — the rule `desugar_let_syntax_impl_tagged` states
                // for a keyword. A binder written in source carries none of
                // its own and stands in the accumulated scopes.
                let scopes = if as_written.is_empty() {
                    new_scopes.clone()
                } else {
                    as_written.with_scope(binding_scope)
                };
                child.define_with_scopes(name.to_string(), scopes, TaggedValue::UNSPECIFIED);
            }
            child
        };

        Self {
            env,
            current_scopes: new_scopes,
            source_map: self.source_map.clone(),
            fs: self.fs.clone(),
            include_dirs: self.include_dirs.clone(),
            early: Rc::clone(&self.early),
            open_forms: Rc::clone(&self.open_forms),
        }
    }

    /// The names a body's internal definitions bind.
    ///
    /// R7RS §5.3.2 gives internal definitions `letrec*` scope: they bind over
    /// the *whole* body, so `(define (f) (define if 3) (+ if 1))` is a legal
    /// program in which `if` is a variable in both forms. That has to be known
    /// before any form is desugared — otherwise the definition's own body, and
    /// every later form, still resolves the name to whatever the enclosing
    /// environment calls it, which since #89 means the value-position check
    /// rejects a legal program and head position silently picks the core form
    /// over the local binding.
    ///
    /// Only definitions *written* in the body are seen here. The ones a macro
    /// use expands into (`define-values`, `define-record-type`) are found by
    /// [`produced_definition_names`] once these are entered.
    ///
    /// [`produced_definition_names`]: Self::produced_definition_names
    fn body_definition_names(
        &self,
        body_tvs: &[TaggedValue],
        shared_heap: &SharedHeap,
    ) -> Vec<(Rc<str>, ScopeSet)> {
        let mut names = Vec::new();
        for tv in body_tvs {
            self.collect_definition_names(*tv, shared_heap, &mut names, &mut OpenNodes::default());
        }
        names
    }

    /// Enter the definitions a body's macro uses produce, on top of the ones
    /// written in it: [`produced_definition_names`], scoped and bound as
    /// [`enter_binding_form`] binds any other body definition. A body whose
    /// macro uses define nothing gets no extra environment.
    ///
    /// [`produced_definition_names`]: Self::produced_definition_names
    /// [`enter_binding_form`]: Self::enter_binding_form
    fn enter_produced_definitions(
        self,
        body_tvs: Vec<TaggedValue>,
        binding_scope: ScopeId,
        shared_heap: &SharedHeap,
    ) -> (Self, Vec<TaggedValue>) {
        let produced = self.produced_definition_names(&body_tvs, shared_heap);
        if produced.is_empty() {
            return (self, body_tvs);
        }
        let body_tvs = Self::scope_body(&produced, &body_tvs, binding_scope, shared_heap);
        (self.enter_binding_form(produced, binding_scope), body_tvs)
    }

    /// The names a body's *macro uses* define — `define-values`,
    /// `define-record-type`, anything whose expansion is a definition — which
    /// [`body_definition_names`] cannot see, since it reads the forms as
    /// written.
    ///
    /// Without them the body's desugar-time reads went wrong in both
    /// directions chibi and Gauche get right: a body-local `else` from
    /// `define-values` or a record accessor was matched as `cond`'s literal,
    /// even from a procedure defined before it, and a body-local `when` was
    /// refused as syntax used as a value.
    ///
    /// Each body form whose head names a macro is expanded here only to read
    /// what it defines, and the expansion is thrown away: the form is
    /// desugared from its original text, and expanded again, with every name
    /// found here bound. That is one extra expansion per macro use at a body's
    /// top level, and it keeps this a pre-pass — nothing it does is visible
    /// except the names. Relinking and source stamping are skipped for the
    /// same reason, and an expansion that fails is ignored, since the dispatch
    /// that desugars the form expands it again and reports the failure.
    ///
    /// Only names from the use site are kept. A name the expansion introduced
    /// carries the scope that expansion minted, and the real expansion mints
    /// a different one, so nothing could reach a binding recorded for it here.
    ///
    /// Asked after the written definitions are entered, so a macro use is
    /// expanded, and its literals are matched, as the real desugar will do it.
    ///
    /// [`body_definition_names`]: Self::body_definition_names
    fn produced_definition_names(
        &self,
        body_tvs: &[TaggedValue],
        shared_heap: &SharedHeap,
    ) -> Vec<(Rc<str>, ScopeSet)> {
        let mut names = Vec::new();
        for tv in body_tvs {
            self.collect_produced_names(*tv, shared_heap, &mut names, 0, &mut OpenNodes::default());
        }
        names
    }

    /// Add the names the macro uses in `tv` define to `out`, descending through
    /// `begin` and into each expansion. `depth` bounds a macro that expands into
    /// a use of itself forever, which the real desugar would not finish either.
    ///
    /// A form reached again inside itself — a datum label can write one — is
    /// not scanned again, and a circular `begin` is scanned once round (#459):
    /// the desugar that follows refuses both.
    fn collect_produced_names(
        &self,
        tv: TaggedValue,
        shared_heap: &SharedHeap,
        out: &mut Vec<(Rc<str>, ScopeSet)>,
        depth: usize,
        open: &mut OpenNodes,
    ) {
        const DEPTH_LIMIT: usize = 64;
        if depth > DEPTH_LIMIT || !tv.is_pair() || !open.enter(tv) {
            return;
        }
        self.collect_produced_names_of_form(tv, shared_heap, out, depth, open);
        open.leave();
    }

    /// [`Self::collect_produced_names`] for a pair it has entered.
    fn collect_produced_names_of_form(
        &self,
        tv: TaggedValue,
        shared_heap: &SharedHeap,
        out: &mut Vec<(Rc<str>, ScopeSet)>,
        depth: usize,
        open: &mut OpenNodes,
    ) {
        let (head, cdr) = {
            let heap = shared_heap.borrow();
            heap.get_pair(tv)
        };
        let Some((head_name, head_scopes)) = self.identifier_of(head, shared_heap) else {
            return;
        };
        match self.resolve_syntax(&head_name, &head_scopes).ok().flatten() {
            Some(SyntaxRef::CoreSyntax(CoreForm::Begin)) => {
                let (forms, _) = shared_heap.borrow().spine(cdr);
                for form in forms {
                    self.collect_produced_names(form, shared_heap, out, depth, open);
                }
            }
            Some(SyntaxRef::Macro(compiled_macro)) => {
                let Ok(expansion) = patina_macros::expand_macro_with_scope(
                    &compiled_macro,
                    tv,
                    shared_heap,
                    Some(patina_macros::Site {
                        env: &self.env,
                        scopes: &self.current_scopes,
                    }),
                ) else {
                    return;
                };
                let mut found = Vec::new();
                self.collect_definition_names(expansion.form, shared_heap, &mut found, open);
                self.collect_produced_names(
                    expansion.form,
                    shared_heap,
                    &mut found,
                    depth + 1,
                    open,
                );
                out.extend(
                    found
                        .into_iter()
                        .filter(|(_, scopes)| !scopes.contains(&expansion.scope)),
                );
            }
            _ => {}
        }
    }

    /// Add the names `tv` defines to `out`, descending through `begin`, which
    /// splices its contents into the body it appears in.
    ///
    /// The head is resolved, not spelled, so a `define` reached under an
    /// import rename counts and one shadowed by a parameter does not.
    ///
    /// A `begin` reached again inside itself is not scanned again, and a
    /// circular one is scanned once round (#459): the desugar that follows
    /// refuses both.
    fn collect_definition_names(
        &self,
        tv: TaggedValue,
        shared_heap: &SharedHeap,
        out: &mut Vec<(Rc<str>, ScopeSet)>,
        open: &mut OpenNodes,
    ) {
        if !tv.is_pair() || !open.enter(tv) {
            return;
        }
        self.collect_definition_names_of_form(tv, shared_heap, out, open);
        open.leave();
    }

    /// [`Self::collect_definition_names`] for a pair it has entered.
    fn collect_definition_names_of_form(
        &self,
        tv: TaggedValue,
        shared_heap: &SharedHeap,
        out: &mut Vec<(Rc<str>, ScopeSet)>,
        open: &mut OpenNodes,
    ) {
        let (head, cdr) = {
            let heap = shared_heap.borrow();
            heap.get_pair(tv)
        };
        let Some((head_name, head_scopes)) = self.identifier_of(head, shared_heap) else {
            return;
        };
        // A pre-pass, so an ambiguous head is not raised here: it reads as
        // "not a definition form", the body is scoped without that name, and
        // the dispatch that actually desugars the form resolves the same head
        // and reports it. Raising twice for one defect, once from a scan the
        // program did not ask for, would only make the message worse.
        match self.resolve_syntax(&head_name, &head_scopes).ok().flatten() {
            Some(SyntaxRef::CoreSyntax(CoreForm::Define)) => {
                let target = {
                    let heap = shared_heap.borrow();
                    cdr.is_pair().then(|| heap.get_pair(cdr).0)
                };
                if let Some(binder) = target.and_then(|t| self.define_target(t, shared_heap)) {
                    out.push(binder);
                }
            }
            Some(SyntaxRef::CoreSyntax(CoreForm::Begin)) => {
                let (forms, _) = shared_heap.borrow().spine(cdr);
                for form in forms {
                    self.collect_definition_names(form, shared_heap, out, open);
                }
            }
            _ => {}
        }
    }

    /// The name a `define` target binds: the symbol itself, or — for the
    /// procedure shorthand, curried arbitrarily deep — the one at the head of
    /// the nested formals.
    ///
    /// The chain of cars can come back on itself — `(define #0=(#0#) 1)` —
    /// and then there is no name (#459). A tortoise at half speed meets the
    /// walk only on such a cycle, as in `Heap::spine`.
    fn define_target(
        &self,
        tv: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Option<(Rc<str>, ScopeSet)> {
        let mut current = tv;
        let mut slow = tv;
        let mut move_slow = false;
        loop {
            if let Some(binder) = self.identifier_of(current, shared_heap) {
                return Some(binder);
            }
            if !current.is_pair() {
                return None;
            }
            let heap = shared_heap.borrow();
            current = heap.car(current);
            if move_slow {
                slow = heap.car(slow);
                if current == slow {
                    return None;
                }
            }
            move_slow = !move_slow;
        }
    }

    /// A symbol or identifier's name and scopes, if `tv` is one.
    fn identifier_of(
        &self,
        tv: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Option<(Rc<str>, ScopeSet)> {
        utils::symbol_or_identifier(tv, &shared_heap.borrow())
    }

    /// What syntax, if any, this identifier names here.
    ///
    /// The one answer to "is this name syntax?", used by all three places that
    /// have to ask: head position, where it selects a form or a macro
    /// expansion; value position, where it is a mistake; and a `set!` target,
    /// where it is also a mistake. Those were three separate lookups with two
    /// different shadowing rules and two different environment queries, and
    /// nothing kept them in step — a binding one of them called syntax and
    /// another did not would silently reopen the `#<macro>`-as-a-value hole.
    ///
    /// There is no shadowing rule here, and that is the point. A local variable
    /// is an ordinary binding in the environment, recorded at the scopes of the
    /// body that binds it, so shadowing is just resolution: the binding with
    /// the largest scope set the reference contains wins, which orders an inner
    /// keyword ahead of an outer variable without anything having to say so.
    ///
    /// This used to be a set of *spellings* consulted before the lookup, which
    /// had no ordering and applied only to a reference written without scopes.
    /// Both halves were wrong, in opposite directions: an outer variable vetoed
    /// an inner `let-syntax` keyword, and a macro-introduced reference skipped
    /// the check entirely so a template naming a definition-site local spelled
    /// `if` was reported as a keyword.
    ///
    /// Hygiene still holds, and by the same rule rather than an exemption: a
    /// macro-introduced reference carries the scopes of the macro's definition
    /// site, so `(let ((if 'captured)) (my-cond #t 'ok))` resolves the
    /// template's `if` where the template was written — the special form. That
    /// is `test_special_form_not_captured` in `patina-tests`' hygiene suite.
    ///
    /// `Err` when two visible bindings of the name are unordered by subset, so
    /// resolution has no most specific one to return. That is a defect in the
    /// program's binding structure and it is settled by the time expansion
    /// reaches here, which is why it is reported now and not at runtime.
    fn resolve_syntax(&self, name: &str, scopes: &ScopeSet) -> Result<Option<SyntaxRef>> {
        self.resolve_reference(name, scopes)
            .map(|(syntax, _)| syntax)
    }

    /// [`resolve_syntax`], also saying whether the name was answered **by its
    /// spelling** rather than by a binding whose scopes select it — which is
    /// the first thing early binding asks of a reference (`early_bound`), and
    /// which this one resolution already knows.
    ///
    /// [`resolve_syntax`]: Self::resolve_syntax
    fn resolve_reference(
        &self,
        name: &str,
        scopes: &ScopeSet,
    ) -> Result<(Option<SyntaxRef>, bool)> {
        // A reference written in source carries no scopes of its own, but it is
        // not therefore at top level: the scopes it stands in are the ones the
        // desugarer has accumulated on the way here. Passing those is what lets
        // an enclosing binder be *seen* rather than merely vetoed by spelling,
        // and so what orders an inner keyword ahead of an outer variable. A
        // macro-introduced reference already carries its own and keeps them.
        let scopes = if scopes.is_empty() {
            &self.current_scopes
        } else {
            scopes
        };
        let (value, selected) = self
            .env
            .resolve_with_scopes(name, scopes)
            .map_err(|e| DesugarError::AmbiguousReference(e.to_string()))?;
        let by_name = !selected;
        let Some(tv) = value else {
            return Ok((None, by_name));
        };
        let heap = self.env.heap().borrow();
        if let Some(form) = heap.get_core_syntax(tv) {
            return Ok((Some(SyntaxRef::CoreSyntax(form)), by_name));
        }
        Ok((heap.get_macro(tv).cloned().map(SyntaxRef::Macro), by_name))
    }

    /// Reject a reference to syntax where a value is expected.
    ///
    /// R7RS puts syntactic keywords and variables in disjoint categories (§3.1)
    /// and its `⟨expression⟩` grammar (§7.1.3) admits only the latter, so
    /// `(procedure? if)` is not a well-formed procedure call at all. Whether to
    /// *report* that is a choice the report leaves open — the reasoning, the
    /// references and the residual cases live in
    /// `crates/patina-tests/tests/syntax_as_a_value.rs`, which is where to look
    /// before changing this.
    ///
    /// Quoted data never reaches here (it desugars to a literal), and neither
    /// do `syntax-rules` patterns and templates, which the macro expander
    /// handles.
    fn reject_syntax_as_value(&self, name: &str, scopes: &ScopeSet) -> Result<()> {
        self.checked_reference(name, scopes).map(|_| ())
    }

    /// [`reject_syntax_as_value`], handing back whether the reference was
    /// answered by its spelling — the one resolution serving both that check
    /// and early binding.
    ///
    /// [`reject_syntax_as_value`]: Self::reject_syntax_as_value
    fn checked_reference(&self, name: &str, scopes: &ScopeSet) -> Result<bool> {
        match self.resolve_reference(name, scopes)? {
            (None, by_name) => Ok(by_name),
            (Some(found), _) => Err(DesugarError::InvalidSyntax(format!(
                "invalid use of syntax as a value: `{name}` is {}",
                found.describe()
            ))),
        }
    }

    /// Create a child desugarer with a new environment (for let-syntax bodies)
    ///
    /// It uses the new environment and scopes.
    fn with_new_env(&self, env: Rc<Environment>, scopes: ScopeSet) -> Self {
        Self {
            env,
            current_scopes: scopes,
            source_map: self.source_map.clone(),
            fs: self.fs.clone(),
            include_dirs: self.include_dirs.clone(),
            early: Rc::clone(&self.early),
            open_forms: Rc::clone(&self.open_forms),
        }
    }

    /// Rewrite a macro expansion's free identifiers so they resolve where the
    /// macro was defined.
    ///
    /// A `syntax-rules` template may name a helper private to the library that
    /// defines the macro. The expansion carries only the bare name, so at the
    /// use site it compiles to a global load that is not there. Each such name
    /// gets a uniquely-named alias in the use site's global environment
    /// pointing back at the definition environment.
    ///
    /// The definition binding wins whenever the two environments disagree
    /// (R7RS 4.3.2), so a use-site binding of the same name does not displace
    /// it. Where both reach the same binding — the common case, since most
    /// template references are to names both sides imported — nothing is
    /// rewritten.
    ///
    /// The names considered are the ones the macro's templates mention, in
    /// either of the two forms a mention takes: a symbol written in the
    /// template (`template_symbols`), or an identifier an enclosing expansion
    /// put there (`inherited_identifiers`), which is all a macro *generated*
    /// by another macro has. Considering only the first left such a macro,
    /// once exported, unable to reach the library that defined it — and,
    /// where the importing program bound the same name, reaching that instead
    /// (issue #402, triage family 47).
    ///
    /// A written macro mentions a name under one identity, its definition
    /// scopes. A generated one can mention it under several, so the decision
    /// and the alias are per identity ([`Aliases`]).
    fn link_definition_env_refs(
        &self,
        expanded: TaggedValue,
        expansion_scope: ScopeId,
        compiled_macro: &patina_core::CompiledMacro,
        shared_heap: &SharedHeap,
    ) -> TaggedValue {
        // A macro a foreign generator defined carries that generator's
        // expansion scope on its template's references, and the record that
        // the expansion was foreign did not outlive the form that ran the
        // generator. Put it back for this form, so that those references are
        // bound early here as they were there (#446).
        if !compiled_macro.foreign_expansions.is_empty() {
            let mut foreign = self.early.foreign.borrow_mut();
            for (scope, env) in &compiled_macro.foreign_expansions {
                foreign.entry(*scope).or_insert_with(|| Rc::clone(env));
            }
        }
        let Some(def_env) = compiled_macro.definition_env.as_ref() else {
            return expanded;
        };
        // Where a name needs no relinking — both sides reach one binding — the
        // reference is still bound to that binding, but later, where it is
        // known to *be* a reference (`early_bound`). That needs to know the
        // expansion came from somewhere else, which only this function does.
        if def_env.root_id() != self.env.root_id() {
            self.early
                .foreign
                .borrow_mut()
                .insert(expansion_scope, Rc::clone(def_env));
        }
        let definition_scopes = &compiled_macro.definition_scopes;
        let template_symbols = &compiled_macro.template_symbols;
        let inherited_identifiers = &compiled_macro.inherited_identifiers;
        if def_env.env_id() == self.env.env_id()
            || (template_symbols.is_empty() && inherited_identifiers.is_empty())
        {
            return expanded;
        }

        // Decide once per *name* rather than once per occurrence: the answer
        // depends only on the two environments, both fixed for this call. This
        // keeps the tree walk to a map lookup per leaf, and skips it entirely
        // when nothing needs relinking — which is the usual outcome, since
        // `let`, `cond` and friends only reference primitives.
        //
        // Aliases must land in the environment the code will actually be
        // resolved in. `self.env` may be a transient child created for a
        // `let-syntax` or internal-define body and dropped when desugaring
        // ends, so walk to the root of the chain.
        let target_env = self.env.root();
        let mut renames: HashMap<Rc<str>, Aliases> = HashMap::new();
        // Each name once: the written ones, then those only an enclosing
        // expansion put here. A name can be both.
        let names = template_symbols.iter().chain(
            inherited_identifiers
                .keys()
                .filter(|name| !template_symbols.contains(*name)),
        );
        for name in names {
            // The cheap test first: it settles nearly every name — `list`,
            // `if` and the rest are one binding on both sides — and the
            // per-mention question below walks the scoped tables, once for
            // each identity a generated macro mentions the name under.
            //
            // It asks whether the two environments reach one *location* by
            // this name, which they do for anything both imported, by any
            // route: an import is the exporting library's location (#406).
            // (An export `share_binding` cannot share — a macro-introduced
            // definition — is still a copy, so it reads as two and is
            // aliased to the library's; right, at the cost of the alias.)
            // Until that was true there was nothing to ask but whether they
            // held equal values, and two variables that are both `0`, `#f`
            // or `'()` — which is what variables start as — read as one
            // binding. A library macro's `(set! X (+ X 1))` over its private
            // `(define X 0)` then bumped the importing program's
            // `(define X 0)`, silently, and only while the two happened to be
            // equal (#407; the `equal` rows of `hygiene_matrix.rs`).
            //
            // Measured 2026-09-19 over every suite file, the compat corpus
            // and both Larceny lanes, some 34,000 decisions: none differs
            // from what the value comparison decided, so this costs no
            // expansion an alias it did not have. It decides differently only
            // where two bindings hold equal values — which is the defect. Nor
            // does asking cost more than comparing did: 12,000 expansions of
            // a library macro, a 16-library start-up and chibi's suite all
            // time within half a percent of `main`, on both backends.
            //
            // The test is about the *name*, so it settles a name only when
            // every mention of it is the name. A generated macro can also
            // mention it under scopes that select another definition of the
            // spelling — one its generator introduced (#408, below) — and
            // those are decided one by one. That takes the library to *have*
            // an introduced definition of the spelling, which for `list`,
            // `if` and the rest it does not, so they still stop here.
            let def_location = def_env.binding_location(name);
            let same_by_name =
                def_location.is_some() && self.env.binding_location(name) == def_location;
            let may_mean_another =
                inherited_identifiers.contains_key(name) && def_env.has_introduced_definition(name);
            if !may_mean_another && (def_location.is_none() || same_by_name) {
                continue;
            }

            // Every identity the macro mentions this name under: the macro's
            // definition scopes for a symbol written in the template, its own
            // scopes for an identifier an enclosing expansion put there.
            let mut identities: Vec<&ScopeSet> = Vec::new();
            if template_symbols.contains(name) {
                identities.push(definition_scopes);
            }
            for scopes in inherited_identifiers.get(name).into_iter().flatten() {
                if !identities.contains(&scopes) {
                    identities.push(scopes);
                }
            }
            let one_identity = identities.len() == 1;

            // A name the definition site bound *lexically* is not free, and
            // aliasing it would defeat the binding it actually names.
            //
            // `get` is the name-only view of the environment and deliberately
            // skips local variables, so it cannot tell `(let ((f …)) …)` around
            // this macro from a global `f` somewhere above — it answers with the
            // global either way. Asking again with the mention's own scopes is
            // what distinguishes them: a different binding means something
            // shadows the by-name view here, and ordinary set-of-scopes
            // resolution is what should decide the reference. Without this, a
            // `let-syntax` transformer's `(f x)` inside `(let ((f …)) …)` was
            // aliased to whatever `f` the enclosing program happened to define
            // — Larceny's `base` measured it as `number->string`.
            //
            // An empty scope set *is* the name-only view, so the top-level
            // and library case — the common one — answers without a walk.
            //
            // An ambiguous answer is also a disagreement: it has not
            // identified a binding, so there is none to alias to.
            //
            // The question is asked of the *binding*, not of its value
            // (`Environment::name_reaches_binding_of`). It compared values
            // until an inherited identifier could reach here (#402), and for
            // those that is not good enough: such an identifier carries its
            // generator's expansion scope, which is exactly what selects a
            // definition the same expansion introduced over another of the
            // same spelling — a plain one beside it, or the one a second run
            // of the generator introduced — and two such definitions
            // routinely start out equal (`(define count 0)`). Compared by
            // value they read as one binding, so both generated macros were
            // aliased onto whichever the name alone reaches and bumped a
            // single counter from outside the library; and once the values
            // diverged the same use was refused, so what a program answered
            // depended on what it had already run. An alias *by name* can
            // only point at what the name alone reaches, so a mention that
            // means another binding never gets one: it gets an alias to the
            // binding itself where the last paragraph below can name it, and
            // is otherwise left to scoped resolution rather than answered
            // with the wrong one. Re-audit if internal defines' scoped
            // bindings ever become relink targets.
            //
            // Decided per identity, and each identity that passes gets an
            // alias of its own, because a generated macro can mention one
            // spelling as two different things: `(let ((tmp 'local)) (list
            // tmp arg))` with the library's own `tmp` arriving through `arg`
            // holds a `tmp` the generator introduced, which the template
            // binds, and a `tmp` with no scopes, which is free. One alias for
            // the spelling renamed both alike and the `let` captured the free
            // one — `(local local)` where chibi and Gauche answer `(local
            // lib)`. A written macro cannot do that (everything written in it
            // shares its definition scopes), which is why one alias per name
            // was enough before #402.
            //
            // **A mention that means another binding gets an alias to *that*
            // binding, where it is a top-level definition a macro introduced**
            // (#408). Leaving it to scoped resolution was the right refusal
            // and not an answer: the use site is another library or a
            // program, whose scoped tables cannot see into this one, so the
            // reference was unbound — or, where the importer had the
            // spelling, the importer's. The shapes are the ordinary ones for
            // a definer with private state: a plain definition of the
            // spelling beside the introduced one, or the definer simply run
            // twice. `Environment::introduced_definition` says which
            // definition the mention's scopes select and how the backend
            // holds it; a lexical binding is still left alone, for the
            // reason above.
            let mut made: Vec<(ScopeSet, TaggedValue)> = Vec::new();
            for identity in identities {
                let alias_to = match def_env.name_reaches_binding_of(name, identity) {
                    // The name is the binding. Nothing to do where the use
                    // site's is the same one, or where it reaches nothing.
                    Ok(true) if def_location.is_none() || same_by_name => continue,
                    Ok(true) => None,
                    Ok(false) => match def_env.introduced_definition(name, identity) {
                        // Inside the library that holds it, scoped resolution
                        // already sees it.
                        Some((home, _)) if Rc::ptr_eq(&home, &target_env) => continue,
                        Some(found) => Some(found),
                        None => continue,
                    },
                    Err(_) => continue,
                };
                let alias = alias_name(name);
                let symbol = shared_heap.borrow_mut().intern_symbol(&alias);
                match alias_to {
                    None => target_env.define_alias(alias, def_env.clone(), name.clone()),
                    Some((home, found)) => {
                        target_env.define_alias_to_introduced(alias, home, name.clone(), found)
                    }
                }
                made.push((identity.clone(), symbol));
            }
            let aliases = match made.as_slice() {
                [] => continue,
                [(_, symbol)] if one_identity => Aliases::Sole(*symbol),
                _ => Aliases::PerIdentity(made),
            };
            renames.insert(name.clone(), aliases);
        }
        if renames.is_empty() {
            return expanded;
        }
        let renames = Renames {
            aliases: &renames,
            expansion_scope,
            open: RefCell::default(),
        };
        self.rewrite_refs(expanded, &renames, 0, shared_heap)
    }

    /// The use site's own name for the *location* a template's variable
    /// reference means, where the use site imported it — so that the reference
    /// goes on meaning that location whatever the spelling later comes to
    /// mean there (#438). `None` leaves the reference as it was written.
    ///
    /// When the use site and the definition site reach one binding by a name,
    /// relinking has nothing to correct, and the template's reference used to
    /// stay the bare name. That is right when it is decided and can be made
    /// wrong afterwards: a global name is looked up when it *runs*, and a
    /// program that then defines the spelling itself — `count`, or `car` —
    /// gets a binding of its own under it (`Environment::define`), which the
    /// template's reference then read and assigned. chibi and Gauche bind a
    /// template's reference when it is expanded, and both keep it the
    /// library's.
    ///
    /// **Called where a reference or an assignment is emitted, and nowhere
    /// else.** A first version renamed the identifier in the expansion's
    /// *syntax*, where relinking's other aliases are made, and review found
    /// what that costs when it happens to every imported name rather than to
    /// the rare one that needs relinking: whether a template's `car` is a
    /// reference is not known until it is resolved. It was a `case` datum
    /// (`((+) …)` stopped matching), a `let` binder (capturing another
    /// expansion's alias), a `define` target, data handed to a quoting macro,
    /// and `cond-expand`'s `not` — five silent wrong answers, none of which a
    /// lane caught, all pinned now in `expansion/imported-names-in-templates.scm`.
    /// Here the position is known: this *is* a variable reference.
    ///
    /// The conditions, each of which leaves the reference alone when it fails:
    ///
    /// - It carries the scope of an expansion of a macro from **another**
    ///   program or library (`EarlyBinding::foreign`, which relinking fills).
    ///   A macro of the use site's own is not bound early: chibi and Gauche
    ///   *disagree* about it (chibi keeps the import, Gauche follows the
    ///   definition), and what Patina already answers is Gauche's.
    /// - No binding's scopes select it — it is not a local variable, nor a
    ///   definition a macro introduced — so it is headed for the by-name view.
    ///   (`by_name`: the resolution that refused syntax as a value already
    ///   knows, and a second one per reference was a measurable cost.)
    /// - By name it reaches, from here and from the root alike, the very
    ///   location the macro's own environment reaches: an import of the use
    ///   site's that is the binding the template meant.
    ///
    /// No name is left out for its spelling. Five were until 2026-09-23,
    /// because some part of Patina decided what a call meant from the name on
    /// its `Var`, and renaming the reference changed what the program did:
    /// `apply` (#443), `call/cc` and `call-with-current-continuation` (#441),
    /// and `call-with-values` and `dynamic-wind` (#442). Each recogniser now
    /// asks what the name is bound to, so the alias is recognised as the name
    /// is. A new fast path has to do the same, or this binding defeats it.
    ///
    /// The alias is `Environment::import_alias`: an ordinary forwarded slot
    /// for the same location, one per imported name rather than one per
    /// expansion — and per name, not per location, because `bound` below is
    /// how `settle_early_bindings` reads back what a reference was written as.
    /// The reference keeps its scopes, which is what lets
    /// `settle_early_bindings` take a decision back.
    ///
    /// Cost, measured 2026-09-19 against `main`, best of 9 interleaved: 2–4% on
    /// 12,000 expansions of templates that each call a dozen imported
    /// procedures — the worst case, since this runs once per such reference —
    /// 1–2% on a 16-library start-up and on chibi's suite, and nothing at run
    /// time. It was 6–8% before the one resolution was shared with the
    /// syntax-as-value check, the per-name part memoised under a fast hasher,
    /// and internal recursion taken off the entry point's bookkeeping.
    fn early_bound(&self, name: &Rc<str>, scopes: &ScopeSet, by_name: bool) -> Option<Rc<str>> {
        if !by_name || scopes.is_empty() {
            return None;
        }
        let def_env = {
            let foreign = self.early.foreign.borrow();
            if foreign.is_empty() {
                return None;
            }
            // The expansion that put it here is the latest one it carries.
            let latest = scopes
                .iter()
                .filter(|scope| foreign.contains_key(scope))
                .max_by_key(|scope| scope.0)?;
            Rc::clone(&foreign[latest])
        };
        // What the *name* means to the macro's environment and to the root
        // here is the same for every reference in a form, so it is asked once
        // per name. What is asked per reference is only whether a frame in
        // between has the spelling — in which case it is somebody's variable.
        let macro_env = def_env.env_id();
        let known = self
            .early
            .imports
            .borrow()
            .get(&macro_env)
            .and_then(|names| names.get(&**name).cloned());
        let import = known.unwrap_or_else(|| {
            let import = self.import_of(name, &def_env);
            self.early
                .imports
                .borrow_mut()
                .entry(macro_env)
                .or_default()
                .insert(Rc::clone(name), import.clone());
            import
        });
        let (location, alias) = import?;
        if self.env.binding_location(name).as_ref() != Some(&location) {
            return None;
        }
        let mut bound = self.early.bound.borrow_mut();
        if !bound.contains_key(&alias) {
            bound.insert(Rc::clone(&alias), Rc::clone(name));
        }
        Some(alias)
    }

    /// The location `name` means to `def_env`, with the root's alias for it,
    /// when the root here imports that very location.
    fn import_of(
        &self,
        name: &Rc<str>,
        def_env: &Rc<Environment>,
    ) -> Option<(patina_core::BindingLocation, Rc<str>)> {
        let target_env = self.env.root();
        let location = def_env.binding_location(name)?;
        if target_env.binding_location(name).as_ref() != Some(&location) {
            return None;
        }
        let alias = target_env.import_alias(name, || Rc::from(alias_name(name)))?;
        Some((location, alias))
    }

    /// A definition whose name carries scopes, for `settle_early_bindings`.
    ///
    /// That is every definition a macro introduced — and also every internal
    /// `define` a program wrote, since a body scopes the names it defines.
    /// Those are recorded too, which is the conservative choice and not a
    /// proven necessity: a recorded definition that is no reference's
    /// candidate changes nothing, and costs a walk of the form only when the
    /// form also bound something early. Recording just the ones that carry a
    /// foreign expansion's scope would save that walk; review of #444 argued
    /// it is unsafe, because a body's scope is added to names *by spelling*
    /// and might reach a template's reference. In the plain case it does not
    /// — a library template's `vector-length` inside a body that defines its
    /// own keeps only its expansion's scope, and answers the library's, as
    /// chibi and Gauche do — but the general claim was not settled either
    /// way, so the filter was not taken.
    fn note_introduced_definition(&self, name: &Rc<str>, scopes: &ScopeSet) {
        if !scopes.is_empty() {
            self.early
                .introduced
                .borrow_mut()
                .push((Rc::clone(name), scopes.clone()));
        }
    }

    /// Take back the early bindings a finished top-level form proved wrong.
    /// (`EarlyBinding::finish_form` is what forgets the form.)
    ///
    /// One shape is not knowable when a reference is emitted: the *same
    /// expansion* also introducing a top-level definition of the name — a
    /// definer whose private `count` happens to share a spelling with an
    /// import of the use site's. That definition is in no environment until
    /// it runs, so nothing's scopes select the reference yet, and it looks
    /// headed for the import. It is known by the end of the form, since an
    /// expansion's output lies within one form: every introduced definition
    /// has been emitted, and a reference whose scopes one of them is a
    /// candidate for goes back to the spelling it was written with.
    ///
    /// A form that defines nothing under scopes, or binds nothing early, is
    /// returned untouched without being walked, and that is nearly all of
    /// them: 2 walks in chibi's suite's 3,980 forms, measured during review.
    /// The rest — a form with an internal `define` *and* an early-bound
    /// reference, which ordinary code does have — is rebuilt node by node,
    /// once.
    fn settle_early_bindings(expr: CoreExpr, (introduced, bound): FinishedForm) -> CoreExpr {
        if introduced.is_empty() || bound.is_empty() {
            return expr;
        }
        fn settle(
            expr: &CoreExpr,
            bound: &FxHashMap<Rc<str>, Rc<str>>,
            introduced: &[(Rc<str>, ScopeSet)],
        ) -> CoreExpr {
            let written = |alias: &Rc<str>, scopes: &ScopeSet| {
                let name = bound.get(alias)?;
                introduced
                    .iter()
                    .any(|(defined, at)| {
                        defined == name && patina_core::scope_resolve::is_candidate(at, scopes)
                    })
                    .then(|| Rc::clone(name))
            };
            let mut settled = expr.map_children(|child| settle(child, bound, introduced));
            match &mut settled.kind {
                CoreExprKind::Var { name, scopes } => {
                    if let Some(original) = written(name, scopes) {
                        *name = original;
                    }
                }
                CoreExprKind::Set { var, scopes, .. } => {
                    if let Some(original) = written(var, scopes) {
                        *var = original;
                    }
                }
                _ => {}
            }
            settled
        }
        settle(&expr, &bound, &introduced)
    }

    /// The alias for `tv`, if it is a reference *this expansion introduced*
    /// and the definition site's binding for its spelling was aliased.
    ///
    /// Only an identifier carrying the expansion's scope qualifies. The
    /// expander puts that scope on everything the template introduced and on
    /// nothing that arrived through a pattern variable — a user's symbol
    /// comes out of the expansion as an identifier too, but one *without*
    /// this scope, since the input flip put it there and the output flip
    /// took it off. So the `list` a template writes is renamed and the
    /// `(list 1 2 3)` the user wrote inside the macro call is not — they can
    /// mean different procedures, and under SRFI 101, where the program's
    /// `list` builds random-access lists and `(chibi test)`'s builds pairs,
    /// they do. Renaming by spelling alone rewrote the user's as well; that
    /// is Larceny family 35.
    fn introduced_alias(
        &self,
        tv: TaggedValue,
        renames: &Renames<'_>,
        shared_heap: &SharedHeap,
    ) -> Option<TaggedValue> {
        let heap = shared_heap.borrow();
        let (name, scopes) = heap.get_identifier_data(tv)?;
        if !scopes.contains(&renames.expansion_scope) {
            return None;
        }
        match renames.aliases.get(&**name)? {
            Aliases::Sole(alias) => Some(*alias),
            // Which mention this occurrence is: the output flip added the
            // expansion's scope to what the template held, and nothing else
            // has touched it yet.
            Aliases::PerIdentity(aliases) => {
                let identity = scopes.without_scope(renames.expansion_scope);
                aliases
                    .iter()
                    .find(|(mention, _)| *mention == identity)
                    .map(|(_, alias)| *alias)
            }
        }
    }

    /// Does this list head *resolve* to `quote`?
    ///
    /// Asked by resolution, not spelling, because after relinking a head can
    /// be spelled `quote.7` and still be `quote`, and under an import that
    /// rebinds `quote` a head spelled `quote` can be a macro. Reached only
    /// for a head [`is_quote_spelling`] admits — that test is answered from
    /// a `&str` the caller already holds, so an ordinary call pays neither
    /// the name clone this makes nor the environment walk.
    fn head_resolves_to_quote(&self, car: TaggedValue, shared_heap: &SharedHeap) -> bool {
        let Some((name, scopes)) = self.identifier_of(car, shared_heap) else {
            return false;
        };
        // An ambiguous head is not `quote`, and saying so is not a guess
        // about the program: the form is walked as an ordinary one and the
        // dispatch raises when it resolves the same head.
        matches!(
            self.resolve_syntax(&name, &scopes),
            Ok(Some(SyntaxRef::CoreSyntax(CoreForm::Quote)))
        )
    }

    /// Rewrite one *form* — a list read the way the evaluator reads it, with a
    /// head that may be a quoting operator.
    ///
    /// The spine is walked here rather than by recursing on each cdr, and that
    /// is the whole point. A cdr is a *tail*, not a form: in `(f quote x)` the
    /// tail is `(quote x)`, so re-reading it as a form saw a quote and treated
    /// everything after the argument `quote` as inert data. Head position is
    /// decided once, here.
    fn rewrite_form(
        &self,
        tv: TaggedValue,
        renames: &Renames<'_>,
        quote_depth: u32,
        shared_heap: &SharedHeap,
    ) -> TaggedValue {
        // Read the head once, inside the borrow: both answers are `Copy`, so
        // nothing needs to outlive it and the head name is never copied out.
        let (car, cdr, rest_depth, head_spelled_quote) = {
            let heap = shared_heap.borrow();
            let (car, cdr) = heap.get_pair(tv);
            let name = heap.get_symbol_or_identifier_name(car);
            let depth = match name {
                Some("quasiquote") => quote_depth + 1,
                Some("unquote") | Some("unquote-splicing") => quote_depth.saturating_sub(1),
                _ => quote_depth,
            };
            (car, cdr, depth, name.is_some_and(is_quote_spelling))
        };

        // A quoted datum denotes itself, so nothing *inside* it is a reference
        // — but the head is one, and it is the occurrence most in need of
        // relinking. A library may export its own `quote`, as SRFI 101 does,
        // and then the use site resolves a template's `quote` to that macro
        // rather than to the `quote` the template meant. Returning the form
        // untouched here is what let the expansion of `(quote x)` contain a
        // `quote` that expanded again, without end — the import half of
        // Larceny family 33. Rewrite the head; leave the datum alone.
        //
        // Only at depth zero. Inside a quasiquote a `(quote b)` is data —
        // two symbols — and an `unquote` within it is still evaluated, so
        // there the form is walked like any other list: its head is a plain
        // symbol the leaf rule leaves alone, and its `,(helper x)` is
        // reached.
        if quote_depth == 0 && head_spelled_quote && self.head_resolves_to_quote(car, shared_heap) {
            let Some(renamed) = self.introduced_alias(car, renames, shared_heap) else {
                return tv;
            };
            return shared_heap.borrow_mut().alloc_pair(renamed, cdr);
        }

        // Flatten the spine so the head can be told from the arguments. The
        // tail is whatever ends the list — `()` for a proper one, an atom for
        // a dotted one.
        // A circular spine is left as it is (#459), like any cycle
        // `rewrite_refs` meets: the desugarer refuses it as code.
        let Some((mut elems, tail)) = list_to_vec_with_tail_tagged(tv, &shared_heap.borrow())
        else {
            return tv;
        };

        // `` `(a . ,e) `` reads as `(quasiquote (a unquote e))`: the unquote
        // keyword sits in the spine's *interior*, in cdr position, and governs
        // the one element after it. Head position alone cannot see that, so
        // the walk above rewrote `e` a level too deep and a library-private
        // helper inside it was never relinked.
        //
        // The dual of the case #68 fixed, and guarded the same way: outside a
        // template `(f unquote x)` is an ordinary call whose second argument
        // happens to be spelled `unquote`, so this fires only at
        // `quote_depth > 0`, and only for the `(… unquote x)` shape that ends
        // the list — which is the only shape a dotted unquote can take.
        let dotted_unquote = (quote_depth > 0 && tail.is_null() && elems.len() >= 3)
            .then(|| {
                let heap = shared_heap.borrow();
                let i = elems.len() - 2;
                matches!(
                    heap.get_symbol_or_identifier_name(elems[i]),
                    Some("unquote") | Some("unquote-splicing")
                )
                .then_some(i + 1)
            })
            .flatten();

        let mut changed = false;
        for (i, e) in elems.iter_mut().enumerate() {
            // The head is read at the enclosing depth; a quoting head governs
            // its arguments, not itself.
            let depth = if i == 0 {
                quote_depth
            } else if Some(i) == dotted_unquote {
                quote_depth.saturating_sub(1)
            } else {
                rest_depth
            };
            let new_e = self.rewrite_refs(*e, renames, depth, shared_heap);
            changed |= new_e != *e;
            *e = new_e;
        }
        let new_tail = self.rewrite_refs(tail, renames, rest_depth, shared_heap);
        changed |= new_tail != tail;

        if !changed {
            return tv;
        }
        shared_heap
            .borrow_mut()
            .list_from_iter_with_tail(elems, new_tail)
    }

    /// Substitute `renames` into evaluated positions of `tv`, for the
    /// identifiers this expansion introduced (`introduced_alias`).
    ///
    /// `quote_depth` tracks quasiquotation: names inside quoted data denote
    /// themselves, not bindings, so rewriting them there would corrupt the
    /// datum. Depth rises through `quasiquote` and falls through `unquote` /
    /// `unquote-splicing`; a `quote` form has only its head rewritten.
    fn rewrite_refs(
        &self,
        tv: TaggedValue,
        renames: &Renames<'_>,
        quote_depth: u32,
        shared_heap: &SharedHeap,
    ) -> TaggedValue {
        if tv.is_pair() || tv.is_vector() {
            // An expansion can be circular: the reader accepts datum labels,
            // and a macro passes what it was given (#459). A pair or vector
            // met again inside itself is left as it is, rather than walked
            // until the stack overflows. As code the desugarer refuses it; as
            // quoted data it needs no relinking.
            if !renames.open.borrow_mut().enter(tv) {
                return tv;
            }
            let rewritten = if tv.is_pair() {
                self.rewrite_form(tv, renames, quote_depth, shared_heap)
            } else {
                self.rewrite_vector(tv, renames, quote_depth, shared_heap)
            };
            renames.open.borrow_mut().leave();
            return rewritten;
        }

        if quote_depth > 0 {
            return tv;
        }
        self.introduced_alias(tv, renames, shared_heap)
            .unwrap_or(tv)
    }

    /// [`Self::rewrite_refs`] for a vector.
    ///
    /// Vector elements are evaluated inside quasiquote, so they have to be
    /// walked. A bare `#(...)` is self-evaluating data and is protected by
    /// `quote_depth` like anything else.
    fn rewrite_vector(
        &self,
        tv: TaggedValue,
        renames: &Renames<'_>,
        quote_depth: u32,
        shared_heap: &SharedHeap,
    ) -> TaggedValue {
        let elems = shared_heap.borrow().vector_slice(tv).to_vec();
        let mut out = Vec::with_capacity(elems.len());
        let mut changed = false;
        for e in elems {
            let new_e = self.rewrite_refs(e, renames, quote_depth, shared_heap);
            changed |= new_e != e;
            out.push(new_e);
        }
        if !changed {
            return tv;
        }
        shared_heap.borrow_mut().alloc_vector(out)
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
        // The entry point, and only that: everything inside a form recurses
        // through `desugar_form`. So returning from here is the end of a
        // top-level form — where early bindings are settled
        // (`settle_early_bindings`) and the form forgotten, whether or not it
        // desugared. Recursing through here instead would forget a form
        // halfway through it, which is what the assertion is for.
        debug_assert!(
            self.early.is_idle() && self.open_forms.borrow().is_empty(),
            "`desugar_tagged` is the per-form entry point; recurse through `desugar_form`"
        );
        let result = self.desugar_form(tagged, shared_heap);
        let finished = self.early.finish_form();
        result.map(|expr| Self::settle_early_bindings(expr, finished))
    }

    /// One form, and what every form recurses through. Internal recursion
    /// comes here rather than through `desugar_tagged`, whose bookkeeping is
    /// per top-level form and was a measurable cost per *node*.
    fn desugar_form(&self, tagged: TaggedValue, shared_heap: &SharedHeap) -> Result<CoreExpr> {
        let _phase = patina_core::scope_trace::enter(patina_core::scope_trace::Phase::Desugar);
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
            let name: Rc<str> = Rc::from(name);
            let scopes = ScopeSet::new();
            self.reject_syntax_as_value(&name, &scopes)?;
            return Ok(CoreExpr::new(CoreExprKind::Var { name, scopes }));
        }

        // Identifier - variable reference with scopes (for hygiene)
        if let Some((name, scopes)) = utils::get_identifier_info(tagged, &heap) {
            drop(heap);
            let by_name = self.checked_reference(&name, &scopes)?;
            let name = self.early_bound(&name, &scopes, by_name).unwrap_or(name);
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
            // An error from inside this form is placed here unless a form
            // nested in it already placed it (#432). Every compound form comes
            // through this line, so this is where a desugar error gets the
            // position no raising site had to pass along.
            let mut expr = self
                .desugar_open_form(tagged, shared_heap)
                .map_err(|e| e.at_opt(source.clone()))?;
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

    /// [`Self::desugar_list_tagged`], for a form that is not circular.
    ///
    /// The reader accepts datum labels, so a program can hold a form whose
    /// spine comes back on itself, `(begin . #0=(1 . #0#))`, or one that
    /// contains itself, `#0=(list 1 #0#)`. Neither is code — R7RS 2.4 allows a
    /// cycle only in a literal — and each walk of the first collected until
    /// memory ran out while the second recursed until the stack overflowed
    /// (#459). Checked here, where every compound form comes through, as
    /// Gauche checks `list?` at the head of each form and chibi `sexp_listp`.
    /// A circular literal is untouched: `quote` does not desugar its datum.
    fn desugar_open_form(&self, form: TaggedValue, shared_heap: &SharedHeap) -> Result<CoreExpr> {
        if shared_heap.borrow().spine_is_circular(form) {
            return Err(DesugarError::InvalidSyntax(format!(
                "a form must be a proper list, not a circular one: {}",
                patina_core::debug_format::format_tagged(form, &shared_heap.borrow())
            )));
        }
        if !self.open_forms.borrow_mut().enter(form) {
            return Err(DesugarError::InvalidSyntax(format!(
                "a form cannot contain itself: {}",
                patina_core::debug_format::format_tagged(form, &shared_heap.borrow())
            )));
        }
        let desugared = self.desugar_list_tagged(form, shared_heap);
        self.open_forms.borrow_mut().leave();
        desugared
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

        // Step 2: Extract operator name and the scopes it was written with
        // (immutable access only). The scopes used to be discarded here and the
        // head looked up without them, while value position looked up *with*
        // them — one of the two ways the two lookups had drifted apart before
        // `resolve_syntax` merged them.
        let (name, head_scopes) = {
            let heap = shared_heap.borrow();
            if let Some(s) = heap.get_symbol_name(car) {
                (Some(Rc::from(s)), ScopeSet::new())
            } else if let Some((id_name, id_scopes)) = utils::get_identifier_info(car, &heap) {
                (Some(id_name), id_scopes)
            } else {
                (None, ScopeSet::new())
            }
        };
        // Immutable borrow released

        // Step 3: Resolve the head symbol, once, for everything a binding can
        // make it mean.
        //
        // A `syntax-rules` macro and a core syntactic keyword are both syntax
        // that the head names, and both are ordinary bindings, so one lookup
        // must find whichever the environment holds. Asking only about macros
        // here is what let `(define-syntax if …)` take effect while
        // `(define (if a b c) …)` did not: the macro was looked up and the
        // procedure was not, so the name match below still claimed the form.
        //
        // `resolve_syntax` is shared with value position and `set!`, so all
        // three agree on what counts as syntax. A name bound to anything else
        // answers `None` here and falls through to an ordinary application,
        // which is what lets a definition shadow a keyword.
        let (macro_to_expand, core_form) = match &name {
            Some(sym) => match self.resolve_syntax(sym, &head_scopes)? {
                Some(SyntaxRef::Macro(m)) => (Some(m), None),
                Some(SyntaxRef::CoreSyntax(form)) => (None, Some(form)),
                None => (None, None),
            },
            _ => (None, None),
        };

        // Handle macro expansion. `expand_macro_with_scope` takes and returns
        // TaggedValues directly, and also hands back the scope this expansion
        // minted, which the relinker below needs.
        if let Some(compiled_macro) = macro_to_expand {
            // Save call-site source location before expansion
            let call_site_source = self.lookup_source(list);

            let patina_macros::MacroExpansion {
                form: expanded_tagged,
                scope: expansion_scope,
            } = patina_macros::expand_macro_with_scope(
                &compiled_macro,
                list, // Pass TaggedValue directly
                shared_heap,
                // Where an input identifier resolves when it meets a literal.
                // R7RS §4.3.2 matches the two by binding, so the input is
                // looked up here, standing in the scopes a reference written
                // here stands in.
                Some(patina_macros::Site {
                    env: &self.env,
                    scopes: &self.current_scopes,
                }),
            )
            .map_err(|e| match e {
                // Refused as every resolution the rule does not determine is,
                // not reported as a failed expansion.
                patina_macros::MacroError::AmbiguousReference(message) => {
                    DesugarError::AmbiguousReference(message)
                }
                // Every `syntax-rules` rule refused the form. The expander's
                // message says so; wrapping it as "Macro expansion failed:
                // Invalid syntax: …" said "Invalid syntax" twice and nothing
                // more (#432).
                patina_macros::MacroError::NoMatchingPattern(name) => DesugarError::InvalidSyntax(
                    format!("no `syntax-rules` pattern of `{name}` matches this use"),
                ),
                other => DesugarError::InvalidSyntax(format!("Macro expansion failed: {}", other)),
            })?;

            // Referential transparency: a template's free identifiers denote what
            // they were bound to where the macro was *defined*. Link any that the
            // use site cannot resolve back to the definition environment before
            // desugaring, otherwise they become bare global loads here and fail.
            let expanded_tagged = self.link_definition_env_refs(
                expanded_tagged,
                expansion_scope,
                &compiled_macro,
                shared_heap,
            );

            // Phase 4: stamp expanded pairs + record macro expansion chain
            if let (Some(src), Some(sm)) = (&call_site_source, &self.source_map) {
                stamp_expansion_source(expanded_tagged, src, sm, shared_heap, 0);
                sm.borrow_mut()
                    .record_expansion(src, compiled_macro.name.to_string());
            }

            // Result is already TaggedValue - continue desugaring
            let mut expr = self.desugar_form(expanded_tagged, shared_heap)?;
            // Use call-site source as fallback if the expanded form has no source
            if expr.source.is_none() {
                expr.source = call_site_source;
            }
            return Ok(expr);
        }

        // A core syntactic keyword, reached through its binding. Dispatch on
        // the *form* rather than on the name it was reached by, which is what
        // makes an import rename work: after `(rename (begin blk))`, `(blk 1 2)`
        // arrives here as `Begin`.
        if let Some(form) = core_form {
            return self.desugar_core_form(form, cdr, shared_heap);
        }

        // `(apply f args)` is lowered to `CoreExpr::Apply`, an optimisation,
        // where the head is bound to the `apply` primitive itself — keyed on
        // the binding, as `core_form` above is (#443). A program's own
        // `(define (apply f xs) …)` is an ordinary call now, where the
        // spelling alone used to lower it and ignore the definition; so is a
        // local `apply`, which resolves to its own binding and not the
        // primitive. The spelling stays as a filter in front: resolving every
        // head's value would cost each application a second lookup, and a
        // head spelled otherwise was never lowered.
        if let Some(sym) = &name
            && sym.as_ref() == APPLY
            && self.head_is_the_apply_primitive(sym, &head_scopes)
        {
            return self.desugar_apply_tagged(list, cdr, shared_heap);
        }

        // Regular application
        self.desugar_app_tagged(list, shared_heap)
    }

    /// Desugar a core syntactic keyword in head position.
    ///
    /// The single dispatch point for the forms the desugarer implements. It
    /// takes a [`CoreForm`] rather than a name so that a keyword reached under
    /// an import rename or a prefix lands in the same arm as one spelled out.
    fn desugar_core_form(
        &self,
        form: CoreForm,
        cdr: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        match form {
            CoreForm::Quote => self.desugar_quote_tagged(cdr, shared_heap),
            CoreForm::Quasiquote => self.desugar_quasiquote_tagged(cdr, shared_heap),
            CoreForm::Lambda => self.desugar_lambda_tagged(cdr, shared_heap),
            CoreForm::If => self.desugar_if_tagged(cdr, shared_heap),
            CoreForm::Set => self.desugar_set_tagged(cdr, shared_heap),
            CoreForm::Define => self.desugar_define_tagged(cdr, shared_heap),
            CoreForm::DefineSyntax => self.desugar_define_syntax_tagged(cdr, shared_heap),
            CoreForm::LetSyntax => self.desugar_let_syntax_tagged(cdr, shared_heap),
            CoreForm::LetrecSyntax => self.desugar_letrec_syntax_tagged(cdr, shared_heap),
            CoreForm::Begin => self.desugar_begin_tagged(cdr, shared_heap),
            CoreForm::Import => self.desugar_import_tagged(cdr, shared_heap),
            CoreForm::CondExpand => self.desugar_cond_expand_tagged(cdr, shared_heap),
            CoreForm::Include => self.desugar_include_tagged(cdr, shared_heap, false),
            CoreForm::IncludeCi => self.desugar_include_tagged(cdr, shared_heap, true),
            CoreForm::SyntaxError => self.desugar_syntax_error_tagged(cdr, shared_heap),
            CoreForm::Expand => self.desugar_expand_tagged(cdr, shared_heap),

            // Auxiliary keywords mean something only inside an enclosing form:
            // `else` inside `cond`, `unquote` inside a template, `syntax-rules`
            // inside `define-syntax`. In head position each is a mistake, and
            // saying which beats reporting that a symbol is not a procedure —
            // which is what `(else 1)` used to report, because `base.sld` bound
            // `else` to the symbol `'else` to get it through an import set.
            //
            // A catch-all rather than the seven variants spelled out again:
            // `CoreForm::is_dispatching` is the one place that classification
            // lives, and listing it twice is how the two come to disagree. The
            // cost is that a *new* dispatching form with no arm above lands
            // here instead of failing to compile — which the assertion turns
            // into a loud failure across the test suite.
            form => {
                debug_assert!(
                    !form.is_dispatching(),
                    "`{form}` is classified as dispatching but has no arm in desugar_core_form"
                );
                Err(DesugarError::InvalidSyntax(format!(
                    "invalid use of auxiliary syntax: {}",
                    form
                )))
            }
        }
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

        // Create a fresh scope for this lambda's bindings
        // This is used for:
        // 1. Non-macro parameters (they have no scopes, need fresh scope)
        // 2. let-syntax inside the lambda (to capture lexical context)
        let binding_scope = ScopeId::fresh();
        let body_scopes = self.current_scopes.with_scope(binding_scope);

        // Desugar body with:
        // 1. The new scope set (for hygiene)
        // 2. The parameters, which shadow outer bindings of those names
        // 3. The body's own internal definitions, which bind over all of it
        let binders = utils::formals_to_binders(&params);
        let body_tvs = Self::scope_body(&binders, &body_tvs, binding_scope, shared_heap);
        let body_desugarer = self.enter_binding_form(binders, binding_scope);

        let defined = body_desugarer.body_definition_names(&body_tvs, shared_heap);
        let body_tvs = Self::scope_body(&defined, &body_tvs, binding_scope, shared_heap);
        let body_desugarer = body_desugarer.enter_binding_form(defined, binding_scope);
        let (body_desugarer, body_tvs) =
            body_desugarer.enter_produced_definitions(body_tvs, binding_scope, shared_heap);

        // Desugar body expressions with internal define-syntax handling
        let body =
            self.desugar_body_tagged(&body_desugarer, &body_tvs, shared_heap, &body_scopes)?;

        Ok(CoreExpr::new(CoreExprKind::Lambda {
            params,
            body,
            // The set the body was desugared with, unconditionally — the
            // `define` shorthand says the same thing at its own `Lambda`.
            // There used to be a whole-lambda `has_macro_scopes` gate that
            // zeroed this when *any* parameter carried scopes, but both
            // readers already dispatch per parameter on `ScopedParam::scopes`
            // before looking here, so the gate only ever spoke for the mixed
            // case — and there it silently disagreed with the shorthand.
            binding_scopes: Rc::new(body_scopes),
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
        let env = initial_desugarer.env.clone();

        let mut body_exprs = Vec::new();
        let mut current_env = env.clone();
        let mut current_desugarer = initial_desugarer.with_new_env(env, body_scopes.clone());

        for tv in body_tvs {
            // Check if this is a define-syntax form BEFORE desugaring.
            // Asked of `current_desugarer`, not `self`: the answer depends on
            // what `define-syntax` is bound to *here*, which the body's own
            // shadows and earlier internal macros can change.
            let define_syntax_info =
                current_desugarer.try_parse_define_syntax_tagged(*tv, shared_heap);

            if let Some((macro_name, binder_scopes, transformer_tv)) = define_syntax_info {
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
                current_desugarer.define_syntax_binding(&new_env, macro_name, binder_scopes, tv);

                current_env = new_env.clone();
                current_desugarer = current_desugarer.with_new_env(new_env, body_scopes.clone());
            } else {
                let desugared = current_desugarer.desugar_form(*tv, shared_heap)?;

                // Every expression is kept, including one that desugars to
                // `Literal(Unspecified)`.
                //
                // This used to drop those, to discard the placeholder a
                // `define-syntax` leaves behind — but that arm is the `if let`
                // above, which never reaches here, so the filter only ever hit
                // *real* expressions that happen to evaluate to unspecified. A
                // bare `(begin)` is one, and dropping it was wrong twice over:
                // as a body's only form it produced "Body must contain at least
                // one expression", and as a body's *last* form it silently
                // handed the body the previous expression's value, where R7RS
                // 4.1.4 says a body's value is its last expression's.
                body_exprs.push(desugared);
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
    /// Returns (macro_name, binder_scopes, transformer_tv) if the TaggedValue is a
    /// (define-syntax name transformer) form. Works directly with TaggedValue.
    fn try_parse_define_syntax_tagged(
        &self,
        tagged: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Option<(Rc<str>, ScopeSet, TaggedValue)> {
        // Must be a pair
        if !tagged.is_pair() {
            return None;
        }

        // Recognized through its binding, not its spelling — the class #88
        // removed for every other keyword, and the last body-position site
        // still deciding by name (audit F4). A shadowed `define-syntax` is an
        // ordinary call, and one reached under an import rename still defines
        // a macro.
        let head = {
            let heap = shared_heap.borrow();
            heap.get_pair(tagged).0
        };
        let is_define_syntax = self
            .identifier_of(head, shared_heap)
            .and_then(|(name, scopes)| self.resolve_syntax(&name, &scopes).ok().flatten())
            .is_some_and(|found| matches!(found, SyntaxRef::CoreSyntax(CoreForm::DefineSyntax)));
        if !is_define_syntax {
            return None;
        }

        let heap = shared_heap.borrow();
        let (_, cdr) = heap.get_pair(tagged);

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

        let (macro_name, binder_scopes) = utils::symbol_or_identifier(name_tv, &heap)?;

        Some((macro_name, binder_scopes, transformer_tv))
    }

    /// Desugar if using TaggedValue
    fn desugar_if_tagged(&self, args: TaggedValue, shared_heap: &SharedHeap) -> Result<CoreExpr> {
        let args_vec = utils::list_to_vec_tagged(args, shared_heap)?;

        match args_vec.len() {
            2 => {
                let test = self.desugar_form(args_vec[0], shared_heap)?;
                let then = self.desugar_form(args_vec[1], shared_heap)?;
                Ok(CoreExpr::new(CoreExprKind::If {
                    test: Rc::new(test),
                    then: Rc::new(then),
                    else_: CoreExpr::rc(CoreExprKind::Literal(TaggedValue::UNSPECIFIED)),
                }))
            }
            3 => {
                let test = self.desugar_form(args_vec[0], shared_heap)?;
                let then = self.desugar_form(args_vec[1], shared_heap)?;
                let else_ = self.desugar_form(args_vec[2], shared_heap)?;
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

        // `set!` refuses syntax for the same reason a value reference does, and
        // by the same call. R7RS §5.3.1 licenses a *definition* over a
        // syntactic keyword — "if ⟨variable⟩ is not bound, or is a syntactic
        // keyword, then the definition will bind ⟨variable⟩ to a new location"
        // — and says nothing of the sort for `set!`, whose ⟨variable⟩ must
        // already be one (§4.1.6, §3.1). chibi rejects `(set! if 5)` with the
        // same message it gives for `(list if)`; Gauche accepts it and then
        // breaks inside its own startup code. Without this, reading syntax was
        // an error while overwriting it silently succeeded.
        let by_name = self.checked_reference(&name, &scopes)?;
        let name = self.early_bound(&name, &scopes, by_name).unwrap_or(name);

        let value = self.desugar_form(args_vec[1], shared_heap)?;

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
            let (name, name_scopes, formals_tv) =
                utils::parse_define_function_tagged(first, shared_heap)?;
            let body_tvs: Vec<_> = args_vec[1..].to_vec();

            if body_tvs.is_empty() {
                return Err(DesugarError::EmptyBody("define".to_string()));
            }

            let params = utils::convert_formals_tagged(formals_tv, shared_heap)?;

            // Create a fresh binding scope for this lambda, and give the body
            // the scopes that include it.
            //
            // The shorthand is a lambda, and its body has to be scoped like
            // one. Taking `self.current_scopes` unchanged left the set *empty*
            // at top level, and an empty scope set is not a narrow scope but no
            // scope at all: `Environment::insert_scoped` routes it to a plain
            // `define`, so `(define (f if) …)` installed a name-visible global
            // `if` that shadowed the special form for every macro-introduced
            // reference as well as its own body. `a_generated_macro_keeps_its_
            // keywords_under_a_shorthand_parameter` pins it.
            let binding_scope = ScopeId::fresh();
            let body_scopes = self.current_scopes.with_scope(binding_scope);

            // Create the body's desugarer, binding the formals and the body's
            // own internal definitions (see `body_definition_names`).
            let binders = utils::formals_to_binders(&params);
            let body_tvs = Self::scope_body(&binders, &body_tvs, binding_scope, shared_heap);
            let body_desugarer = self.enter_binding_form(binders, binding_scope);

            let defined = body_desugarer.body_definition_names(&body_tvs, shared_heap);
            let body_tvs = Self::scope_body(&defined, &body_tvs, binding_scope, shared_heap);
            let body_desugarer = body_desugarer.enter_binding_form(defined, binding_scope);
            let (body_desugarer, body_tvs) =
                body_desugarer.enter_produced_definitions(body_tvs, binding_scope, shared_heap);

            let body: Vec<CoreExpr> = body_tvs
                .iter()
                .map(|tv| body_desugarer.desugar_form(*tv, shared_heap))
                .collect::<Result<Vec<_>>>()?;

            self.note_introduced_definition(&name, &name_scopes);
            return Ok(CoreExpr::new(CoreExprKind::Define {
                name,
                scopes: name_scopes,
                value: CoreExpr::rc(CoreExprKind::Lambda {
                    params,
                    body,
                    binding_scopes: Rc::new(body_scopes.clone()),
                }),
            }));
        }

        // Simple variable define: (define name value)
        //
        // The scope set travels with the name. A macro-introduced binding is
        // only distinguishable from the same name introduced by another
        // expansion by its scopes, so dropping them here is what made a
        // recursive macro's per-element temporaries collapse onto one.
        let (name, name_scopes) = self.identifier_of(first, shared_heap).ok_or_else(|| {
            DesugarError::InvalidSyntax("define requires a symbol as first argument".to_string())
        })?;

        if args_vec.len() != 2 {
            return Err(DesugarError::WrongArgCount {
                form: "define".to_string(),
                expected: "2".to_string(),
                got: args_vec.len(),
            });
        }

        let value_tv = args_vec[1];
        let value = self.desugar_form(value_tv, shared_heap)?;

        self.note_introduced_definition(&name, &name_scopes);
        Ok(CoreExpr::new(CoreExprKind::Define {
            name,
            scopes: name_scopes,
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
            .map(|tv| self.desugar_form(*tv, shared_heap))
            .collect::<Result<Vec<_>>>()?;

        Ok(CoreExpr::new(CoreExprKind::Begin(body)))
    }

    /// Whether the head `name`, written with `scopes`, is bound to the `apply`
    /// primitive — what lets `(apply f args)` be lowered to `CoreExpr::Apply`
    /// (#443). Asked of the value, so an import rename or prefix of the real
    /// `apply` still is it, and anything else of that spelling is not.
    fn head_is_the_apply_primitive(&self, name: &str, scopes: &ScopeSet) -> bool {
        // As `resolve_reference` does: a head written in source stands in the
        // scopes the desugarer has accumulated, so a local `apply` is seen.
        let scopes = if scopes.is_empty() {
            &self.current_scopes
        } else {
            scopes
        };
        let Ok((Some(value), _)) = self.env.resolve_with_scopes(name, scopes) else {
            return false;
        };
        let heap = self.env.heap().borrow();
        heap.get_procedure(value).is_some_and(|procedure| {
            matches!(
                procedure.as_ref(),
                patina_core::procedure::Procedure::Primitive { name: APPLY, .. }
            )
        })
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

        let func = self.desugar_form(args_vec[0], shared_heap)?;
        let operands: Vec<CoreExpr> = args_vec[1..]
            .iter()
            .map(|tv| self.desugar_form(*tv, shared_heap))
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

        let func = self.desugar_form(exprs[0], shared_heap)?;
        let operands: Vec<CoreExpr> = exprs[1..]
            .iter()
            .map(|tv| self.desugar_form(*tv, shared_heap))
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

    /// Desugar quasiquote: `(quasiquote template)` → `Quasiquote`, holding
    /// what the template builds with its unquoted expressions desugared here,
    /// in this form (`quasiquote.rs`).
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
        let template = self.derive_quasi_template(args_vec[0], shared_heap)?;
        Ok(CoreExpr::new(CoreExprKind::Quasiquote(template)))
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

        let Some((name, binder_scopes)) = self.identifier_of(args_vec[0], shared_heap) else {
            return Err(DesugarError::InvalidSyntax(
                "define-syntax requires (define-syntax name transformer)".to_string(),
            ));
        };

        // Compile macro immediately and install in environment
        let env = &self.env;

        let compiled_macro = self.compile_syntax_rules_tagged(
            args_vec[1],
            shared_heap,
            name.clone(),
            env,
            &self.current_scopes,
        )?;

        // Install in environment
        let tv = env.heap().borrow_mut().alloc_macro(Rc::new(compiled_macro));
        self.define_syntax_binding(env, name, binder_scopes, tv);

        Ok(CoreExpr::new(CoreExprKind::Literal(
            TaggedValue::UNSPECIFIED,
        )))
    }

    /// Both written body forms and expanded `define-syntax` forms bind the
    /// keyword at its own scopes (#269). A caller-supplied name with no
    /// scopes stands in the body's context, just like a source reference.
    /// Keep the existing name-visible top-level behaviour separate (#427).
    fn define_syntax_binding(
        &self,
        env: &Environment,
        name: Rc<str>,
        binder_scopes: ScopeSet,
        value: TaggedValue,
    ) {
        if self.current_scopes.is_empty() {
            env.define(name, value);
        } else {
            let scopes = if binder_scopes.is_empty() {
                self.current_scopes.clone()
            } else {
                binder_scopes
            };
            env.define_with_scopes(name, scopes, value);
        }
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
            expr: Rc::new(self.desugar_form(args_vec[0], shared_heap)?),
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
        let env = &self.env;

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

        // Determine compilation environment, and the scopes the transformers
        // are written in.
        //
        // R7RS §4.3.1: only `letrec-syntax` makes the keywords it binds visible
        // inside its own transformers. Under `let-syntax` a transformer is
        // written *outside* them, so its template's free identifiers must
        // resolve where the form appears — at `self.current_scopes`, without
        // the fresh scope — and a sibling keyword is simply not in scope there.
        // Giving both forms the body's scopes is what made `let-syntax`
        // resolve a sibling, which is half of family 23.
        // Both forms compile their transformers in the environment the form
        // appears in. `letrec-syntax` used to get a fresh child here, but the
        // keywords are installed into `body_env` below and never into this one,
        // so the child was empty and looked up identically to its parent — the
        // sibling visibility that distinguishes `letrec-syntax` comes from
        // set-of-scopes resolution against `body_env`, which is why
        // `rec-scope` in `let_syntax_body_definitions_and_transformer_scope`
        // passes. Its one observable effect was that `definition_env` could
        // never equal the use-site environment, so `link_definition_env_refs`
        // ran its whole per-symbol loop for every `letrec-syntax` macro.
        let compile_env = env.clone();
        let transformer_scopes = if is_letrec {
            definition_scopes.clone()
        } else {
            // A distinct fresh scope, not the *absence* of one. What a
            // `let-syntax` transformer must not carry is `let_syntax_scope`,
            // which is what would let its template resolve a sibling keyword;
            // it still needs a scope set of its own, because the template
            // compiler treats an empty one as "no scopes available" and falls
            // back to marks-and-ribs hygiene (`compile_template`'s symbol
            // case), where identifiers introduced by different expansions of
            // one rule collapse into a single identity —
            // `test_generated_template_capture_keeps_expansions_distinct`.
            // Extra scopes on a reference are harmless to resolution: a
            // binding matches when its scopes are a *subset* of the
            // reference's, so this only withholds the one binding it should.
            self.current_scopes.with_scope(ScopeId::fresh())
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

            // The binder's own scopes as written, kept beside its name: when
            // this whole `let-syntax` came out of a template, they are the
            // identity that references introduced by that same expansion carry.
            let Some((name, binder_scopes)) = self.identifier_of(binding_vec[0], shared_heap)
            else {
                return Err(DesugarError::InvalidSyntax(
                    "Macro name must be a symbol".to_string(),
                ));
            };

            // `letrec-syntax` puts its scope on its transformers as well as
            // its body (Racket's `letrec-syntaxes+values` does the same), so
            // a scoped identifier inside a transformer — a sibling keyword a
            // template generated — can reach that keyword's binding. Under
            // `let-syntax` the transformers are outside the form's scope and
            // are left as written.
            let transformer = if is_letrec {
                patina_macros::add_scope_to_scoped_identifiers(
                    binding_vec[1],
                    let_syntax_scope,
                    shared_heap,
                )
            } else {
                binding_vec[1]
            };
            let compiled_macro = self.compile_syntax_rules_tagged(
                transformer,
                shared_heap,
                name.clone(),
                &compile_env,
                &transformer_scopes,
            )?;

            macro_bindings.push((name, binder_scopes, compiled_macro));
        }

        // Create environment with macro bindings.
        //
        // Bound *with* the body's scopes, not unscoped: a local variable is a
        // scoped binding now, so a keyword left unscoped could never outrank an
        // enclosing variable of the same spelling — set-of-scopes resolution
        // prefers the largest scope set the reference contains, and an unscoped
        // binding is only reached when nothing scoped matches at all. Binding
        // the keyword at `definition_scopes` is what makes the inner
        // `let-syntax` win over an outer `(let ((f …)) …)`, which is R7RS
        // §4.3.1 and what `let_syntax_body_definitions_and_transformer_scope`
        // pins.
        let body_env = Rc::new(Environment::with_parent(env.clone()));
        for (name, binder_scopes, compiled_macro) in macro_bindings {
            let tv = body_env
                .heap()
                .borrow_mut()
                .alloc_macro(Rc::new(compiled_macro));
            // Bound at the binder's scopes *as written* plus this form's —
            // Racket's rule for a binder — where a binder written in source
            // carries none of its own and stands in the scopes accumulated
            // so far, exactly as a reference written in source does. A
            // reference reaches the binding when the binding's scopes are a
            // subset of its own, which is: a symbol in the body (resolved
            // with `definition_scopes`); an empty-scoped identifier in the
            // body, which is a user's symbol that passed through a pattern
            // variable and resolves the same way; and a scoped identifier in
            // the body, which `add_scope_to_scoped_identifiers` gave
            // `let_syntax_scope` above. That last case is chibi's `(m k)`:
            // `m`'s template binds `n` and references it as `(n z)`, binder
            // and reference `bound-identifier=?`.
            //
            // What does *not* reach it: a reference a transformer introduces,
            // which carries the transformer's scopes and — under `let-syntax`
            // — never this form's, so a sibling keyword stays invisible
            // (R7RS §4.3.1) and an outer keyword of the same spelling is
            // found instead; and a reference another macro's template
            // introduces into the body, which carries only that expansion's
            // scopes. Binding the keyword unscoped as well, as #124 did,
            // made it reachable from every reference of that spelling, and a
            // `let-syntax ((quote …))` around a *call* captured the `(quote
            // d)` in the callee's template until the stack went — the
            // `let-syntax` half of Larceny family 33.
            let as_written = if binder_scopes.is_empty() {
                &self.current_scopes
            } else {
                &binder_scopes
            };
            body_env.define_with_scopes(
                name.to_string(),
                as_written.with_scope(let_syntax_scope),
                tv,
            );
        }

        let body_desugarer = self.with_new_env(body_env, definition_scopes.clone());

        // The body as written gets this form's scope on every identifier that
        // carries scopes of its own — what Racket does on entering a binding
        // form, and what lets the binding above tell a reference *in* the
        // body from one a transformer will introduce later. A body with no
        // such identifiers, the usual one, comes back unchanged.
        let body_tvs: Vec<TaggedValue> = args_vec[1..]
            .iter()
            .map(|&tv| {
                patina_macros::add_scope_to_scoped_identifiers(tv, let_syntax_scope, shared_heap)
            })
            .collect();

        let desugared_body =
            self.desugar_body_tagged(&body_desugarer, &body_tvs, shared_heap, &definition_scopes)?;

        // R7RS §4.3.1: the body of `let-syntax` is a ⟨body⟩, so a definition in
        // it is local to it and does not reach the enclosing body.
        //
        // Asked of the *desugared* body, not the forms as written. A definition
        // here is very often one a macro produced — that is much of the point of
        // binding a keyword around a body — and `(def x 56)` is a macro call
        // until it is expanded, so reading the source forms saw no definition
        // and let the `x` it binds escape and overwrite the enclosing one. That
        // is the first half of Larceny's family 23.
        //
        // Still asked, rather than wrapping unconditionally: the wrapper costs a
        // closure and puts the body out of tail position, and a body with no
        // definitions needs neither.
        let has_internal_defines = body_binds_definitions(&desugared_body);

        if has_internal_defines {
            Ok(CoreExpr::new(CoreExprKind::App {
                func: CoreExpr::rc(CoreExprKind::Lambda {
                    params: patina_ir::Formals::Fixed(vec![]),
                    body: desugared_body,
                    // The set this body was desugared with. Nothing reads it
                    // while the wrapper takes no parameters, but it is the
                    // answer that stays right if one is ever added.
                    binding_scopes: Rc::new(definition_scopes.clone()),
                }),
                args: vec![],
            }))
        } else if desugared_body.len() == 1 {
            Ok(desugared_body.into_iter().next().unwrap())
        } else {
            Ok(CoreExpr::new(CoreExprKind::Begin(desugared_body)))
        }
    }

    /// Desugar cond-expand using TaggedValue: (cond-expand clause ...)
    fn desugar_cond_expand_tagged(
        &self,
        args: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<CoreExpr> {
        use crate::cond_expand::evaluate_feature_requirement_tagged;

        let clauses = utils::list_to_vec_tagged(args, shared_heap)?;

        if clauses.is_empty() {
            return Err(DesugarError::InvalidSyntax(
                "cond-expand requires at least one clause".to_string(),
            ));
        }

        // From the heap, which is per interpreter instance — so a backend that
        // named itself is visible here, and to `(features)`, and to the library
        // parser, without any of them being threaded through the others.
        let features = shared_heap.borrow_mut().features_and_close().clone();

        let can_load_library =
            |lib_name: &[String]| crate::cond_expand::library_available(shared_heap, lib_name);

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
            .map(|tv| self.desugar_form(*tv, shared_heap))
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

            // Resolve the path: beside the including file if it is there,
            // otherwise relative to the cwd. The second is chibi's convention
            // and what a program run from its own directory relies on; the
            // first is what every implementation that runs Larceny's suite
            // does for a file that includes a sibling, and it wins only when
            // the file actually exists there, so nothing that resolved before
            // resolves differently now.
            let path = match base_dir {
                Some(ref base) if self.fs.file_exists(&base.join(&filename)) => {
                    base.join(&filename)
                }
                _ => std::path::PathBuf::from(&filename),
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

            // Desugar each expression from the included file, with that
            // file's directory on the stack so its own relative includes
            // resolve beside it. Popped on the error path too: a desugarer
            // outlives one failed `include` (the REPL's does), and a stale
            // entry would misdirect the next one.
            // A path resolved from the cwd is bare ("x.scm") and its parent
            // is "", which must not become the innermost directory — that
            // would hide the program's own directory from the nested
            // includes; leave the stack alone and they see what this one saw.
            let pushed = path
                .parent()
                .filter(|d| !d.as_os_str().is_empty())
                .map(|d| d.to_path_buf());
            if let Some(dir) = pushed.clone() {
                self.include_dirs.borrow_mut().push(dir);
            }
            let desugared: Result<Vec<CoreExpr>> = parsed_exprs
                .into_iter()
                .map(|expr_tv| self.desugar_form(expr_tv, shared_heap))
                .collect();
            if pushed.is_some() {
                self.include_dirs.borrow_mut().pop();
            }
            all_exprs.extend(desugared?);
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

    /// The directory a relative `include` path is tried against first: the
    /// innermost entry of `include_dirs` — the directory of the file
    /// currently being desugared — or, for a program that has no file, the
    /// directory of the source the parser was given, when that is a real
    /// path. `None` means only the cwd is available.
    ///
    /// This used to walk every location in the source map and take the first
    /// with a file path. The map is a `HashMap`, so which file won was not
    /// deterministic, and a library's forms (parsed without a source map)
    /// could only ever be resolved against whatever *program* was in it.
    fn resolve_include_base_dir(&self) -> Option<std::path::PathBuf> {
        if let Some(dir) = self.include_dirs.borrow().last() {
            return Some(dir.clone());
        }
        self.source_map.as_ref().and_then(|sm| {
            let sm = sm.borrow();
            let source = sm.primary_source()?;
            if source.starts_with('<') || source.is_empty() {
                return None;
            }
            std::path::Path::new(source)
                .parent()
                .map(|p| p.to_path_buf())
        })
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
        let rules = self.parse_macro_rules_tagged(&list[rules_start..], &name, shared_heap)?;

        let mut compiler = Compiler::with_env_and_scopes(
            literals,
            custom_ellipsis,
            env.clone(),
            scopes.clone(),
            env.heap().clone(),
        );
        let macro_name = name.clone();
        let mut compiled = compiler.compile_macro(name, rules).map_err(|e| {
            DesugarError::InvalidSyntax(format!("Failed to compile macro {macro_name}: {e}"))
        })?;
        compiled.foreign_expansions = self.foreign_expansions_carried_by(&compiled);
        Ok(compiled)
    }

    /// The foreign expansions (`EarlyBinding::foreign`) whose scopes the
    /// identifiers `compiled` inherited carry, for its `foreign_expansions`.
    ///
    /// Asked while the form that expanded the generator is still being
    /// desugared, which is the last moment the record is there: it is emptied
    /// at the end of the form (#446).
    fn foreign_expansions_carried_by(
        &self,
        compiled: &patina_core::CompiledMacro,
    ) -> Vec<(ScopeId, Rc<Environment>)> {
        let foreign = self.early.foreign.borrow();
        if foreign.is_empty() {
            return Vec::new();
        }
        let mut carried: Vec<(ScopeId, Rc<Environment>)> = Vec::new();
        for scopes in compiled.inherited_identifiers.values().flatten() {
            for scope in scopes.iter() {
                if let Some(env) = foreign.get(scope)
                    && !carried.iter().any(|(seen, _)| seen == scope)
                {
                    carried.push((*scope, Rc::clone(env)));
                }
            }
        }
        carried
    }

    /// Parse the literals list from TaggedValue: (lit1 lit2 ...)
    fn parse_literals_list_tagged(
        &self,
        literals_tv: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<Vec<IdentifierKey>> {
        let items = utils::list_to_vec_tagged(literals_tv, shared_heap)?;
        let heap = shared_heap.borrow();
        let mut literals = Vec::new();
        for item in items {
            // The scopes are kept, not discarded: literal membership is decided
            // by identifier identity, so an introduced literal must not match a
            // substituted pattern identifier of the same name.
            if let Some(key) = IdentifierKey::from_heap(item, &heap) {
                literals.push(key);
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
    /// pattern/template pairs directly as TaggedValue. `name` is for error
    /// messages — a rule-shape error in a library with fifty macros is
    /// undiagnosable without it, and this message is the most frequent row
    /// in the compat harness's parse-error histogram.
    fn parse_macro_rules_tagged(
        &self,
        rules_tvs: &[TaggedValue],
        name: &str,
        shared_heap: &SharedHeap,
    ) -> Result<Vec<(TaggedValue, TaggedValue)>> {
        if rules_tvs.is_empty() {
            return Err(DesugarError::InvalidSyntax(format!(
                "syntax-rules must have at least one rule, in macro {name}"
            )));
        }

        let mut rules = Vec::new();
        for &rule_tv in rules_tvs {
            let rule_list = utils::list_to_vec_tagged(rule_tv, shared_heap)?;

            if rule_list.len() != 2 {
                return Err(DesugarError::InvalidSyntax(format!(
                    "Each syntax-rules rule must have exactly 2 elements \
                     (pattern template), in macro {name}"
                )));
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
        if let CoreExprKind::Define { name, value, .. } = result.kind {
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
        if let CoreExprKind::Define { name, value, .. } = result.kind {
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
        if let CoreExprKind::Define { name, value, .. } = result.kind {
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
        if let CoreExprKind::Define { name, value, .. } = result.kind {
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
