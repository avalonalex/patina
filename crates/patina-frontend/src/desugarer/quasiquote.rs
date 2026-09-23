//! Deriving what a quasiquote template builds — the desugarer's half of
//! quasiquote (`patina_core::QuasiTemplate` says why there are two).
//!
//! **Where it runs is the point.** An unquoted expression is ordinary code,
//! desugared here by the desugarer that met the `quasiquote` — so with the
//! form's local keywords, its local variables and its early bindings. It used
//! to be desugared after the fact, by the lowering pass, with a fresh
//! desugarer over the global environment: a `let-syntax` keyword was unbound
//! inside an unquote, a local variable spelled like a macro was taken for
//! the macro, and a library template's reference was not bound early the way
//! the same reference outside the template was (#445).
//!
//! **One derivation, for both backends.** It began as the VM compiler's own
//! pass while the tree-walker evaluated templates with a separate walker, and
//! the two derived "last", "list context" and "tail" independently — so they
//! disagreed on template shapes neither report pins down, the VM answering
//! where the tree-walker refused (issue #276). Deriving the structure once,
//! before either backend lowers anything, is what makes them agree by
//! construction rather than by keeping two files in step.

use super::{DesugarError, Desugarer, Result};
use patina_core::{QuasiConstructor, QuasiTemplate, SharedHeap, TaggedValue};
use std::rc::Rc;

impl Desugarer {
    /// The structure `template` builds, with its unquoted expressions
    /// desugared here.
    pub(super) fn derive_quasi_template(
        &self,
        template: TaggedValue,
        shared_heap: &SharedHeap,
    ) -> Result<QuasiTemplate> {
        Derivation {
            desugarer: self,
            heap: shared_heap,
        }
        .template(template, 0)
    }
}

/// What one template is derived through.
struct Derivation<'a> {
    /// For the unquoted sub-expressions, which are ordinary code.
    desugarer: &'a Desugarer,
    heap: &'a SharedHeap,
}

