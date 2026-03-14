//! Quasiquote expansion for the VM compiler.
//!
//! Transforms `CoreExprKind::Quasiquote(template)` into `CoreExprKind::App`
//! calls to `cons`, `list`, `append`, `list->vector`, etc.
//!
//! This runs before the main 5-pass compiler pipeline so that the compiler
//! never needs to handle `Quasiquote` directly.

use patina_core::core_expr::{CoreExpr, CoreExprKind};
use patina_core::heap::SharedHeap;
use patina_core::tagged_value::TaggedValue;
use patina_frontend::Desugarer;
use patina_runtime::environment::Environment;
use std::rc::Rc;

/// Recursively expand all `Quasiquote` nodes in a `CoreExpr` tree.
///
/// Must be called before the compiler pipeline. Requires the shared heap
/// (to walk tagged-value templates) and the environment (to create a
/// desugarer for unquote sub-expressions).
pub fn expand_quasiquotes(expr: &CoreExpr, heap: &SharedHeap, env: &Rc<Environment>) -> CoreExpr {
    let desugarer = Desugarer::with_env(env.clone());
    expand_qq_expr(expr, heap, &desugarer)
}

/// Recursively walk a CoreExpr tree, expanding any Quasiquote nodes.
fn expand_qq_expr(expr: &CoreExpr, heap: &SharedHeap, desugarer: &Desugarer) -> CoreExpr {
    let kind = match &expr.kind {
        CoreExprKind::Quasiquote(template) => {
            return expand_template(*template, heap, desugarer, 0);
        }

        // Recursively walk child expressions
        CoreExprKind::Lambda {
            params,
            body,
            binding_scope,
        } => CoreExprKind::Lambda {
            params: params.clone(),
            body: body
                .iter()
                .map(|e| expand_qq_expr(e, heap, desugarer))
                .collect(),
            binding_scope: *binding_scope,
        },

        CoreExprKind::If { test, then, else_ } => CoreExprKind::If {
            test: Rc::new(expand_qq_expr(test, heap, desugarer)),
            then: Rc::new(expand_qq_expr(then, heap, desugarer)),
            else_: Rc::new(expand_qq_expr(else_, heap, desugarer)),
        },

        CoreExprKind::Set { var, scopes, value } => CoreExprKind::Set {
            var: var.clone(),
            scopes: scopes.clone(),
            value: Rc::new(expand_qq_expr(value, heap, desugarer)),
        },

        CoreExprKind::Begin(exprs) => CoreExprKind::Begin(
            exprs
                .iter()
                .map(|e| expand_qq_expr(e, heap, desugarer))
                .collect(),
        ),

        CoreExprKind::Define { name, value } => CoreExprKind::Define {
            name: name.clone(),
            value: Rc::new(expand_qq_expr(value, heap, desugarer)),
        },

        CoreExprKind::App { func, args } => CoreExprKind::App {
            func: Rc::new(expand_qq_expr(func, heap, desugarer)),
            args: args
                .iter()
                .map(|e| expand_qq_expr(e, heap, desugarer))
                .collect(),
        },

        CoreExprKind::Apply { func, args } => CoreExprKind::Apply {
            func: Rc::new(expand_qq_expr(func, heap, desugarer)),
            args: args
                .iter()
                .map(|e| expand_qq_expr(e, heap, desugarer))
                .collect(),
        },

        CoreExprKind::Expand { expr: inner } => CoreExprKind::Expand {
            expr: Rc::new(expand_qq_expr(inner, heap, desugarer)),
        },

        // Leaf nodes: no expansion needed
        CoreExprKind::Literal(_)
        | CoreExprKind::Var { .. }
        | CoreExprKind::Quote(_)
        | CoreExprKind::Import { .. } => expr.kind.clone(),
    };

    CoreExpr {
        kind,
        source: expr.source.clone(),
    }
}

// ─── Template expansion ──────────────────────────────────────────────────────

