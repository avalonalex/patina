use patina_core::{CoreExpr, Formals, Symbol};

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
///         match &expr.kind {
///             CoreExprKind::App { func, args } if is_add(func) && all_literals(args) => {
///                 // Fold (+ 1 2) to 3
///                 CoreExpr::new(CoreExprKind::Literal(eval_add(args)))
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
