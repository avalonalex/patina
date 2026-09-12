//! H3 adds one axis at a time to H1's binding graph. Source variants still go
//! through Program::variants, including the same nullary-call poison guard.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Baseline,
    GeneratedMacro,
    Imports,
    Expansion,
    Ellipsis,
    Derived,
    Literals,
    IntroducedGlobal,
}

pub const AXES: [Axis; 8] = [
    Axis::Baseline,
    Axis::GeneratedMacro,
    Axis::Imports,
    Axis::Expansion,
    Axis::Ellipsis,
    Axis::Derived,
    Axis::Literals,
    Axis::IntroducedGlobal,
];

impl Axis {
    pub fn name(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::GeneratedMacro => "generated-macro",
            Self::Imports => "imports",
            Self::Expansion => "expansion-depth",
            Self::Ellipsis => "ellipsis-depth",
            Self::Derived => "derived-binders",
            Self::Literals => "pattern-literals",
            Self::IntroducedGlobal => "introduced-global",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExtendedCase {
    pub base: Case,
    pub axis: Axis,
    pub depth: usize,
    pub width: usize,
    pub literal_shadowed: bool,
}

#[derive(Clone, Debug)]
pub struct Source {
    pub main: String,
    pub library: Option<String>,
}

impl ExtendedCase {
    pub fn historical() -> Self {
        let mut base = Case::generated(Binder::Let, Site::Outside, Action::Read, 285);
        base.padding = 2;
        Self {
            base,
            axis: Axis::Baseline,
            depth: 1,
            width: 1,
            literal_shadowed: false,
        }
    }

    pub fn exclusion(&self) -> Option<&'static str> {
        if self.base.site == Site::Inside
            && matches!(self.axis, Axis::Imports | Axis::IntroducedGlobal)
        {
            Some("library/private-global definitions are top-level in this grammar")
        } else {
            None
        }
    }

    fn program(&self) -> Program {
        assert!(self.exclusion().is_none());
        let mut p = self.base.program();
        let helper = p.fresh("h3-helper");
        let target = p.template_target;
        let template = |id| match self.base.action {
            Action::Read => Expr::Reference(id),
            Action::Write => Expr::Assign(id, Box::new(Expr::Integer(self.base.assigned))),
        };
        let definition = match self.axis {
            Axis::Baseline | Axis::Derived => return p,
            Axis::GeneratedMacro => Expr::InstallMacro(
                helper,
                vec![MACRO],
                vec![Expr::DefineMacro(MACRO, Box::new(template(target)))],
            ),
            Axis::Expansion => {
                let mut definitions = Vec::new();
                let mut body = template(target);
                for index in 1..self.depth {
                    let id = p.fresh(&format!("h3-expansion-{index}"));
                    definitions.push(Expr::DefineMacro(id, Box::new(body)));
                    body = Expr::CallMacro(id);
                }
                definitions.push(Expr::DefineMacro(MACRO, Box::new(body)));
                Expr::Definitions(definitions)
            }
            Axis::Ellipsis => Expr::Definitions(vec![
                Expr::RepeatingMacro(
                    helper,
                    self.depth,
                    self.width,
                    self.base.local,
                    Box::new(template(target)),
                ),
                Expr::DefineMacro(
                    MACRO,
                    Box::new(Expr::Invoke(
                        helper,
                        vec![Expr::RepeatedData(self.depth, self.width, self.base.local)],
                    )),
                ),
            ]),
            Axis::Literals => {
                let literal = p.fresh("h3-token");
                let shadow = p.fresh("h3-token");
                let wrong = Expr::Integer(799);
                let (matched, fallback) = if self.literal_shadowed {
                    (wrong, template(target))
                } else {
                    (template(target), wrong)
                };
                let call = Expr::Invoke(
                    helper,
                    vec![Expr::Reference(if self.literal_shadowed {
                        shadow
                    } else {
                        literal
                    })],
                );
                let call = if self.literal_shadowed {
                    Expr::Bind(Binder::Let, shadow, Box::new(Expr::Integer(2)), vec![call])
                } else {
                    call
                };
                Expr::Definitions(vec![
                    Expr::Define(literal, Box::new(Expr::Integer(1))),
                    Expr::LiteralMacro(helper, literal, Box::new(matched), Box::new(fallback)),
                    Expr::DefineMacro(MACRO, Box::new(call)),
                ])
            }
            Axis::IntroducedGlobal => {
                // This identity belongs to the installer's expansion, never to
                // the source-written global, even though their spellings collide.
                let private = p.fresh(&self.base.spelling);
                let getter = p.fresh("h3-private-value");
                p.template_target = private;
                p.renamed_global = private;
                p.observe(Expr::Invoke(getter, Vec::new()));
                Expr::InstallMacro(
                    helper,
                    vec![MACRO, getter],
                    vec![
                        Expr::Define(private, Box::new(Expr::Integer(self.base.global + 10))),
                        Expr::DefineMacro(MACRO, Box::new(template(private))),
                        Expr::Getter(getter, private),
                    ],
                )
            }
            Axis::Imports => {
                let private = p.fresh(&self.base.spelling);
                let getter = p.fresh("h3-library-value");
                p.template_target = private;
                p.renamed_global = private;
                let mut library = vec![Expr::Define(
                    private,
                    Box::new(Expr::Integer(self.base.global + 10)),
                )];
                let body = if self.base.action == Action::Write {
                    // Keep assignment inside the owning library. A direct set!
                    // emitted into the importer is outside the agreed subset;
                    // Racket refuses it (retained boundary case in Track H).
                    let setter = p.fresh("h3-library-set");
                    let parameter = p.fresh("h3-new-value");
                    library.push(Expr::Setter(setter, parameter, private));
                    p.template_target = setter;
                    Expr::Invoke(setter, vec![Expr::Integer(self.base.assigned)])
                } else {
                    template(private)
                };
                library.push(Expr::DefineMacro(MACRO, Box::new(body)));
                library.push(Expr::Getter(getter, private));
                p.library = Some(library);
                p.observe(Expr::Invoke(getter, Vec::new()));
                // MACRO's identity is supplied by the import instead.
                Expr::Definitions(Vec::new())
            }
        };
        assert_eq!(replace_definition(&mut p.forms, &definition), 1);
        p
    }

    pub fn sources(&self) -> Vec<Source> {
        self.program().variants(self.base.seed, self.base.poison).iter().map(|p| {
            let mut forms = p.forms.clone();
            let value = forms.pop().expect("final observation").render(&p.names);
            let imports = if p.library.is_some() { " (h3 generated)" } else { "" };
            let body = forms.iter().map(|e| e.render(&p.names)).collect::<Vec<_>>().join("\n");
            Source {
                main: format!("(import (scheme base) (scheme write){imports})\n{body}\n(display \"H3-VALUE \" )\n(write {value})\n(newline)\n"),
                library: p.library.as_ref().map(|forms| format!(
                    "(define-library (h3 generated)\n (export {} h3-library-value)\n (import (scheme base))\n (begin {}))\n",
                    p.names[MACRO.0], forms.iter().map(|e| e.render(&p.names)).collect::<Vec<_>>().join("\n")
                )),
            }
        }).collect()
    }

    pub fn reductions(&self) -> Vec<Self> {
        let mut cases = self
            .base
            .reductions()
            .into_iter()
            .map(|base| Self {
                base,
                ..self.clone()
            })
            .collect::<Vec<_>>();
        let minimum_depth = if self.axis == Axis::Expansion { 2 } else { 1 };
        if self.depth > minimum_depth {
            cases.push(Self {
                depth: self.depth - 1,
                ..self.clone()
            });
        }
        if self.width > 1 {
            cases.push(Self {
                width: self.width - 1,
                ..self.clone()
            });
        }
        // The advertised axis, literal binding relation, form/site/action and
        // surviving reference identities are retained throughout reduction.
        cases
    }
}

impl Program {
    fn fresh(&mut self, spelling: &str) -> Id {
        let id = Id(self.names.len());
        self.names.push(spelling.into());
        id
    }

