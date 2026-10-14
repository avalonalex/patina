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

use super::{DesugarError, Desugarer, FormCx, Result};
use patina_core::{Cx, Lit, QuasiConstructor, QuasiTemplate, Value};
use rustc_hash::FxHashSet;
use std::cell::RefCell;
use std::rc::Rc;

impl Desugarer<'_> {
    /// The structure `template` builds, with its unquoted expressions
    /// desugared here.
    pub(super) fn derive_quasi_template<'gc>(
        &self,
        template: Value<'gc>,
        cx: &mut FormCx<'_, 'gc>,
    ) -> Result<QuasiTemplate> {
        Derivation {
            desugarer: self,
            active: RefCell::default(),
        }
        .template(template, 0, cx)
    }
}

/// What one template is derived through.
struct Derivation<'a> {
    /// For the unquoted sub-expressions, which are ordinary code.
    desugarer: &'a Desugarer<'a>,
    /// The pairs and vectors being derived, innermost last — so that one
    /// reached again inside itself is refused (`within`).
    active: RefCell<FxHashSet<u64>>,
}

impl Derivation<'_> {
    /// Derive a template at quasiquotation `depth`: zero at the outermost
    /// level, where an `unquote` is evaluated.
    fn template<'gc>(
        &self,
        template: Value<'gc>,
        depth: i32,
        cx: &mut FormCx<'_, 'gc>,
    ) -> Result<QuasiTemplate> {
        // Self-evaluating atoms
        if template.is_fixnum() || template.is_boolean() || template.is_char() || template.is_null()
        {
            return Ok(QuasiTemplate::Datum(cx.literals.add(template)));
        }

        // Symbols → quote as-is
        if cx.is_symbol(template) {
            return Ok(QuasiTemplate::Datum(cx.literals.add(template)));
        }

        // Identifiers → convert to plain symbol (strip scope marks), as
        // `quote` does. Without this, identifiers inside quasiquote templates
        // retain hygiene marks and won't be `eq?` to the same-named symbol
        // from a quote.
        if cx.is_identifier(template) {
            let name: Option<String> = cx
                .get_symbol_or_identifier_name(template)
                .map(|name| String::from(&*name));
            if let Some(name) = name {
                let sym = cx.intern_symbol(&name);
                return Ok(QuasiTemplate::Datum(cx.literals.add(sym)));
            }
            return Ok(QuasiTemplate::Datum(cx.literals.add(template)));
        }

        // Strings, bytevectors → quote
        if template.is_string() || cx.is_bytevector(template) {
            return Ok(QuasiTemplate::Datum(cx.literals.add(template)));
        }

        // Vectors: expand elements, use list->vector
        if template.is_vector() {
            return self.within(template, cx, |cx| self.vector_template(template, depth, cx));
        }

        // Pairs: the interesting case
        if template.is_pair() {
            return self.within(template, cx, |cx| self.form_template(template, depth, cx));
        }

        // Other types: quote as-is
        Ok(QuasiTemplate::Datum(cx.literals.add(template)))
    }

    /// Derive the compound `node` — a pair or a vector — with `derive`,
    /// refusing it if it is reached again while still being derived.
    ///
    /// The reader accepts a datum label, so a template can be circular, and
    /// the walk had no check: `` `#0=(#0#) `` recursed until the stack
    /// overflowed and aborted the process. A cycle through a *tail* never
    /// comes back here, since `pair_template` walks a spine itself, so it
    /// checks its own; `operand_list` checks an operand list's.
    ///
    /// The nodes being derived are also the template's depth, which is held to
    /// [`MAX_FORM_DEPTH`] as code is, and the walk's stack grows with it
    /// (#617).
    ///
    /// [`MAX_FORM_DEPTH`]: patina_core::walk::MAX_FORM_DEPTH
    fn within<'a, 'gc>(
        &self,
        node: Value<'gc>,
        cx: &mut FormCx<'a, 'gc>,
        derive: impl FnOnce(&mut FormCx<'a, 'gc>) -> Result<QuasiTemplate>,
    ) -> Result<QuasiTemplate> {
        if self.active.borrow().len() >= patina_core::walk::MAX_FORM_DEPTH {
            return Err(DesugarError::nested_too_deeply(node, cx));
        }
        if !self.active.borrow_mut().insert(node.raw_bits()) {
            return Err(circular());
        }
        let derived = patina_core::walk::ensure_sufficient_stack(|| derive(cx));
        self.active.borrow_mut().remove(&node.raw_bits());
        derived
    }

    /// Derive a pair: a nested template, an unquotation, or a list.
    fn form_template<'gc>(
        &self,
        template: Value<'gc>,
        depth: i32,
        cx: &mut FormCx<'_, 'gc>,
    ) -> Result<QuasiTemplate> {
        let (car, cdr) = cx.get_pair(template);

        // Compared under the one borrow rather than copied out as a `String`:
        // this runs for every pair of every template.
        let keyword = cx.get_symbol_or_identifier_name(car).and_then(|name| {
            ["quasiquote", "unquote", "unquote-splicing"]
                .into_iter()
                .find(|keyword| *keyword == &*name)
        });

        match keyword {
            Some("quasiquote") => {
                // Nested quasiquote: increment depth. One operand, like the
                // outermost one: the rest used to be dropped, so that
                // `(quasiquote 2 3)` came back as `(quasiquote 2)`.
                let inner = self.single_operand(cdr, cx, "quasiquote", |n| {
                    format!("quasiquote: a nested template takes one expression, not {n}")
                })?;
                let derived = self.template(inner, depth + 1, cx)?;
                // Reconstruct (quasiquote <derived>) using a plain symbol
                // (car may be an identifier with scope marks; we need a bare
                // symbol)
                let qq_sym = cx.intern_symbol("quasiquote");
                Ok(list_of(vec![
                    QuasiTemplate::Datum(cx.literals.add(qq_sym)),
                    derived,
                ]))
            }

            Some("unquote") if depth == 0 => {
                // A template that *is* an unquote, rather than one holding an
                // element that is. R6RS's multi-operand form is a splice and a
                // splice belongs in a list or vector template, so there is
                // nothing here to splice into: one operand, and
                // `single_operand` refuses none.
                //
                // Extra operands are refused rather than dropped. Taking the
                // first silently was the behaviour before multi-operand
                // unquote existed, and once every other position inserts all
                // of them, accepting a form here and discarding half of it is
                // the one answer that teaches the reader something false.
                let inner = self.single_operand(cdr, cx, "unquote", |n| {
                    format!(
                        "unquote: a template that is itself an unquote takes one expression, \
                         not {n}; several can only be spliced into a list or vector template"
                    )
                })?;
                self.unquoted(inner, cx)
            }

            Some("unquote-splicing") if depth == 0 => {
                // Splicing where there is no list to splice into. R6RS
                // confines a splice to a list or vector template and Gauche
                // raises here; Patina inserts the value, which is what it has
                // always done and what the register records against both
                // oracles. Extra operands are refused, for the reason the
                // `unquote` arm gives: taking the first dropped the rest.
                let inner = self.single_operand(cdr, cx, "unquote-splicing", |n| {
                    format!(
                        "unquote-splicing: a template that is itself a splice takes one \
                         expression, not {n}; several can only be spliced into a list or \
                         vector template"
                    )
                })?;
                self.unquoted(inner, cx)
            }

            // Inside a nested quasiquote the form is rebuilt as data, so the
            // operand *list* is what must survive: derived one level
            // shallower, as the list template it is. Rebuilding a fixed two
            // elements dropped every operand after the first, which is what
            // made ``(foo ,,@q) lose its splice.
            Some(keyword) => self.rebuild_unquotation(cdr, depth, keyword, cx),

            // Regular pair: derive car and cdr, handling unquote-splicing in
            // list context
            None => self.pair_template(template, depth, cx),
        }
    }

    /// The one operand of a form that takes exactly one, or the refusal
    /// `refuse` words for the count it has.
    ///
    /// Refused rather than read as far as it goes: reading the first operand
    /// and ignoring the rest is how a nested `quasiquote` and a template that
    /// is itself a splice came to discard what the program wrote.
    fn single_operand<'gc>(
        &self,
        operands: Value<'gc>,
        cx: &Cx<'gc>,
        keyword: &str,
        refuse: impl FnOnce(usize) -> String,
    ) -> Result<Value<'gc>> {
        let operands = operand_list(operands, cx).ok_or_else(|| {
            invalid(&format!(
                "{keyword}: operands must be a proper, finite list"
            ))
        })?;
        match operands.as_slice() {
            [only] => Ok(*only),
            _ => Err(invalid(&refuse(operands.len()))),
        }
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
    fn rebuild_unquotation<'gc>(
        &self,
        operands: Value<'gc>,
        depth: i32,
        keyword: &str,
        cx: &mut FormCx<'_, 'gc>,
    ) -> Result<QuasiTemplate> {
        // Checked here as well as in element position, so that whether a
        // form is well-formed does not depend on how deeply it is nested:
        // `(unquote . x)` was refused at depth 0 and quietly rebuilt at depth
        // 1, because `pair_template` reads an improper tail as a dotted list.
        if operand_list(operands, cx).is_none() {
            return Err(invalid(&format!(
                "{keyword}: operands must be a proper, finite list"
            )));
        }
        let derived_operands = self.pair_template(operands, depth - 1, cx)?;
        let sym = cx.intern_symbol(keyword);
        let head = list_of(vec![QuasiTemplate::Datum(cx.literals.add(sym))]);
        Ok(QuasiTemplate::Build(
            QuasiConstructor::Append,
            vec![head, derived_operands],
        ))
    }

    /// Derive a pair/list template, handling unquote-splicing in list
    /// elements.
    fn pair_template<'gc>(
        &self,
        template: Value<'gc>,
        depth: i32,
        cx: &mut FormCx<'_, 'gc>,
    ) -> Result<QuasiTemplate> {
        // Collect segments: each segment is either a list of normal elements
        // or a splice expression. This lets us generate efficient code:
        //   `(a b ,@xs c d) → (append (list 'a 'b) xs (list 'c 'd))
        let mut segments: Vec<Segment> = Vec::new();
        let mut current_elems: Vec<QuasiTemplate> = Vec::new();
        let mut current = template;
        let mut tail: Option<QuasiTemplate> = None;
        // The spine is walked here, not through `template`, so a circular one
        // — `#0=(a . #0#)`, which collected elements until memory ran out —
        // is caught here: `slow` follows at half speed and meets `current`
        // only on a cycle, as in `Heap::list_to_vec`.
        let mut slow = template;
        let mut steps = 0usize;

        loop {
            if current.is_null() {
                break;
            }

            if !current.is_pair() {
                // Improper list tail
                tail = Some(self.template(current, depth, cx)?);
                break;
            }

            // Every iteration that does not end the walk moves `current` on
            // by one pair, so `steps` is its index.
            if steps > 0 {
                if steps.is_multiple_of(2) {
                    slow = cx.cdr(slow);
                }
                if current == slow {
                    return Err(circular());
                }
            }
            steps += 1;

            let (car, cdr) = cx.get_pair(current);

            // Check for tail unquote: current IS (unquote expr) — from dotted
            // pair after splice
            if depth == 0 && cx.is_named(car, "unquote") {
                let operand = cx.try_pair(cdr);
                if let Some((uq_expr, rest)) = operand
                    && rest.is_null()
                {
                    tail = Some(self.unquoted(uq_expr, cx)?);
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
                let (inner_car, inner_cdr) = cx.get_pair(car);
                let (splicing, unquoting) = (
                    cx.is_named(inner_car, "unquote-splicing"),
                    cx.is_named(inner_car, "unquote"),
                );

                if splicing || unquoting {
                    let keyword = if splicing {
                        "unquote-splicing"
                    } else {
                        "unquote"
                    };
                    let operands = operand_list(inner_cdr, cx).ok_or_else(|| {
                        invalid(&format!(
                            "{keyword}: operands must be a proper, finite list"
                        ))
                    })?;
                    for operand in operands {
                        let derived = self.unquoted(operand, cx)?;
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
                let (cdr_car, cdr_cdr) = cx.get_pair(cdr);

                let is_unquote = cx.is_named(cdr_car, "unquote");
                let operand = cx.try_pair(cdr_cdr);
                if is_unquote
                    && let Some((unquote_expr, rest)) = operand
                    && rest.is_null()
                {
                    // This is (... car . ,expr)
                    current_elems.push(self.template(car, depth, cx)?);
                    tail = Some(self.unquoted(unquote_expr, cx)?);
                    break;
                }
            }

            // Regular element
            current_elems.push(self.template(car, depth, cx)?);
            current = cdr;
        }

        // Flush remaining elements
        if !current_elems.is_empty() {
            segments.push(Segment::List(current_elems));
        }

        // `(append seg1 … segN tail)` — or, with one part, that part alone,
        // and the empty list with none.
        let mut parts: Vec<QuasiTemplate> =
            segments.into_iter().map(Segment::into_template).collect();
        parts.extend(tail);
        Ok(match parts.len() {
            0 => QuasiTemplate::Datum(Lit::NULL),
            1 => parts.pop().expect("one part"),
            _ => QuasiTemplate::Build(QuasiConstructor::Append, parts),
        })
    }

    /// Derive a vector template: convert to list, derive with pair logic
    /// (handles splicing), then convert back with list->vector.
    fn vector_template<'gc>(
        &self,
        template: Value<'gc>,
        depth: i32,
        cx: &mut FormCx<'_, 'gc>,
    ) -> Result<QuasiTemplate> {
        // Convert vector to a proper list on the heap, then use pair
        // derivation which handles unquote-splicing correctly.
        let elements = cx.vector_slice(template).to_vec();
        let list = cx.list_from_iter(elements);

        // Derive as a list (handles splicing, unquote, etc.)
        let list_template = if list.is_null() {
            QuasiTemplate::Datum(Lit::NULL)
        } else {
            self.pair_template(list, depth, cx)?
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
    fn unquoted<'gc>(&self, expr: Value<'gc>, cx: &mut FormCx<'_, 'gc>) -> Result<QuasiTemplate> {
        let expr = self.desugarer.desugar_expression(expr, cx)?;
        Ok(QuasiTemplate::Unquoted(Rc::new(expr)))
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

enum Segment {
    List(Vec<QuasiTemplate>),
    Splice(QuasiTemplate),
}

impl Segment {
    /// The template for this run of the list: `(list …)` of its elements,
    /// or the spliced expression itself.
    fn into_template(self) -> QuasiTemplate {
        match self {
            Segment::List(elems) => list_of(elems),
            Segment::Splice(expr) => expr,
        }
    }
}

/// `(list e1 e2 ... eN)`, or the empty list when there are none.
fn list_of(elems: Vec<QuasiTemplate>) -> QuasiTemplate {
    if elems.is_empty() {
        return QuasiTemplate::Datum(Lit::NULL);
    }
    QuasiTemplate::Build(QuasiConstructor::List, elems)
}

/// The refusal of a malformed template.
fn invalid(message: &str) -> DesugarError {
    DesugarError::InvalidSyntax(message.to_string())
}

/// The refusal of a template that contains itself.
fn circular() -> DesugarError {
    invalid("quasiquote: a template cannot be circular")
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
fn operand_list<'gc>(cdr: Value<'gc>, cx: &Cx<'gc>) -> Option<Vec<Value<'gc>>> {
    cx.list_to_vec(cdr)
}