/// Expand a quasiquote template TaggedValue into a CoreExpr that constructs
/// the result at runtime.
fn expand_template(
    template: TaggedValue,
    heap: &SharedHeap,
    desugarer: &Desugarer,
    depth: i32,
) -> CoreExpr {
    // Self-evaluating atoms
    if template.is_fixnum() || template.is_boolean() || template.is_char() || template.is_null() {
        return CoreExpr::new(CoreExprKind::Quote(template));
    }

    // Symbols → quote as-is
    if heap.borrow().is_symbol(template) {
        return CoreExpr::new(CoreExprKind::Quote(template));
    }

    // Identifiers → convert to plain symbol (strip scope marks) for consistency
    // with the tree-walker's quasiquote evaluator, which does the same conversion.
    // Without this, identifiers inside quasiquote templates retain hygiene marks
    // and won't be `eq?` to the same-named symbol from a quote.
    if heap.borrow().is_identifier(template) {
        let name: Option<String> = heap
            .borrow()
            .get_symbol_or_identifier_name(template)
            .map(String::from);
        if let Some(name) = name {
            let sym = heap.borrow_mut().intern_symbol(&name);
            return CoreExpr::new(CoreExprKind::Quote(sym));
        }
        return CoreExpr::new(CoreExprKind::Quote(template));
    }

    // Strings, bytevectors → quote
    if template.is_string() || heap.borrow().is_bytevector(template) {
        return CoreExpr::new(CoreExprKind::Quote(template));
    }

    // Vectors: expand elements, use list->vector
    if template.is_vector() {
        return expand_vector_template(template, heap, desugarer, depth);
    }

    // Pairs: the interesting case
    if template.is_pair() {
        let (car, cdr) = {
            let h = heap.borrow();
            (h.car(template), h.cdr(template))
        };

        // Check for special forms at car
        let sym_name: Option<String> = heap
            .borrow()
            .get_symbol_or_identifier_name(car)
            .map(String::from);

        if let Some(ref name) = sym_name {
            match name.as_str() {
                "quasiquote" => {
                    // Nested quasiquote: increment depth
                    let (inner, _rest) = pair_parts(cdr, heap);
                    let expanded = expand_template(inner, heap, desugarer, depth + 1);
                    // Reconstruct (quasiquote <expanded>) using a plain symbol
                    // (car may be an identifier with scope marks; we need a bare symbol)
                    let qq_sym = heap.borrow_mut().intern_symbol("quasiquote");
                    return make_list_call(vec![
                        CoreExpr::new(CoreExprKind::Quote(qq_sym)),
                        expanded,
                    ]);
                }

                "unquote" => {
                    let (inner, _rest) = pair_parts(cdr, heap);
                    if depth == 0 {
                        // Evaluate the unquote expression
                        return desugar_tagged(inner, heap, desugarer);
                    } else {
                        // Inside nested quasiquote: decrement depth
                        let expanded = expand_template(inner, heap, desugarer, depth - 1);
                        let uq_sym = heap.borrow_mut().intern_symbol("unquote");
                        return make_list_call(vec![
                            CoreExpr::new(CoreExprKind::Quote(uq_sym)),
                            expanded,
                        ]);
                    }
                }

                "unquote-splicing" => {
                    if depth == 0 {
                        // Splicing at top level is an error in standard Scheme,
                        // but we just return the expanded form
                        let (inner, _rest) = pair_parts(cdr, heap);
                        return desugar_tagged(inner, heap, desugarer);
                    } else {
                        let (inner, _rest) = pair_parts(cdr, heap);
                        let expanded = expand_template(inner, heap, desugarer, depth - 1);
                        let uqs_sym = heap.borrow_mut().intern_symbol("unquote-splicing");
                        return make_list_call(vec![
                            CoreExpr::new(CoreExprKind::Quote(uqs_sym)),
                            expanded,
                        ]);
                    }
                }

                _ => {}
            }
        }

        // Regular pair: expand car and cdr, handling unquote-splicing in list context
        return expand_pair_template(template, heap, desugarer, depth);
    }

    // Other types: quote as-is
    CoreExpr::new(CoreExprKind::Quote(template))
}

/// Expand a pair/list template, handling unquote-splicing in list elements.
fn expand_pair_template(
    template: TaggedValue,
    heap: &SharedHeap,
    desugarer: &Desugarer,
    depth: i32,
) -> CoreExpr {
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
            tail_expr = Some(expand_template(current, heap, desugarer, depth));
            break;
        }

        let (car, cdr) = {
            let h = heap.borrow();
            (h.car(current), h.cdr(current))
        };

        // Check for tail unquote: current IS (unquote expr) — from dotted pair after splice
        if depth == 0 && heap.borrow().is_named(car, "unquote") && cdr.is_pair() {
            let (uq_expr, rest) = pair_parts(cdr, heap);
            if rest.is_null() {
                tail_expr = Some(desugar_tagged(uq_expr, heap, desugarer));
                break;
            }
        }

        // Check for (unquote-splicing expr) at this element position
        if depth == 0 && car.is_pair() {
            let (inner_car, inner_cdr) = {
                let h = heap.borrow();
                (h.car(car), h.cdr(car))
            };

            if heap.borrow().is_named(inner_car, "unquote-splicing") {
                let (splice_expr, _rest) = pair_parts(inner_cdr, heap);

                // Flush accumulated elements
                if !current_elems.is_empty() {
                    segments.push(Segment::List(std::mem::take(&mut current_elems)));
                }

                // Add splice segment
                segments.push(Segment::Splice(desugar_tagged(
                    splice_expr,
                    heap,
                    desugarer,
                )));

                current = cdr;
                continue;
            }
        }

        // Check for dotted-pair unquote: (a b . ,x)
        if depth == 0 && cdr.is_pair() {
            let (cdr_car, cdr_cdr) = {
                let h = heap.borrow();
                (h.car(cdr), h.cdr(cdr))
            };

            if heap.borrow().is_named(cdr_car, "unquote") && cdr_cdr.is_pair() {
                let (unquote_expr, rest) = pair_parts(cdr_cdr, heap);
                if rest.is_null() {
                    // This is (... car . ,expr)
                    current_elems.push(expand_template(car, heap, desugarer, depth));
                    tail_expr = Some(desugar_tagged(unquote_expr, heap, desugarer));
                    break;
                }
            }
        }

        // Regular element
        current_elems.push(expand_template(car, heap, desugarer, depth));
        current = cdr;
    }

    // Flush remaining elements
    if !current_elems.is_empty() {
        segments.push(Segment::List(std::mem::take(&mut current_elems)));
    }

    // Generate code from segments
    if segments.is_empty() {
        // Empty list
        return match tail_expr {
            Some(tail) => tail,
            None => CoreExpr::new(CoreExprKind::Quote(TaggedValue::NULL)),
        };
    }

    if segments.len() == 1 && tail_expr.is_none() {
        // Single segment, no tail
        return match segments.into_iter().next().unwrap() {
            Segment::List(elems) => make_list_call(elems),
            Segment::Splice(expr) => expr,
        };
    }

    // Multiple segments or has tail: use append
    let mut append_args: Vec<CoreExpr> = Vec::new();
    for seg in segments {
        match seg {
            Segment::List(elems) => append_args.push(make_list_call(elems)),
            Segment::Splice(expr) => append_args.push(expr),
        }
    }
    if let Some(tail) = tail_expr {
        append_args.push(tail);
    }

    // (append seg1 seg2 ... segN)
    make_app("append", append_args)
}

