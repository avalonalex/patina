use patina_core::{CoreExpr, CoreExprKind, Formals, ScopeId, ScopeSet, Symbol, TaggedValue};

/// Visitor trait for walking `CoreExpr` trees.
///
/// Implement `ExprVisitor` to traverse the IR for analysis passes: free-variable
/// collection, expression counting, tail-position marking, etc.
///
/// ## How to use
///
/// Override only the variants you care about.  All per-variant methods default to
/// recursing into children (or no-op for leaf nodes).  `visit_expr` dispatches to
/// the correct per-variant method via `walk_expr`.
///
/// ```ignore
/// struct FreeVarCollector {
///     bound: Vec<Symbol>,
///     free: std::collections::HashSet<String>,
/// }
///
/// impl ExprVisitor for FreeVarCollector {
///     fn visit_var(&mut self, name: &Symbol, _scopes: &ScopeSet) {
///         if !self.bound.contains(name) {
///             self.free.insert(name.to_string());
///         }
///     }
///
///     fn visit_lambda(&mut self, params: &Formals, body: &[CoreExpr],
///                     _binding_scope: Option<ScopeId>) {
///         let prev = self.bound.len();
///         // push params as bound names
///         self.bound.truncate(prev); // restore after visiting body
///     }
/// }
/// ```
///
/// ## Transforms
///
/// For tree-rewriting passes use `CoreExpr::map_children` instead; it already
/// handles structural rebuilding with source-location preservation.
pub trait ExprVisitor {
    /// Entry point called for every node.
    ///
    /// The default dispatches to the per-variant method via `walk_expr`.
    /// Override this if you need to intercept *every* node before the
    /// per-variant dispatch — for example, to count all nodes:
    ///
    /// ```ignore
    /// fn visit_expr(&mut self, expr: &CoreExpr) {
    ///     self.count += 1;
    ///     self.walk_expr(expr); // continue recursion
    /// }
    /// ```
    fn visit_expr(&mut self, expr: &CoreExpr) {
        self.walk_expr(expr);
    }

    // ── Leaf nodes (no sub-expressions) ──────────────────────────────────

    /// Visit a literal value (number, boolean, string, …).
    fn visit_literal(&mut self, _val: &TaggedValue) {}

    /// Visit a variable reference.
    fn visit_var(&mut self, _name: &Symbol, _scopes: &ScopeSet) {}

    /// Visit a quoted datum.
    fn visit_quote(&mut self, _val: &TaggedValue) {}

    /// Visit a quasiquoted template.
    /// The template is stored as a `TaggedValue`; unquote/splicing is handled
    /// by the evaluator, not during IR traversal.
    fn visit_quasiquote(&mut self, _val: &TaggedValue) {}

    /// Visit an import declaration.
    /// Import sets are kept as `TaggedValue` declarative data, not sub-expressions.
    fn visit_import(&mut self, _import_sets: &[TaggedValue]) {}

    // ── Interior nodes (default recurses into children) ───────────────────

    /// Visit a lambda abstraction.  Default recurses into the body.
    fn visit_lambda(
        &mut self,
        _params: &Formals,
        body: &[CoreExpr],
        _binding_scope: Option<ScopeId>,
    ) {
        for expr in body {
            self.visit_expr(expr);
        }
    }

    /// Visit a conditional.  Default visits test, then-branch, and else-branch.
    fn visit_if(&mut self, test: &CoreExpr, then: &CoreExpr, else_: &CoreExpr) {
        self.visit_expr(test);
        self.visit_expr(then);
        self.visit_expr(else_);
    }

    /// Visit an assignment.  Default recurses into the value expression.
    fn visit_set(&mut self, _var: &Symbol, _scopes: &ScopeSet, value: &CoreExpr) {
        self.visit_expr(value);
    }

    /// Visit a sequence.  Default recurses into each expression in order.
    fn visit_begin(&mut self, exprs: &[CoreExpr]) {
        for expr in exprs {
            self.visit_expr(expr);
        }
    }

    /// Visit a top-level definition.  Default recurses into the value expression.
    fn visit_define(&mut self, _name: &Symbol, value: &CoreExpr) {
        self.visit_expr(value);
    }

    /// Visit a macro-expansion debug node.  Default recurses into the inner expression.
    fn visit_expand(&mut self, expr: &CoreExpr) {
        self.visit_expr(expr);
    }

