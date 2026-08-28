//! Quasiquote expansion for the VM compiler.
//!
//! Transforms `CoreExprKind::Quasiquote(template)` into `CoreExprKind::App`
//! calls to `list`, `append` and `list->vector`.
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

use crate::error::CompileError;
use patina_core::core_expr::{CoreExpr, CoreExprKind};
use patina_core::heap::SharedHeap;
use patina_core::procedure::Procedure;
use patina_core::tagged_value::TaggedValue;
use patina_frontend::Desugarer;
use patina_primitives::PrimitiveRegistry;
use patina_runtime::environment::Environment;
use std::cell::Cell;
use std::rc::Rc;

/// Recursively expand all `Quasiquote` nodes in a `CoreExpr` tree.
///
/// Must be called before the compiler pipeline. Requires the shared heap
/// (to walk tagged-value templates), the environment (to create a desugarer
/// for unquote sub-expressions) and the registry (for the constructors the
/// expansion calls — see the module doc for why they are not looked up by
/// name).
pub fn expand_quasiquotes(
    expr: &CoreExpr,
    heap: &SharedHeap,
    env: &Rc<Environment>,
    registry: &PrimitiveRegistry,
) -> Result<CoreExpr, CompileError> {
    let cx = Expansion {
        desugarer: Desugarer::with_env(env.clone()),
        heap,
        registry,
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
    registry: &'a PrimitiveRegistry,
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
    /// Built the way `VmState::install_primitives` builds every primitive,
    /// registry index included, so a call through it dispatches by index
    /// like a call through the global of the same name would. It differs
    /// from that global in one way only: nothing the program imports or
    /// defines can redirect it.
    fn constructor(&self, which: Constructor) -> Result<TaggedValue, CompileError> {
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
        let qualified_name = format!("scheme.base/{}", which.name());
        let (index, prim) = self
            .registry
            .resolve_index(&qualified_name)
            .and_then(|i| self.registry.get_by_index(i).map(|p| (i, p)))
            .ok_or_else(|| {
                CompileError::Internal(format!(
                    "quasiquote expansion needs the primitive {qualified_name}, which is not registered"
                ))
            })?;
        let proc = Procedure::primitive(
            prim.name,
            prim.arity.clone(),
            Rc::from(qualified_name.as_str()),
            Some(index),
        );
        let tv = self.heap.borrow_mut().alloc_procedure(proc);
        slot.set(Some(tv));
        Ok(tv)
    }
}

/// Recursively walk a CoreExpr tree, expanding any Quasiquote nodes.
fn expand_qq_expr(expr: &CoreExpr, cx: &Expansion<'_>) -> Result<CoreExpr, CompileError> {
    let each = |exprs: &[CoreExpr]| -> Result<Vec<CoreExpr>, CompileError> {
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
) -> Result<CoreExpr, CompileError> {
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
                    let (inner, _rest) = pair_parts(cdr, cx.heap);
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
                    let (inner, _rest) = pair_parts(cdr, cx.heap);
                    if depth == 0 {
                        // Evaluate the unquote expression
                        return desugar_tagged(inner, cx);
                    } else {
                        // Inside nested quasiquote: decrement depth
                        let expanded = expand_template(inner, cx, depth - 1)?;
                        let uq_sym = cx.heap.borrow_mut().intern_symbol("unquote");
                        return make_list_call(
                            cx,
                            vec![CoreExpr::new(CoreExprKind::Quote(uq_sym)), expanded],
                        );
                    }
                }

                "unquote-splicing" => {
                    if depth == 0 {
                        // Splicing at top level is an error in standard Scheme,
                        // but we just return the expanded form
                        let (inner, _rest) = pair_parts(cdr, cx.heap);
                        return desugar_tagged(inner, cx);
                    } else {
                        let (inner, _rest) = pair_parts(cdr, cx.heap);
                        let expanded = expand_template(inner, cx, depth - 1)?;
                        let uqs_sym = cx.heap.borrow_mut().intern_symbol("unquote-splicing");
                        return make_list_call(
                            cx,
                            vec![CoreExpr::new(CoreExprKind::Quote(uqs_sym)), expanded],
                        );
                    }
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

/// Expand a pair/list template, handling unquote-splicing in list elements.
fn expand_pair_template(
    template: TaggedValue,
    cx: &Expansion<'_>,
    depth: i32,
) -> Result<CoreExpr, CompileError> {
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
            let (uq_expr, rest) = pair_parts(cdr, cx.heap);
            if rest.is_null() {
                tail_expr = Some(desugar_tagged(uq_expr, cx)?);
                break;
            }
        }

        // Check for (unquote-splicing expr) at this element position
        if depth == 0 && car.is_pair() {
            let (inner_car, inner_cdr) = {
                let h = cx.heap.borrow();
                (h.car(car), h.cdr(car))
            };

            if cx.heap.borrow().is_named(inner_car, "unquote-splicing") {
                let (splice_expr, _rest) = pair_parts(inner_cdr, cx.heap);

                // Flush accumulated elements
                if !current_elems.is_empty() {
                    segments.push(Segment::List(std::mem::take(&mut current_elems)));
                }

                // Add splice segment
                segments.push(Segment::Splice(desugar_tagged(splice_expr, cx)?));

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
                let (unquote_expr, rest) = pair_parts(cdr_cdr, cx.heap);
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
) -> Result<CoreExpr, CompileError> {
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

/// Get car and cdr from a pair.
fn pair_parts(tv: TaggedValue, heap: &SharedHeap) -> (TaggedValue, TaggedValue) {
    let h = heap.borrow();
    (h.car(tv), h.cdr(tv))
}

/// Desugar a TaggedValue expression (from an unquote) into CoreExpr.
///
/// The failure is a real program error, not an internal one: `,if` names a
/// syntactic keyword, and #89 made the desugarer say so. Reporting it as
/// itself is what lets `` `(1 ,if) `` produce the same diagnostic the bare
/// `if` gets, instead of the panic this used to be.
fn desugar_tagged(tv: TaggedValue, cx: &Expansion<'_>) -> Result<CoreExpr, CompileError> {
    let core_expr = cx
        .desugarer
        .desugar_tagged(tv, cx.heap)
        .map_err(|e| CompileError::Desugar(e.to_string()))?;
    // Recursively expand any nested quasiquotes
    expand_qq_expr(&core_expr, cx)
}

/// Build `(list e1 e2 ... eN)` as a CoreExpr::App.
fn make_list_call(cx: &Expansion<'_>, elems: Vec<CoreExpr>) -> Result<CoreExpr, CompileError> {
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
) -> Result<CoreExpr, CompileError> {
    Ok(CoreExpr::new(CoreExprKind::App {
        func: Rc::new(CoreExpr::new(CoreExprKind::Literal(cx.constructor(which)?))),
        args,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_core::heap::Heap;
    use patina_core::tagged_value::TaggedValue;
    use std::cell::RefCell;

    fn make_heap() -> SharedHeap {
        Rc::new(RefCell::new(Heap::new()))
    }

    fn make_env(heap: &SharedHeap) -> Rc<Environment> {
        Rc::new(Environment::with_heap(heap.clone()))
    }

    fn make_registry() -> PrimitiveRegistry {
        let mut registry = PrimitiveRegistry::new();
        patina_primitives::register_all(&mut registry);
        registry
    }

    #[test]
    fn expand_self_evaluating() {
        let heap = make_heap();
        let env = make_env(&heap);
        let template = TaggedValue::fixnum(42);
        let expr = CoreExpr::new(CoreExprKind::Quasiquote(template));
        let expanded =
            expand_quasiquotes(&expr, &heap, &env, &make_registry()).expect("template desugars");

        match &expanded.kind {
            CoreExprKind::Quote(v) => assert_eq!(v.as_fixnum(), Some(42)),
            other => panic!("expected Quote, got {:?}", other),
        }
    }

    #[test]
    fn expand_symbol() {
        let heap = make_heap();
        let env = make_env(&heap);
        let sym = heap.borrow_mut().intern_symbol("foo");
        let expr = CoreExpr::new(CoreExprKind::Quasiquote(sym));
        let expanded =
            expand_quasiquotes(&expr, &heap, &env, &make_registry()).expect("template desugars");

        match &expanded.kind {
            CoreExprKind::Quote(v) => assert!(heap.borrow().is_symbol(*v)),
            other => panic!("expected Quote, got {:?}", other),
        }
    }

    #[test]
    fn expand_list_no_unquotes() {
        let heap = make_heap();
        let env = make_env(&heap);
        let a = heap.borrow_mut().intern_symbol("a");
        let b = heap.borrow_mut().intern_symbol("b");
        let c = heap.borrow_mut().intern_symbol("c");
        let template = {
            let mut h = heap.borrow_mut();
            let t3 = h.alloc_pair(c, TaggedValue::NULL);
            let t2 = h.alloc_pair(b, t3);
            h.alloc_pair(a, t2)
        };
        let expr = CoreExpr::new(CoreExprKind::Quasiquote(template));
        let expanded =
            expand_quasiquotes(&expr, &heap, &env, &make_registry()).expect("template desugars");

        // Should become (<list primitive> 'a 'b 'c) — the primitive itself,
        // not a reference to whatever `list` names where the template sits.
        match &expanded.kind {
            CoreExprKind::App { func, args } => {
                assert_eq!(args.len(), 3);
                match &func.kind {
                    CoreExprKind::Literal(v) => {
                        let proc = heap.borrow().get_procedure(*v).expect("a procedure");
                        match proc.as_ref() {
                            Procedure::Primitive { qualified_name, .. } => {
                                assert_eq!(&**qualified_name, "scheme.base/list")
                            }
                            other => panic!("expected the list primitive, got {other:?}"),
                        }
                    }
                    other => panic!("expected Literal(<list primitive>), got {:?}", other),
                }
            }
            other => panic!("expected App, got {:?}", other),
        }
    }
}
