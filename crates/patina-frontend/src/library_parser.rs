//! Library Declaration Parser
//!
//! Parses R7RS define-library forms into structured data.
//!
//! Syntax:
//! ```scheme
//! (define-library (library name parts ...)
//!   (export identifier ...)
//!   (import (scheme base) ...)
//!   (begin
//!     definitions and expressions)
//!   (include "file1.scm" "file2.scm"))
//! ```

use crate::ParseError;
use crate::cond_expand::{parse_library_name_tagged, tagged_list_to_vec};
use patina_core::{Cx, TaggedValue, Value};

/// Declarations that carry a library's implementation, so skipping one leaves
/// the library parsed and empty rather than merely incomplete.
///
/// The leniency policy is written for declarations that contribute nothing —
/// a vendor annotation, or the inert `#f` a cond-expand branch for another
/// implementation leaves behind. These are the opposite, and the failure mode
/// of skipping one is the `(chibi filesystem)` hazard Track L records three
/// times over: the library loads defining nothing and the error surfaces at
/// the importer, naming an export instead of the cause.
///
/// Kept as a rule rather than only as a dispatch arm because the shape check
/// runs *before* the keyword dispatch: `(include-shared . "x")` never reaches
/// the arm that refuses `(include-shared "x")`, and would otherwise take the
/// lenient path on the strength of being a recognized keyword.
const NEVER_SKIP: [&str; 1] = ["include-shared"];

/// The declaration keywords this parser implements — the same list the
/// dispatch in `parse_declaration_tagged` matches on, named here so a
/// malformed one can be reported as itself rather than as "unknown".
const KNOWN_DECLARATIONS: [&str; 8] = [
    "export",
    "import",
    "begin",
    "include",
    "include-ci",
    "cond-expand",
    "include-library-declarations",
    "include-shared",
];

// Re-export library types from runtime (they moved there to fix dependency issues)
pub use patina_runtime::library_loader::{ExportSpec, ImportSet};

/// Represents a body element in a library definition.
///
/// R7RS §5.6.1: The expressions from all `begin`, `include` and `include-ci`
/// library declarations are expanded in that environment in the order in which
/// they occur in the library.
#[derive(Debug, Clone)]
pub enum BodyElement {
    /// Inline code from `(begin expr1 expr2 ...)`
    ///
    /// The expressions, as words: a parsed library outlives the context it
    /// was read in, and its loader keeps them (`ParsedLibrary`).
    Begin(Vec<TaggedValue>),

    /// Files to include: `(include "file1.scm" "file2.scm" ...)`
    /// The bool indicates case-insensitive mode (include-ci)
    Include {
        /// File paths relative to the .sld file
        paths: Vec<String>,
        /// If true, parse with case-insensitive reader (include-ci)
        case_insensitive: bool,
    },

    /// Library declarations to include: `(include-library-declarations "file.scm" ...)`
    ///
    /// R7RS §5.6.1: The contents of the file are spliced directly into the
    /// current library definition. The file should contain library declarations
    /// like export, import, begin, include, etc.
    IncludeLibraryDeclarations {
        /// File paths relative to the .sld file
        paths: Vec<String>,
    },
}

/// Whether unknown `define-library` declarations abort the load.
///
/// Default is lenient (warn to stderr and skip the declaration), so one
/// vendor-specific clause does not make an otherwise-portable library
/// unloadable. Set `PATINA_STRICT_LIBRARY_SYNTAX=1` to restore the error.
fn strict_library_syntax() -> bool {
    std::env::var_os("PATINA_STRICT_LIBRARY_SYNTAX").is_some_and(|v| v != "0")
}

/// A parsed library definition
#[derive(Debug, Clone)]
pub struct LibraryDefinition {
    /// Library name as list of symbols: (scheme base) → ["scheme", "base"]
    pub name: Vec<String>,

    /// Export specifications
    pub exports: Vec<ExportSpec>,

    /// Import sets
    pub imports: Vec<ImportSet>,

    /// Library body elements in declaration order.
    /// Must be resolved by the loader to produce actual body expressions.
    pub body_elements: Vec<BodyElement>,
}

