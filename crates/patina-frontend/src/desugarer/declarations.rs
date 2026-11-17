//! Variable declarations encountered while expanding one top-level form.
//!
//! A definition changes how subsequent identifiers expand before its value is
//! evaluated (#463). Keep that information in lookup-only environments: writing
//! placeholders into the program would destroy old values, detach imports, and
//! expose unfinished declarations to library initializers run during expansion.

use super::*;
use patina_core::BindingCell;

#[derive(Clone)]
struct Declaration {
    env: Rc<Environment>,
    name: Rc<str>,
    scopes: ScopeSet,
    /// The declaration's location, which every view of it imports: a free
    /// cell holding `UNSPECIFIED`, `Some` exactly when `scopes` is empty.
    /// A value of the context that desugars this form, made by `declare`
    /// beneath it. The form's `Declarations` drop with the form
    /// (`Desugarer::desugar_tagged`), and nothing collects beneath a
    /// context, so it is never held across a collection (#834).
    cell: Option<BindingCell>,
}

#[derive(Default, Clone)]
pub(super) struct Declarations {
    bindings: Vec<Declaration>,
    views: FxHashMap<u64, Rc<Environment>>,
}

impl Declarations {
    /// Declare `name` at `scopes` in `env`. An unscoped declaration gets a
    /// location of its own, a free cell the context allocates, which every
    /// view imports (#834); a scoped one is bound in each view under its
    /// scopes.
    pub fn declare(
        &mut self,
        env: Rc<Environment>,
        name: Rc<str>,
        scopes: ScopeSet,
        cx: &mut Cx<'_>,
    ) {
        self.forget(&env, &name, &scopes);
        let cell = scopes.is_empty().then(|| cx.new_declaration_cell());
        self.bindings.push(Declaration {
            env,
            name,
            scopes,
            cell,
        });
        self.views.clear();
    }

    pub fn forget(&mut self, env: &Environment, name: &str, scopes: &ScopeSet) {
        self.bindings.retain(|binding| {
            binding.env.env_id() != env.env_id()
                || binding.name.as_ref() != name
                || binding.scopes != *scopes
        });
        self.views.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Use the ordinary environment resolver, including its scope ordering and
    /// ambiguity checks. The views are never captured by compiled macros or
    /// runtime aliases; those retain the real, live environments.
    ///
    /// Through a shared context: the view is a frame, whose binds and
    /// imports take no borrow and allocate nothing (#834).
    pub fn view(&mut self, env: &Rc<Environment>, cx: &Cx<'_>) -> Rc<Environment> {
        if self.bindings.is_empty() {
            return env.clone();
        }
        if let Some(view) = self.views.get(&env.env_id()) {
            return view.clone();
        }
        let mut declarations = Vec::new();
        let mut ancestor = Some(env);
        while let Some(current) = ancestor {
            declarations.extend(
                self.bindings
                    .iter()
                    .filter(|d| d.env.env_id() == current.env_id()),
            );
            ancestor = current.parent();
        }
        let view = if declarations.is_empty() {
            env.clone()
        } else {
            #[expect(
                clippy::disallowed_methods,
                reason = "a lookup-only view over `env`: it binds `UNSPECIFIED` and the form's \
                          declaration cells, values of the form's context, beneath which nothing \
                          collects; it lives for one form's expansion"
            )]
            let view = Rc::new(Environment::child_frame(env.clone()));
            for declaration in declarations.into_iter().rev() {
                if let Some(cell) = declaration.cell {
                    // Every view must name the same declaration location, or
                    // a local macro would mistake its own global for a foreign
                    // binding and relink it to the value preceding the define.
                    // Each imports the one cell, so each answers its `Cell`.
                    cx.import_cell_in_frame(
                        NamespaceView::of(&view),
                        declaration.name.clone(),
                        cell,
                    );
                } else {
                    cx.define_with_scopes_in_frame(
                        NamespaceView::of(&view),
                        declaration.name.clone(),
                        declaration.scopes.clone(),
                        Value::UNSPECIFIED,
                    );
                }
            }
            view
        };
        self.views.insert(env.env_id(), view.clone());
        view
    }
}

impl Desugarer<'_> {
    pub(super) fn expansion_env(&self, env: &Rc<Environment>, cx: &Cx<'_>) -> Rc<Environment> {
        self.declarations.borrow_mut().view(env, cx)
    }

    pub(super) fn declare_top_level_variable(
        &self,
        name: &Rc<str>,
        scopes: &ScopeSet,
        cx: &mut Cx<'_>,
    ) {
        if self.top_level.get() {
            let env = self
                .splicing
                .as_ref()
                .map_or(&self.env, |context| &context.env);
            self.declarations
                .borrow_mut()
                .declare(env.clone(), name.clone(), scopes.clone(), cx);
            self.early.imports.borrow_mut().clear();
        }
    }

    /// Expand `form`, a use of the macro object `mac`, whose body is
    /// `compiled`.
    ///
    /// Both sites of its literal comparisons are given the form's relinked
    /// names (#837): a literal of a generated macro and an input of this
    /// use may each be an identifier relinking renamed, whose meaning is
    /// the form's table's and no environment's. `put_back_carried` has
    /// put the macro's own back before this is called.
    pub(super) fn expand_macro<'gc>(
        &self,
        mac: Value<'gc>,
        compiled: &patina_core::CompiledMacro,
        form: Value<'gc>,
        cx: &mut FormCx<'_, 'gc>,
    ) -> std::result::Result<patina_macros::MacroExpansion<'gc>, patina_macros::MacroError> {
        let use_env = self.expansion_env(&self.env, cx);
        let definition_env = compiled
            .definition_env
            .as_ref()
            .map(|env| self.expansion_env(env, cx));
        let depth = self.open_forms.borrow().depth();
        let names = FormRelinks {
            early: &self.early,
            literals: cx.literals,
        };
        let relinked: Option<&dyn patina_macros::RelinkedNames> =
            self.early.any_relinked.get().then_some(&names);
        patina_macros::macro_expander::expand_macro_at_sites(
            mac,
            form,
            cx,
            definition_env.as_ref().map(|env| patina_macros::Site {
                env,
                scopes: &compiled.definition_scopes,
                relinked,
            }),
            Some(patina_macros::Site {
                env: &use_env,
                scopes: &self.current_scopes,
                relinked,
            }),
            depth,
        )
    }
}

/// A form's relinked names as the literal matcher asks for them (#837): the
/// form's table, and the pool its links are entries of.
struct FormRelinks<'a> {
    early: &'a EarlyBinding,
    literals: &'a LiteralPool,
}

impl patina_macros::RelinkedNames for FormRelinks<'_> {
    fn binding<'gc>(
        &self,
        name: &str,
        scopes: &ScopeSet,
        cx: &Cx<'gc>,
    ) -> Option<(patina_core::BindingLocation, Option<Value<'gc>>)> {
        if scopes.is_empty() {
            return None;
        }
        let (_, link) = self.early.relinked(name, scopes)?;
        // The pool is the form's, so the link is one of its entries, and
        // its record is live beneath the form's context. The location is
        // read now and held across nothing.
        let linkable = cx.linkable(self.literals, link);
        Some((cx.linked_location(linkable), cx.linked_value(linkable)))
    }
}
