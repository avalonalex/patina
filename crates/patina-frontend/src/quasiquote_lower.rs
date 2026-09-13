//! Lowering `CoreExprKind::Quasiquote(template)` into `CoreExprKind::App`
//! calls to `list`, `append` and `list->vector`.
//!
//! **One implementation, for both backends.** It began as the VM compiler's
//! own pass while the tree-walker evaluated templates with a separate walker,
//! and the two derived "last", "list context" and "tail" independently — so
//! they disagreed on template shapes neither report pins down, the VM
//! answering where the tree-walker refused (issue #276). Deriving the
//! structure once, before either backend lowers anything, is what makes them
//! agree by construction rather than by keeping two files in step.
//!
//! Those three are the registry's own primitives, referenced as *values* —
//! a `Literal` in operator position — and not by name. A quasiquote denotes
//! the structure it writes, whatever `list` means where it appears: a
//! program that imports SRFI 101 has `cons` and `list` build random-access
//! lists, and `` `(1 ,x) `` in it must still be a pair. Looked up by name,
//! it was not, and every `(chibi test)` assertion under that import broke
//! on its own info alist (Larceny triage family 34). The tree-walker
//! builds the structure directly and never had the problem.
//!
//! This runs before the main 5-pass compiler pipeline so that the compiler
//! never needs to handle `Quasiquote` directly.

use crate::Desugarer;
use patina_core::core_expr::{CoreExpr, CoreExprKind};
use patina_core::heap::SharedHeap;
use patina_core::tagged_value::TaggedValue;
use patina_runtime::environment::Environment;
use std::cell::Cell;
use std::rc::Rc;

/// Why a template could not be lowered.
///
/// Two cases, and the distinction is the caller's to render: a `Desugar` is
/// the program's fault — `,if` names a syntactic keyword — while `Internal`
/// means a constructor the expansion needs was not supplied.
#[derive(Debug)]
pub enum QuasiquoteError {
    /// An unquoted sub-expression is not valid code.
    Desugar(String),
    /// A list constructor this pass calls could not be resolved.
    Internal(String),
}

impl std::fmt::Display for QuasiquoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuasiquoteError::Desugar(m) | QuasiquoteError::Internal(m) => write!(f, "{m}"),
        }
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

/// Recursively expand all `Quasiquote` nodes in a `CoreExpr` tree.
///
/// Must be called before either backend lowers the tree. Requires the shared
/// heap (to walk tagged-value templates), the environment (to create a
/// desugarer for unquote sub-expressions) and a resolver for the constructors
/// the expansion calls — see the module doc for why they are values rather
/// than names, and [`ConstructorResolver`] for why the caller supplies them.
pub fn lower_quasiquotes(
    expr: &CoreExpr,
    heap: &SharedHeap,
    env: &Rc<Environment>,
    constructors: ConstructorResolver<'_>,
) -> Result<CoreExpr, QuasiquoteError> {
    let cx = Expansion {
        desugarer: Desugarer::with_env(env.clone()),
        heap,
        constructors,
        list: Cell::default(),
        append: Cell::default(),
        list_to_vector: Cell::default(),
    };
    expand_qq_expr(expr, &cx)
}

/// What one compilation unit's templates expand through.
struct Expansion<'a> {
    /// For the unquoted sub-expressions, which are ordinary code.
    desugarer: Desugarer,
    heap: &'a SharedHeap,
    constructors: ConstructorResolver<'a>,
    /// The constructor procedures, allocated the first time a template needs
    /// each: most units have no quasiquote at all, and one that has a
    /// hundred shares three objects.
    list: Cell<Option<TaggedValue>>,
    append: Cell<Option<TaggedValue>>,
    list_to_vector: Cell<Option<TaggedValue>>,
}

/// The list constructors an expansion calls. `cons` is not among them: a
/// dotted tail goes through `append`, whose last argument may be anything.
#[derive(Clone, Copy)]
enum Constructor {
    List,
    Append,
    ListToVector,
}

impl Constructor {
    fn name(self) -> &'static str {
        match self {
            Constructor::List => "list",
            Constructor::Append => "append",
            Constructor::ListToVector => "list->vector",
        }
    }
}

