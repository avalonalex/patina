//! Lowering `CoreExprKind::Quasiquote` into `CoreExprKind::App` calls to
//! `list`, `append` and `list->vector`.
//!
//! **Only the backend's half.** What a template builds, and every unquoted
//! expression in it, the desugarer derived where it met the template
//! (`desugarer/quasiquote.rs`; `patina_core::QuasiTemplate` says why the job
//! is split). What is left here is putting the constructors in: each
//! [`QuasiTemplate::Build`] becomes a call, each `Datum` a `Quote`, and each
//! unquoted expression is lowered in turn, since it may hold a template of
//! its own. Nothing here can find a program wrong; a failure means a
//! constructor was not supplied.
//!
//! Those three are the registry's own primitives, referenced as *values* —
//! a `Literal` in operator position — and not by name. A quasiquote denotes
//! the structure it writes, whatever `list` means where it appears: a
//! program that imports SRFI 101 has `cons` and `list` build random-access
//! lists, and `` `(1 ,x) `` in it must still be a pair. Looked up by name,
//! it was not, and every `(chibi test)` assertion under that import broke
//! on its own info alist (Larceny triage family 34). That was the VM's
//! defect alone while the tree-walker built the structure directly; since
//! both go through here, the guarantee is one and so is the risk.
//!
//! Runs before either backend lowers anything — ahead of the VM's five-pass
//! pipeline and ahead of the CPS transform — so neither needs to handle
//! `Quasiquote` at all. Nothing downstream of this carries that node.

use patina_core::core_expr::{CoreExpr, CoreExprKind};
use patina_core::tagged_value::TaggedValue;
use patina_core::{QuasiConstructor, QuasiTemplate};
use std::cell::Cell;
use std::rc::Rc;

/// A list constructor the lowering calls could not be resolved.
///
/// Never the program's fault: a malformed template is refused by the
/// desugarer, before this runs.
#[derive(Debug)]
pub struct QuasiquoteError(pub String);

impl std::fmt::Display for QuasiquoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Resolves `list`, `append` and `list->vector` to procedure *values*.
///
/// A closure rather than the primitive registry itself, and that is a layering
/// choice worth naming: `patina-primitives` depends on this crate, so a pass
/// living here cannot name `PrimitiveRegistry` without a cycle. Each backend
/// supplies its own resolver over its own registry and heap, and both get the
/// same lowering.
pub type ConstructorResolver<'a> = &'a dyn Fn(&str) -> Option<TaggedValue>;

/// Recursively lower all `Quasiquote` nodes in a `CoreExpr` tree.
///
/// Must be called before either backend lowers the tree. Takes a resolver
/// for the constructors the templates are built with — see the module doc
/// for why they are values rather than names, and [`ConstructorResolver`]
/// for why the caller supplies them.
pub fn lower_quasiquotes(
    expr: &CoreExpr,
    constructors: ConstructorResolver<'_>,
) -> Result<CoreExpr, QuasiquoteError> {
    let cx = Lowering {
        constructors,
        list: Cell::default(),
        append: Cell::default(),
        list_to_vector: Cell::default(),
    };
    lower_expr(expr, &cx)
}

/// What one compilation unit's templates are lowered through.
struct Lowering<'a> {
    constructors: ConstructorResolver<'a>,
    /// The constructor procedures, allocated the first time a template needs
    /// each: most units have no quasiquote at all, and one that has a
    /// hundred shares three objects.
    list: Cell<Option<TaggedValue>>,
    append: Cell<Option<TaggedValue>>,
    list_to_vector: Cell<Option<TaggedValue>>,
}

