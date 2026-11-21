//! Literal matching: when an input matches a literal in a `syntax-rules`
//! pattern.
//!
//! R7RS §4.3.2 gives two ways for an input identifier to match a literal: the
//! two occurrences have the same lexical binding, or they are the same
//! identifier and neither has one. Both are answered here by *resolving* the
//! two — the literal where the macro was defined, the input where it is used —
//! against the scoped bindings the desugarer records for every binding form it
//! enters (`enter_binding_form`).

use crate::macro_expander::Site;
use patina_core::{Cx, NamespaceView, Value};
use patina_runtime::{ScopeId, ScopeSet};

/// Does `input` match the pattern literal `lit`?
///
/// Identifiers match when both reach the same local binding, or neither
/// reaches one and they are spelled alike and not two different globals
/// ([`distinct_globals`]) — or, spelled differently, when neither is local and
/// both name one global value ([`denotes_same_binding`]).
/// A literal that is not an identifier is a datum and compares as one.
///
/// An identifier relinking renamed (#837) is neither kind of global: the
/// spelling it was given is bound nowhere, and what it means is the record
/// its site's [`RelinkedNames`] answers. Local bindings still come first, a
/// template's own binder of the spelling among them. Then, with neither
/// local:
///
/// | Literal | Input | Match when |
/// |---|---|---|
/// | not relinked | not relinked | the rules above, untouched |
/// | relinked | relinked | the two records' cells are one location, or hold one heap object |
/// | one relinked, one spelling | | the other's by-name location in its site is the record's |
/// | one relinked, two spellings | | the same, or both hold one heap object |
///
/// So a program's own binding of the spelling, or no binding at all, never
/// matches a relinked identifier by being spelled like it, and two
/// identifiers relinked to one binding match however each is spelled.
///
/// [`RelinkedNames`]: crate::macro_expander::RelinkedNames
///
/// This used to be a spelling test with a veto: the desugarer kept the set of
/// names bound anywhere around the use site, and an input whose spelling was
/// in it could not match. A set of spellings cannot say *which* binding, and
/// the rules added to decide whose identifiers the veto could reach moved the
/// error between shapes rather than removing it. A template that binds `token`
/// and passes it to a helper whose literal is an outer `token` took the
/// literal arm; the same template passing the user's own `token` — the outer
/// one, and so the literal's binding — was refused. Triage family 41; chibi and
/// Gauche answer both by binding, and so does this.
///
/// `Err` is an ambiguous resolution. It is refused rather than treated as a
/// mismatch, which would hand the reference to the next rule to decide.
pub fn matches_literal<'gc>(
    lit: Value<'gc>,
    input: Value<'gc>,
    cx: &Cx<'gc>,
    definition: Option<Site<'_>>,
    use_site: Option<Site<'_>>,
    macro_scope: Option<ScopeId>,
) -> Result<bool, String> {
    let Some(lit_name) = cx.get_symbol_or_identifier_name(lit) else {
        return Ok(cx.tagged_values_equal(lit, input));
    };
    let Some(input_name) = cx.get_symbol_or_identifier_name(input) else {
        return Ok(false);
    };
    let (lit_name, input_name) = (&*lit_name, &*input_name);
    // The expansion flipped its own scope onto the input, so an input
    // identifier carrying nothing else was written at the use site. The scope
    // can stay in the set it is resolved with: it was minted after every
    // binding that exists, so no candidate carries it.
    //
    // Each question below reads bindings through `cx`, whose namespace steps
    // borrow the heap shared, beside the two names' borrows above: a read
    // here may not become a write.
    let lit_binding = || local_binding(lit_name, scopes_of(lit, cx), None, definition, cx);
    let input_binding =
        || local_binding(input_name, scopes_of(input, cx), macro_scope, use_site, cx);
    // What relinking gave each side, asked with the identifier's own
    // scopes. Nothing is computed for a site with no relinked names, which
    // is every site of a form that relinked nothing.
    let lit_relinked = || relinked_binding(lit, lit_name, definition, cx);
    let input_relinked = || relinked_binding(input, input_name, use_site, cx);

    if lit_name == input_name {
        let (lit_local, input_local) = (lit_binding()?, input_binding()?);
        if lit_local.is_some() || input_local.is_some() {
            return Ok(lit_local == input_local);
        }
        return Ok(match (lit_relinked(), input_relinked()) {
            (None, None) => !distinct_globals(lit_name, definition, use_site, cx),
            (Some(lit), Some(input)) => one_relinked_binding(&lit, &input),
            (Some(lit), None) => is_named_by(&lit, input_name, use_site, false, cx),
            (None, Some(input)) => is_named_by(&input, lit_name, definition, false, cx),
        });
    }
    // Spelled differently, the two can still be one binding — a global
    // imported under a rename, or one binding two expansions relinked. A
    // local binding is never renamed, so either side reaching one rules it
    // out.
    let same = match (lit_relinked(), input_relinked()) {
        (None, None) => denotes_same_binding(lit_name, input_name, definition, use_site, cx),
        (Some(lit), Some(input)) => one_relinked_binding(&lit, &input),
        (Some(lit), None) => is_named_by(&lit, input_name, use_site, true, cx),
        (None, Some(input)) => is_named_by(&input, lit_name, definition, true, cx),
    };
    Ok(same && lit_binding()?.is_none() && input_binding()?.is_none())
}