impl LibraryDefinition {
    /// Parse a define-library form
    ///
    /// Expects a list: (define-library <name> <declaration>*)
    ///
    /// Uses the interpreter's library catalogue, which the context's instance
    /// carries. Standalone parser heaps have no libraries; callers can instead
    /// supply an explicit checker through `from_tagged_with_library_checker`.
    pub fn from_tagged<'gc>(tv: Value<'gc>, cx: &Cx<'gc>) -> Result<Self, ParseError> {
        Self::from_tagged_with_library_checker(tv, cx, &|name| {
            crate::cond_expand::library_available(cx.instance(), name)
        })
    }

    /// Parse a define-library form with a library availability checker.
    ///
    /// The `can_load_library` callback is used for `(library <name>)` requirements
    /// in cond-expand clauses. It should return true if the named library can be loaded.
    ///
    /// Expects a list: (define-library <name> <declaration>*)
    pub fn from_tagged_with_library_checker<'gc>(
        tv: Value<'gc>,
        cx: &Cx<'gc>,
        can_load_library: &dyn Fn(&[String]) -> bool,
    ) -> Result<Self, ParseError> {
        let list = tagged_list_to_vec(tv, cx)?;

        if list.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "Empty define-library form".to_string(),
            ));
        }

        // First element is `define-library` (R7RS) or `library` (R6RS §7.1).
        let r6rs = cx.get_symbol_or_identifier_name(list[0]).as_deref() == Some("library");
        if r6rs && !crate::dialect::allow_r6rs() {
            return Err(ParseError::InvalidSyntax(
                "R6RS (library ...) form is not R7RS; use define-library \
                 (or pass --allow-r6rs to read it)"
                    .to_string(),
            ));
        }
        if !r6rs {
            // Anything else is reported against `define-library`, so the
            // message names the form the caller almost certainly meant.
            Self::expect_symbol_tagged(list[0], cx, "define-library")?;
        }

        if list.len() < 2 {
            let keyword = if r6rs { "library" } else { "define-library" };
            return Err(ParseError::InvalidSyntax(format!(
                "{keyword} requires a library name"
            )));
        }

        // Second element is the library name
        let name = parse_library_name_tagged(list[1], cx)?;

        // R6RS spells the body as bare forms after the export and import
        // clauses, where R7RS wraps it in `(begin …)`. Splitting at the first
        // form that is not an `export` or `import` clause is the same reading
        // as R6RS's fixed name/export/import/body order, without insisting the
        // two clauses be present or in that sequence. For `define-library`
        // there is no body to split off and every form is a declaration.
        let split = if r6rs {
            list[2..]
                .iter()
                .position(|&form| !Self::is_export_or_import_clause(form, cx))
                .map_or(list.len(), |offset| 2 + offset)
        } else {
            list.len()
        };

        // Rest are declarations
        let mut exports = Vec::new();
        let mut imports = Vec::new();
        let mut body_elements = Vec::new();

        for &decl in &list[2..split] {
            Self::parse_declaration_tagged(
                decl,
                &mut exports,
                &mut imports,
                &mut body_elements,
                can_load_library,
                cx,
                0,
            )?;
        }

        if split < list.len() {
            body_elements.push(BodyElement::Begin(words(&list[split..])));
        }

        Ok(LibraryDefinition {
            name,
            exports,
            imports,
            body_elements,
        })
    }

    /// Parse a library declaration from a TaggedValue, `depth` `cond-expand`s
    /// inside the library's own declarations. Declarations nest as code does,
    /// and are held to the same limit, [`MAX_FORM_DEPTH`], with the stack
    /// grown as they go (#617).
    ///
    /// [`MAX_FORM_DEPTH`]: patina_core::walk::MAX_FORM_DEPTH
    fn parse_declaration_tagged<'gc>(
        tv: Value<'gc>,
        exports: &mut Vec<ExportSpec>,
        imports: &mut Vec<ImportSet>,
        body_elements: &mut Vec<BodyElement>,
        can_load_library: &dyn Fn(&[String]) -> bool,
        cx: &Cx<'gc>,
        depth: usize,
    ) -> Result<(), ParseError> {
        if depth >= patina_core::walk::MAX_FORM_DEPTH {
            return Err(ParseError::InvalidSyntax(format!(
                "library declaration nested too deeply: more than {} levels",
                patina_core::walk::MAX_FORM_DEPTH
            )));
        }
        patina_core::walk::ensure_sufficient_stack(|| {
            Self::parse_declaration_node(
                tv,
                exports,
                imports,
                body_elements,
                can_load_library,
                cx,
                depth,
            )
        })
    }

    /// [`Self::parse_declaration_tagged`] for one declaration.
    fn parse_declaration_node<'gc>(
        tv: Value<'gc>,
        exports: &mut Vec<ExportSpec>,
        imports: &mut Vec<ImportSet>,
        body_elements: &mut Vec<BodyElement>,
        can_load_library: &dyn Fn(&[String]) -> bool,
        cx: &Cx<'gc>,
        depth: usize,
    ) -> Result<(), ParseError> {
        // A declaration that is not a proper list has no keyword to dispatch
        // on, so it falls under the same policy as an unknown keyword below:
        // skip it rather than abort the library. srfi-42 ships
        // `(cond-expand (stklos ...) (else #f))`, where the `#f` is inert
        // padding everywhere but stklos.
        let list = match tagged_list_to_vec(tv, cx) {
            Ok(list) => list,
            Err(_) => {
                // The shape test fires before anything looks at the head, so a
                // *known* keyword in a malformed declaration was reported as
                // "unknown library declaration `pair`" and its exports quietly
                // dropped — the importer then failed on an unrelated unbound
                // variable. Name the keyword when the head still has one; the
                // lenient policy is unchanged, only what it says (F3).
                let keyword = {
                    cx.try_pair(tv)
                        .and_then(|(car, _)| cx.get_symbol_or_identifier_name(car))
                        .map(|s| s.to_string())
                };
                return Self::skip_or_reject_declaration(
                    keyword.as_deref(),
                    &Self::describe_malformed(tv, cx),
                );
            }
        };

        if list.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "Empty library declaration".to_string(),
            ));
        }

        // First element determines the type
        let keyword = {
            cx.get_symbol_or_identifier_name(list[0])
                .map(|s| s.to_string())
        };

        if let Some(keyword) = keyword {
            match keyword.as_str() {
                "export" => {
                    for &spec in &list[1..] {
                        exports.push(Self::parse_export_spec_tagged(spec, cx)?);
                    }
                    Ok(())
                }
                "import" => {
                    for &set in &list[1..] {
                        imports.push(Self::parse_import_set_tagged(set, cx)?);
                    }
                    Ok(())
                }
                "begin" => {
                    body_elements.push(BodyElement::Begin(words(&list[1..])));
                    Ok(())
                }
                "include" => {
                    let paths = Self::parse_include_paths_tagged(&list[1..], cx)?;
                    body_elements.push(BodyElement::Include {
                        paths,
                        case_insensitive: false,
                    });
                    Ok(())
                }
                "include-ci" => {
                    let paths = Self::parse_include_paths_tagged(&list[1..], cx)?;
                    body_elements.push(BodyElement::Include {
                        paths,
                        case_insensitive: true,
                    });
                    Ok(())
                }
                "cond-expand" => Self::parse_cond_expand_tagged(
                    &list[1..],
                    exports,
                    imports,
                    body_elements,
                    can_load_library,
                    cx,
                    depth,
                ),
                "include-library-declarations" => {
                    let paths = Self::parse_include_paths_tagged(&list[1..], cx)?;
                    body_elements.push(BodyElement::IncludeLibraryDeclarations { paths });
                    Ok(())
                }
                // Not an unknown clause, and deliberately not on the lenient
                // path: `include-shared` names a compiled shared object that
                // *is* the library's implementation, so skipping it leaves the
                // library parsed and empty and the failure surfaces at the
                // importer, naming an export instead of the cause — the
                // `(chibi filesystem)` hazard recorded in Track L §L2. Refuse
                // it here, where the cause is still in hand.
                //
                // A branch of a `cond-expand` that is not taken never reaches
                // this dispatch, so a library whose `include-shared` sits in
                // an implementation-specific branch (chibi's `(chibi crypto
                // sha2)` and `(chibi math linalg)` both do) keeps its portable
                // path.
                "include-shared" => Err(ParseError::NativeExtensionRequired(
                    Self::parse_shared_object_name(&list[1..], cx),
                )),
                _ => Self::skip_or_reject_declaration(
                    Some(keyword.as_str()),
                    &format!("unknown declaration `{keyword}`"),
                ),
            }
        } else {
            let described = cx.type_name(list[0]).to_string();
            Self::skip_or_reject_declaration(
                None,
                &format!("a declaration headed by a {described}"),
            )
        }
    }

    /// Describe a declaration that is not a proper list, naming its keyword
    /// when the head is one this parser knows.
    fn describe_malformed<'gc>(tv: Value<'gc>, cx: &Cx<'gc>) -> String {
        let head = cx.try_pair(tv).map(|(car, _)| car);
        let keyword = head.and_then(|car| cx.get_symbol_or_identifier_name(car));
        match keyword {
            Some(keyword) if KNOWN_DECLARATIONS.contains(&&*keyword) => format!(
                "malformed `{keyword}` declaration; everything it would have \
                 contributed goes with it"
            ),
            Some(keyword) => format!("unknown declaration `{keyword}`"),
            None => format!(
                "a declaration that is not a proper list (a {})",
                cx.type_name(tv)
            ),
        }
    }

    /// Handle a `define-library` declaration this parser does not implement,
    /// or cannot read in the shape it was written.
    ///
    /// Portable `.sld` files occasionally carry vendor-specific declarations,
    /// and cond-expand branches meant for other implementations can leave
    /// shapes that are not declarations at all. Skipping one keeps the rest of
    /// the library loadable; aborting the whole load is opt-in via
    /// `PATINA_STRICT_LIBRARY_SYNTAX=1`.
    ///
    /// `keyword` names the head when there still is one, so a declaration in
    /// `NEVER_SKIP` is refused whatever shape it arrived in — leniency is for
    /// declarations whose loss costs nothing.
    fn skip_or_reject_declaration(
        keyword: Option<&str>,
        described: &str,
    ) -> Result<(), ParseError> {
        if let Some(keyword) = keyword
            && NEVER_SKIP.contains(&keyword)
        {
            return Err(ParseError::InvalidSyntax(format!(
                "define-library: cannot skip {described} — it carries the library's \
                 implementation, and skipping it would leave the library empty"
            )));
        }
        if strict_library_syntax() {
            Err(ParseError::InvalidSyntax(format!(
                "define-library: rejected {described}"
            )))
        } else {
            eprintln!("warning: define-library: ignoring {described}");
            Ok(())
        }
    }

    /// Name the shared object an `include-shared` declaration asks for, for
    /// the error that refuses it.
    ///
    /// The first string, because that is what the declaration carries — all
    /// seven occurrences across `compat/vendor/` write exactly one — and
    /// because the one consumer, the compat harness, reads back one quoted
    /// name. Nothing is rejected here: this is reporting rather than loading,
    /// and a second error about the shape of a declaration we are refusing
    /// anyway would bury the real one.
    fn parse_shared_object_name<'gc>(values: &[Value<'gc>], cx: &Cx<'gc>) -> String {
        values
            .iter()
            .filter_map(|&tv| cx.get_string_contents(tv))
            .find(|name| !name.is_empty())
            .unwrap_or_else(|| "<unnamed>".to_string())
    }

    /// Parse include file paths
    fn parse_include_paths_tagged<'gc>(
        values: &[Value<'gc>],
        cx: &Cx<'gc>,
    ) -> Result<Vec<String>, ParseError> {
        if values.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "include requires at least one filename".to_string(),
            ));
        }
        let mut paths = Vec::new();
        for &tv in values {
            if let Some(s) = cx.get_string_contents(tv) {
                paths.push(s);
            } else {
                return Err(ParseError::InvalidSyntax(format!(
                    "include filename must be a string, got: {}",
                    cx.type_name(tv)
                )));
            }
        }
        Ok(paths)
    }

    /// Parse cond-expand declaration in library context (tagged version)
    fn parse_cond_expand_tagged<'gc>(
        clauses: &[Value<'gc>],
        exports: &mut Vec<ExportSpec>,
        imports: &mut Vec<ImportSet>,
        body_elements: &mut Vec<BodyElement>,
        can_load_library: &dyn Fn(&[String]) -> bool,
        cx: &Cx<'gc>,
        depth: usize,
    ) -> Result<(), ParseError> {
        use crate::cond_expand::evaluate_feature_requirement_tagged;

        if clauses.is_empty() {
            return Err(ParseError::InvalidSyntax(
                "cond-expand requires at least one clause".to_string(),
            ));
        }

        // The instance's registry, the same one the desugarer and `(features)`
        // read — so a `cond-expand` in a `.sld` declaration and one in its
        // `begin` body cannot answer differently.
        let features = cx.instance().features_and_close().clone();

        for &clause in clauses {
            let clause_list = tagged_list_to_vec(clause, cx)?;

            if clause_list.is_empty() {
                return Err(ParseError::InvalidSyntax(
                    "Empty cond-expand clause".to_string(),
                ));
            }

            // Check for else clause
            let is_else =
                { cx.get_symbol_or_identifier_name(clause_list[0]).as_deref() == Some("else") };

            let matches = if is_else {
                true
            } else {
                evaluate_feature_requirement_tagged(
                    clause_list[0],
                    cx,
                    &features,
                    can_load_library,
                )?
            };

            if matches {
                for &decl in &clause_list[1..] {
                    Self::parse_declaration_tagged(
                        decl,
                        exports,
                        imports,
                        body_elements,
                        can_load_library,
                        cx,
                        depth + 1,
                    )?;
                }
                return Ok(());
            }
        }

        Ok(())
    }

    /// Parse an export spec
    fn parse_export_spec_tagged<'gc>(
        tv: Value<'gc>,
        cx: &Cx<'gc>,
    ) -> Result<ExportSpec, ParseError> {
        // Check if it's a simple symbol
        {
            if let Some(name) = cx.get_symbol_or_identifier_name(tv) {
                return Ok(ExportSpec::Identifier(name.to_string()));
            }
            if tv.is_pair() {
                // fall through to list parsing below
            } else {
                return Err(ParseError::InvalidSyntax(format!(
                    "Invalid export spec: {}",
                    cx.type_name(tv)
                )));
            }
        }

        // (rename internal external)
        let list = tagged_list_to_vec(tv, cx)?;

        if list.len() != 3 {
            return Err(ParseError::InvalidSyntax(
                "rename export requires exactly 3 elements".to_string(),
            ));
        }

        Self::expect_symbol_tagged(list[0], cx, "rename")?;
        let internal = cx
            .get_symbol_or_identifier_name(list[1])
            .ok_or_else(|| {
                ParseError::InvalidSyntax("rename internal name must be a symbol".to_string())
            })?
            .to_string();

        let external = cx
            .get_symbol_or_identifier_name(list[2])
            .ok_or_else(|| {
                ParseError::InvalidSyntax("rename external name must be a symbol".to_string())
            })?
            .to_string();

        Ok(ExportSpec::Rename { internal, external })
    }

    /// Parse an import set
    pub fn parse_import_set_tagged<'gc>(
        tv: Value<'gc>,
        cx: &Cx<'gc>,
    ) -> Result<ImportSet, ParseError> {
        Self::parse_import_set_at(tv, cx, 0)
    }

    /// [`Self::parse_import_set_tagged`] for an import set `depth` modifiers
    /// inside the one being parsed. Modifiers nest as code does, and are held
    /// to the same limit, [`MAX_FORM_DEPTH`], with the stack grown as they go
    /// (#617).
    ///
    /// [`MAX_FORM_DEPTH`]: patina_core::walk::MAX_FORM_DEPTH
    fn parse_import_set_at<'gc>(
        tv: Value<'gc>,
        cx: &Cx<'gc>,
        depth: usize,
    ) -> Result<ImportSet, ParseError> {
        if depth >= patina_core::walk::MAX_FORM_DEPTH {
            return Err(ParseError::InvalidSyntax(format!(
                "import set nested too deeply: more than {} levels",
                patina_core::walk::MAX_FORM_DEPTH
            )));
        }
        patina_core::walk::ensure_sufficient_stack(|| Self::parse_import_set_node(tv, cx, depth))
    }

    /// [`Self::parse_import_set_at`] for one import set.
    fn parse_import_set_node<'gc>(
        tv: Value<'gc>,
        cx: &Cx<'gc>,
        depth: usize,
    ) -> Result<ImportSet, ParseError> {
        {
            if !tv.is_pair() {
                return Err(ParseError::InvalidSyntax(format!(
                    "Invalid import set: {}",
                    cx.type_name(tv)
                )));
            }
        }

        let list = tagged_list_to_vec(tv, cx)?;

        if list.is_empty() {
            return Err(ParseError::InvalidSyntax("Empty import set".to_string()));
        }

        // Import-set keywords and operands are names in the import grammar,
        // not variable references. Macro expansion can wrap them in scoped
        // identifiers (#546); accept those just as library-name parsing does.
        // The enclosing `import` form is still recognized by its binding.
        let first_sym = {
            cx.get_symbol_or_identifier_name(list[0])
                .map(|s| s.to_string())
        };

        if let Some(ref first) = first_sym {
            match first.as_str() {
                "only" => return Self::parse_only_import_tagged(&list, cx, depth),
                "except" => return Self::parse_except_import_tagged(&list, cx, depth),
                "prefix" => return Self::parse_prefix_import_tagged(&list, cx, depth),
                "rename" => return Self::parse_rename_import_tagged(&list, cx, depth),
                _ => {}
            }
        }

        // It's a library name
        parse_library_name_tagged(tv, cx).map(ImportSet::Library)
    }

    /// Parse (only <import-set> id1 id2 ...)
    fn parse_only_import_tagged<'gc>(
        list: &[Value<'gc>],
        cx: &Cx<'gc>,
        depth: usize,
    ) -> Result<ImportSet, ParseError> {
        if list.len() < 2 {
            return Err(ParseError::InvalidSyntax(
                "only requires an import set".to_string(),
            ));
        }

        let import_set = Box::new(Self::parse_import_set_at(list[1], cx, depth + 1)?);
        let mut identifiers = Vec::new();

        for &id_tv in &list[2..] {
            if let Some(name) = cx.get_symbol_or_identifier_name(id_tv) {
                identifiers.push(name.to_string());
            } else {
                return Err(ParseError::InvalidSyntax(
                    "only identifiers must be symbols".to_string(),
                ));
            }
        }

        Ok(ImportSet::Only {
            import_set,
            identifiers,
        })
    }

    /// Parse (except <import-set> id1 id2 ...)
    fn parse_except_import_tagged<'gc>(
        list: &[Value<'gc>],
        cx: &Cx<'gc>,
        depth: usize,
    ) -> Result<ImportSet, ParseError> {
        if list.len() < 2 {
            return Err(ParseError::InvalidSyntax(
                "except requires an import set".to_string(),
            ));
        }

        let import_set = Box::new(Self::parse_import_set_at(list[1], cx, depth + 1)?);
        let mut identifiers = Vec::new();

        for &id_tv in &list[2..] {
            if let Some(name) = cx.get_symbol_or_identifier_name(id_tv) {
                identifiers.push(name.to_string());
            } else {
                return Err(ParseError::InvalidSyntax(
                    "except identifiers must be symbols".to_string(),
                ));
            }
        }

        Ok(ImportSet::Except {
            import_set,
            identifiers,
        })
    }

    /// Parse (prefix <import-set> prefix-id)
    fn parse_prefix_import_tagged<'gc>(
        list: &[Value<'gc>],
        cx: &Cx<'gc>,
        depth: usize,
    ) -> Result<ImportSet, ParseError> {
        if list.len() != 3 {
            return Err(ParseError::InvalidSyntax(
                "prefix requires exactly 2 arguments".to_string(),
            ));
        }

        let import_set = Box::new(Self::parse_import_set_at(list[1], cx, depth + 1)?);
        let prefix = {
            cx.get_symbol_or_identifier_name(list[2])
                .ok_or_else(|| ParseError::InvalidSyntax("prefix must be a symbol".to_string()))?
                .to_string()
        };

        Ok(ImportSet::Prefix { import_set, prefix })
    }

    /// Parse (rename <import-set> (old1 new1) (old2 new2) ...)
    fn parse_rename_import_tagged<'gc>(
        list: &[Value<'gc>],
        cx: &Cx<'gc>,
        depth: usize,
    ) -> Result<ImportSet, ParseError> {
        if list.len() < 2 {
            return Err(ParseError::InvalidSyntax(
                "rename requires an import set".to_string(),
            ));
        }

        let import_set = Box::new(Self::parse_import_set_at(list[1], cx, depth + 1)?);
        let mut renames = Vec::new();

        for &pair_tv in &list[2..] {
            let pair = tagged_list_to_vec(pair_tv, cx)?;

            if pair.len() != 2 {
                return Err(ParseError::InvalidSyntax(
                    "rename pair must have exactly 2 elements".to_string(),
                ));
            }
            let old = cx
                .get_symbol_or_identifier_name(pair[0])
                .ok_or_else(|| {
                    ParseError::InvalidSyntax("rename old name must be a symbol".to_string())
                })?
                .to_string();

            let new = cx
                .get_symbol_or_identifier_name(pair[1])
                .ok_or_else(|| {
                    ParseError::InvalidSyntax("rename new name must be a symbol".to_string())
                })?
                .to_string();

            renames.push((old, new));
        }

        Ok(ImportSet::Rename {
            import_set,
            renames,
        })
    }

    /// Does this form open with `export` or `import`?
    ///
    /// Used only to find where an R6RS library's clauses end and its body
    /// begins. A body form with either head would be a call to a procedure of
    /// that name, which R6RS does not permit in this position and Patina does
    /// not define.
    fn is_export_or_import_clause<'gc>(tv: Value<'gc>, cx: &Cx<'gc>) -> bool {
        if !tv.is_pair() {
            return false;
        }
        matches!(
            cx.get_symbol_or_identifier_name(cx.car(tv)).as_deref(),
            Some("export") | Some("import")
        )
    }

    /// Helper: expect a specific symbol
    fn expect_symbol_tagged<'gc>(
        tv: Value<'gc>,
        cx: &Cx<'gc>,
        expected: &str,
    ) -> Result<(), ParseError> {
        if let Some(name) = cx.get_symbol_or_identifier_name(tv) {
            if &*name == expected {
                Ok(())
            } else {
                Err(ParseError::InvalidSyntax(format!(
                    "Expected '{}', got '{}'",
                    expected, name
                )))
            }
        } else {
            Err(ParseError::InvalidSyntax(format!(
                "Expected symbol '{}', got: {}",
                expected,
                cx.type_name(tv)
            )))
        }
    }
}