impl Lowering<'_> {
    /// The `scheme.base` primitive for `which`, as a procedure value.
    ///
    /// The caller's resolver decides what it is — the VM builds it the way
    /// `VmState::install_primitives` builds every primitive, registry index
    /// included, so a call through it dispatches by index like a call through
    /// the global of the same name. It differs from that global in one way
    /// only: nothing the program imports or defines can redirect it.
    ///
    /// Allocated once per unit and cached: most units have no quasiquote at
    /// all, and one with a hundred templates shares three objects.
    fn constructor(&self, which: QuasiConstructor) -> Result<TaggedValue, QuasiquoteError> {
        // Selected by `match`, so adding a constructor is a compile error
        // here rather than an out-of-bounds index at run time.
        let slot = match which {
            QuasiConstructor::List => &self.list,
            QuasiConstructor::Append => &self.append,
            QuasiConstructor::ListToVector => &self.list_to_vector,
        };
        if let Some(tv) = slot.get() {
            return Ok(tv);
        }
        let tv = (self.constructors)(which.name()).ok_or_else(|| {
            QuasiquoteError(format!(
                "quasiquote lowering needs the primitive {}, which is not registered",
                which.name()
            ))
        })?;
        slot.set(Some(tv));
        Ok(tv)
    }
}

/// Recursively walk a CoreExpr tree, lowering any Quasiquote nodes.
fn lower_expr(expr: &CoreExpr, cx: &Lowering<'_>) -> Result<CoreExpr, QuasiquoteError> {
    let each = |exprs: &[CoreExpr]| -> Result<Vec<CoreExpr>, QuasiquoteError> {
        exprs.iter().map(|e| lower_expr(e, cx)).collect()
    };
    let kind = match &expr.kind {
        CoreExprKind::Quasiquote(template) => return lower_template(template, cx),

        // Recursively walk child expressions
        CoreExprKind::Lambda {
            params,
            body,
            binding_scopes,
        } => CoreExprKind::Lambda {
            params: params.clone(),
            body: each(body)?,
            binding_scopes: binding_scopes.clone(),
        },

        CoreExprKind::If { test, then, else_ } => CoreExprKind::If {
            test: Rc::new(lower_expr(test, cx)?),
            then: Rc::new(lower_expr(then, cx)?),
            else_: Rc::new(lower_expr(else_, cx)?),
        },

        CoreExprKind::Set { var, scopes, value } => CoreExprKind::Set {
            var: var.clone(),
            scopes: scopes.clone(),
            value: Rc::new(lower_expr(value, cx)?),
        },

        CoreExprKind::Begin(exprs) => CoreExprKind::Begin(each(exprs)?),

        CoreExprKind::Define {
            name,
            scopes,
            value,
        } => CoreExprKind::Define {
            name: name.clone(),
            scopes: scopes.clone(),
            value: Rc::new(lower_expr(value, cx)?),
        },

        CoreExprKind::App { func, args } => CoreExprKind::App {
            func: Rc::new(lower_expr(func, cx)?),
            args: each(args)?,
        },

        CoreExprKind::Apply { func, args } => CoreExprKind::Apply {
            func: Rc::new(lower_expr(func, cx)?),
            args: each(args)?,
        },

        CoreExprKind::Expand { expr: inner } => CoreExprKind::Expand {
            expr: Rc::new(lower_expr(inner, cx)?),
        },

        // Leaf nodes: no lowering needed
        CoreExprKind::Literal(_)
        | CoreExprKind::Var { .. }
        | CoreExprKind::Quote(_)
        | CoreExprKind::Import { .. } => expr.kind.clone(),
    };

    Ok(CoreExpr {
        kind,
        source: expr.source.clone(),
    })
}

/// The expression that builds what `template` describes.
fn lower_template(
    template: &QuasiTemplate,
    cx: &Lowering<'_>,
) -> Result<CoreExpr, QuasiquoteError> {
    Ok(match template {
        QuasiTemplate::Datum(datum) => CoreExpr::new(CoreExprKind::Quote(*datum)),
        QuasiTemplate::Unquoted(expr) => lower_expr(expr, cx)?,
        // `(<constructor> arg1 arg2 ...)` with the primitive itself in
        // operator position.
        QuasiTemplate::Build(which, parts) => CoreExpr::new(CoreExprKind::App {
            func: Rc::new(CoreExpr::new(CoreExprKind::Literal(
                cx.constructor(*which)?,
            ))),
            args: parts
                .iter()
                .map(|part| lower_template(part, cx))
                .collect::<Result<_, _>>()?,
        }),
    })
}
