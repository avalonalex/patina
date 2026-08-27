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
use patina_core::{CoreForm, SharedHeap, TaggedValue};
use patina_ir::{CoreExpr, CoreExprKind};
use patina_macros::IdentifierKey;
use patina_macros::macro_expander::utils::list_to_vec_with_tail_tagged;
use patina_runtime::{Environment, ScopeId, ScopeSet};
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
    aliases: &'a HashMap<Rc<str>, TaggedValue>,
    /// The expansion's own scope — see `MacroExpansion::scope`.
    expansion_scope: ScopeId,
}

/// Monotonic counter for the unique names given to definition-environment
/// aliases. Never reused, so an alias cannot collide with a user binding or
/// with an alias from another expansion.
fn next_alias_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
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

    /// Names that are shadowed by local bindings (lambda parameters)
    /// These should not be treated as macro calls
    shadowed_names: std::collections::HashSet<Rc<str>>,

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
            shadowed_names: std::collections::HashSet::new(),
            source_map: None,
            fs: std::sync::Arc::new(patina_core::NativeFs),
            include_dirs: Rc::new(RefCell::new(Vec::new())),
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
            shadowed_names: std::collections::HashSet::new(),
            source_map: Some(source_map),
            fs: std::sync::Arc::new(patina_core::NativeFs),
            include_dirs: Rc::new(RefCell::new(Vec::new())),
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
            shadowed_names: std::collections::HashSet::new(),
            source_map: None,
            fs: std::sync::Arc::new(patina_core::NativeFs),
            include_dirs: Rc::new(RefCell::new(Vec::new())),
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
            include_dirs: self.include_dirs.clone(),
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
        let names: Vec<Rc<str>> = new_shadows.into_iter().collect();

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
            names.is_empty() || !new_scopes.is_empty(),
            "local bindings need a scope of their own: {:?}",
            names
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
            for name in &names {
                child.define_with_scopes(
                    name.to_string(),
                    new_scopes.clone(),
                    TaggedValue::UNSPECIFIED,
                );
            }
            child
        };

        // Still recorded by spelling for the *literal* matcher, which compares
        // spellings rather than bindings (`is_literal_shadowed_tagged`). That
        // is the one place shadowing has not moved to bindings yet; it is the
        // triage doc's "spelling-based literal matching" item, and it is a
        // separate change from this one.
        let mut shadowed = self.shadowed_names.clone();
        shadowed.extend(names);

        Self {
            env,
            current_scopes: new_scopes,
            shadowed_names: shadowed,
            source_map: self.source_map.clone(),
            fs: self.fs.clone(),
            include_dirs: self.include_dirs.clone(),
        }
    }

    /// Check if a name is shadowed by a local binding
    fn is_shadowed(&self, name: &str) -> bool {
        self.shadowed_names.contains(name)
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
    /// Only definitions *written* in the body are seen, not ones a macro use
    /// expands into (`define-record-type`, `define-values`). That is the same
    /// coarseness `resolve_syntax` already documents, and in the same
    /// direction: a missed shadow, never an invented one.
    fn body_definition_names(
        &self,
        body_tvs: &[TaggedValue],
        shared_heap: &SharedHeap,
    ) -> Vec<Rc<str>> {
        let mut names = Vec::new();
        for tv in body_tvs {
            self.collect_definition_names(*tv, shared_heap, &mut names);
        }
        names
    }

    /// Add the names `tv` defines to `out`, descending through `begin`, which
    /// splices its contents into the body it appears in.
    ///
    /// The head is resolved, not spelled, so a `define` reached under an
    /// import rename counts and one shadowed by a parameter does not.
    fn collect_definition_names(
        &self,
        tv: TaggedValue,
        shared_heap: &SharedHeap,
        out: &mut Vec<Rc<str>>,
    ) {
        if !tv.is_pair() {
            return;
        }
        let (head, cdr) = {
            let heap = shared_heap.borrow();
            heap.get_pair(tv)
        };
        let Some((head_name, head_scopes)) = self.identifier_of(head, shared_heap) else {
            return;
        };
        match self.resolve_syntax(&head_name, &head_scopes) {
            Some(SyntaxRef::CoreSyntax(CoreForm::Define)) => {
                let target = {
                    let heap = shared_heap.borrow();
                    cdr.is_pair().then(|| heap.get_pair(cdr).0)
                };
                if let Some(name) = target.and_then(|t| self.define_target_name(t, shared_heap)) {
                    out.push(name);
                }
            }
            Some(SyntaxRef::CoreSyntax(CoreForm::Begin)) => {
                let mut current = cdr;
                while current.is_pair() {
                    let (car, next) = {
                        let heap = shared_heap.borrow();
                        heap.get_pair(current)
                    };
                    self.collect_definition_names(car, shared_heap, out);
                    current = next;
                }
            }
            _ => {}
        }
    }

    /// The name a `define` target binds: the symbol itself, or — for the
    /// procedure shorthand, curried arbitrarily deep — the one at the head of
    /// the nested formals.
    fn define_target_name(&self, tv: TaggedValue, shared_heap: &SharedHeap) -> Option<Rc<str>> {
        let mut current = tv;
        loop {
            if let Some((name, _)) = self.identifier_of(current, shared_heap) {
                return Some(name);
            }
            if !current.is_pair() {
                return None;
            }
            current = {
                let heap = shared_heap.borrow();
                heap.get_pair(current).0
            };
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
    fn resolve_syntax(&self, name: &str, scopes: &ScopeSet) -> Option<SyntaxRef> {
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
        let tv = self.env.get_with_scopes(name, scopes)?;
        let heap = self.env.heap().borrow();
        if let Some(form) = heap.get_core_syntax(tv) {
            return Some(SyntaxRef::CoreSyntax(form));
        }
        heap.get_macro(tv).cloned().map(SyntaxRef::Macro)
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
        match self.resolve_syntax(name, scopes) {
            None => Ok(()),
            Some(found) => Err(DesugarError::InvalidSyntax(format!(
                "invalid use of syntax as a value: `{name}` is {}",
                found.describe()
            ))),
        }
    }

    /// Create a child desugarer with a new environment (for let-syntax bodies)
    ///
    /// This inherits the current shadowed_names and uses the new environment and scopes.
    fn with_new_env(&self, env: Rc<Environment>, scopes: ScopeSet) -> Self {
        Self {
            env,
            current_scopes: scopes,
            shadowed_names: self.shadowed_names.clone(),
            source_map: self.source_map.clone(),
            fs: self.fs.clone(),
            include_dirs: self.include_dirs.clone(),
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
    /// it. Where both resolve to the same value — the common case, since most
    /// template references are to primitives copied into both — nothing is
    /// rewritten.
    fn link_definition_env_refs(
        &self,
        expanded: TaggedValue,
        expansion_scope: ScopeId,
        definition_env: Option<&Rc<Environment>>,
        definition_scopes: &ScopeSet,
        template_symbols: &HashSet<Rc<str>>,
        shared_heap: &SharedHeap,
    ) -> TaggedValue {
        let Some(def_env) = definition_env else {
            return expanded;
        };
        if def_env.env_id() == self.env.env_id() || template_symbols.is_empty() {
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
        let mut renames: HashMap<Rc<str>, TaggedValue> = HashMap::new();
        for name in template_symbols {
            let Some(def_value) = def_env.get(name) else {
                continue;
            };
            // A name the definition site bound *lexically* is not free, and
            // aliasing it would defeat the binding it actually names.
            //
            // `get` is the name-only view of the environment and deliberately
            // skips local variables, so it cannot tell `(let ((f …)) …)` around
            // this macro from a global `f` somewhere above — it answers with the
            // global either way. Asking again with the macro's own scopes is
            // what distinguishes them: a different answer means something
            // lexical shadows the by-name view here, and ordinary set-of-scopes
            // resolution is what should decide the reference. Without this, a
            // `let-syntax` transformer's `(f x)` inside `(let ((f …)) …)` was
            // aliased to whatever `f` the enclosing program happened to define
            // — Larceny's `base` measured it as `number->string`.
            // Skipped when the macro has no definition scopes of its own — the
            // top-level and library case, and the common one. `get_with_scopes`
            // returns `get` unchanged for an empty scope set, so the comparison
            // could only ever be with itself, at the cost of a second walk of
            // the environment chain per template symbol per expansion.
            if !definition_scopes.is_empty()
                && def_env.get_with_scopes(name, definition_scopes) != Some(def_value)
            {
                continue;
            }
            if self.env.get(name) == Some(def_value) {
                continue;
            }
            let alias = format!("{}.{}", name, next_alias_id());
            let symbol = shared_heap.borrow_mut().intern_symbol(&alias);
            target_env.define_alias(alias, def_env.clone(), name.clone());
            renames.insert(name.clone(), symbol);
        }
        if renames.is_empty() {
            return expanded;
        }
        let renames = Renames {
            aliases: &renames,
            expansion_scope,
        };
        self.rewrite_refs(expanded, &renames, 0, shared_heap)
    }

    /// The alias for `tv`, if it is a reference *this expansion introduced*
    /// and the definition site's binding for its spelling was aliased.
    ///
    /// Only an identifier carrying the expansion's scope qualifies. The
    /// expander puts that scope on everything the template introduced and on
    /// nothing that arrived through a pattern variable, so the `list` a
    /// template writes is renamed and the `(list 1 2 3)` the user wrote
    /// inside the macro call is not — they can mean different procedures,
    /// and under SRFI 101, where the program's `list` builds random-access
    /// lists and `(chibi test)`'s builds pairs, they do. Renaming by
    /// spelling alone rewrote the user's as well; that is Larceny family 35.
    /// A plain symbol never qualifies: the expander leaves the user's symbols
    /// as symbols, and turns what it introduces into identifiers.
    fn introduced_alias(
        &self,
        tv: TaggedValue,
        renames: &Renames<'_>,
        shared_heap: &SharedHeap,
    ) -> Option<TaggedValue> {
        let heap = shared_heap.borrow();
        let (name, scopes) = heap.get_identifier_data_any(tv)?;
        if !scopes.contains(&renames.expansion_scope) {
            return None;
        }
        renames.aliases.get(&*name).copied()
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
        // Compute the depth inside the borrow: the answer is a `u32`, so
        // nothing needs to outlive it and the head name is never copied out.
        // `is_quote` rather than an early return, so the head name is still
        // never copied out of the borrow.
        let (rest_depth, is_quote) = {
            let heap = shared_heap.borrow();
            let (car, _) = heap.get_pair(tv);
            match heap.get_symbol_or_identifier_name(car) {
                Some("quote") => (quote_depth, true),
                Some("quasiquote") => (quote_depth + 1, false),
                Some("unquote") | Some("unquote-splicing") => {
                    (quote_depth.saturating_sub(1), false)
                }
                _ => (quote_depth, false),
            }
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
        // Only a `quote` the template introduced, and only when `quote` is
        // being relinked at all — when the definition and use sites disagree
        // about what it means. Otherwise the form is returned unchanged,
        // exactly as before.
        if is_quote {
            let (car, cdr) = shared_heap.borrow().get_pair(tv);
            let Some(renamed) = self.introduced_alias(car, renames, shared_heap) else {
                return tv;
            };
            return shared_heap.borrow_mut().alloc_pair(renamed, cdr);
        }

        // Flatten the spine so the head can be told from the arguments. The
        // tail is whatever ends the list — `()` for a proper one, an atom for
        // a dotted one.
        let (mut elems, tail) = list_to_vec_with_tail_tagged(tv, &shared_heap.borrow());

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
    /// `unquote-splicing`; `quote` is opaque outright.
    fn rewrite_refs(
        &self,
        tv: TaggedValue,
        renames: &Renames<'_>,
        quote_depth: u32,
        shared_heap: &SharedHeap,
    ) -> TaggedValue {
        if tv.is_pair() {
            return self.rewrite_form(tv, renames, quote_depth, shared_heap);
        }

        if tv.is_vector() {
            // Vector elements are evaluated inside quasiquote, so they have to
            // be walked. A bare `#(...)` is self-evaluating data and is
            // protected by `quote_depth` like anything else.
            let (len, elems) = {
                let heap = shared_heap.borrow();
                let len = heap.vector_len(tv);
                let elems: Vec<TaggedValue> = (0..len).map(|i| heap.vector_ref(tv, i)).collect();
                (len, elems)
            };
            let mut out = Vec::with_capacity(len);
            let mut changed = false;
            for e in elems {
                let new_e = self.rewrite_refs(e, renames, quote_depth, shared_heap);
                changed |= new_e != e;
                out.push(new_e);
            }
            if !changed {
                return tv;
            }
            return shared_heap.borrow_mut().alloc_vector(out);
        }

        if quote_depth > 0 {
            return tv;
        }
        self.introduced_alias(tv, renames, shared_heap)
            .unwrap_or(tv)
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
            let name: Rc<str> = Rc::from(name);
            let scopes = ScopeSet::new();
            self.reject_syntax_as_value(&name, &scopes)?;
            return Ok(CoreExpr::new(CoreExprKind::Var { name, scopes }));
        }

        // Identifier - variable reference with scopes (for hygiene)
        if let Some((name, scopes)) = utils::get_identifier_info(tagged, &heap) {
            self.reject_syntax_as_value(&name, &scopes)?;
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

        // Step 2: Extract operator name and the scopes it was written with
        // (immutable access only). The scopes used to be discarded here and the
        // head looked up without them, while value position looked up *with*
        // them — one of the two ways the two lookups had drifted apart before
        // `resolve_syntax` merged them.
        let (name, head_scopes, is_macro_introduced) = {
            let heap = shared_heap.borrow();
            if let Some(s) = heap.get_symbol_name(car) {
                (Some(Rc::from(s)), ScopeSet::new(), false)
            } else if let Some((id_name, id_scopes)) = utils::get_identifier_info(car, &heap) {
                let introduced = !id_scopes.is_empty();
                (Some(id_name), id_scopes, introduced)
            } else {
                (None, ScopeSet::new(), false)
            }
        };
        // Immutable borrow released

        // Determine if this name is shadowed by a local binding. A
        // macro-introduced head is exempt: it carries its own scopes, and
        // hygiene means the template's `if` is still the special form even
        // where the use site bound `if`. `resolve_syntax` applies the same
        // rule from the scopes, so this is only for the `apply` check below.
        let is_shadowed = name
            .as_ref()
            .map(|n| !is_macro_introduced && self.is_shadowed(n))
            .unwrap_or(false);

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
            Some(sym) => match self.resolve_syntax(sym, &head_scopes) {
                Some(SyntaxRef::Macro(m)) => (Some(m), None),
                Some(SyntaxRef::CoreSyntax(form)) => (None, Some(form)),
                None => (None, None),
            },
            _ => (None, None),
        };

        // Handle macro expansion
        // Uses expand_macro_with_shadowed_tagged which:
        // - Accepts TaggedValue input directly
        // - Returns TaggedValue output directly
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
                &self.shadowed_names,
                // The use site's environment, for the half of R7RS §4.3.2 that
                // compares bindings rather than spellings: an auxiliary
                // keyword imported under a rename still names its own literal.
                Some(&self.env),
            )
            .map_err(|e| DesugarError::InvalidSyntax(format!("Macro expansion failed: {}", e)))?;

            // Referential transparency: a template's free identifiers denote what
            // they were bound to where the macro was *defined*. Link any that the
            // use site cannot resolve back to the definition environment before
            // desugaring, otherwise they become bare global loads here and fail.
            let expanded_tagged = self.link_definition_env_refs(
                expanded_tagged,
                expansion_scope,
                compiled_macro.definition_env.as_ref(),
                &compiled_macro.definition_scopes,
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

        // A core syntactic keyword, reached through its binding. Dispatch on
        // the *form* rather than on the name it was reached by, which is what
        // makes an import rename work: after `(rename (begin blk))`, `(blk 1 2)`
        // arrives here as `Begin`.
        if let Some(form) = core_form {
            return self.desugar_core_form(form, cdr, shared_heap);
        }

        // `apply` is the last head symbol recognized by spelling. It is not a
        // keyword and has no marker: the desugarer special-cases it as an
        // optimization, but it is also a real procedure binding, so it is
        // checked whatever the environment holds. That makes it the one name
        // whose meaning still ignores the binding just resolved — and the
        // reason `(define (apply a b) …)` is silently ignored, which is the
        // defect this design fixed for every other name. Tracked separately;
        // the fix is to key the lowering on the binding, as `core_form` above
        // does.
        if let Some(sym) = &name
            && !is_shadowed
            && sym.as_ref() == "apply"
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
        // 3. The body's own internal definitions, which bind over all of it
        let body_desugarer = self.with_shadowed_names(param_names, body_scopes.clone());
        let defined = body_desugarer.body_definition_names(&body_tvs, shared_heap);
        let body_desugarer = body_desugarer.with_shadowed_names(defined, body_scopes.clone());

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
            .and_then(|(name, scopes)| self.resolve_syntax(&name, &scopes))
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

        // `set!` refuses syntax for the same reason a value reference does, and
        // by the same call. R7RS §5.3.1 licenses a *definition* over a
        // syntactic keyword — "if ⟨variable⟩ is not bound, or is a syntactic
        // keyword, then the definition will bind ⟨variable⟩ to a new location"
        // — and says nothing of the sort for `set!`, whose ⟨variable⟩ must
        // already be one (§4.1.6, §3.1). chibi rejects `(set! if 5)` with the
        // same message it gives for `(list if)`; Gauche accepts it and then
        // breaks inside its own startup code. Without this, reading syntax was
        // an error while overwriting it silently succeeded.
        self.reject_syntax_as_value(&name, &scopes)?;

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

            // Create body desugarer with shadowed names — the formals, and the
            // body's own internal definitions (see `body_definition_names`).
            let param_names = utils::formals_to_names(&params);
            let body_desugarer = self.with_shadowed_names(param_names, body_scopes.clone());
            let defined = body_desugarer.body_definition_names(&body_tvs, shared_heap);
            let body_desugarer = body_desugarer.with_shadowed_names(defined, body_scopes);

            let body: Vec<CoreExpr> = body_tvs
                .iter()
                .map(|tv| body_desugarer.desugar_tagged(*tv, shared_heap))
                .collect::<Result<Vec<_>>>()?;

            return Ok(CoreExpr::new(CoreExprKind::Define {
                name,
                scopes: name_scopes,
                value: CoreExpr::rc(CoreExprKind::Lambda {
                    params,
                    body,
                    binding_scope: Some(binding_scope),
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
        let value = self.desugar_tagged(value_tv, shared_heap)?;

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
            let (name, binder_scopes): (Rc<str>, ScopeSet) = {
                let heap = shared_heap.borrow();
                if let Some(s) = heap.get_symbol_name(binding_vec[0]) {
                    (Rc::from(s), ScopeSet::new())
                } else if let Some((id_name, id_scopes)) =
                    utils::get_identifier_info(binding_vec[0], &heap)
                {
                    (id_name, id_scopes)
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
            // Bound only *with* the body's scopes. Set-of-scopes resolution
            // matches a binding when its scopes are a subset of the
            // reference's, so this reaches exactly the references lexically
            // inside this body — which is what R7RS §4.3.1 makes a `let-syntax`
            // keyword visible to, and nothing else.
            //
            // Not also bound unscoped, as it was. An unscoped binding is
            // reachable from *every* reference of that spelling, including one
            // introduced by a macro defined elsewhere, which carries only its
            // own expansion's scopes and is supposed to resolve at that macro's
            // definition site. That is the `let-syntax` half of Larceny family
            // 33: `(quote d)` in a top-level macro's template was captured by a
            // `let-syntax ((quote …))` merely surrounding the *call*, and since
            // the captured expansion introduces `quote` again, it re-captured
            // itself until the stack went.
            body_env.define_with_scopes(name.to_string(), definition_scopes.clone(), tv);

            // Also under the binder's own scopes, when it has any — that is,
            // when this whole `let-syntax` was introduced by a template. Its
            // body came from that same template, so a reference in it carries
            // the expansion's scopes and not `let_syntax_scope`, which the
            // desugarer minted only now; binding under the binder's set is what
            // makes the two meet. This is `bound-identifier=?` — same spelling,
            // same scopes — and chibi's §4.3 `(m k)` is exactly it: `m`'s
            // template binds `n` and then references it as `(n z)`.
            //
            // Guarded on non-empty, which is what keeps this from being the old
            // unscoped binding under another name: a `let-syntax` written in
            // source has a plain symbol here, and giving *that* an
            // empty-scoped binding is the family-33 capture removed above.
            if !binder_scopes.is_empty() {
                body_env.define_with_scopes(name.to_string(), binder_scopes, tv);
            }
        }

        let body_desugarer = self.with_new_env(body_env, definition_scopes.clone());

        // Get body TaggedValues
        let body_tvs: Vec<TaggedValue> = args_vec[1..].to_vec();

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
                    binding_scope: Some(ScopeId::fresh()),
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
                .map(|expr_tv| self.desugar_tagged(expr_tv, shared_heap))
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

        let mut compiler = Compiler::with_env_scopes_and_shadowed(
            literals,
            custom_ellipsis,
            env.clone(),
            scopes.clone(),
            &self.shadowed_names,
            env.heap().clone(),
        );
        let macro_name = name.clone();
        compiler.compile_macro(name, rules).map_err(|e| {
            DesugarError::InvalidSyntax(format!("Failed to compile macro {macro_name}: {e}"))
        })
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
