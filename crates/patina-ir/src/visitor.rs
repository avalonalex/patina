use crate::core_expr::{CoreExpr, Formals, Symbol};

/// Visitor trait for traversing and transforming CoreExpr
///
/// This enables nanopass-style compiler passes. Each pass implements
/// this trait to transform expressions.
///
/// Example usage:
/// ```ignore
/// struct ConstantFoldingPass;
///
/// impl ExprVisitor for ConstantFoldingPass {
///     type Output = CoreExpr;
///
///     fn visit_expr(&mut self, expr: &CoreExpr) -> CoreExpr {
///         match expr {
///             CoreExpr::App { func, args } if is_add(func) && all_literals(args) => {
///                 // Fold (+ 1 2) to 3
///                 CoreExpr::Literal(eval_add(args))
///             }
///             _ => self.visit_children(expr)
///         }
///     }
/// }
/// ```
pub trait ExprVisitor {
    /// Output type of the visitor
    type Output;

    /// Visit an expression
    fn visit_expr(&mut self, expr: &CoreExpr) -> Self::Output;

    /// Visit a lambda expression
    fn visit_lambda(&mut self, params: &Formals, body: &[CoreExpr]) -> Self::Output {
        let _ = (params, body);
        unimplemented!("Subclasses should override visit_lambda if needed")
    }

    /// Visit an if expression
    fn visit_if(&mut self, test: &CoreExpr, then: &CoreExpr, else_: &CoreExpr) -> Self::Output {
        let _ = (test, then, else_);
        unimplemented!("Subclasses should override visit_if if needed")
    }

    /// Visit a begin expression
    fn visit_begin(&mut self, exprs: &[CoreExpr]) -> Self::Output {
        let _ = exprs;
        unimplemented!("Subclasses should override visit_begin if needed")
    }

    /// Visit an application
    fn visit_app(&mut self, func: &CoreExpr, args: &[CoreExpr]) -> Self::Output {
        let _ = (func, args);
        unimplemented!("Subclasses should override visit_app if needed")
    }

    /// Visit a variable reference
    fn visit_var(&mut self, var: &Symbol) -> Self::Output {
        let _ = var;
        unimplemented!("Subclasses should override visit_var if needed")
    }
}

/// Helper trait for mapping over CoreExpr children
///
/// This is useful for implementing visitors that need to recursively
/// transform all children.
impl CoreExpr {
    /// Map a function over all immediate children of this expression
    ///
    /// This is useful for implementing recursive transformations:
    /// ```ignore
    /// fn my_transform(&self, expr: &CoreExpr) -> CoreExpr {
    ///     // Transform children first
    ///     let expr = expr.map_children(|child| self.my_transform(child));
    ///
    ///     // Then apply transformation to current node
    ///     match expr {
    ///         // ... pattern match and transform
    ///     }
    /// }
    /// ```
    pub fn map_children<F>(&self, f: F) -> CoreExpr
    where
        F: Fn(&CoreExpr) -> CoreExpr,
    {
        match self {
            CoreExpr::Literal(_)
            | CoreExpr::Var(_)
            | CoreExpr::Quote(_)
            | CoreExpr::Quasiquote(_) => self.clone(),

            CoreExpr::Lambda { params, body } => CoreExpr::Lambda {
                params: params.clone(),
                body: body.iter().map(&f).collect(),
            },

            CoreExpr::If { test, then, else_ } => CoreExpr::If {
                test: Box::new(f(test)),
                then: Box::new(f(then)),
                else_: Box::new(f(else_)),
            },

            CoreExpr::Set { var, value } => CoreExpr::Set {
                var: var.clone(),
                value: Box::new(f(value)),
            },

            CoreExpr::Begin(exprs) => CoreExpr::Begin(exprs.iter().map(&f).collect()),

            CoreExpr::Define { name, value } => CoreExpr::Define {
                name: name.clone(),
                value: Box::new(f(value)),
            },

            CoreExpr::DefineSyntax { name, transformer } => CoreExpr::DefineSyntax {
                name: name.clone(),
                transformer: transformer.clone(), // transformer is Value (data), not transformed
            },

            CoreExpr::Import { import_sets } => CoreExpr::Import {
                import_sets: import_sets.clone(), // import_sets are Values (data), not CoreExpr
            },

            CoreExpr::Parameterize { bindings, body } => CoreExpr::Parameterize {
                bindings: bindings
                    .iter()
                    .map(|(param, val)| (f(param), f(val)))
                    .collect(),
                body: body.iter().map(&f).collect(),
            },

            CoreExpr::Expand { expr } => CoreExpr::Expand {
                expr: Box::new(f(expr)),
            },

            CoreExpr::CaseLambda { clauses } => CoreExpr::CaseLambda {
                clauses: clauses
                    .iter()
                    .map(|clause| crate::CaseLambdaClause {
                        params: clause.params.clone(),
                        body: clause.body.iter().map(&f).collect(),
                    })
                    .collect(),
            },

            CoreExpr::App { func, args } => CoreExpr::App {
                func: Box::new(f(func)),
                args: args.iter().map(&f).collect(),
            },

            CoreExpr::Apply { func, args } => CoreExpr::Apply {
                func: Box::new(f(func)),
                args: args.iter().map(&f).collect(),
            },

            CoreExpr::PrimCall { prim, args } => CoreExpr::PrimCall {
                prim: *prim,
                args: args.iter().map(&f).collect(),
            },

            CoreExpr::Let { bindings, body } => CoreExpr::Let {
                bindings: bindings
                    .iter()
                    .map(|(var, val)| (var.clone(), f(val)))
                    .collect(),
                body: Box::new(f(body)),
            },
        }
    }
}