impl Expansion<'_> {
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
    fn constructor(&self, which: Constructor) -> Result<TaggedValue, QuasiquoteError> {
        // Selected by `match`, so adding a constructor is a compile error
        // here rather than an out-of-bounds index at run time.
        let slot = match which {
            Constructor::List => &self.list,
            Constructor::Append => &self.append,
            Constructor::ListToVector => &self.list_to_vector,
        };
        if let Some(tv) = slot.get() {
            return Ok(tv);
        }
        let tv = (self.constructors)(which.name()).ok_or_else(|| {
            QuasiquoteError::Internal(format!(
                "quasiquote lowering needs the primitive {}, which is not registered",
                which.name()
            ))
        })?;
        slot.set(Some(tv));
        Ok(tv)
    }
}

/// Recursively walk a CoreExpr tree, expanding any Quasiquote nodes.
fn expand_qq_expr(expr: &CoreExpr, cx: &Expansion<'_>) -> Result<CoreExpr, QuasiquoteError> {
    let each = |exprs: &[CoreExpr]| -> Result<Vec<CoreExpr>, QuasiquoteError> {
        exprs.iter().map(|e| expand_qq_expr(e, cx)).collect()
    };
    let kind = match &expr.kind {
        CoreExprKind::Quasiquote(template) => {
            return expand_template(*template, cx, 0);
        }

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
            test: Rc::new(expand_qq_expr(test, cx)?),
            then: Rc::new(expand_qq_expr(then, cx)?),
            else_: Rc::new(expand_qq_expr(else_, cx)?),
        },

        CoreExprKind::Set { var, scopes, value } => CoreExprKind::Set {
            var: var.clone(),
            scopes: scopes.clone(),
            value: Rc::new(expand_qq_expr(value, cx)?),
        },

        CoreExprKind::Begin(exprs) => CoreExprKind::Begin(each(exprs)?),

        CoreExprKind::Define {
            name,
            scopes,
            value,
        } => CoreExprKind::Define {
            name: name.clone(),
            scopes: scopes.clone(),
            value: Rc::new(expand_qq_expr(value, cx)?),
        },

        CoreExprKind::App { func, args } => CoreExprKind::App {
            func: Rc::new(expand_qq_expr(func, cx)?),
            args: each(args)?,
        },

        CoreExprKind::Apply { func, args } => CoreExprKind::Apply {
            func: Rc::new(expand_qq_expr(func, cx)?),
            args: each(args)?,
        },

        CoreExprKind::Expand { expr: inner } => CoreExprKind::Expand {
            expr: Rc::new(expand_qq_expr(inner, cx)?),
        },

        // Leaf nodes: no expansion needed
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

// ─── Template expansion ──────────────────────────────────────────────────────

/// Expand a quasiquote template TaggedValue into a CoreExpr that constructs
/// the result at runtime.
fn expand_template(
    template: TaggedValue,
    cx: &Expansion<'_>,
    depth: i32,
) -> Result<CoreExpr, QuasiquoteError> {
    // Self-evaluating atoms
    if template.is_fixnum() || template.is_boolean() || template.is_char() || template.is_null() {
        return Ok(CoreExpr::new(CoreExprKind::Quote(template)));
    }

    // Symbols → quote as-is
    if cx.heap.borrow().is_symbol(template) {
        return Ok(CoreExpr::new(CoreExprKind::Quote(template)));
    }

    // Identifiers → convert to plain symbol (strip scope marks) for consistency
    // with the tree-walker's quasiquote evaluator, which does the same conversion.
    // Without this, identifiers inside quasiquote templates retain hygiene marks
    // and won't be `eq?` to the same-named symbol from a quote.
    if cx.heap.borrow().is_identifier(template) {
        let name: Option<String> = cx
            .heap
            .borrow()
            .get_symbol_or_identifier_name(template)
            .map(String::from);
        if let Some(name) = name {
            let sym = cx.heap.borrow_mut().intern_symbol(&name);
            return Ok(CoreExpr::new(CoreExprKind::Quote(sym)));
        }
        return Ok(CoreExpr::new(CoreExprKind::Quote(template)));
    }

    // Strings, bytevectors → quote
    if template.is_string() || cx.heap.borrow().is_bytevector(template) {
        return Ok(CoreExpr::new(CoreExprKind::Quote(template)));
    }

    // Vectors: expand elements, use list->vector
    if template.is_vector() {
        return expand_vector_template(template, cx, depth);
    }

    // Pairs: the interesting case
    if template.is_pair() {
        let (car, cdr) = {
            let h = cx.heap.borrow();
            (h.car(template), h.cdr(template))
        };

        // Check for special forms at car
        let sym_name: Option<String> = cx
            .heap
            .borrow()
            .get_symbol_or_identifier_name(car)
            .map(String::from);

        if let Some(ref name) = sym_name {
            match name.as_str() {
                "quasiquote" => {
                    // Nested quasiquote: increment depth
                    let (inner, _rest) = pair_parts(cdr, cx.heap, "quasiquote")?;
                    let expanded = expand_template(inner, cx, depth + 1)?;
                    // Reconstruct (quasiquote <expanded>) using a plain symbol
                    // (car may be an identifier with scope marks; we need a bare symbol)
                    let qq_sym = cx.heap.borrow_mut().intern_symbol("quasiquote");
                    return make_list_call(
                        cx,
                        vec![CoreExpr::new(CoreExprKind::Quote(qq_sym)), expanded],
                    );
                }

                "unquote" => {
                    if depth == 0 {
                        // A template that *is* an unquote, rather than one
                        // holding an element that is. R6RS's multi-operand
                        // form is a splice, and a splice belongs in a list or
                        // vector template, so there is nothing here for
                        // several operands to be spliced into; one operand it
                        // is, and `pair_parts` refuses none.
                        let (inner, _rest) = pair_parts(cdr, cx.heap, "unquote")?;
                        return desugar_tagged(inner, cx);
                    }
                    // Inside a nested quasiquote the form is rebuilt as data,
                    // so the operand *list* is what must survive: expanded one
                    // level shallower, as the list template it is. Rebuilding
                    // a fixed two elements dropped every operand after the
                    // first, which is what made ``(foo ,,@q) lose its splice.
                    return rebuild_unquotation(cdr, cx, depth, "unquote");
                }

                "unquote-splicing" => {
                    if depth == 0 {
                        // Splicing where there is no list to splice into.
                        // R6RS confines a splice to a list or vector template
                        // and Gauche raises here; Patina inserts the value,
                        // which is what it has always done and what the
                        // register records against both oracles.
                        let (inner, _rest) = pair_parts(cdr, cx.heap, "unquote-splicing")?;
                        return desugar_tagged(inner, cx);
                    }
                    return rebuild_unquotation(cdr, cx, depth, "unquote-splicing");
                }

                _ => {}
            }
        }

        // Regular pair: expand car and cdr, handling unquote-splicing in list context
        return expand_pair_template(template, cx, depth);
    }

    // Other types: quote as-is
    Ok(CoreExpr::new(CoreExprKind::Quote(template)))
}

/// Rebuild `(<keyword> . operands)` as data, one quasiquote level shallower.
///
/// Reached only at depth > 0, where the form is not evaluated but written
/// back into the structure. The operands are expanded as the list template
/// they are, so a splice among them still splices —
/// ``(foo ,,@q) rebuilds as `(foo (unquote <the elements of q>)) — and then
/// the keyword is consed on. `append` rather than a cons because the
/// constructor set deliberately has none; see `Constructor`.
fn rebuild_unquotation(
    operands: TaggedValue,
    cx: &Expansion<'_>,
    depth: i32,
    keyword: &str,
) -> Result<CoreExpr, QuasiquoteError> {
    let expanded_operands = expand_pair_template(operands, cx, depth - 1)?;
    let sym = cx.heap.borrow_mut().intern_symbol(keyword);
    let head = make_list_call(cx, vec![CoreExpr::new(CoreExprKind::Quote(sym))])?;
    make_app(cx, Constructor::Append, vec![head, expanded_operands])
}

/// Expand a pair/list template, handling unquote-splicing in list elements.
fn expand_pair_template(
    template: TaggedValue,
    cx: &Expansion<'_>,
    depth: i32,
) -> Result<CoreExpr, QuasiquoteError> {
    // Collect segments: each segment is either a list of normal elements
    // or a splice expression. This lets us generate efficient code:
    //   `(a b ,@xs c d) → (append (list 'a 'b) xs (list 'c 'd))
    let mut segments: Vec<Segment> = Vec::new();
    let mut current_elems: Vec<CoreExpr> = Vec::new();
    let mut current = template;
    let mut tail_expr: Option<CoreExpr> = None;

    loop {
        if current.is_null() {
            break;
        }

        if !current.is_pair() {
            // Improper list tail
            tail_expr = Some(expand_template(current, cx, depth)?);
            break;
        }

        let (car, cdr) = {
            let h = cx.heap.borrow();
            (h.car(current), h.cdr(current))
        };

        // Check for tail unquote: current IS (unquote expr) — from dotted pair after splice
        if depth == 0 && cx.heap.borrow().is_named(car, "unquote") && cdr.is_pair() {
            let (uq_expr, rest) = pair_parts(cdr, cx.heap, "unquote")?;
            if rest.is_null() {
                tail_expr = Some(desugar_tagged(uq_expr, cx)?);
                break;
            }
        }

        // An element that is itself `(unquote …)` or `(unquote-splicing …)`.
        //
        // Both take any number of operands, which is the R6RS 11.17 reading:
        // `(unquote e1 … en)` inserts n values and the splicing spelling
        // splices n lists. R7RS 7.1.4 admits exactly one of each, so this is
        // an extension — a deliberate one, matching Gauche, Chez and Larceny,
        // whose suite asserts it. chibi and Racket take the other reading and
        // the register records both.
        //
        // The one-operand case is the whole of ordinary code and stays on the
        // cheap path: an unquote contributes to `current_elems` like any
        // element, so `(a ,x b)` remains one `list` call rather than an
        // `append` of three segments — the shape family 34's review measured
        // at +40% when it was routed through segments.
        if depth == 0 && car.is_pair() {
            let (inner_car, inner_cdr) = {
                let h = cx.heap.borrow();
                (h.car(car), h.cdr(car))
            };
            let splicing = cx.heap.borrow().is_named(inner_car, "unquote-splicing");
            let unquoting = cx.heap.borrow().is_named(inner_car, "unquote");

            if splicing || unquoting {
                let keyword = if splicing {
                    "unquote-splicing"
                } else {
                    "unquote"
                };
                let operands = operand_list(inner_cdr, cx.heap).ok_or_else(|| {
                    QuasiquoteError::Desugar(format!("{keyword}: operands must be a proper list"))
                })?;
                for operand in operands {
                    let expanded = desugar_tagged(operand, cx)?;
                    if splicing {
                        // A splice ends the run of plain elements before it.
                        if !current_elems.is_empty() {
                            segments.push(Segment::List(std::mem::take(&mut current_elems)));
                        }
                        segments.push(Segment::Splice(expanded));
                    } else {
                        current_elems.push(expanded);
                    }
                }
                current = cdr;
                continue;
            }
        }

        // Check for dotted-pair unquote: (a b . ,x)
        if depth == 0 && cdr.is_pair() {
            let (cdr_car, cdr_cdr) = {
                let h = cx.heap.borrow();
                (h.car(cdr), h.cdr(cdr))
            };

            if cx.heap.borrow().is_named(cdr_car, "unquote") && cdr_cdr.is_pair() {
                let (unquote_expr, rest) = pair_parts(cdr_cdr, cx.heap, "unquote")?;
                if rest.is_null() {
                    // This is (... car . ,expr)
                    current_elems.push(expand_template(car, cx, depth)?);
                    tail_expr = Some(desugar_tagged(unquote_expr, cx)?);
                    break;
                }
            }
        }

        // Regular element
        current_elems.push(expand_template(car, cx, depth)?);
        current = cdr;
    }

    // Flush remaining elements
    if !current_elems.is_empty() {
        segments.push(Segment::List(std::mem::take(&mut current_elems)));
    }

    // Generate code from segments
    if segments.is_empty() {
        // Empty list
        return Ok(match tail_expr {
            Some(tail) => tail,
            None => CoreExpr::new(CoreExprKind::Quote(TaggedValue::NULL)),
        });
    }

    if segments.len() == 1 && tail_expr.is_none() {
        // Single segment, no tail
        return Ok(match segments.into_iter().next().unwrap() {
            Segment::List(elems) => make_list_call(cx, elems)?,
            Segment::Splice(expr) => expr,
        });
    }

    // Multiple segments or has tail: use append
    let mut append_args: Vec<CoreExpr> = Vec::new();
    for seg in segments {
        match seg {
            Segment::List(elems) => append_args.push(make_list_call(cx, elems)?),
            Segment::Splice(expr) => append_args.push(expr),
        }
    }
    if let Some(tail) = tail_expr {
        append_args.push(tail);
    }

    // (append seg1 seg2 ... segN)
    make_app(cx, Constructor::Append, append_args)
}

/// Expand a vector template: convert to list, expand with pair logic (handles splicing),
/// then convert back with list->vector.
fn expand_vector_template(
    template: TaggedValue,
    cx: &Expansion<'_>,
    depth: i32,
) -> Result<CoreExpr, QuasiquoteError> {
    // Convert vector to a proper list on the heap, then use pair expansion
    // which handles unquote-splicing correctly.
    let elements = cx.heap.borrow().vector_slice(template).to_vec();

    // Build a heap list from the vector elements
    let list = cx.heap.borrow_mut().list_from_iter(elements);

    // Expand as a list (handles splicing, unquote, etc.)
    let list_expr = if list.is_null() {
        CoreExpr::new(CoreExprKind::Quote(TaggedValue::NULL))
    } else {
        expand_pair_template(list, cx, depth)?
    };

    // (list->vector <list-expr>)
    make_app(cx, Constructor::ListToVector, vec![list_expr])
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

enum Segment {
    List(Vec<CoreExpr>),
    Splice(CoreExpr),
}

/// The operands of an `(unquote …)` or `(unquote-splicing …)` form, as a
/// proper list.
///
/// R6RS 11.17 writes both with a `*`: `(unquote <qq template D-1>*)`. R7RS
/// 7.1.4 gives each exactly one operand and 4.2.8 makes anything else an
/// error, so accepting a list here is an extension, taken deliberately —
/// Gauche, Chez and Larceny read it this way and Larceny's suite asserts it.
/// `None` means the form is improper, which no reading accepts.
fn operand_list(cdr: TaggedValue, heap: &SharedHeap) -> Option<Vec<TaggedValue>> {
    let h = heap.borrow();
    let mut out = Vec::new();
    let mut current = cdr;
    while !current.is_null() {
        if !current.is_pair() {
            return None;
        }
        out.push(h.car(current));
        current = h.cdr(current);
    }
    Some(out)
}

/// Get car and cdr from what a template promised would be a pair.
///
/// Checked, because the operand list of `(unquote …)` is written by the
/// program and can be empty: `` `(a (unquote)) `` reaches here with `tv`
/// null. `Heap::car` on a non-pair is a `debug_assert` and, in release, a
/// read of whatever the tagged value points at — that pair of behaviours is
/// what this returns an error instead of. R7RS 7.1.4 gives `unquote` exactly
/// one template, so a form with none is not a `<qq template>` and saying so
/// is the whole fix; whether to *accept* it, as R6RS 11.17's zero-or-more
/// grammar and Gauche do, is a separate decision this does not take.
fn pair_parts(
    tv: TaggedValue,
    heap: &SharedHeap,
    form: &str,
) -> Result<(TaggedValue, TaggedValue), QuasiquoteError> {
    if !tv.is_pair() {
        return Err(QuasiquoteError::Desugar(format!(
            "{form}: expected one expression after the keyword"
        )));
    }
    let h = heap.borrow();
    Ok((h.car(tv), h.cdr(tv)))
}

/// Desugar a TaggedValue expression (from an unquote) into CoreExpr.
///
/// The failure is a real program error, not an internal one: `,if` names a
/// syntactic keyword, and #89 made the desugarer say so. Reporting it as
/// itself is what lets `` `(1 ,if) `` produce the same diagnostic the bare
/// `if` gets, instead of the panic this used to be.
fn desugar_tagged(tv: TaggedValue, cx: &Expansion<'_>) -> Result<CoreExpr, QuasiquoteError> {
    let core_expr = cx
        .desugarer
        .desugar_tagged(tv, cx.heap)
        .map_err(|e| QuasiquoteError::Desugar(e.to_string()))?;
    // Recursively expand any nested quasiquotes
    expand_qq_expr(&core_expr, cx)
}

/// Build `(list e1 e2 ... eN)` as a CoreExpr::App.
fn make_list_call(cx: &Expansion<'_>, elems: Vec<CoreExpr>) -> Result<CoreExpr, QuasiquoteError> {
    if elems.is_empty() {
        return Ok(CoreExpr::new(CoreExprKind::Quote(TaggedValue::NULL)));
    }
    make_app(cx, Constructor::List, elems)
}

/// Build `(<constructor> arg1 arg2 ...)` with the primitive itself in
/// operator position.
fn make_app(
    cx: &Expansion<'_>,
    which: Constructor,
    args: Vec<CoreExpr>,
) -> Result<CoreExpr, QuasiquoteError> {
    Ok(CoreExpr::new(CoreExprKind::App {
        func: Rc::new(CoreExpr::new(CoreExprKind::Literal(cx.constructor(which)?))),
        args,
    }))
}