/// Expand a vector template: convert to list, expand with pair logic (handles splicing),
/// then convert back with list->vector.
fn expand_vector_template(
    template: TaggedValue,
    heap: &SharedHeap,
    desugarer: &Desugarer,
    depth: i32,
) -> CoreExpr {
    // Convert vector to a proper list on the heap, then use pair expansion
    // which handles unquote-splicing correctly.
    let borrowed = heap.borrow();
    let len = borrowed.vector_len(template);
    let elements: Vec<TaggedValue> = (0..len).map(|i| borrowed.vector_ref(template, i)).collect();
    drop(borrowed);

    // Build a heap list from the vector elements
    let list = {
        let mut h = heap.borrow_mut();
        let mut result = TaggedValue::NULL;
        for elem in elements.into_iter().rev() {
            result = h.alloc_pair(elem, result);
        }
        result
    };

    // Expand as a list (handles splicing, unquote, etc.)
    let list_expr = if list.is_null() {
        CoreExpr::new(CoreExprKind::Quote(TaggedValue::NULL))
    } else {
        expand_pair_template(list, heap, desugarer, depth)
    };

    // (list->vector <list-expr>)
    make_app("list->vector", vec![list_expr])
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
fn desugar_tagged(tv: TaggedValue, heap: &SharedHeap, desugarer: &Desugarer) -> CoreExpr {
    match desugarer.desugar_tagged(tv, heap) {
        Ok(core_expr) => {
            // Recursively expand any nested quasiquotes
            expand_qq_expr(&core_expr, heap, desugarer)
        }
        Err(e) => {
            panic!("quasiquote: failed to desugar unquote: {}", e);
        }
    }
}

/// Build `(list e1 e2 ... eN)` as a CoreExpr::App.
fn make_list_call(elems: Vec<CoreExpr>) -> CoreExpr {
    if elems.is_empty() {
        return CoreExpr::new(CoreExprKind::Quote(TaggedValue::NULL));
    }
    make_app("list", elems)
}

/// Build `(name arg1 arg2 ...)` where `name` is looked up as a global variable.
fn make_app(name: &str, args: Vec<CoreExpr>) -> CoreExpr {
    CoreExpr::new(CoreExprKind::App {
        func: Rc::new(CoreExpr::new(CoreExprKind::Var {
            name: Rc::from(name),
            scopes: patina_core::ScopeSet::new(),
        })),
        args,
    })
}

/// Build `(cons a b)`.
#[allow(dead_code)]
fn make_cons(car: CoreExpr, cdr: CoreExpr) -> CoreExpr {
    make_app("cons", vec![car, cdr])
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

    #[test]
    fn expand_self_evaluating() {
        let heap = make_heap();
        let env = make_env(&heap);
        let template = TaggedValue::fixnum(42);
        let expr = CoreExpr::new(CoreExprKind::Quasiquote(template));
        let expanded = expand_quasiquotes(&expr, &heap, &env);

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
        let expanded = expand_quasiquotes(&expr, &heap, &env);

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
        let expanded = expand_quasiquotes(&expr, &heap, &env);

        // Should become (list 'a 'b 'c)
        match &expanded.kind {
            CoreExprKind::App { func, args } => {
                assert_eq!(args.len(), 3);
                match &func.kind {
                    CoreExprKind::Var { name, .. } => assert_eq!(name.as_ref(), "list"),
                    other => panic!("expected Var(list), got {:?}", other),
                }
            }
            other => panic!("expected App, got {:?}", other),
        }
    }
}
