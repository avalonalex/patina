//! Syntactic keywords as environment bindings.
//!
//! Patina used to recognize `begin`, `if`, `lambda` and the rest by spelling,
//! in every scope, unconditionally — they had no entry in any environment.
//! Import sets and export resolution are binding-based, so neither could reach
//! them: `(export begin)` had nothing to resolve, `(rename (begin blk))` bound
//! nothing, `(except … begin)` excepted nothing.
//!
//! A [`CoreForm`] is that missing binding. It lives in the environment like any
//! other value, so the import machinery carries it without knowing what it is,
//! and the desugarer dispatches on the *form* rather than on the name it was
//! reached by. See `PRD/macro/SYNTAX_KEYWORD_BINDINGS_DESIGN.md`.

/// A syntactic keyword, as the value its name is bound to.
///
/// The two groups behave differently at the one place that matters — head
/// position of a form — and the split is the reason `patina-runtime` used to
/// keep two hand-written name lists side by side:
///
/// - [`CoreForm::is_dispatching`] forms select a desugaring rule.
/// - The rest are auxiliary: they mean something only inside an enclosing form
///   (`else` inside `cond`, `unquote` inside a template, `syntax-rules` inside
///   `define-syntax`), so in head position each is an error.
///
/// `apply` is deliberately absent. The desugarer special-cases it, but it is
/// also a real procedure binding, so it resolves the ordinary way and needs no
/// marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreForm {
    // Dispatching forms — each selects a `Desugarer::desugar_*` method.
    Quote,
    Quasiquote,
    Lambda,
    If,
    Set,
    Define,
    DefineSyntax,
    LetSyntax,
    LetrecSyntax,
    Begin,
    Import,
    CondExpand,
    Include,
    IncludeCi,
    SyntaxError,
    Expand,

    // Auxiliary keywords — meaningful only inside an enclosing form.
    Unquote,
    UnquoteSplicing,
    SyntaxRules,
    Underscore,
    Ellipsis,
    Else,
    Arrow,
}

/// Every `CoreForm`, in declaration order.
///
/// The one place that enumerates them: the intern table is built from this, so
/// a new variant cannot be added without also being allocated. It is the half
/// the compiler cannot check — a variant missing from [`CoreForm::name`] fails
/// to compile, a variant missing from here does not.
pub const ALL_CORE_FORMS: &[CoreForm] = &[
    CoreForm::Quote,
    CoreForm::Quasiquote,
    CoreForm::Lambda,
    CoreForm::If,
    CoreForm::Set,
    CoreForm::Define,
    CoreForm::DefineSyntax,
    CoreForm::LetSyntax,
    CoreForm::LetrecSyntax,
    CoreForm::Begin,
    CoreForm::Import,
    CoreForm::CondExpand,
    CoreForm::Include,
    CoreForm::IncludeCi,
    CoreForm::SyntaxError,
    CoreForm::Expand,
    CoreForm::Unquote,
    CoreForm::UnquoteSplicing,
    CoreForm::SyntaxRules,
    CoreForm::Underscore,
    CoreForm::Ellipsis,
    CoreForm::Else,
    CoreForm::Arrow,
];

impl CoreForm {
    /// The name this form is written with in source.
    ///
    /// This is its *canonical* spelling, not a claim about how it was reached:
    /// after `(rename (begin blk))` the same form answers `"begin"` while being
    /// bound to `blk`. Use it for diagnostics and for seeding an environment,
    /// never to decide what a use site means.
    pub fn name(self) -> &'static str {
        match self {
            CoreForm::Quote => "quote",
            CoreForm::Quasiquote => "quasiquote",
            CoreForm::Lambda => "lambda",
            CoreForm::If => "if",
            CoreForm::Set => "set!",
            CoreForm::Define => "define",
            CoreForm::DefineSyntax => "define-syntax",
            CoreForm::LetSyntax => "let-syntax",
            CoreForm::LetrecSyntax => "letrec-syntax",
            CoreForm::Begin => "begin",
            CoreForm::Import => "import",
            CoreForm::CondExpand => "cond-expand",
            CoreForm::Include => "include",
            CoreForm::IncludeCi => "include-ci",
            CoreForm::SyntaxError => "syntax-error",
            CoreForm::Expand => "expand",
            CoreForm::Unquote => "unquote",
            CoreForm::UnquoteSplicing => "unquote-splicing",
            CoreForm::SyntaxRules => "syntax-rules",
            CoreForm::Underscore => "_",
            CoreForm::Ellipsis => "...",
            CoreForm::Else => "else",
            CoreForm::Arrow => "=>",
        }
    }

    /// The form written as `name`, if any.
    pub fn from_name(name: &str) -> Option<CoreForm> {
        ALL_CORE_FORMS.iter().copied().find(|f| f.name() == name)
    }

    /// Does this form select a desugaring rule in head position?
    ///
    /// False for the auxiliary keywords, which are an error there.
    pub fn is_dispatching(self) -> bool {
        !matches!(
            self,
            CoreForm::Unquote
                | CoreForm::UnquoteSplicing
                | CoreForm::SyntaxRules
                | CoreForm::Underscore
                | CoreForm::Ellipsis
                | CoreForm::Else
                | CoreForm::Arrow
        )
    }
}

impl std::fmt::Display for CoreForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// `ALL_CORE_FORMS` is hand-written beside an enum the compiler checks, so
    /// it is the half that can silently fall behind. A missing entry would
    /// leave the form unallocated and unbound — recognized nowhere, with no
    /// compile error to say so.
    #[test]
    fn all_core_forms_has_no_duplicates_and_unique_names() {
        let names: HashSet<&str> = ALL_CORE_FORMS.iter().map(|f| f.name()).collect();
        assert_eq!(
            names.len(),
            ALL_CORE_FORMS.len(),
            "ALL_CORE_FORMS has a duplicate entry or two forms share a name"
        );
        for &form in ALL_CORE_FORMS {
            assert_eq!(CoreForm::from_name(form.name()), Some(form));
        }
    }

    #[test]
    fn apply_is_not_a_core_form() {
        // It is special-cased by the desugarer but is also a real procedure
        // binding, so it resolves the ordinary way.
        assert_eq!(CoreForm::from_name("apply"), None);
    }

    #[test]
    fn auxiliary_keywords_do_not_dispatch() {
        assert!(CoreForm::Begin.is_dispatching());
        assert!(CoreForm::Quote.is_dispatching());
        assert!(!CoreForm::Else.is_dispatching());
        assert!(!CoreForm::Ellipsis.is_dispatching());
        assert!(!CoreForm::SyntaxRules.is_dispatching());
    }
}