/// The words of `values`, for a body a parsed library keeps.
fn words(values: &[Value<'_>]) -> Vec<TaggedValue> {
    values.iter().map(|v| v.tagged()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `f` in a context over a fresh heap.
    fn with_cx<R>(f: impl for<'gc> FnOnce(&mut Cx<'gc>) -> R) -> R {
        patina_core::NoGcScope::with_cx(&patina_core::new_shared_heap(), f)
    }

    fn sym<'gc>(s: &str, cx: &mut Cx<'gc>) -> Value<'gc> {
        cx.intern_symbol(s)
    }

    fn fixnum<'gc>(n: i64) -> Value<'gc> {
        Value::fixnum(n)
    }

    fn str_val<'gc>(s: &str, cx: &mut Cx<'gc>) -> Value<'gc> {
        cx.alloc_string(s.to_string())
    }

    fn list<'gc>(items: Vec<Value<'gc>>, cx: &mut Cx<'gc>) -> Value<'gc> {
        cx.list_from_iter(items)
    }

    #[test]
    fn test_parse_simple_library() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(vec![sym("export", cx), sym("foo", cx), sym("bar", cx)], cx),
                    list(
                        vec![
                            sym("import", cx),
                            list(vec![sym("scheme", cx), sym("base", cx)], cx),
                        ],
                        cx,
                    ),
                    list(vec![sym("begin", cx)], cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();

            assert_eq!(parsed.name, vec!["test", "lib"]);
            assert_eq!(parsed.exports.len(), 2);
            assert_eq!(parsed.imports.len(), 1);
        });
    }

    #[test]
    fn test_parse_export_rename() {
        with_cx(|cx| {
            let export_spec = list(
                vec![
                    sym("rename", cx),
                    sym("internal-name", cx),
                    sym("external-name", cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::parse_export_spec_tagged(export_spec, cx).unwrap();

            assert_eq!(
                parsed,
                ExportSpec::Rename {
                    internal: "internal-name".to_string(),
                    external: "external-name".to_string(),
                }
            );
        });
    }

    #[test]
    fn test_parse_import_only() {
        with_cx(|cx| {
            let import_set = list(
                vec![
                    sym("only", cx),
                    list(vec![sym("scheme", cx), sym("base", cx)], cx),
                    sym("car", cx),
                    sym("cdr", cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::parse_import_set_tagged(import_set, cx).unwrap();

            match parsed {
                ImportSet::Only { identifiers, .. } => {
                    assert_eq!(identifiers, vec!["car", "cdr"]);
                }
                _ => panic!("Expected Only variant"),
            }
        });
    }

    #[test]
    fn test_parse_import_prefix() {
        with_cx(|cx| {
            let import_set = list(
                vec![
                    sym("prefix", cx),
                    list(vec![sym("scheme", cx), sym("base", cx)], cx),
                    sym("scheme:", cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::parse_import_set_tagged(import_set, cx).unwrap();

            match parsed {
                ImportSet::Prefix { prefix, .. } => {
                    assert_eq!(prefix, "scheme:");
                }
                _ => panic!("Expected Prefix variant"),
            }
        });
    }

    #[test]
    fn test_unknown_declaration_skipped_leniently() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    // A vendor-specific declaration Patina does not know.
                    list(vec![sym("declare", cx), sym("pure", cx)], cx),
                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                    list(vec![sym("begin", cx), sym("foo-def", cx)], cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx)
                .expect("unknown declaration must not abort the load");

            // The unknown clause is dropped; everything around it survives.
            assert_eq!(parsed.name, vec!["test", "lib"]);
            assert_eq!(parsed.exports.len(), 1);
            assert_eq!(parsed.body_elements.len(), 1);
        });
    }

    // =========================================================================
    // Integer Library Names (R7RS §5.6)
    // =========================================================================

    #[test]
    fn test_parse_library_name_with_integer() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("srfi", cx), fixnum(1)], cx),
                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.name, vec!["srfi", "1"]);
        });
    }

    #[test]
    fn test_parse_library_name_integer_in_middle() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("srfi", cx), fixnum(1), sym("lists", cx)], cx),
                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.name, vec!["srfi", "1", "lists"]);
        });
    }

    #[test]
    fn test_parse_library_name_multiple_integers() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("lib", cx), fixnum(1), fixnum(2), fixnum(3)], cx),
                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.name, vec!["lib", "1", "2", "3"]);
        });
    }

    #[test]
    fn test_parse_library_name_zero() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("lib", cx), fixnum(0)], cx),
                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.name, vec!["lib", "0"]);
        });
    }

    #[test]
    fn test_parse_library_name_large_integer() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("srfi", cx), fixnum(125)], cx),
                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.name, vec!["srfi", "125"]);
        });
    }

    #[test]
    fn test_parse_library_name_negative_integer_rejected() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), fixnum(-1)], cx),
                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                ],
                cx,
            );

            let result = LibraryDefinition::from_tagged(lib_def, cx);
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(
                err.to_string().contains("non-negative"),
                "Error should mention non-negative: {}",
                err
            );
        });
    }

    #[test]
    fn empty_import_modifiers_are_valid() {
        with_cx(|cx| {
            for modifier in ["only", "except", "rename"] {
                let set = list(
                    vec![
                        sym(modifier, cx),
                        list(vec![sym("scheme", cx), sym("base", cx)], cx),
                    ],
                    cx,
                );
                assert!(LibraryDefinition::parse_import_set_tagged(set, cx).is_ok());
            }
        });
    }

    #[test]
    fn test_parse_import_set_with_integer() {
        with_cx(|cx| {
            let import_set = list(vec![sym("srfi", cx), fixnum(1)], cx);

            let parsed = LibraryDefinition::parse_import_set_tagged(import_set, cx).unwrap();

            match parsed {
                ImportSet::Library(name) => {
                    assert_eq!(name, vec!["srfi", "1"]);
                }
                _ => panic!("Expected Library variant"),
            }
        });
    }

    #[test]
    fn test_parse_import_only_with_integer_library() {
        with_cx(|cx| {
            let import_set = list(
                vec![
                    sym("only", cx),
                    list(vec![sym("srfi", cx), fixnum(1)], cx),
                    sym("xcons", cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::parse_import_set_tagged(import_set, cx).unwrap();

            match parsed {
                ImportSet::Only {
                    import_set,
                    identifiers,
                } => {
                    match *import_set {
                        ImportSet::Library(name) => {
                            assert_eq!(name, vec!["srfi", "1"]);
                        }
                        _ => panic!("Expected Library variant in Only"),
                    }
                    assert_eq!(identifiers, vec!["xcons"]);
                }
                _ => panic!("Expected Only variant"),
            }
        });
    }

    // =========================================================================
    // Include Declaration (R7RS §5.6.1)
    // =========================================================================

    #[test]
    fn test_parse_include_single_file() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                    list(vec![sym("include", cx), str_val("impl.scm", cx)], cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.name, vec!["test", "lib"]);
            assert_eq!(parsed.body_elements.len(), 1);

            match &parsed.body_elements[0] {
                BodyElement::Include {
                    paths,
                    case_insensitive,
                } => {
                    assert_eq!(paths, &vec!["impl.scm".to_string()]);
                    assert!(!case_insensitive);
                }
                _ => panic!("Expected Include variant"),
            }
        });
    }

    #[test]
    fn test_parse_include_multiple_files() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(
                        vec![
                            sym("include", cx),
                            str_val("a.scm", cx),
                            str_val("b.scm", cx),
                            str_val("c.scm", cx),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();

            match &parsed.body_elements[0] {
                BodyElement::Include { paths, .. } => {
                    assert_eq!(
                        paths,
                        &vec![
                            "a.scm".to_string(),
                            "b.scm".to_string(),
                            "c.scm".to_string()
                        ]
                    );
                }
                _ => panic!("Expected Include variant"),
            }
        });
    }

    #[test]
    fn test_parse_include_with_subdirectory() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("srfi", cx), fixnum(1)], cx),
                    list(
                        vec![sym("include", cx), str_val("1/predicates.scm", cx)],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();

            match &parsed.body_elements[0] {
                BodyElement::Include { paths, .. } => {
                    assert_eq!(paths, &vec!["1/predicates.scm".to_string()]);
                }
                _ => panic!("Expected Include variant"),
            }
        });
    }

    #[test]
    fn test_parse_include_ci() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(vec![sym("include-ci", cx), str_val("legacy.scm", cx)], cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();

            match &parsed.body_elements[0] {
                BodyElement::Include {
                    paths,
                    case_insensitive,
                } => {
                    assert_eq!(paths, &vec!["legacy.scm".to_string()]);
                    assert!(case_insensitive);
                }
                _ => panic!("Expected Include variant"),
            }
        });
    }

    #[test]
    fn test_parse_include_empty_rejected() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(vec![sym("include", cx)], cx),
                ],
                cx,
            );

            let result = LibraryDefinition::from_tagged(lib_def, cx);
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("at least one filename")
            );
        });
    }

    #[test]
    fn test_parse_include_non_string_rejected() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(vec![sym("include", cx), sym("foo", cx)], cx),
                ],
                cx,
            );

            let result = LibraryDefinition::from_tagged(lib_def, cx);
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("must be a string"));
        });
    }

    #[test]
    fn test_parse_mixed_begin_and_include() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                    list(vec![sym("begin", cx), sym("expr1", cx)], cx),
                    list(vec![sym("include", cx), str_val("impl.scm", cx)], cx),
                    list(vec![sym("begin", cx), sym("expr2", cx)], cx),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.body_elements.len(), 3);

            assert!(matches!(&parsed.body_elements[0], BodyElement::Begin(_)));
            assert!(matches!(
                &parsed.body_elements[1],
                BodyElement::Include { .. }
            ));
            assert!(matches!(&parsed.body_elements[2], BodyElement::Begin(_)));
        });
    }

    #[test]
    fn test_parse_body_elements_order_preserved() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("order", cx)], cx),
                    list(vec![sym("begin", cx), sym("first", cx)], cx),
                    list(vec![sym("include", cx), str_val("second.scm", cx)], cx),
                    list(vec![sym("begin", cx), sym("third", cx)], cx),
                    list(
                        vec![
                            sym("include", cx),
                            str_val("fourth.scm", cx),
                            str_val("fifth.scm", cx),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.body_elements.len(), 4);

            // Verify order is preserved
            match &parsed.body_elements[0] {
                BodyElement::Begin(exprs) => {
                    assert_eq!(exprs[0], cx.intern_symbol("first").tagged());
                }
                _ => panic!("Expected Begin"),
            }

            match &parsed.body_elements[1] {
                BodyElement::Include { paths, .. } => {
                    assert_eq!(paths[0], "second.scm");
                }
                _ => panic!("Expected Include"),
            }

            match &parsed.body_elements[2] {
                BodyElement::Begin(exprs) => {
                    assert_eq!(exprs[0], cx.intern_symbol("third").tagged());
                }
                _ => panic!("Expected Begin"),
            }

            match &parsed.body_elements[3] {
                BodyElement::Include { paths, .. } => {
                    assert_eq!(paths.len(), 2);
                    assert_eq!(paths[0], "fourth.scm");
                    assert_eq!(paths[1], "fifth.scm");
                }
                _ => panic!("Expected Include"),
            }
        });
    }

    // =========================================================================
    // cond-expand Declaration (R7RS §5.6.1)
    // =========================================================================

    #[test]
    fn test_cond_expand_simple_feature() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(
                        vec![
                            sym("cond-expand", cx),
                            list(
                                vec![
                                    sym("r7rs", cx),
                                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                                ],
                                cx,
                            ),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.exports.len(), 1);
            assert!(matches!(&parsed.exports[0], ExportSpec::Identifier(s) if s == "foo"));
        });
    }

    #[test]
    fn test_cond_expand_patina_feature() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(
                        vec![
                            sym("cond-expand", cx),
                            list(
                                vec![
                                    sym("patina", cx),
                                    list(vec![sym("export", cx), sym("bar", cx)], cx),
                                ],
                                cx,
                            ),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.exports.len(), 1);
            assert!(matches!(&parsed.exports[0], ExportSpec::Identifier(s) if s == "bar"));
        });
    }

    #[test]
    fn test_cond_expand_else_clause() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(
                        vec![
                            sym("cond-expand", cx),
                            list(
                                vec![
                                    sym("nonexistent", cx),
                                    list(vec![sym("export", cx), sym("bad", cx)], cx),
                                ],
                                cx,
                            ),
                            list(
                                vec![
                                    sym("else", cx),
                                    list(vec![sym("export", cx), sym("good", cx)], cx),
                                ],
                                cx,
                            ),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.exports.len(), 1);
            assert!(matches!(&parsed.exports[0], ExportSpec::Identifier(s) if s == "good"));
        });
    }

    #[test]
    fn test_cond_expand_no_match() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(
                        vec![
                            sym("cond-expand", cx),
                            list(
                                vec![
                                    sym("nonexistent", cx),
                                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                                ],
                                cx,
                            ),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.exports.len(), 0);
        });
    }

    #[test]
    fn test_cond_expand_multiple_declarations() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(
                        vec![
                            sym("cond-expand", cx),
                            list(
                                vec![
                                    sym("r7rs", cx),
                                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                                    list(vec![sym("export", cx), sym("bar", cx)], cx),
                                ],
                                cx,
                            ),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.exports.len(), 2);
        });
    }

    #[test]
    fn test_cond_expand_and_requirement() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(
                        vec![
                            sym("cond-expand", cx),
                            list(
                                vec![
                                    list(
                                        vec![sym("and", cx), sym("r7rs", cx), sym("patina", cx)],
                                        cx,
                                    ),
                                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                                ],
                                cx,
                            ),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.exports.len(), 1);
        });
    }

    #[test]
    fn test_cond_expand_or_requirement() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(
                        vec![
                            sym("cond-expand", cx),
                            list(
                                vec![
                                    list(
                                        vec![
                                            sym("or", cx),
                                            sym("nonexistent", cx),
                                            sym("r7rs", cx),
                                        ],
                                        cx,
                                    ),
                                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                                ],
                                cx,
                            ),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.exports.len(), 1);
        });
    }

    #[test]
    fn test_cond_expand_not_requirement() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(
                        vec![
                            sym("cond-expand", cx),
                            list(
                                vec![
                                    list(vec![sym("not", cx), sym("nonexistent", cx)], cx),
                                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                                ],
                                cx,
                            ),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.exports.len(), 1);
        });
    }

    #[test]
    fn test_cond_expand_with_begin() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(vec![sym("export", cx), sym("x", cx)], cx),
                    list(
                        vec![
                            sym("cond-expand", cx),
                            list(
                                vec![
                                    sym("r7rs", cx),
                                    list(
                                        vec![
                                            sym("begin", cx),
                                            list(
                                                vec![sym("define", cx), sym("x", cx), fixnum(1)],
                                                cx,
                                            ),
                                        ],
                                        cx,
                                    ),
                                ],
                                cx,
                            ),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.body_elements.len(), 1);
            assert!(matches!(&parsed.body_elements[0], BodyElement::Begin(_)));
        });
    }

    #[test]
    fn test_cond_expand_with_include() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(vec![sym("export", cx), sym("foo", cx)], cx),
                    list(
                        vec![
                            sym("cond-expand", cx),
                            list(
                                vec![
                                    sym("r7rs", cx),
                                    list(vec![sym("include", cx), str_val("impl.scm", cx)], cx),
                                ],
                                cx,
                            ),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.body_elements.len(), 1);
            match &parsed.body_elements[0] {
                BodyElement::Include { paths, .. } => {
                    assert_eq!(paths[0], "impl.scm");
                }
                _ => panic!("Expected Include"),
            }
        });
    }

    #[test]
    fn test_cond_expand_first_match_wins() {
        with_cx(|cx| {
            let lib_def = list(
                vec![
                    sym("define-library", cx),
                    list(vec![sym("test", cx), sym("lib", cx)], cx),
                    list(
                        vec![
                            sym("cond-expand", cx),
                            list(
                                vec![
                                    sym("r7rs", cx),
                                    list(vec![sym("export", cx), sym("first", cx)], cx),
                                ],
                                cx,
                            ),
                            list(
                                vec![
                                    sym("patina", cx),
                                    list(vec![sym("export", cx), sym("second", cx)], cx),
                                ],
                                cx,
                            ),
                        ],
                        cx,
                    ),
                ],
                cx,
            );

            let parsed = LibraryDefinition::from_tagged(lib_def, cx).unwrap();
            assert_eq!(parsed.exports.len(), 1);
            assert!(matches!(&parsed.exports[0], ExportSpec::Identifier(s) if s == "first"));
        });
    }
}