/// What a relinked identifier means: where its record's cell is, and what
/// the cell holds.
type RelinkedBinding<'gc> = (patina_core::BindingLocation, Option<Value<'gc>>);

/// The binding relinking gave the identifier `tv`, spelled `name`, if its
/// site's table answers it (#837).
fn relinked_binding<'gc>(
    tv: Value<'gc>,
    name: &str,
    site: Option<Site<'_>>,
    cx: &Cx<'gc>,
) -> Option<RelinkedBinding<'gc>> {
    let names = site?.relinked?;
    names.binding(name, &scopes_of(tv, cx), cx)
}

/// Whether two relinked identifiers mean one binding: their records' cells
/// are one location, or hold one heap object, as [`denotes_same_binding`]
/// compares two names (a frame keyword relinked twice is two snapshot
/// records over two cells, holding one macro).
fn one_relinked_binding(lit: &RelinkedBinding<'_>, input: &RelinkedBinding<'_>) -> bool {
    lit.0 == input.0 || one_heap_object(lit.1, input.1)
}

/// Whether the name `name`, resolved by name in `site`, is the binding
/// `relinked` means: its location is the record's cell's. Where the two
/// are spelled differently (`by_value`), also where both hold one heap
/// object, [`denotes_same_binding`]'s rule. A name that is unbound, or has
/// no site to be looked up in, is not it.
fn is_named_by<'gc>(
    relinked: &RelinkedBinding<'gc>,
    name: &str,
    site: Option<Site<'_>>,
    by_value: bool,
    cx: &Cx<'gc>,
) -> bool {
    let Some(site) = site else {
        return false;
    };
    cx.binding_location(NamespaceView::of(site.env), name)
        .is_some_and(|location| location == relinked.0)
        || (by_value && one_heap_object(relinked.1, cx.lookup(site.env, name)))
}

/// Whether both values are one heap object: the rule
/// [`denotes_same_binding`] holds two names to, which keeps two unrelated
/// bindings that merely both hold `#t` or `0` apart.
fn one_heap_object(a: Option<Value<'_>>, b: Option<Value<'_>>) -> bool {
    match (a.map(Value::tagged), b.map(Value::tagged)) {
        (Some(a), Some(b)) => a.is_object() && a == b,
        _ => false,
    }
}