    /// Visit a function application.
    /// Default recurses into the function expression and each argument.
    fn visit_app(&mut self, func: &CoreExpr, args: &[CoreExpr]) {
        self.visit_expr(func);
        for arg in args {
            self.visit_expr(arg);
        }
    }

    /// Visit an `apply` form.
    /// Default recurses into the function expression and each argument
    /// (the last argument is the tail list, handled identically here).
    fn visit_apply(&mut self, func: &CoreExpr, args: &[CoreExpr]) {
        self.visit_expr(func);
        for arg in args {
            self.visit_expr(arg);
        }
    }

    // ── Dispatch ──────────────────────────────────────────────────────────

    /// Dispatch `expr` to the appropriate per-variant method.
    ///
    /// Call this inside a `visit_expr` override after doing your own work, to
    /// continue the default recursion:
    ///
    /// ```ignore
    /// fn visit_expr(&mut self, expr: &CoreExpr) {
    ///     self.count += 1;
    ///     self.walk_expr(expr);
    /// }
    /// ```
    fn walk_expr(&mut self, expr: &CoreExpr) {
        match &expr.kind {
            CoreExprKind::Literal(val) => self.visit_literal(val),
            CoreExprKind::Var { name, scopes } => self.visit_var(name, scopes),
            CoreExprKind::Quote(val) => self.visit_quote(val),
            CoreExprKind::Quasiquote(val) => self.visit_quasiquote(val),
            CoreExprKind::Lambda {
                params,
                body,
                binding_scope,
            } => self.visit_lambda(params, body, *binding_scope),
            CoreExprKind::If { test, then, else_ } => self.visit_if(test, then, else_),
            CoreExprKind::Set { var, scopes, value } => self.visit_set(var, scopes, value),
            CoreExprKind::Begin(exprs) => self.visit_begin(exprs),
            CoreExprKind::Define { name, value } => self.visit_define(name, value),
            CoreExprKind::Import { import_sets } => self.visit_import(import_sets),
            CoreExprKind::Expand { expr } => self.visit_expand(expr),
            CoreExprKind::App { func, args } => self.visit_app(func, args),
            CoreExprKind::Apply { func, args } => self.visit_apply(func, args),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_core::{CoreExpr, CoreExprKind, Formals, ScopeSet, ScopedParam, TaggedValue};
    use std::collections::HashSet;

    // ── helpers ──────────────────────────────────────────────────────────

    fn var(name: &str) -> CoreExpr {
        CoreExpr::new(CoreExprKind::Var {
            name: name.into(),
            scopes: ScopeSet::new(),
        })
    }

    fn lit() -> CoreExpr {
        CoreExpr::new(CoreExprKind::Literal(TaggedValue::UNSPECIFIED))
    }

    fn app(func: CoreExpr, args: Vec<CoreExpr>) -> CoreExpr {
        CoreExpr::new(CoreExprKind::App {
            func: std::rc::Rc::new(func),
            args,
        })
    }

    fn if_(test: CoreExpr, then: CoreExpr, else_: CoreExpr) -> CoreExpr {
        CoreExpr::new(CoreExprKind::If {
            test: std::rc::Rc::new(test),
            then: std::rc::Rc::new(then),
            else_: std::rc::Rc::new(else_),
        })
    }

    fn lambda(params: Vec<&str>, body: Vec<CoreExpr>) -> CoreExpr {
        CoreExpr::new(CoreExprKind::Lambda {
            params: Formals::Fixed(
                params
                    .into_iter()
                    .map(|n| ScopedParam::simple(n.into()))
                    .collect(),
            ),
            body,
            binding_scope: None,
        })
    }

    // ── Node counter ─────────────────────────────────────────────────────

    struct NodeCounter(usize);

    impl ExprVisitor for NodeCounter {
        fn visit_expr(&mut self, expr: &CoreExpr) {
            self.0 += 1;
            self.walk_expr(expr);
        }
    }

    #[test]
    fn test_node_counter() {
        // (if (+ 1 2) x y)  →  if + 1 2 x y = 7 nodes (if, app, var(+), lit, lit, var(x), var(y))
        let expr = if_(app(var("+"), vec![lit(), lit()]), var("x"), var("y"));
        let mut counter = NodeCounter(0);
        counter.visit_expr(&expr);
        assert_eq!(counter.0, 7);
    }

    #[test]
    fn test_node_counter_nested() {
        // (define f (lambda (x) x))  →  define, lambda, var(x) = 3 nodes
        let expr = CoreExpr::new(CoreExprKind::Define {
            name: "f".into(),
            value: std::rc::Rc::new(lambda(vec!["x"], vec![var("x")])),
        });
        let mut counter = NodeCounter(0);
        counter.visit_expr(&expr);
        assert_eq!(counter.0, 3);
    }

    // ── Free variable collector ───────────────────────────────────────────

    struct FreeVarCollector {
        bound: Vec<Symbol>,
        free: HashSet<String>,
    }

    impl FreeVarCollector {
        fn new() -> Self {
            Self {
                bound: vec![],
                free: HashSet::new(),
            }
        }
    }

    impl ExprVisitor for FreeVarCollector {
        fn visit_var(&mut self, name: &Symbol, _scopes: &ScopeSet) {
            if !self.bound.contains(name) {
                self.free.insert(name.to_string());
            }
        }

        fn visit_lambda(
            &mut self,
            params: &Formals,
            body: &[CoreExpr],
            _binding_scope: Option<ScopeId>,
        ) {
            let prev_len = self.bound.len();
            match params {
                Formals::Fixed(ps) => {
                    for p in ps {
                        self.bound.push(p.name.clone());
                    }
                }
                Formals::Variadic(p) => self.bound.push(p.name.clone()),
                Formals::Mixed { fixed, rest } => {
                    for p in fixed {
                        self.bound.push(p.name.clone());
                    }
                    self.bound.push(rest.name.clone());
                }
            }
            for expr in body {
                self.visit_expr(expr);
            }
            self.bound.truncate(prev_len);
        }
    }

    #[test]
    fn test_free_vars_simple() {
        // (lambda (x) (+ x y))  →  free: {+, y},  bound: {x}
        let expr = lambda(vec!["x"], vec![app(var("+"), vec![var("x"), var("y")])]);
        let mut collector = FreeVarCollector::new();
        collector.visit_expr(&expr);
        assert!(collector.free.contains("+"));
        assert!(collector.free.contains("y"));
        assert!(!collector.free.contains("x"));
    }

    #[test]
    fn test_free_vars_nested_lambda() {
        // (lambda (x) (lambda (y) (+ x y z)))  →  free: {+, z}
        let inner = lambda(
            vec!["y"],
            vec![app(var("+"), vec![var("x"), var("y"), var("z")])],
        );
        let outer = lambda(vec!["x"], vec![inner]);
        let mut collector = FreeVarCollector::new();
        collector.visit_expr(&outer);
        assert!(collector.free.contains("+"));
        assert!(collector.free.contains("z"));
        assert!(!collector.free.contains("x"));
        assert!(!collector.free.contains("y"));
    }

    // ── Default walk doesn't panic ────────────────────────────────────────

    struct NoopVisitor;
    impl ExprVisitor for NoopVisitor {}

    #[test]
    fn test_default_walk_all_variants() {
        let exprs = vec![
            lit(),
            var("x"),
            CoreExpr::new(CoreExprKind::Quote(TaggedValue::UNSPECIFIED)),
            CoreExpr::new(CoreExprKind::Quasiquote(TaggedValue::UNSPECIFIED)),
            lambda(vec!["a"], vec![var("a")]),
            if_(lit(), lit(), lit()),
            CoreExpr::new(CoreExprKind::Set {
                var: "x".into(),
                scopes: ScopeSet::new(),
                value: std::rc::Rc::new(lit()),
            }),
            CoreExpr::new(CoreExprKind::Begin(vec![lit(), var("x")])),
            CoreExpr::new(CoreExprKind::Define {
                name: "x".into(),
                value: std::rc::Rc::new(lit()),
            }),
            CoreExpr::new(CoreExprKind::Import {
                import_sets: vec![],
            }),
            CoreExpr::new(CoreExprKind::Expand {
                expr: std::rc::Rc::new(lit()),
            }),
            app(var("f"), vec![lit()]),
            CoreExpr::new(CoreExprKind::Apply {
                func: std::rc::Rc::new(var("f")),
                args: vec![lit()],
            }),
        ];

        let mut visitor = NoopVisitor;
        for expr in &exprs {
            visitor.visit_expr(expr); // must not panic
        }
    }
}