    fn observe(&mut self, expr: Expr) {
        let Some(Expr::Primitive("list", observations)) = self.forms.last_mut() else {
            panic!("observation list")
        };
        observations.push(expr);
    }
}

fn replace_definition(forms: &mut [Expr], replacement: &Expr) -> usize {
    forms
        .iter_mut()
        .map(|expr| match expr {
            Expr::DefineMacro(id, _) if *id == MACRO => {
                *expr = replacement.clone();
                1
            }
            Expr::Define(_, inner) => {
                replace_definition(std::slice::from_mut(inner.as_mut()), replacement)
            }
            Expr::Bind(_, _, _, body) | Expr::Body(body) => replace_definition(body, replacement),
            _ => 0,
        })
        .sum()
}

pub fn sweep(seed: u64) -> Vec<ExtendedCase> {
    let mut cases = Vec::new();
    for axis in AXES {
        let binders: &[Binder] = match axis {
            Axis::Baseline => &BINDERS,
            Axis::Derived => &[
                Binder::LetValues,
                Binder::LetStarValues,
                Binder::LetrecStar,
                Binder::DefineValues,
            ],
            _ => &[Binder::Let, Binder::InternalDefine],
        };
        for &binder in binders {
            for site in [Site::Outside, Site::Inside] {
                for action in [Action::Read, Action::Write] {
                    let index = cases.len();
                    cases.push(ExtendedCase {
                        base: Case::generated(
                            binder,
                            site,
                            action,
                            seed.checked_add(index as u64).expect("seed overflow"),
                        ),
                        axis,
                        depth: match axis {
                            Axis::Expansion => 2 + index % 3,
                            Axis::Ellipsis => 1 + index % 3,
                            _ => 1,
                        },
                        width: if axis == Axis::Ellipsis {
                            1 + (index / 3) % 3
                        } else {
                            1
                        },
                        literal_shadowed: axis == Axis::Literals && index % 4 >= 2,
                    });
                }
            }
        }
    }
    cases
}