/// The local binding `name` reaches from `site`, named by its scope set; `None`
/// for a global or an unbound name.
///
/// `own` is what the identifier carries. Carrying nothing but `ignoring` means
/// it was written at the site, and so stands in the site's scopes.
fn local_binding(
    name: &str,
    own: ScopeSet,
    ignoring: Option<ScopeId>,
    site: Option<Site<'_>>,
    cx: &Cx<'_>,
) -> Result<Option<ScopeSet>, String> {
    let Some(site) = site else {
        return Ok(None);
    };
    let scopes = if own.iter().all(|scope| Some(*scope) == ignoring) {
        site.scopes
    } else {
        &own
    };
    cx.scoped_binding_of(NamespaceView::of(site.env), name, scopes)
        .map_err(|ambiguous| ambiguous.to_string())
}

/// Whether `name` is a global on both sides, and a different one on each.
///
/// Neither side reaching a local binding used to settle a pair spelled alike
/// as a match, whatever the two globals were. So once a program defined over
/// an imported `else`, `(cond (#f 0) (else 1))` still took the else clause —
/// the program's variable and `(scheme base)`'s keyword read as one binding
/// because neither is local. chibi and Gauche evaluate the variable (#450).
/// The same shape for a template's *reference* was #407: an import is the
/// exporting library's location (#406), so where the two names lead is a
/// question [`Environment::binding_location`] answers exactly.
///
/// Only two bound names at different locations *holding different values*
/// are told apart. A name bound on one side and unbound on the other matches
/// as it did, as does a name with no site to look it up in.
///
/// The value check was for the one import that was not the library's
/// location: an export with no plain binding to share — a definition a macro
/// introduced, or one reached through an alias — was installed as a copy, a
/// location of the importer's own holding what the library's holds. By
/// location alone a library that exported such a name and used it as its
/// macro's literal stopped matching its own importer's use of it, where
/// chibi, Gauche and the matcher before #450 all match. No import is a copy
/// now: every import is the exporter's cell (#834), #427 refuses an
/// introduced definition's bare export, and a spelling relinking gave an
/// identifier is bound nowhere, so it is no export (#846, #837). The check
/// stays for a binding of the use site's own that holds what the library's
/// holds,
/// the copy's shape, as a host's `define` of an import's value makes
/// (`an_import_installed_as_a_copy_still_matches_the_librarys_literal`). A
/// program's definition over an import holds a value of its own, so this
/// does not undo #450.
///
/// [`Environment::binding_location`]: patina_runtime::Environment::binding_location
fn distinct_globals(
    name: &str,
    definition: Option<Site<'_>>,
    use_site: Option<Site<'_>>,
    cx: &Cx<'_>,
) -> bool {
    let (Some(definition), Some(use_site)) = (definition, use_site) else {
        return false;
    };
    match (
        cx.binding_location(NamespaceView::of(definition.env), name),
        cx.binding_location(NamespaceView::of(use_site.env), name),
    ) {
        (Some(lit), Some(input)) if lit != input => {
            cx.lookup(definition.env, name).map(Value::tagged)
                != cx.lookup(use_site.env, name).map(Value::tagged)
        }
        _ => false,
    }
}

fn scopes_of<'gc>(tv: Value<'gc>, cx: &Cx<'gc>) -> ScopeSet {
    cx.get_identifier_data_any(tv)
        .map(|(_, scopes)| scopes)
        .unwrap_or_default()
}

