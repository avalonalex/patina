//! Variable declarations encountered while expanding one top-level form.
//!
//! A definition changes how subsequent identifiers expand before its value is
//! evaluated (#463). Keep that information in lookup-only environments: writing
//! placeholders into the program would destroy old values, detach imports, and
//! expose unfinished declarations to library initializers run during expansion.

use super::*;

#[derive(Clone)]
struct Declaration {
    env: Rc<Environment>,
    name: Rc<str>,
    scopes: ScopeSet,
    marker: Rc<Environment>,
}

#[derive(Default, Clone)]
pub(super) struct Declarations {
    bindings: Vec<Declaration>,
    views: FxHashMap<u64, Rc<Environment>>,
}

impl Declarations {
    pub fn declare(&mut self, env: Rc<Environment>, name: Rc<str>, scopes: ScopeSet) {
        self.forget(&env, &name, &scopes);
        #[expect(
            clippy::disallowed_methods,
            reason = "a lookup-only marker for one declaration: it binds only `UNSPECIFIED`, an \
                      immediate, so it holds no heap value, and it lives for one form's expansion"
        )]
        let marker = Rc::new(Environment::with_parent(env.clone()));
        marker.define_with_scopes(name.clone(), scopes.clone(), TaggedValue::UNSPECIFIED);
        self.bindings.push(Declaration {
            env,
            name,
            scopes,
            marker,
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
    pub fn view(&mut self, env: &Rc<Environment>) -> Rc<Environment> {
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
                reason = "a lookup-only view over `env`: it binds only `UNSPECIFIED` and shared \
                          declaration markers, no heap value, and lives for one form's expansion"
            )]
            let view = Rc::new(Environment::with_parent(env.clone()));
            for declaration in declarations.into_iter().rev() {
                if declaration.scopes.is_empty() {
                    // Every view must name the same declaration location, or
                    // a local macro would mistake its own global for a foreign
                    // binding and relink it to the value preceding the define.
                    view.share_binding(
                        declaration.name.clone(),
                        &declaration.marker,
                        &declaration.name,
                    );
                } else {
                    view.define_with_scopes(
                        declaration.name.clone(),
                        declaration.scopes.clone(),
                        TaggedValue::UNSPECIFIED,
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
    pub(super) fn expansion_env(&self, env: &Rc<Environment>) -> Rc<Environment> {
        self.declarations.borrow_mut().view(env)
    }

    pub(super) fn declare_top_level_variable(&self, name: &Rc<str>, scopes: &ScopeSet) {
        if self.top_level.get() {
            let env = self
                .splicing
                .as_ref()
                .map_or(&self.env, |context| &context.env);
            self.declarations
                .borrow_mut()
                .declare(env.clone(), name.clone(), scopes.clone());
            self.early.imports.borrow_mut().clear();
        }
    }

    pub(super) fn expand_macro(
        &self,
        compiled: &patina_core::CompiledMacro,
        form: TaggedValue,
        heap: &SharedHeap,
    ) -> std::result::Result<patina_macros::MacroExpansion, patina_macros::MacroError> {
        let use_env = self.expansion_env(&self.env);
        let definition_env = compiled
            .definition_env
            .as_ref()
            .map(|env| self.expansion_env(env));
        patina_macros::macro_expander::expand_macro_at_sites(
            compiled,
            form,
            heap,
            definition_env.as_ref().map(|env| patina_macros::Site {
                env,
                scopes: &compiled.definition_scopes,
            }),
            Some(patina_macros::Site {
                env: &use_env,
                scopes: &self.current_scopes,
            }),
        )
    }
}
