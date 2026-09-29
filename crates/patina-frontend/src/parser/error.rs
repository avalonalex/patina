use crate::SourceMap;
use crate::lexer::{LexError, ReadSpan, Token};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    /// Location belongs to the error, including when lookahead defers it.
    /// Library declaration errors built from values may have no text span.
    #[error("{error}")]
    Located {
        #[source]
        error: Box<ParseError>,
        span: ReadSpan,
        opening: Option<(u32, u32)>,
    },

    #[error("{0}")]
    LexError(#[from] LexError),

    /// The input ended where a datum was required: `parse` was called with
    /// nothing left but whitespace and comments. `parse_next` reports that
    /// case as `Ok(None)` instead, so a caller reading forms until the end
    /// of the input never sees this.
    #[error("Unexpected end of input")]
    UnexpectedEof,

    /// The input ended inside a datum. `line` and `column` are where that
    /// datum — the outermost one being read — began: the end of the input
    /// is where the reader stopped, not where the problem is, and a
    /// truncated file is found by the form it cut short.
    #[error("Unexpected end of input inside the datum beginning at line {line}, column {column}")]
    IncompleteDatum { line: u32, column: u32 },

    #[error("Unexpected token: {0}")]
    UnexpectedToken(Token),

    #[error("Invalid syntax: {0}")]
    InvalidSyntax(String),

    #[error("Undefined datum label: #{0}#")]
    UndefinedLabel(usize),

    #[error("Duplicate datum label: #{0}=")]
    DuplicateLabel(usize),

    /// A `define-library` reached an `include-shared` declaration: its
    /// implementation is a compiled shared object, which Patina cannot load.
    ///
    /// Its own variant rather than an `InvalidSyntax` string because the
    /// compatibility harness classifies on it — the library is out of scope
    /// pending FFI, not broken — and because a caller that wants to say so in
    /// its own words needs to be able to tell it apart. See
    /// `LibraryError::NativeExtensionRequired`.
    #[error("include-shared \"{0}\"")]
    NativeExtensionRequired(String),
}

impl ParseError {
    /// The category without its optional source annotation.
    pub fn kind(&self) -> &Self {
        match self {
            Self::Located { error, .. } => error.kind(),
            other => other,
        }
    }

    pub fn span(&self) -> Option<ReadSpan> {
        match self {
            Self::Located { span, .. } => Some(*span),
            Self::LexError(error) => error.span(),
            _ => None,
        }
    }

    fn opening(&self) -> Option<(u32, u32)> {
        match self {
            Self::Located { opening, error, .. } => opening.or_else(|| error.opening()),
            Self::LexError(error) => error.opening(),
            _ => None,
        }
    }

    pub(super) fn at(self, fallback: ReadSpan) -> Self {
        if matches!(self, Self::Located { .. }) {
            return self;
        }
        let span = self.span().unwrap_or(fallback);
        Self::Located {
            error: Box::new(self),
            span,
            opening: None,
        }
    }

    pub(super) fn within(mut self, at: Option<(u32, u32)>) -> Self {
        if matches!(self.kind(), Self::UnexpectedToken(_))
            && let Self::Located { opening, .. } = &mut self
        {
            *opening = opening.or(at);
        }
        self
    }

    /// Render against the source being read, without looking up the value
    /// that failed to parse: no such value necessarily exists (#357).
    pub fn format_with_source(&self, source_map: &SourceMap) -> String {
        let span = self.span().or_else(|| match self.kind() {
            // Preserve formatting of an error constructed directly by an API
            // caller, as opposed to one located by the parser itself.
            Self::IncompleteDatum { line, column } => Some(ReadSpan::point(crate::ReaderState {
                line: *line,
                column: *column,
                ..crate::ReaderState::START
            })),
            _ => None,
        });
        let Some(span) = span else {
            return self.to_string();
        };
        let source = source_map.primary_source().unwrap_or("<unknown>");
        let loc = span.location(source);
        let mut parts = vec![self.to_string(), format!("  at {loc}")];
        if let Some(context) = source_map.format_context(&loc) {
            parts.push(context);
        }
        if let Some((line, column)) = self.opening() {
            parts.push(format!("  opened at {source}:{line}:{column}"));
        }
        parts.join("\n")
    }

    /// Loading paths own the file's text but may not have a SourceMap. Build
    /// context only on failure, before their error wrappers keep just text.
    pub fn format_in_source(&self, name: &str, text: &str) -> String {
        let mut map = SourceMap::new();
        map.set_primary_source(name);
        map.set_source_text(text.to_owned());
        self.format_with_source(&map)
    }

    /// Whether the input ran out part-way through something, as opposed to
    /// text that stays wrong however much more follows.
    ///
    /// A reader fed one line at a time — `read` on a file or on stdin, and a
    /// REPL deciding whether to keep taking lines — needs more input for
    /// these and must report every other error where it stands. The lexer's
    /// unterminated constructs belong here beside `IncompleteDatum`: a string
    /// and a block comment may both span lines, so an unterminated one at the
    /// end of the buffer says only that the datum is not finished yet.
    pub fn is_incomplete(&self) -> bool {
        match self.kind() {
            ParseError::IncompleteDatum { .. } => true,
            ParseError::LexError(error) => error.is_incomplete(),
            _ => false,
        }
    }
}