/// R7RS §4.3.2's same-binding half for two *differently spelled* names: a
/// keyword imported under a rename still matches its own literal.
///
/// ```scheme
/// (import (scheme base) (rename (scheme base) (else alt)))
/// (cond (#f 1) (alt 42))     ;; chibi => 42
/// ```
///
/// Each name is resolved in its own environment — the literal's in the one the
/// macro was defined in, the input's at the use site — which is what the report
/// specifies. [`matches_literal`] asks only about names spelled differently,
/// and only once neither reaches a local binding; it settles a pair spelled
/// alike by binding before it gets here.
///
/// The comparison is on the *value*, and only when that value is a heap object.
/// Environments hold values, not binding identities (an import under a rename
/// copies the value into a fresh binding, so slot identity would say "different"
/// for the very case this exists for), and restricting it to heap objects keeps
/// two unrelated names that merely both hold `#t` or `0` from being called the
/// same binding. Syntactic keywords — what auxiliary literals actually are —
/// are interned markers, so identity is exact for them.
fn denotes_same_binding(
    lit_name: &str,
    input_name: &str,
    definition: Option<Site<'_>>,
    use_site: Option<Site<'_>>,
    cx: &Cx<'_>,
) -> bool {
    let (Some(definition), Some(use_site)) = (definition, use_site) else {
        return false;
    };
    let lit_value = cx.lookup(definition.env, lit_name).map(Value::tagged);
    let input_value = cx.lookup(use_site.env, input_name).map(Value::tagged);
    match (lit_value, input_value) {
        (Some(lit_value), Some(input_value)) => lit_value.is_object() && lit_value == input_value,
        _ => false,
    }
}

/// The matcher's rule for an identifier relinking renamed (#837), one case
/// for each row of [`matches_literal`]'s table, over a hand-made table of
/// relinked names: what the desugarer's form table answers is here an
/// identifier's spelling and scopes mapped to a binding of the `library`
/// environment.
#[cfg(test)]
mod relinked_tests {
    use super::*;
    use crate::macro_expander::RelinkedNames;
    use patina_core::{BindingLocation, TaggedValue};
    use patina_runtime::Environment;
    use std::rc::Rc;

    /// Each relinked identifier, by spelling and scope, with the name its
    /// binding has in `library`.
    struct Table {
        library: Rc<Environment>,
        entries: Vec<(&'static str, usize, &'static str)>,
    }

