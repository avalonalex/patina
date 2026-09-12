//! H1's bounded language. Identities and reference edges are assigned here,
//! before serialization, without consulting Patina's parser or resolver.
//! The 28 starting shapes follow hygiene_matrix.rs's binder/site/action product.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binder {
    Lambda,
    Let,
    LetStar,
    Letrec,
    NamedLet,
    InternalDefine,
    Do,
}

pub const BINDERS: [Binder; 7] = [
    Binder::Lambda,
    Binder::Let,
    Binder::LetStar,
    Binder::Letrec,
    Binder::NamedLet,
    Binder::InternalDefine,
    Binder::Do,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    Outside,
    Inside,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Id(usize);

const GLOBAL: Id = Id(0);
const LOCAL: Id = Id(1);
const MACRO: Id = Id(2);
const RESULT: Id = Id(3);
const EFFECTS: Id = Id(4);
const LOOP: Id = Id(5);
const OBSERVATION: Id = Id(6);
const POISON: Id = Id(7);
const PAD: usize = 8;

// Builtins/keywords and quoted data are separate from variable occurrences.
// Macro calls are explicitly nullary; their templates refer to definition-site
// identities. No eval, constructed symbols, imports, literals or ellipses.
#[derive(Clone, Debug)]
enum Expr {
    Integer(i64),
    Quote(&'static str),
    Reference(Id),
    Assign(Id, Box<Expr>),
    Define(Id, Box<Expr>),
    Primitive(&'static str, Vec<Expr>),
    Begin(Vec<Expr>),
    Body(Vec<Expr>),
    Bind(Binder, Id, Box<Expr>, Vec<Expr>),
    DefineMacro(Id, Box<Expr>),
    CallMacro(Id),
}

impl Expr {
    fn render(&self, names: &[String]) -> String {
        let name = |id: &Id| &names[id.0];
        let sequence = |body: &[Expr]| {
            body.iter()
                .map(|e| e.render(names))
                .collect::<Vec<_>>()
                .join(" ")
        };
        match self {
            Self::Integer(n) => n.to_string(),
            Self::Quote(datum) => format!("'{datum}"),
            Self::Reference(id) => name(id).clone(),
            Self::Assign(id, value) => format!("(set! {} {})", name(id), value.render(names)),
            Self::Define(id, value) => format!("(define {} {})", name(id), value.render(names)),
            Self::Primitive(op, args) => format!("({op} {})", sequence(args)),
            Self::Begin(body) => format!("(begin {})", sequence(body)),
            Self::Body(body) => format!("(let () {})", sequence(body)),
            Self::DefineMacro(id, template) => format!(
                "(define-syntax {} (syntax-rules () (({}) {})))",
                name(id),
                name(id),
                template.render(names)
            ),
            Self::CallMacro(id) => format!("({})", name(id)),
            Self::Bind(form, id, initial, body) => {
                let n = name(id);
                let value = initial.render(names);
                let body = sequence(body);
                match form {
                    Binder::Lambda => format!("((lambda ({n}) {body}) {value})"),
                    Binder::Let => format!("(let (({n} {value})) {body})"),
                    Binder::LetStar => format!("(let* (({n} {value})) {body})"),
                    Binder::Letrec => format!("(letrec (({n} {value})) {body})"),
                    Binder::NamedLet => format!("(let {} (({n} {value})) {body})", names[LOOP.0]),
                    Binder::InternalDefine => format!("((lambda () (define {n} {value}) {body}))"),
                    // The step reference belongs to this binder, even on rename.
                    Binder::Do => format!("(do (({n} {value} {n})) (#t {body}))"),
                }
            }
        }
    }

    fn poison_calls(&mut self, value: i64) -> usize {
        match self {
            // The wrapper's body contains no argument or source-level reference
            // whose binding could change. Only a definition-site template can
            // observe this collision. Wrapping a whole body would be unsound.
            Self::CallMacro(_) => {
                *self = Self::Bind(
                    Binder::Let,
                    POISON,
                    Box::new(Self::Integer(value)),
                    vec![self.clone()],
                );
                1
            }
            Self::Define(_, inner) | Self::Assign(_, inner) => inner.poison_calls(value),
            Self::Primitive(_, body) | Self::Begin(body) | Self::Body(body) => {
                body.iter_mut().map(|e| e.poison_calls(value)).sum()
            }
            Self::Bind(_, _, initial, body) => {
                initial.poison_calls(value)
                    + body
                        .iter_mut()
                        .map(|e| e.poison_calls(value))
                        .sum::<usize>()
            }
            // Templates are a different lexical context, not use-site code.
            Self::DefineMacro(_, _) | Self::Integer(_) | Self::Quote(_) | Self::Reference(_) => 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Case {
    pub seed: u64,
    pub binder: Binder,
    pub site: Site,
    pub action: Action,
    pub global: i64,
    pub local: i64,
    pub assigned: i64,
    pub poison: i64,
    pub padding: usize,
    pub effects: bool,
    pub spelling: String,
}

impl Case {
    pub fn baseline(binder: Binder, site: Site, action: Action) -> Self {
        Self {
            seed: 285,
            binder,
            site,
            action,
            global: 1,
            local: 5,
            assigned: 99,
            poison: 199,
            padding: 0,
            effects: false,
            spelling: "x".into(),
        }
    }

    pub fn generated(binder: Binder, site: Site, action: Action, seed: u64) -> Self {
        // An explicit reproducible stream; no process-global RNG or new runtime
        // dependency. Shapes are enumerated to guarantee axis coverage.
        let mut state = seed;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            state >> 32
        };
        Self {
            seed,
            binder,
            site,
            action,
            global: 1 + (next() % 4) as i64,
            local: 5 + (next() % 4) as i64,
            assigned: 99 + (next() % 4) as i64,
            poison: 199 + (next() % 4) as i64,
            padding: (next() % 3) as usize,
            effects: true,
            spelling: ["x", "collision", "h1-value"][(next() % 3) as usize].into(),
        }
    }

    fn program(&self) -> Program {
        let target = match self.site {
            Site::Outside => GLOBAL,
            Site::Inside => LOCAL,
        };
        let template = match self.action {
            Action::Read => Expr::Reference(target),
            Action::Write => Expr::Assign(target, Box::new(Expr::Integer(self.assigned))),
        };
        let definition = Expr::DefineMacro(MACRO, Box::new(template));
        let call = Expr::CallMacro(MACRO);
        let record = |tag| {
            Expr::Assign(
                EFFECTS,
                Box::new(Expr::Primitive(
                    "cons",
                    vec![
                        Expr::Primitive("list", vec![Expr::Quote(tag), Expr::Reference(LOCAL)]),
                        Expr::Reference(EFFECTS),
                    ],
                )),
            )
        };
        let mut body = match (self.action, self.effects) {
            (Action::Read, false) => call,
            (Action::Write, false) => Expr::Begin(vec![call, Expr::Reference(LOCAL)]),
            (Action::Read, true) => Expr::Begin(vec![
                record("before"),
                Expr::Bind(
                    Binder::Let,
                    OBSERVATION,
                    Box::new(call),
                    vec![record("after"), Expr::Reference(OBSERVATION)],
                ),
            ]),
            (Action::Write, true) => Expr::Begin(vec![
                record("before"),
                call,
                record("after"),
                Expr::Reference(LOCAL),
            ]),
        };
        for index in 0..self.padding {
            body = Expr::Bind(
                Binder::Let,
                Id(PAD + index),
                Box::new(Expr::Integer(index as i64)),
                vec![body],
            );
        }
        let body = match self.site {
            Site::Outside => vec![body],
            // A do result clause is not a definition context; preserve the
            // matrix's explicit empty let rather than generating invalid code.
            Site::Inside if self.binder == Binder::Do => {
                vec![Expr::Body(vec![definition.clone(), body])]
            }
            Site::Inside => vec![definition.clone(), body],
        };
        let inner = Expr::Bind(
            self.binder,
            LOCAL,
            Box::new(Expr::Integer(self.local)),
            body,
        );
        let mut forms = vec![Expr::Define(GLOBAL, Box::new(Expr::Integer(self.global)))];
        if self.effects {
            forms.push(Expr::Define(EFFECTS, Box::new(Expr::Quote("()"))));
        }
        if self.site == Site::Outside {
            forms.push(definition);
        }
        // Definition forces effects before observation, independent of Scheme's
        // unspecified operand evaluation order. The final list only reads cells.
        forms.push(Expr::Define(RESULT, Box::new(inner)));
        let mut result = vec![
            Expr::Reference(RESULT),
            Expr::Reference(GLOBAL),
            Expr::Quote("x"),
        ];
        if self.effects {
            result.push(Expr::Primitive("reverse", vec![Expr::Reference(EFFECTS)]));
        }
        forms.push(Expr::Primitive("list", result));
        let mut names: Vec<_> = [
            self.spelling.as_str(),
            self.spelling.as_str(),
            "h1-m",
            "h1-result",
            "h1-effects",
            "h1-loop",
            "h1-observation",
            "h1-poison",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        names.extend((0..self.padding).map(|i| format!("h1-padding-{i}")));
        Program {
            names,
            forms,
            template_target: target,
        }
    }

    pub fn sources(&self) -> Vec<String> {
        let original = self.program();
        let mut local = original.clone();
        local.rename(LOCAL, format!("h1-local-{}", self.seed));
        let mut global = original.clone();
        global.rename(GLOBAL, format!("h1-global-{}", self.seed));
        let mut poison = original.clone();
        poison.names[POISON.0] = poison.names[poison.template_target.0].clone();
        let wrapped: usize = poison
            .forms
            .iter_mut()
            .map(|e| e.poison_calls(self.poison))
            .sum();
        assert_eq!(wrapped, 1, "exactly one eligible nullary use-site call");
        let mut uniform = original.clone();
        // A supplementary spelling permutation, not the capture-avoidance test.
        uniform.names[GLOBAL.0] = "h1-permuted".into();
        uniform.names[LOCAL.0] = "h1-permuted".into();
        [original, local, global, poison, uniform]
            .iter()
            .map(Program::render)
            .collect()
    }

    /// Reductions keep the binder form, site, action, identities and reference
    /// edges. Rebuild the paired variants after each reduction; never shrink
    /// serialized strings separately. The caller retains only the same failure.
    pub fn reductions(&self) -> Vec<Self> {
        let mut out = Vec::new();
        if self.padding > 0 {
            let mut next = self.clone();
            next.padding -= 1;
            out.push(next);
        }
        if self.effects {
            let mut next = self.clone();
            next.effects = false;
            out.push(next);
        }
        for (which, minimum) in [(0, 1), (1, 5), (2, 99), (3, 199)] {
            let mut next = self.clone();
            let value = match which {
                0 => &mut next.global,
                1 => &mut next.local,
                2 => &mut next.assigned,
                _ => &mut next.poison,
            };
            if *value > minimum {
                *value = minimum;
                out.push(next);
            }
        }
        if self.spelling != "x" {
            let mut next = self.clone();
            next.spelling = "x".into();
            out.push(next);
        }
        out
    }
}

#[derive(Clone, Debug)]
struct Program {
    names: Vec<String>,
    forms: Vec<Expr>,
    template_target: Id,
}

impl Program {
    fn rename(&mut self, id: Id, fresh: String) {
        assert!(!self.names.contains(&fresh), "renamed binder must be fresh");
        // Definitions and every reference render through this one identity.
        self.names[id.0] = fresh;
    }

    fn render(&self) -> String {
        self.forms
            .iter()
            .map(|expr| expr.render(&self.names))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub const VARIANTS: [&str; 5] = [
    "original",
    "rename-local",
    "rename-global",
    "poison-shadow",
    "uniform-permutation",
];