impl Derivation<'_> {
    /// Derive a template at quasiquotation `depth`: zero at the outermost
    /// level, where an `unquote` is evaluated.
    fn template(&self, template: TaggedValue, depth: i32) -> Result<QuasiTemplate> {
        let heap = self.heap;

        // Self-evaluating atoms
        if template.is_fixnum() || template.is_boolean() || template.is_char() || template.is_null()
        {
            return Ok(QuasiTemplate::Datum(template));
        }

        // Symbols → quote as-is
        if heap.borrow().is_symbol(template) {
            return Ok(QuasiTemplate::Datum(template));
        }

        // Identifiers → convert to plain symbol (strip scope marks), as
        // `quote` does. Without this, identifiers inside quasiquote templates
        // retain hygiene marks and won't be `eq?` to the same-named symbol
        // from a quote.
        if heap.borrow().is_identifier(template) {
            let name: Option<String> = heap
                .borrow()
                .get_symbol_or_identifier_name(template)
                .map(String::from);
            if let Some(name) = name {
                let sym = heap.borrow_mut().intern_symbol(&name);
                return Ok(QuasiTemplate::Datum(sym));
            }
            return Ok(QuasiTemplate::Datum(template));
        }

        // Strings, bytevectors → quote
        if template.is_string() || heap.borrow().is_bytevector(template) {
            return Ok(QuasiTemplate::Datum(template));
        }

        // Vectors: expand elements, use list->vector
        if template.is_vector() {
            return self.vector_template(template, depth);
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
                        let (inner, _rest) = pair_parts(cdr, heap, "quasiquote")?;
                        let derived = self.template(inner, depth + 1)?;
                        // Reconstruct (quasiquote <derived>) using a plain
                        // symbol (car may be an identifier with scope marks;
                        // we need a bare symbol)
                        let qq_sym = heap.borrow_mut().intern_symbol("quasiquote");
                        return Ok(list_of(vec![QuasiTemplate::Datum(qq_sym), derived]));
                    }

                    "unquote" => {
                        if depth == 0 {
                            // A template that *is* an unquote, rather than one
                            // holding an element that is. R6RS's multi-operand
                            // form is a splice and a splice belongs in a list
                            // or vector template, so there is nothing here to
                            // splice into: one operand, and `pair_parts`
                            // refuses none.
                            //
                            // Extra operands are refused rather than dropped.
                            // Taking the first silently was the behaviour
                            // before multi-operand unquote existed, and once
                            // every other position inserts all of them,
                            // accepting a form here and discarding half of it
                            // is the one answer that teaches the reader
                            // something false.
                            let operands = operand_list(cdr, heap).ok_or_else(|| {
                                invalid("unquote: operands must be a proper, finite list")
                            })?;
                            let [inner] = operands.as_slice() else {
                                return Err(invalid(&format!(
                                    "unquote: a template that is itself an unquote takes one \
                                     expression, not {}; several can only be spliced into a \
                                     list or vector template",
                                    operands.len()
                                )));
                            };
                            return self.unquoted(*inner);
                        }
                        // Inside a nested quasiquote the form is rebuilt as
                        // data, so the operand *list* is what must survive:
                        // derived one level shallower, as the list template it
                        // is. Rebuilding a fixed two elements dropped every
                        // operand after the first, which is what made
                        // ``(foo ,,@q) lose its splice.
                        return self.rebuild_unquotation(cdr, depth, "unquote");
                    }

                    "unquote-splicing" => {
                        if depth == 0 {
                            // Splicing where there is no list to splice into.
                            // R6RS confines a splice to a list or vector
                            // template and Gauche raises here; Patina inserts
                            // the value, which is what it has always done and
                            // what the register records against both oracles.
                            let (inner, _rest) = pair_parts(cdr, heap, "unquote-splicing")?;
                            return self.unquoted(inner);
                        }
                        return self.rebuild_unquotation(cdr, depth, "unquote-splicing");
                    }

                    _ => {}
                }
            }

            // Regular pair: derive car and cdr, handling unquote-splicing in
            // list context
            return self.pair_template(template, depth);
        }

        // Other types: quote as-is
        Ok(QuasiTemplate::Datum(template))
    }

    /// Rebuild `(<keyword> . operands)` as data, one quasiquote level
    /// shallower.
    ///
    /// Reached only at depth > 0, where the form is not evaluated but written
    /// back into the structure. The operands are derived as the list template
    /// they are, so a splice among them still splices —
    /// ``(foo ,,@q) rebuilds as `(foo (unquote <the elements of q>)) — and
    /// then the keyword is consed on. `append` rather than a cons because the
    /// constructor set deliberately has none; see `QuasiConstructor`.
    fn rebuild_unquotation(
        &self,
        operands: TaggedValue,
        depth: i32,
        keyword: &str,
    ) -> Result<QuasiTemplate> {
        // Checked here as well as in element position, so that whether a
        // form is well-formed does not depend on how deeply it is nested:
        // `(unquote . x)` was refused at depth 0 and quietly rebuilt at depth
        // 1, because `pair_template` reads an improper tail as a dotted list.
        if operand_list(operands, self.heap).is_none() {
            return Err(invalid(&format!(
                "{keyword}: operands must be a proper, finite list"
            )));
        }
        let derived_operands = self.pair_template(operands, depth - 1)?;
        let sym = self.heap.borrow_mut().intern_symbol(keyword);
        let head = list_of(vec![QuasiTemplate::Datum(sym)]);
        Ok(QuasiTemplate::Build(
            QuasiConstructor::Append,
            vec![head, derived_operands],
        ))
    }

    /// Derive a pair/list template, handling unquote-splicing in list
    /// elements.
    fn pair_template(&self, template: TaggedValue, depth: i32) -> Result<QuasiTemplate> {
        let heap = self.heap;
        // Collect segments: each segment is either a list of normal elements
        // or a splice expression. This lets us generate efficient code:
        //   `(a b ,@xs c d) → (append (list 'a 'b) xs (list 'c 'd))
        let mut segments: Vec<Segment> = Vec::new();
        let mut current_elems: Vec<QuasiTemplate> = Vec::new();
        let mut current = template;
        let mut tail: Option<QuasiTemplate> = None;

        loop {
            if current.is_null() {
                break;
            }

            if !current.is_pair() {
                // Improper list tail
                tail = Some(self.template(current, depth)?);
                break;
            }

            let (car, cdr) = {
                let h = heap.borrow();
                (h.car(current), h.cdr(current))
            };

            // Check for tail unquote: current IS (unquote expr) — from dotted
            // pair after splice
            if depth == 0 && heap.borrow().is_named(car, "unquote") && cdr.is_pair() {
                let (uq_expr, rest) = pair_parts(cdr, heap, "unquote")?;
                if rest.is_null() {
                    tail = Some(self.unquoted(uq_expr)?);
                    break;
                }
            }

            // An element that is itself `(unquote …)` or `(unquote-splicing …)`.
            //
            // Both take any number of operands, which is the R6RS 11.17
            // reading: `(unquote e1 … en)` inserts n values and the splicing
            // spelling splices n lists. R7RS 7.1.4 admits exactly one of each,
            // so this is an extension — a deliberate one, matching Gauche,
            // Chez and Larceny, whose suite asserts it. chibi and Racket take
            // the other reading and the register records both.
            //
            // The one-operand case is the whole of ordinary code and stays on
            // the cheap path: an unquote contributes to `current_elems` like
            // any element, so `(a ,x b)` remains one `list` call rather than
            // an `append` of three segments — the shape family 34's review
            // measured at +40% when it was routed through segments.
            if depth == 0 && car.is_pair() {
                let (inner_car, inner_cdr) = {
                    let h = heap.borrow();
                    (h.car(car), h.cdr(car))
                };
                let (splicing, unquoting) = {
                    let h = heap.borrow();
                    (
                        h.is_named(inner_car, "unquote-splicing"),
                        h.is_named(inner_car, "unquote"),
                    )
                };

                if splicing || unquoting {
                    let keyword = if splicing {
                        "unquote-splicing"
                    } else {
                        "unquote"
                    };
                    let operands = operand_list(inner_cdr, heap).ok_or_else(|| {
                        invalid(&format!(
                            "{keyword}: operands must be a proper, finite list"
                        ))
                    })?;
                    for operand in operands {
                        let derived = self.unquoted(operand)?;
                        if splicing {
                            // A splice ends the run of plain elements before it.
                            if !current_elems.is_empty() {
                                segments.push(Segment::List(std::mem::take(&mut current_elems)));
                            }
                            segments.push(Segment::Splice(derived));
                        } else {
                            current_elems.push(derived);
                        }
                    }
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
                    let (unquote_expr, rest) = pair_parts(cdr_cdr, heap, "unquote")?;
                    if rest.is_null() {
                        // This is (... car . ,expr)
                        current_elems.push(self.template(car, depth)?);
                        tail = Some(self.unquoted(unquote_expr)?);
                        break;
                    }
                }
            }

            // Regular element
            current_elems.push(self.template(car, depth)?);
            current = cdr;
        }

        // Flush remaining elements
        if !current_elems.is_empty() {
            segments.push(Segment::List(std::mem::take(&mut current_elems)));
        }

        // Build from segments
        if segments.is_empty() {
            // Empty list
            return Ok(tail.unwrap_or(QuasiTemplate::Datum(TaggedValue::NULL)));
        }

        if segments.len() == 1 && tail.is_none() {
            // Single segment, no tail
            return Ok(match segments.into_iter().next().unwrap() {
                Segment::List(elems) => list_of(elems),
                Segment::Splice(expr) => expr,
            });
        }

        // Multiple segments or has tail: use append
        let mut append_args: Vec<QuasiTemplate> = segments
            .into_iter()
            .map(|seg| match seg {
                Segment::List(elems) => list_of(elems),
                Segment::Splice(expr) => expr,
            })
            .collect();
        append_args.extend(tail);

        // (append seg1 seg2 ... segN)
        Ok(QuasiTemplate::Build(QuasiConstructor::Append, append_args))
    }

    /// Derive a vector template: convert to list, derive with pair logic
    /// (handles splicing), then convert back with list->vector.
    fn vector_template(&self, template: TaggedValue, depth: i32) -> Result<QuasiTemplate> {
        // Convert vector to a proper list on the heap, then use pair
        // derivation which handles unquote-splicing correctly.
        let elements = self.heap.borrow().vector_slice(template).to_vec();
        let list = self.heap.borrow_mut().list_from_iter(elements);

        // Derive as a list (handles splicing, unquote, etc.)
        let list_template = if list.is_null() {
            QuasiTemplate::Datum(TaggedValue::NULL)
        } else {
            self.pair_template(list, depth)?
        };

        // (list->vector <list>)
        Ok(QuasiTemplate::Build(
            QuasiConstructor::ListToVector,
            vec![list_template],
        ))
    }

    /// An unquoted expression, desugared by the desugarer standing in the
    /// form — the whole of #445. A nested template inside it is desugared
    /// into a `Quasiquote` of its own on the way.
    fn unquoted(&self, expr: TaggedValue) -> Result<QuasiTemplate> {
        let expr = self.desugarer.desugar_form(expr, self.heap)?;
        Ok(QuasiTemplate::Unquoted(Rc::new(expr)))
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

enum Segment {
    List(Vec<QuasiTemplate>),
    Splice(QuasiTemplate),
}

/// `(list e1 e2 ... eN)`, or the empty list when there are none.
fn list_of(elems: Vec<QuasiTemplate>) -> QuasiTemplate {
    if elems.is_empty() {
        return QuasiTemplate::Datum(TaggedValue::NULL);
    }
    QuasiTemplate::Build(QuasiConstructor::List, elems)
}

/// The refusal of a malformed template.
fn invalid(message: &str) -> DesugarError {
    DesugarError::InvalidSyntax(message.to_string())
}

/// The operands of an `(unquote …)` or `(unquote-splicing …)` form, as a
/// proper list.
///
/// R6RS 11.17 writes both with a `*`: `(unquote <qq template D-1>*)`. R7RS
/// 7.1.4 gives each exactly one operand and 4.2.8 makes anything else an
/// error, so accepting a list here is an extension, taken deliberately —
/// Gauche, Chez and Larceny read it this way and Larceny's suite asserts it.
///
/// `Heap::list_to_vec` rather than a walk of its own, and not only to avoid
/// a duplicate: it stops on a cycle where a hand-rolled loop does not. A
/// template may hold one, because the reader accepts a datum label —
/// `` `(a #0=(unquote . #0#)) `` — and the first version of this function
/// allocated until the process died on exactly that. `None` covers both
/// refusals, an improper list and a circular one, which is what the caller
/// wants: no reading of either report accepts either.
fn operand_list(cdr: TaggedValue, heap: &SharedHeap) -> Option<Vec<TaggedValue>> {
    heap.borrow().list_to_vec(cdr)
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
) -> Result<(TaggedValue, TaggedValue)> {
    if !tv.is_pair() {
        return Err(invalid(&format!(
            "{form}: expected one expression after the keyword"
        )));
    }
    let h = heap.borrow();
    Ok((h.car(tv), h.cdr(tv)))
}