    impl RelinkedNames for Table {
        fn binding<'gc>(
            &self,
            name: &str,
            scopes: &ScopeSet,
            cx: &Cx<'gc>,
        ) -> Option<(BindingLocation, Option<Value<'gc>>)> {
            let (_, _, bound) = self
                .entries
                .iter()
                .find(|(spelling, scope, _)| *spelling == name && *scopes == one(*scope))?;
            let location = cx.binding_location(NamespaceView::of(&self.library), bound)?;
            Some((location, cx.lookup(&self.library, bound)))
        }
    }

    fn one(scope: usize) -> ScopeSet {
        ScopeSet::singleton(ScopeId(scope))
    }

    /// The environments of every case: `library` binds `lit` and `twin` to
    /// one heap object (a bytevector: the value rule compares objects, and
    /// a keyword is one) in two cells, `other` to another, and `count` and
    /// `zero` to the fixnum 0 in two cells; `importer` imports `lit` under
    /// the spelling `lit.7` and under `renamed`; `program` binds a `lit.7`
    /// and a `lit` of its own; `empty` binds nothing.
    struct World {
        heap: patina_core::SharedHeap,
        library: Rc<Environment>,
        importer: Rc<Environment>,
        program: Rc<Environment>,
        empty: Rc<Environment>,
    }

    fn world() -> World {
        let heap = patina_core::new_shared_heap();
        let library = Rc::new(Environment::with_heap(&heap));
        let object = |n: u8| heap.borrow_mut().alloc_bytevector(vec![n]);
        let shared = object(1);
        library.define(&heap, "lit", shared);
        library.define(&heap, "twin", shared);
        library.define(&heap, "other", object(2));
        library.define(&heap, "count", TaggedValue::fixnum(0));
        library.define(&heap, "zero", TaggedValue::fixnum(0));
        let importer = Rc::new(Environment::with_heap(&heap));
        let cell = library.cell_of(&heap, "lit").expect("the library's lit");
        importer.import_cell(&heap, "lit.7", cell);
        importer.import_cell(&heap, "renamed", cell);
        let program = Rc::new(Environment::with_heap(&heap));
        program.define(&heap, "lit.7", object(3));
        program.define(&heap, "lit", object(4));
        let empty = Rc::new(Environment::with_heap(&heap));
        World {
            heap,
            library,
            importer,
            program,
            empty,
        }
    }

    /// An identifier: `(spelling, Some(scope))`, or a plain symbol.
    type Ident = (&'static str, Option<usize>);

    /// Whether the literal `lit`, of a macro defined in `definition`,
    /// matches the input `input`, used in `use_site`, with `table`'s
    /// relinked names on both sites.
    fn matches(
        world: &World,
        table: &Table,
        (lit, definition): (Ident, &Rc<Environment>),
        (input, use_site): (Ident, &Rc<Environment>),
    ) -> bool {
        patina_core::NoGcScope::with_cx(&world.heap, |cx| {
            let mut make = |(name, scope): Ident| match scope {
                Some(scope) => cx.alloc_identifier(Rc::from(name), one(scope)),
                None => cx.intern_symbol(name),
            };
            let (lit, input) = (make(lit), make(input));
            let no_scopes = ScopeSet::new();
            let site = |env| {
                Some(Site {
                    env,
                    scopes: &no_scopes,
                    relinked: Some(table as &dyn RelinkedNames),
                })
            };
            matches_literal(lit, input, cx, site(definition), site(use_site), None)
                .expect("no ambiguity")
        })
    }

    fn table(world: &World) -> Table {
        Table {
            library: Rc::clone(&world.library),
            entries: vec![
                ("lit.7", 7, "lit"),
                ("lit.9", 9, "lit"),
                ("twin.7", 7, "twin"),
                ("other.7", 7, "other"),
                ("count.7", 7, "count"),
                ("count.9", 9, "count"),
                ("zero.7", 7, "zero"),
            ],
        }
    }

    /// Two relinked identifiers match where their records' cells are one
    /// location, however each is spelled and whatever the cell holds, or
    /// where two cells hold one heap object; and not otherwise, two cells
    /// that both hold `0` included.
    #[test]
    fn two_relinked_identifiers_match_by_location_or_one_heap_object() {
        let world = world();
        let table = table(&world);
        let case = |lit: Ident, input: Ident| {
            matches(&world, &table, (lit, &world.empty), (input, &world.empty))
        };
        assert!(
            case(("lit.7", Some(7)), ("lit.7", Some(7))),
            "one identifier"
        );
        assert!(
            case(("lit.7", Some(7)), ("lit.9", Some(9))),
            "one location under two spellings"
        );
        assert!(
            case(("count.7", Some(7)), ("count.9", Some(9))),
            "one location, which holds no heap object"
        );
        assert!(
            case(("lit.7", Some(7)), ("twin.7", Some(7))),
            "two cells that hold one heap object"
        );
        assert!(
            !case(("lit.7", Some(7)), ("other.7", Some(7))),
            "two cells, two objects"
        );
        assert!(
            !case(("count.7", Some(7)), ("zero.7", Some(7))),
            "two cells that both hold 0 are two bindings"
        );
    }

    /// One relinked identifier and a name spelled like it: a match only
    /// where the name, in its own site, is the record's location. A
    /// program's own binding of the spelling and an unbound name are not
    /// it, on either side, nor is a binding of the spelling that holds the
    /// very object the record's cell holds: holding one object is the rule
    /// for two spellings only (#837's mutation stage).
    #[test]
    fn a_relinked_identifier_and_a_name_of_its_spelling_match_by_location() {
        let world = world();
        let table = table(&world);
        let relinked: Ident = ("lit.7", Some(7));
        let name: Ident = ("lit.7", None);
        let same_object = Rc::new(Environment::with_heap(&world.heap));
        let shared = world
            .library
            .get(&world.heap, "lit")
            .expect("the library's lit");
        same_object.define(&world.heap, "lit.7", shared);
        for (site, expected, what) in [
            (&world.importer, true, "the name is the record's location"),
            (
                &world.program,
                false,
                "the program's own binding of the spelling",
            ),
            (
                &same_object,
                false,
                "another binding of the spelling, which holds the record's object",
            ),
            (&world.empty, false, "an unbound name"),
        ] {
            assert_eq!(
                matches(&world, &table, (relinked, &world.empty), (name, site)),
                expected,
                "a relinked literal, and an input where {what}"
            );
            assert_eq!(
                matches(&world, &table, (name, site), (relinked, &world.empty)),
                expected,
                "a relinked input, and a literal where {what}"
            );
        }
    }

    /// One relinked identifier and a name spelled otherwise: a match where
    /// the name is the record's location, or where both hold one heap
    /// object; not where it is another binding or unbound.
    #[test]
    fn a_relinked_identifier_and_a_name_spelled_otherwise_match_by_location_or_value() {
        let world = world();
        let table = table(&world);
        let relinked: Ident = ("lit.7", Some(7));
        for (name, site, expected, what) in [
            (
                "lit",
                &world.library,
                true,
                "the binding itself, by its own name",
            ),
            (
                "renamed",
                &world.importer,
                true,
                "an import of it under a rename",
            ),
            (
                "twin",
                &world.library,
                true,
                "another cell that holds the same object",
            ),
            ("other", &world.library, false, "another binding"),
            ("lit", &world.program, false, "the program's own binding"),
            ("lit", &world.empty, false, "an unbound name"),
        ] {
            let name: Ident = (name, None);
            assert_eq!(
                matches(&world, &table, (relinked, &world.empty), (name, site)),
                expected,
                "a relinked literal, and an input that is {what}"
            );
            assert_eq!(
                matches(&world, &table, (name, site), (relinked, &world.empty)),
                expected,
                "a relinked input, and a literal that is {what}"
            );
        }
    }

    /// Local bindings come first: where a binding's scopes select a
    /// relinked identifier on its side, a template's own binder of the
    /// spelling, the table is not asked, and the comparison is of the two
    /// local bindings.
    #[test]
    fn a_local_binding_of_a_relinked_spelling_is_compared_first() {
        let world = world();
        let table = table(&world);
        let local = Rc::new(Environment::with_heap(&world.heap));
        local.define_with_scopes(&world.heap, "lit.7", one(7), TaggedValue::UNSPECIFIED);
        let relinked: Ident = ("lit.7", Some(7));
        assert!(
            matches(&world, &table, (relinked, &local), (relinked, &local)),
            "both sides reach the one local binding"
        );
        assert!(
            !matches(&world, &table, (relinked, &local), (relinked, &world.empty)),
            "a local binding on one side only, though the table answers both alike"
        );
        assert!(
            !matches(
                &world,
                &table,
                (relinked, &local),
                (("lit.9", Some(9)), &world.empty)
            ),
            "and under two spellings"
        );
    }

    /// With no relinked names on either site, and for identifiers the
    /// table does not answer, the comparison is what it was.
    #[test]
    fn identifiers_the_table_does_not_answer_compare_as_before() {
        let world = world();
        let table = table(&world);
        let plain: Ident = ("lit", None);
        assert!(
            matches(
                &world,
                &table,
                (plain, &world.library),
                (plain, &world.library)
            ),
            "one global"
        );
        assert!(
            !matches(
                &world,
                &table,
                (plain, &world.library),
                (plain, &world.program)
            ),
            "two globals of one spelling that hold different values"
        );
        assert!(
            matches(
                &world,
                &table,
                (plain, &world.library),
                (("renamed", None), &world.importer)
            ),
            "an import under a rename"
        );
        // An identifier of the spelling whose scope the table does not have.
        assert!(
            !matches(
                &world,
                &table,
                (("lit.7", Some(8)), &world.program),
                (("lit.7", Some(7)), &world.empty)
            ),
            "an unrelinked identifier of the spelling is the name"
        );
    }
}
