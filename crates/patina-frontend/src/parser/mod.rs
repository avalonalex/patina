mod datum;
mod error;
mod number;
pub use error::ParseError;

use crate::lexer::{LexError, Lexer, ReadSpan, ReaderState, Spanned, Token};
use crate::source_map::SourceMap;
use patina_core::error::SourceLocation;
use patina_core::{Cx, Value};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Where a parser takes its tokens from.
///
/// A parser over text lexes as it goes. A [`crate::reader::Reader`], fed text
/// in pieces, has lexed its tokens already and kept them, and reads a datum by
/// replaying that datum's own tokens — so text is lexed once however many
/// times the reader was asked whether a datum had finished (#341).
enum TokenSource {
    Lexing(Lexer),
    Replay {
        tokens: std::vec::IntoIter<Spanned>,
        /// Where the token before the one last returned ended: what a lexer
        /// reports, since it records that as it starts reading a token.
        prev_end: ReaderState,
        /// Where the token last returned ended.
        last_end: ReaderState,
    },
}

impl TokenSource {
    fn next_token(&mut self) -> Result<Spanned, LexError> {
        match self {
            TokenSource::Lexing(lexer) => lexer.next_token(),
            TokenSource::Replay {
                tokens,
                prev_end,
                last_end,
            } => {
                *prev_end = *last_end;
                // Running out of tokens is the end of what this parser was
                // given, which is the end of its input.
                let spanned = tokens.next().unwrap_or(Spanned {
                    token: Token::Eof,
                    line: last_end.line,
                    column: last_end.column,
                    start: *last_end,
                    end: *last_end,
                });
                *last_end = spanned.end;
                Ok(spanned)
            }
        }
    }

    fn prev_token_end(&self) -> ReaderState {
        match self {
            TokenSource::Lexing(lexer) => lexer.prev_token_end_state(),
            TokenSource::Replay { prev_end, .. } => *prev_end,
        }
    }
}

pub struct Parser {
    source: TokenSource,
    current_token: Token,
    /// Line of the current token (1-based)
    current_token_line: u32,
    /// Column of the current token (1-based)
    current_token_column: u32,
    /// Where the current token begins, with the reader state there.
    current_token_start: ReaderState,
    current_token_end: ReaderState,
    /// The folding mode after lexing this token, also carried by token replay.
    current_token_fold_case: bool,
    /// Optional source map for recording source positions of parsed forms
    source_map: Option<Rc<RefCell<SourceMap>>>,
    /// Name of the source being parsed (e.g., file path, "<repl>", "<eval>")
    source_name: Rc<str>,
    /// Where the outermost datum being read began, so that running out of
    /// input however deep inside it reports that position.
    datum_start: ReaderState,
    /// How many lists, vectors and bytevectors the datum being read is inside.
    nesting: usize,
    /// A lexical error met in the token after a finished outermost datum,
    /// kept for the next read (see `advance_past_datum`).
    deferred: Option<ParseError>,
    /// State reached by the last parse_next, excluding its token lookahead.
    read_state: ReaderState,
    /// A completed outer datum on a port must not fetch the following token.
    port_boundary: Option<ReaderState>,
}

/// The datum labels of the outermost datum being read (R7RS 2.4), which is
/// their scope: made for each such datum and dropped with it, so a parser
/// holds no value between reads (#704).
#[derive(Default)]
struct Labels<'gc> {
    /// Maps label number to the labelled value
    defined: HashMap<usize, Value<'gc>>,
    /// Labels referenced by `#n#` before (or without) their `#n=`. Checked
    /// once the outermost datum is complete: one still undefined then is an
    /// error, not a placeholder left in the datum.
    pending: Vec<(usize, ReadSpan)>,
}

impl Parser {
    /// Create a new parser over `input`. It reads into the heap of the
    /// context each read is given, and holds no value between reads.
    pub fn new(input: &str) -> Result<Self, ParseError> {
        Self::from_lexer(Lexer::new(input))
    }

    /// Parse one datum directly from the port's persistent input cursor.
    /// No tokens or Scheme values are cached between calls.
    pub fn read_from_port<'gc>(
        port: Rc<patina_core::Port>,
        cx: &mut Cx<'gc>,
    ) -> Result<Option<Value<'gc>>, ParseError> {
        Self::from_lexer(Lexer::from_port(port)?)?.parse_next(cx)
    }

    fn from_lexer(lexer: Lexer) -> Result<Self, ParseError> {
        Self::from_source(TokenSource::Lexing(lexer))
    }

    /// A parser over `tokens`, already lexed, the first of them beginning at
    /// `at`: how a [`crate::reader::Reader`] reads a datum whose tokens it
    /// holds. See [`TokenSource`].
    pub fn from_tokens(tokens: Vec<Spanned>, at: ReaderState) -> Result<Self, ParseError> {
        Self::from_source(TokenSource::Replay {
            tokens: tokens.into_iter(),
            prev_end: at,
            last_end: at,
        })
    }

    fn from_source(mut source: TokenSource) -> Result<Self, ParseError> {
        let spanned = source.next_token()?;
        Ok(Parser {
            source,
            current_token: spanned.token,
            current_token_line: spanned.line,
            current_token_column: spanned.column,
            current_token_start: spanned.start,
            current_token_end: spanned.end,
            current_token_fold_case: spanned.end.fold_case,
            datum_start: spanned.start,
            source_map: None,
            source_name: Rc::from("<unknown>"),
            nesting: 0,
            deferred: None,
            read_state: spanned.start,
            port_boundary: None,
        })
    }

    /// Create a new parser with source map tracking.
    ///
    /// Program identifiers have distinct syntax identities and complete
    /// spans. Container slots retain immediate-datum spans as well. Ordinary
    /// datum callers should use `new` or `read_from_port`.
    pub fn new_with_source_map(
        input: &str,
        source_name: Rc<str>,
        source_map: Rc<RefCell<SourceMap>>,
    ) -> Result<Self, ParseError> {
        Self::new_with_source_map_and_fold_case(input, source_name, source_map, false)
    }

    /// Source-mapped input continuing an interactive session's directive mode.
    pub fn new_with_source_map_and_fold_case(
        input: &str,
        source_name: Rc<str>,
        source_map: Rc<RefCell<SourceMap>>,
        fold_case: bool,
    ) -> Result<Self, ParseError> {
        // Store source text for caret-style error display, and where it came
        // from, for resolving a relative `include` beside the program.
        {
            let mut sm = source_map.borrow_mut();
            sm.set_source_text(input.to_string());
            sm.set_primary_source(&source_name);
        }
        let mut lexer = Lexer::new(input);
        lexer.set_initial_fold_case(fold_case);
        let mut parser = Self::from_lexer(lexer)?;
        parser.source_map = Some(source_map);
        parser.source_name = source_name;
        Ok(parser)
    }

    /// Record the positions of what this parser reads into `source_map` under
    /// `source_name`, as [`Parser::new_with_source_map`] does, leaving the
    /// map's source text to the caller.
    pub fn recording_into(
        mut self,
        source_name: Rc<str>,
        source_map: Rc<RefCell<SourceMap>>,
    ) -> Self {
        source_map.borrow_mut().set_primary_source(&source_name);
        self.source_map = Some(source_map);
        self.source_name = source_name;
        self
    }

    /// Create a parser with case-folding enabled.
    ///
    /// Used by `include-ci` to read files in case-insensitive mode.
    /// Identifiers and character names fold as if by string-foldcase.
    pub fn new_case_insensitive(input: &str) -> Result<Self, ParseError> {
        Self::from_lexer(Lexer::new_case_insensitive(input))
    }

    /// Char offset just past the last token consumed by the most recent
    /// `parse()` call. The parser keeps a one-token lookahead, so after
    /// `parse()` returns a datum this is the end of that datum's final
    /// token; everything from this offset onward (including whitespace
    /// before the lookahead token) is unconsumed input. Used by `read` to
    /// preserve the remainder for subsequent input operations.
    pub fn consumed_end(&self) -> usize {
        self.port_boundary
            .unwrap_or_else(|| self.source.prev_token_end())
            .offset
    }

    /// State after the last `parse_next`: the datum's end, or the end of
    /// comments/directives at EOF. A lookahead directive is not consumed.
    pub fn read_state(&self) -> ReaderState {
        self.read_state
    }

    fn source_location(&self, line: u32, col: u32) -> Option<SourceLocation> {
        self.source_map.as_ref().map(|sm| {
            sm.borrow().location(
                &self.source_name,
                line,
                col,
                self.current_token_end.line,
                self.current_token_end.column,
            )
        })
    }

    /// Program syntax has distinct identifiers; datum readers continue to
    /// return interned symbols without allocating syntax metadata.
    fn identifier<'gc>(&self, cx: &mut Cx<'gc>, name: &str) -> Value<'gc> {
        if self.source_map.is_some() {
            cx.alloc_source_identifier(Rc::from(name))
        } else {
            cx.intern_symbol(name)
        }
    }

    /// Parse program text with retained source, for loaders that do not own
    /// an interactive source map. Compiled expressions retain the document.
    pub fn new_program(input: &str, name: &str, fold_case: bool) -> Result<Self, ParseError> {
        let mut lexer = Lexer::new(input);
        lexer.set_initial_fold_case(fold_case);
        let mut parser = Self::from_lexer(lexer)?;
        let mut map = SourceMap::new();
        map.set_source_text(input.to_owned());
        map.set_primary_source(name);
        parser.source_name = Rc::from(name);
        parser.source_map = Some(Rc::new(RefCell::new(map)));
        Ok(parser)
    }

    fn advance(&mut self) -> Result<(), ParseError> {
        let spanned = self.source.next_token()?;
        self.set_current(spanned);
        Ok(())
    }

    fn set_current(&mut self, spanned: Spanned) {
        self.current_token = spanned.token;
        self.current_token_line = spanned.line;
        self.current_token_column = spanned.column;
        self.current_token_start = spanned.start;
        self.current_token_end = spanned.end;
        self.current_token_fold_case = spanned.end.fold_case;
    }

    fn current_span(&self) -> ReadSpan {
        ReadSpan {
            start: self.current_token_start,
            end: self.current_token_end,
        }
    }

    /// Move past the last token of a datum.
    ///
    /// When that token ends the outermost datum being read, a lexical error
    /// in the token after it belongs to what follows, not to this datum: it
    /// is kept for the next read rather than returned now, taking the finished
    /// datum with it. Otherwise a form followed by a bad token never ran —
    /// `(display 1)` and then `#\bogus` printed nothing — where chibi and
    /// Gauche run the form and then report the token, and where a program
    /// read a line at a time, which parses the form before the next line
    /// arrives, ran it.
    fn advance_past_datum(&mut self) -> Result<(), ParseError> {
        if self.nesting == 0
            && matches!(&self.source, TokenSource::Lexing(lexer) if lexer.is_port())
        {
            self.port_boundary = Some(self.current_token_end);
            self.current_token = Token::Eof;
            return Ok(());
        }
        match self.source.next_token() {
            Ok(spanned) => {
                self.set_current(spanned);
                Ok(())
            }
            Err(error) if self.nesting == 0 => {
                self.deferred = Some(error.into());
                self.current_token = Token::Eof;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Parse the next datum, or return `None` when only whitespace and
    /// complete comments remain. Input that ends inside a datum is an
    /// `IncompleteDatum` error, as required of the Scheme `read` procedure
    /// — so is an incomplete datum comment — and a caller reading forms
    /// until the end of the input must treat only `Ok(None)` as that end.
    pub fn parse_next<'gc>(&mut self, cx: &mut Cx<'gc>) -> Result<Option<Value<'gc>>, ParseError> {
        let result = self.at_datum(cx).and_then(|present| {
            if present {
                self.parse(cx).map(Some)
            } else {
                Ok(None)
            }
        });
        self.read_state = match &result {
            Ok(Some(_)) => self
                .port_boundary
                .unwrap_or_else(|| self.source.prev_token_end()),
            Ok(None) => self.current_token_end,
            Err(error) if matches!(error.kind(), ParseError::IncompleteDatum { .. }) => {
                self.current_token_end
            }
            Err(error) => {
                // A label error can point back into a completed datum. Its
                // diagnostic position must not roll back directives read
                // since then, nor commit directives in the next token.
                let consumed = self.source.prev_token_end();
                error.span().map_or(consumed, |span| {
                    if span.end.offset > consumed.offset {
                        span.end
                    } else {
                        consumed
                    }
                })
            }
        };
        result
    }

    /// Consume any leading datum comments and say whether a datum follows,
    /// recording where it begins for `incomplete_datum`.
    ///
    /// Each comment in a *sequential* run is its own datum, so `#;(1) #;`
    /// is reported against the second `#;` rather than the first. A *nested*
    /// run — `#; #;` — is one construct, and like an unfinished nested list
    /// is reported against where that construct starts.
    fn at_datum(&mut self, cx: &mut Cx<'_>) -> Result<bool, ParseError> {
        if self.port_boundary.take().is_some() {
            self.advance()?;
        }
        if let Some(error) = self.deferred.take() {
            return Err(error);
        }
        loop {
            self.datum_start = self.current_token_start;
            if self.current_token != Token::DatumComment {
                return Ok(self.current_token != Token::Eof);
            }
            self.advance()?; // consume #;
            self.skip_datum(cx)?; // skip the commented datum
        }
    }

    /// The error for input that ran out inside the datum being read.
    fn incomplete_datum(&self) -> ParseError {
        let ReaderState { line, column, .. } = self.datum_start;
        ParseError::IncompleteDatum { line, column }.at(ReadSpan::point(self.datum_start))
    }

    /// Parse one datum, which must be there: `UnexpectedEof` when only
    /// whitespace and comments remain. Use `parse_next` to read up to the
    /// end of the input.
    pub fn parse<'gc>(&mut self, cx: &mut Cx<'gc>) -> Result<Value<'gc>, ParseError> {
        self.nesting = 0;
        // A label's scope is its outermost datum (R7RS 2.4): the table is
        // made for this one and dropped with it, also after a failed datum,
        // so one bad form cannot poison the next with "Duplicate datum
        // label".
        let mut labels = Labels::default();
        if !self.at_datum(cx)? {
            return Err(ParseError::UnexpectedEof.at(self.current_span()));
        }
        let tv = self.parse_expr(cx, &mut labels)?;
        self.finish_datum(cx, &mut labels, tv)
    }

    /// Resolve the placeholders of a complete outermost datum, and reject a
    /// `#n#` whose `#n=` never came: R7RS 2.4 scopes labels to the datum,
    /// so nothing later can define it, and a placeholder object left in the
    /// data would be garbage to every consumer.
    fn finish_datum<'gc>(
        &mut self,
        cx: &mut Cx<'gc>,
        labels: &mut Labels<'gc>,
        tv: Value<'gc>,
    ) -> Result<Value<'gc>, ParseError> {
        // Without a forward or self-reference, no placeholder was allocated.
        // Walking ordinary data here is needless work and used to overflow
        // the Rust stack on a long, flat list (#355).
        if labels.pending.is_empty() {
            return Ok(tv);
        }
        if let Some(&(n, span)) = labels
            .pending
            .iter()
            .find(|(n, _)| !labels.defined.contains_key(n))
        {
            return Err(ParseError::UndefinedLabel(n).at(span));
        }
        Self::resolve_label_aliases(cx, labels)?;
        Ok(Self::resolve_labels(cx, labels, tv))
    }

    /// Collapse aliases before patching containers (#565). A chain of labels
    /// must end in an actual datum: `#0=#0#` has no value to substitute. Real
    /// cycles such as `#0=(a . #0#)` end in a pair here and are patched below.
    /// Memoizing every traversed label makes long forward chains linear, and
    /// the explicit path avoids consuming the Rust stack.
    fn resolve_label_aliases<'gc>(
        cx: &Cx<'gc>,
        labels: &mut Labels<'gc>,
    ) -> Result<(), ParseError> {
        // None marks a label on the current path; Some is its final value.
        let mut resolved: HashMap<usize, Option<Value<'gc>>> = HashMap::new();
        let mut path = Vec::new();
        for &(start, span) in &labels.pending {
            let mut label = start;
            let value = loop {
                match resolved.get(&label) {
                    Some(Some(value)) => break *value,
                    Some(None) => {
                        return Err(ParseError::InvalidSyntax(format!(
                            "Datum label #{label}= refers to itself without a datum"
                        ))
                        .at(span));
                    }
                    None => {}
                }
                resolved.insert(label, None);
                path.push(label);
                let value = labels.defined[&label];
                if let Some(next) = cx.label_placeholder(value) {
                    label = next;
                } else {
                    break value;
                }
            };
            for alias in path.drain(..) {
                resolved.insert(alias, Some(value));
                labels.defined.insert(alias, value);
            }
        }
        Ok(())
    }

    /// Parse every datum up to the end of the input, for files that hold
    /// several top-level forms (included files, library bodies).
    ///
    /// Ends after trailing whitespace and comments — a datum comment
    /// included — and fails with `IncompleteDatum` if the input ends inside
    /// a datum, so a file cut short is never read as a shorter whole one.
    pub fn parse_all<'gc>(&mut self, cx: &mut Cx<'gc>) -> Result<Vec<Value<'gc>>, ParseError> {
        let mut exprs = Vec::new();
        while let Some(expr) = self.parse_next(cx)? {
            exprs.push(expr);
        }
        Ok(exprs)
    }

    /// Consume whatever is left of the input without building it, reporting
    /// only whether it reads: a remainder that ends inside a datum is an
    /// `IncompleteDatum` error, and everything complete is discarded.
    ///
    /// For callers that want one datum but must not let a truncated tail pass
    /// unnoticed. Reading the remainder with `parse_next` would answer the
    /// same question, but would allocate every discarded datum on the
    /// caller's heap for the garbage collector to find later.
    pub fn skip_rest(&mut self, cx: &mut Cx<'_>) -> Result<(), ParseError> {
        while self.at_datum(cx)? {
            self.skip_datum(cx)?;
        }
        Ok(())
    }

    /// Scan the complete number before converting its validated parts.
    fn parse_number<'gc>(&self, cx: &mut Cx<'gc>, input: &str) -> Result<Value<'gc>, ParseError> {
        number::parse(input, 10, cx).map_err(|error| {
            let mut at = self.current_token_start;
            for ch in input[..error.offset].chars() {
                at.advance(ch);
            }
            ParseError::InvalidSyntax(error.describe(input)).at(ReadSpan::point(at))
        })
    }

    /// The entire string must satisfy the same number grammar as the reader.
    /// A radix in the string overrides the optional argument (R7RS 6.2.7).
    /// Scanning the string directly avoids token copies, synthetic prefixes,
    /// and the lexer's whitespace/comment/directive handling.
    pub fn number_from_str<'gc>(s: &str, radix: u32, cx: &mut Cx<'gc>) -> Option<Value<'gc>> {
        if !matches!(radix, 2 | 8 | 10 | 16) {
            return None;
        }
        number::parse(s, radix, cx).ok()
    }

    /// Resolve all LabelPlaceholder values in a parsed datum.
    ///
    /// After parsing completes, any #n# references that appeared before
    /// the corresponding #n= label was fully parsed will be LabelPlaceholder
    /// values. This function walks the structure and replaces them with
    /// the actual labelled values.
    ///
    /// For cyclic structures, this mutates pairs/vectors in place to create
    /// actual cycles (rather than copies).
    fn resolve_labels<'gc>(cx: &mut Cx<'gc>, labels: &Labels<'gc>, tv: Value<'gc>) -> Value<'gc> {
        use std::collections::HashSet;

        // finish_datum checked definitions and collapsed aliases.
        let resolve = |value: Value<'gc>, cx: &Cx<'gc>| {
            cx.label_placeholder(value)
                .map_or(value, |label| labels.defined[&label])
        };

        // Patch existing containers in place to preserve sharing and cycles.
        let tv = resolve(tv, cx);
        let mut pending = vec![tv];
        let mut visited = HashSet::new();
        while let Some(container) = pending.pop() {
            if !(container.is_pair() || container.is_vector()) || !visited.insert(container) {
                continue;
            }
            if container.is_pair() {
                let (car, cdr) = cx.get_pair(container);
                let new_car = resolve(car, cx);
                let new_cdr = resolve(cdr, cx);
                if new_car != car {
                    cx.set_car(container, new_car);
                }
                if new_cdr != cdr {
                    cx.set_cdr(container, new_cdr);
                }
                // Visit the car first, leaving just the next cdr pending
                // for a flat list, regardless of the length of its spine.
                pending.extend([new_cdr, new_car]);
            } else {
                for i in 0..cx.vector_len(container) {
                    let element = cx.vector_ref(container, i);
                    let resolved = resolve(element, cx);
                    if resolved != element {
                        cx.vector_set(container, i, resolved);
                    }
                    if resolved.is_pair() || resolved.is_vector() {
                        pending.push(resolved);
                    }
                }
            }
        }
        tv
    }
}

impl patina_runtime::HasDiagnostic for ParseError {
    fn diagnostic(&self) -> patina_runtime::Diagnostic {
        patina_runtime::Diagnostic::new(patina_runtime::DiagnosticKind::Parse, self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_traits::ToPrimitive;

    /// Run `f` in a context over a fresh cx.
    fn with_cx<R>(f: impl for<'gc> FnOnce(&mut Cx<'gc>) -> R) -> R {
        patina_core::NoGcScope::with_cx(&patina_core::new_shared_heap(), f)
    }

    #[test]
    fn ambiguous_numeric_tokens_follow_the_identifier_folding_mode() {
        with_cx(|cx| {
            for name in ["+INC", "-INDEX", "+NaN.0abc", "+INF.0tail"] {
                let mut parser = Parser::new_case_insensitive(name).unwrap();
                let value = parser.parse(cx).unwrap();
                assert_eq!(
                    cx.get_symbol_name(value).as_deref(),
                    Some(name.to_lowercase().as_str()),
                    "{name}"
                );
            }
        });
    }

    #[test]
    fn identifier_fallback_does_not_accept_malformed_numbers_or_bytes() {
        with_cx(|cx| {
            for text in [
                "12abc",
                "+12abc",
                "-.5abc",
                "#d+id",
                "#e+nan.0abc",
                "+id#oops",
                "+inf.0{tail",
                "#u8(+id)",
                "#u8(+inf.0)",
            ] {
                let mut parser = Parser::new(text).unwrap();
                assert!(parser.parse_next(cx).is_err(), "{text}");
            }
        });
    }

    #[test]
    fn test_parse_next_distinguishes_clean_and_incomplete_eof() {
        with_cx(|cx| {
            for input in [
                "",
                " \n",
                "; comment",
                "#| comment |#",
                "#; (1 2)",
                "#; #; 1 2",
            ] {
                let mut parser = Parser::new(input).unwrap();
                assert!(parser.parse_next(cx).unwrap().is_none(), "{input:?}");
            }
            for input in [
                "(", "#(", "#u8(", "'", "`", ",", ",@", "(1 .", "#1=", "#;", "#; (",
            ] {
                let mut parser = Parser::new(input).unwrap();
                assert!(
                    matches!(
                        parser.parse_next(cx).as_ref().map_err(ParseError::kind),
                        Err(ParseError::IncompleteDatum { .. })
                    ),
                    "{input:?}"
                );
            }
        });
    }

    #[test]
    fn test_parse_next_after_a_complete_datum() {
        with_cx(|cx| {
            let mut parser = Parser::new("1 #; (2 3)").unwrap();
            assert_eq!(
                parser
                    .parse_next(cx)
                    .unwrap()
                    .unwrap()
                    .as_fixnum_unchecked(),
                1
            );
            assert!(parser.parse_next(cx).unwrap().is_none());
            let mut parser = Parser::new("1 (").unwrap();
            assert_eq!(
                parser
                    .parse_next(cx)
                    .unwrap()
                    .unwrap()
                    .as_fixnum_unchecked(),
                1
            );
            assert!(matches!(
                parser.parse_next(cx).as_ref().map_err(ParseError::kind),
                Err(ParseError::IncompleteDatum { line: 1, column: 3 })
            ));
        });
    }

    #[test]
    fn test_incomplete_datum_names_where_the_datum_began() {
        with_cx(|cx| {
            for (input, line, column) in [
                ("(", 1, 1),
                ("42\n  (1 (2", 2, 3),
                ("'", 1, 1),
                ("#(1 #(2", 1, 1),
                ("#u8(1", 1, 1),
                ("(1 .", 1, 1),
                ("#1=(a", 1, 1),
                ("1 2 #;", 1, 5),
                ("1 #;(2) #;", 1, 9),
            ] {
                let mut parser = Parser::new(input).unwrap();
                let err = loop {
                    match parser.parse_next(cx) {
                        Ok(Some(_)) => continue,
                        Ok(None) => panic!("{input:?} ended cleanly"),
                        Err(e) => break e,
                    }
                };
                assert!(
                    matches!(err.kind(), ParseError::IncompleteDatum { line: l, column: c } if (*l, *c) == (line, column)),
                    "{input:?}: {err}"
                );
            }
        });
    }

    #[test]
    fn test_parse_requires_a_datum_where_parse_next_ends_cleanly() {
        with_cx(|cx| {
            for input in ["", "   ", "; a comment", "#|block|#", "#;(1) ", "#; #; 1 2"] {
                assert!(
                    matches!(
                        Parser::new(input)
                            .unwrap()
                            .parse(cx)
                            .as_ref()
                            .map_err(ParseError::kind),
                        Err(ParseError::UnexpectedEof)
                    ),
                    "{input:?}"
                );
                assert!(
                    Parser::new(input)
                        .unwrap()
                        .parse_next(cx)
                        .unwrap()
                        .is_none(),
                    "{input:?}"
                );
            }
        });
    }

    /// Shapes that used to leave the `IncompleteDatum` contract by another
    /// door: a dotted list breaks out of its loop and meets the closing
    /// guard with nothing left, and a bytevector element position blamed the
    /// missing element for not being a byte.
    #[test]
    fn test_running_out_after_a_dotted_tail_or_a_byte_is_an_incomplete_datum() {
        with_cx(|cx| {
            for input in [
                "(1 . 2",
                "'(a . b",
                "(1 . (2",
                "#;(1 . 2",
                "#u8(1 #;2",
                "#u8(#;1",
            ] {
                let err = Parser::new(input).unwrap().parse_next(cx).unwrap_err();
                assert!(
                    matches!(err.kind(), ParseError::IncompleteDatum { .. }),
                    "{input:?}: {err}"
                );
            }
        });
    }

    /// Skipping a commented-out datum must accept everything parsing one
    /// does, a datum comment before the closer included.
    #[test]
    fn test_a_datum_comment_may_sit_before_the_closer_of_a_skipped_list() {
        with_cx(|cx| {
            for input in [
                "#;(a #;b) 42",
                "#;(a #;(b c)) 42",
                "#;#(a #;b) 42",
                "#;(a b #;c) 42",
            ] {
                let tv = Parser::new(input)
                    .unwrap()
                    .parse_next(cx)
                    .unwrap_or_else(|e| panic!("{input:?}: {e}"))
                    .unwrap_or_else(|| panic!("{input:?}: no datum"));
                assert_eq!(tv.as_fixnum(), Some(42), "{input:?}");
            }
        });
    }

    #[test]
    fn test_parse_all_ends_after_a_trailing_datum_comment() {
        with_cx(|cx| {
            let all = Parser::new("1 2 #;(3)").unwrap().parse_all(cx).unwrap();
            assert_eq!(all.len(), 2);
            let all = Parser::new("#;(0) 1 #;2 #|c|# ; c\n")
                .unwrap()
                .parse_all(cx)
                .unwrap();
            assert_eq!(all.len(), 1);
        });
    }

    #[test]
    fn test_parse_all_rejects_input_cut_short_inside_a_datum() {
        with_cx(|cx| {
            for (input, line, column) in [("1 2 (3", 1, 5), ("(1)\n(2\n", 2, 1), ("1 #;", 1, 3)] {
                let err = Parser::new(input).unwrap().parse_all(cx).unwrap_err();
                assert!(
                    matches!(err.kind(), ParseError::IncompleteDatum { line: l, column: c } if (*l, *c) == (line, column)),
                    "{input:?}: {err}"
                );
            }
        });
    }

    /// A parser reading a source from its start drops a byte order mark.
    /// A reader carrying on part-way through one does not; `reader.rs` has
    /// that half.
    #[test]
    fn the_start_of_a_source_drops_a_byte_order_mark() {
        with_cx(|cx| {
            let from_start = Parser::new("\u{feff}42").unwrap().parse_next(cx);
            assert!(matches!(from_start, Ok(Some(v)) if v.as_fixnum() == Some(42)));
        });
    }

    /// A lexical error in the token after a complete datum belongs to what
    /// follows: the datum is read, and the error comes with the next read.
    #[test]
    fn a_bad_token_after_a_datum_is_reported_by_the_next_read() {
        with_cx(|cx| {
            for (text, rest) in [
                ("(display 1)\n#\\bogus", "#\\bogus"),
                ("'a {", "{"),
                ("#(1) #u9", "#u9"),
                ("42 \"unterminated", "\"unterminated"),
            ] {
                let mut parser = Parser::new(text).unwrap();
                assert!(parser.parse_next(cx).unwrap().is_some(), "{text:?}");
                let unread: String = text.chars().skip(parser.consumed_end()).collect();
                assert_eq!(unread.trim_start(), rest, "{text:?}");
                assert!(parser.parse_next(cx).is_err(), "{text:?}");
            }
            // Inside a datum the error is the datum's own.
            assert!(Parser::new("(a #\\bogus)").unwrap().parse_next(cx).is_err());
            // A string cut short after a datum is still input that ran out.
            let mut parser = Parser::new("(a) \"abc").unwrap();
            parser.parse_next(cx).unwrap();
            assert!(parser.parse_next(cx).unwrap_err().is_incomplete());
        });
    }

    #[test]
    fn test_parse_atom() {
        with_cx(|cx| {
            let mut parser = Parser::new("42").unwrap();
            let result = parser.parse(cx).unwrap();
            // Should be a fixnum
            assert!(result.is_fixnum());
            assert_eq!(result.as_fixnum_unchecked(), 42);
        });
    }

    #[test]
    fn test_parse_list() {
        with_cx(|cx| {
            let mut parser = Parser::new("(+ 1 2)").unwrap();
            let result = parser.parse(cx).unwrap();
            // Should be a pair
            assert!(result.is_pair());
        });
    }

    #[test]
    fn test_consumed_end_after_atom() {
        with_cx(|cx| {
            // "5 40": parsing one datum consumes exactly the "5"; the space
            // and the "40" remain unconsumed
            let mut parser = Parser::new("5 40").unwrap();
            let result = parser.parse(cx).unwrap();
            assert_eq!(result.as_fixnum_unchecked(), 5);
            assert_eq!(parser.consumed_end(), 1);

            // A second parse from the same parser picks up the next datum
            let result = parser.parse(cx).unwrap();
            assert_eq!(result.as_fixnum_unchecked(), 40);
            assert_eq!(parser.consumed_end(), 4);
        });
    }

    #[test]
    fn test_consumed_end_after_list() {
        with_cx(|cx| {
            let mut parser = Parser::new("(a b) (c)").unwrap();
            let result = parser.parse(cx).unwrap();
            assert!(result.is_pair());
            assert_eq!(parser.consumed_end(), 5); // just past the closing paren
        });
    }

    #[test]
    fn test_consumed_end_with_leading_whitespace_and_comments() {
        with_cx(|cx| {
            let mut parser = Parser::new("  ; note\n  7 8").unwrap();
            let result = parser.parse(cx).unwrap();
            assert_eq!(result.as_fixnum_unchecked(), 7);
            assert_eq!(parser.consumed_end(), 12); // just past the "7"
        });
    }

    #[test]
    fn test_parse_small_integer() {
        with_cx(|cx| {
            // Small integers should parse as fixnum
            let mut parser = Parser::new("42").unwrap();
            let result = parser.parse(cx).unwrap();
            assert!(result.is_fixnum());
            assert_eq!(result.as_fixnum_unchecked(), 42);
        });
    }

    #[test]
    fn test_parse_i64_max() {
        with_cx(|cx| {
            // i64::MAX is larger than fixnum range, should be BigInteger
            let mut parser = Parser::new("9223372036854775807").unwrap();
            let result = parser.parse(cx).unwrap();
            // This is larger than fixnum range, so it's a BigInteger on heap
            assert!(result.is_object());
        });
    }

    #[test]
    fn test_parse_i64_min() {
        with_cx(|cx| {
            // i64::MIN is larger magnitude than fixnum range
            let mut parser = Parser::new("-9223372036854775808").unwrap();
            let result = parser.parse(cx).unwrap();
            // This is outside fixnum range, so it's a BigInteger on heap
            assert!(result.is_object());
        });
    }

    #[test]
    fn test_parse_beyond_i64_max() {
        with_cx(|cx| {
            // i64::MAX + 1 should parse as BigInteger on heap
            let mut parser = Parser::new("9223372036854775808").unwrap();
            let result = parser.parse(cx).unwrap();
            assert!(result.is_object());
            // Check it's a BigInt
            assert!(cx.is_bigint(result));
        });
    }

    #[test]
    fn test_parse_large_bigint() {
        with_cx(|cx| {
            // Very large number should parse as BigInteger
            let mut parser = Parser::new("10000000000000000000").unwrap();
            let result = parser.parse(cx).unwrap();
            assert!(result.is_object());
            assert!(cx.is_bigint(result));
        });
    }

    #[test]
    fn test_parse_huge_bigint() {
        with_cx(|cx| {
            // Astronomically large number (2^100) should parse as BigInteger
            let mut parser = Parser::new("1267650600228229401496703205376").unwrap();
            let result = parser.parse(cx).unwrap();
            assert!(result.is_object());
            assert!(cx.is_bigint(result));
        });
    }

    #[test]
    fn test_parse_negative_beyond_i64_min() {
        with_cx(|cx| {
            // i64::MIN - 1 should parse as BigInteger
            let mut parser = Parser::new("-9223372036854775809").unwrap();
            let result = parser.parse(cx).unwrap();
            assert!(result.is_object());
            assert!(cx.is_bigint(result));
        });
    }

    #[test]
    fn test_parse_bigint_in_expression() {
        with_cx(|cx| {
            // Large integers in expressions should parse as BigInteger
            let mut parser = Parser::new("(+ 10000000000000000000 1)").unwrap();
            let result = parser.parse(cx).unwrap();

            // The result should be a list (pair chain)
            assert!(result.is_pair());
            let car = cx.car(result);
            let cdr = cx.cdr(result);
            assert!(cx.get_symbol_name(car).is_some()); // The '+'

            assert!(cdr.is_pair());
            let arg1 = cx.car(cdr);
            // The first argument should be BigInteger
            let bigint = cx
                .get_bigint(arg1)
                .expect("Expected BigInteger for first arg");
            assert_eq!(bigint.to_string(), "10000000000000000000");
        });
    }

    #[test]
    fn test_parse_positive_infinity() {
        with_cx(|cx| {
            let mut parser = Parser::new("+inf.0").unwrap();
            let result = parser.parse(cx).unwrap();
            let f = cx.get_real(result).expect("Expected Real");
            assert!(f.is_infinite() && f.is_sign_positive());
        });
    }

    #[test]
    fn test_parse_negative_infinity() {
        with_cx(|cx| {
            let mut parser = Parser::new("-inf.0").unwrap();
            let result = parser.parse(cx).unwrap();
            let f = cx.get_real(result).expect("Expected Real");
            assert!(f.is_infinite() && f.is_sign_negative());
        });
    }

    #[test]
    fn test_parse_nan() {
        with_cx(|cx| {
            let mut parser = Parser::new("+nan.0").unwrap();
            let result = parser.parse(cx).unwrap();
            let f = cx.get_real(result).expect("Expected Real");
            assert!(f.is_nan());
        });
    }

    #[test]
    fn test_parse_special_floats_case_insensitive() {
        with_cx(|cx| {
            // Test case-insensitive parsing of inf.0 and nan.0
            let test_cases = [
                ("+INF.0", f64::INFINITY),
                ("+Inf.0", f64::INFINITY),
                ("+iNf.0", f64::INFINITY),
                ("-INF.0", f64::NEG_INFINITY),
                ("-Inf.0", f64::NEG_INFINITY),
            ];

            for (input, expected) in test_cases {
                let mut parser = Parser::new(input).unwrap();
                let result = parser.parse(cx).unwrap();
                let f = cx
                    .get_real(result)
                    .unwrap_or_else(|| panic!("For input '{}': expected Real", input));
                assert_eq!(
                    f.is_infinite(),
                    expected.is_infinite(),
                    "For input '{}': expected infinite",
                    input
                );
                assert_eq!(
                    f.is_sign_positive(),
                    expected.is_sign_positive(),
                    "For input '{}': sign mismatch",
                    input
                );
            }

            // Test NaN separately (can't use == for NaN)
            let nan_cases = ["+NAN.0", "+Nan.0", "+nAn.0", "-NAN.0", "-nan.0"];
            for input in nan_cases {
                let mut parser = Parser::new(input).unwrap();
                let result = parser.parse(cx).unwrap();
                let f = cx
                    .get_real(result)
                    .unwrap_or_else(|| panic!("For input '{}': expected Real (NaN)", input));
                assert!(f.is_nan(), "For input '{}': expected NaN, got {}", input, f);
            }
        });
    }

    #[test]
    fn test_parse_infinity_in_expression() {
        with_cx(|cx| {
            let mut parser = Parser::new("(+ +inf.0 1)").unwrap();
            let result = parser.parse(cx).unwrap();

            assert!(result.is_pair());
            let car = cx.car(result);
            let cdr = cx.cdr(result);
            assert!(cx.get_symbol_name(car).is_some()); // The '+'

            assert!(cdr.is_pair());
            let arg1 = cx.car(cdr);
            let f = cx.get_real(arg1).expect("Expected Real for first arg");
            assert!(f.is_infinite() && f.is_sign_positive());
        });
    }

    // ========== Datum Comment Tests ==========

    #[test]
    fn test_datum_comment_simple() {
        with_cx(|cx| {
            // #; abc def -> def
            let mut parser = Parser::new("#; abc def").unwrap();
            let result = parser.parse(cx).unwrap();
            assert_eq!(cx.get_symbol_name(result).as_deref(), Some("def"));
        });
    }

    #[test]
    fn test_datum_comment_in_list() {
        with_cx(|cx| {
            // (#;sqrt abs -16) -> (abs -16)
            let mut parser = Parser::new("(#;sqrt abs -16)").unwrap();
            let result = parser.parse(cx).unwrap();

            // Should be a list with two elements: abs and -16
            assert!(result.is_pair());
            let car = cx.car(result);
            assert_eq!(cx.get_symbol_name(car).as_deref(), Some("abs"));
        });
    }

    #[test]
    fn test_datum_comment_multiple() {
        with_cx(|cx| {
            // (a #; #;b c d) -> (a d)
            // First #; skips b, second #; skips c
            let mut parser = Parser::new("(a #; #;b c d)").unwrap();
            let result = parser.parse(cx).unwrap();

            // Should be a list (a d)
            assert!(result.is_pair());
            assert_eq!(cx.get_symbol_name(cx.car(result)).as_deref(), Some("a"));

            let cdr = cx.cdr(result);
            assert!(cdr.is_pair());
            assert_eq!(cx.get_symbol_name(cx.car(cdr)).as_deref(), Some("d"));
            assert!(cx.cdr(cdr).is_null());
        });
    }

    #[test]
    fn test_datum_comment_before_closing_paren() {
        with_cx(|cx| {
            // (a b #;c) -> (a b) — found by the compat corpus: srfi-14's
            // reference implementation comments out a trailing argument, which
            // transitively blocked 19 vendored packages.
            let mut parser = Parser::new("(a b #;c)").unwrap();
            let result = parser.parse(cx).unwrap();
            assert_eq!(cx.get_symbol_name(cx.car(result)).as_deref(), Some("a"));
            let cdr = cx.cdr(result);
            assert_eq!(cx.get_symbol_name(cx.car(cdr)).as_deref(), Some("b"));
            assert!(cx.cdr(cdr).is_null());
        });
    }

    #[test]
    fn test_datum_comment_only_list_is_empty() {
        with_cx(|cx| {
            // (#;a) -> ()
            let mut parser = Parser::new("(#;a)").unwrap();
            let result = parser.parse(cx).unwrap();
            assert!(result.is_null());
        });
    }

    #[test]
    fn test_datum_comment_before_vector_close() {
        with_cx(|cx| {
            // #(1 #;2) -> #(1)
            let mut parser = Parser::new("#(1 #;2)").unwrap();
            let result = parser.parse(cx).unwrap();
            assert_eq!(cx.vector_len(result), 1);
        });
    }

    #[test]
    fn test_datum_comment_in_bytevector() {
        with_cx(|cx| {
            // #u8(1 #;2 3) -> #u8(1 3)
            let mut parser = Parser::new("#u8(1 #;2 3)").unwrap();
            let result = parser.parse(cx).unwrap();
            let bytes = cx.get_bytevector_bytes(result).unwrap();
            assert_eq!(bytes, &[1, 3]);
        });
    }

    #[test]
    fn test_datum_comment_nested_list() {
        with_cx(|cx| {
            // (a #;(b #;c d) e) -> (a e)
            let mut parser = Parser::new("(a #;(b #;c d) e)").unwrap();
            let result = parser.parse(cx).unwrap();

            assert!(result.is_pair());
            assert_eq!(cx.get_symbol_name(cx.car(result)).as_deref(), Some("a"));

            let cdr = cx.cdr(result);
            assert!(cdr.is_pair());
            assert_eq!(cx.get_symbol_name(cx.car(cdr)).as_deref(), Some("e"));
            assert!(cx.cdr(cdr).is_null());
        });
    }

    #[test]
    fn test_datum_comment_dotted_list() {
        with_cx(|cx| {
            // (a . #;b c) -> (a . c)
            let mut parser = Parser::new("(a . #;b c)").unwrap();
            let result = parser.parse(cx).unwrap();

            assert!(result.is_pair());
            assert_eq!(cx.get_symbol_name(cx.car(result)).as_deref(), Some("a"));
            assert_eq!(cx.get_symbol_name(cx.cdr(result)).as_deref(), Some("c"));
        });
    }

    #[test]
    fn test_datum_comment_before_tail() {
        with_cx(|cx| {
            // (a . b #;c) -> This should skip c after the dotted tail
            // According to tests: this should return (a . b) with c skipped
            let mut parser = Parser::new("(a . b #;c)").unwrap();
            let result = parser.parse(cx).unwrap();

            assert!(result.is_pair());
            assert_eq!(cx.get_symbol_name(cx.car(result)).as_deref(), Some("a"));
            assert_eq!(cx.get_symbol_name(cx.cdr(result)).as_deref(), Some("b"));
        });
    }

    #[test]
    fn test_datum_comment_with_line_comment() {
        with_cx(|cx| {
            // #; ; comment\n def ghi -> ghi
            // The #; comments out the first datum after it, which is 'def'
            // (the ; comment is just a line comment consumed as whitespace)
            let mut parser = Parser::new("#; ; comment\n def ghi").unwrap();
            let result = parser.parse(cx).unwrap();
            assert_eq!(cx.get_symbol_name(result).as_deref(), Some("ghi"));
        });
    }

    #[test]
    fn test_datum_comment_vector() {
        with_cx(|cx| {
            // #;#(1 2 3) 42 -> 42
            let mut parser = Parser::new("#;#(1 2 3) 42").unwrap();
            let result = parser.parse(cx).unwrap();
            assert!(result.is_fixnum());
            assert_eq!(result.as_fixnum_unchecked(), 42);
        });
    }

    #[test]
    fn test_datum_comment_quoted() {
        with_cx(|cx| {
            // #;'foo bar -> bar
            let mut parser = Parser::new("#;'foo bar").unwrap();
            let result = parser.parse(cx).unwrap();
            assert_eq!(cx.get_symbol_name(result).as_deref(), Some("bar"));
        });
    }

    // ========== Datum Comment Error Cases ==========
    // These test cases correspond to R7RS read errors that require guard to test
    // at the Scheme level. By testing at the Rust level, we ensure correct behavior.

    #[test]
    fn test_datum_comment_error_dotted_head() {
        with_cx(|cx| {
            // (#;a . b) -> error: no datum before dot
            let mut parser = Parser::new("(#;a . b)").unwrap();
            let result = parser.parse(cx);
            assert!(
                result.is_err(),
                "Expected error for (#;a . b), got {:?}",
                result
            );
        });
    }

    #[test]
    fn test_datum_comment_error_dotted_tail() {
        with_cx(|cx| {
            // (a . #;b) -> error: no datum after dot
            let mut parser = Parser::new("(a . #;b)").unwrap();
            let result = parser.parse(cx);
            assert!(
                result.is_err(),
                "Expected error for (a . #;b), got {:?}",
                result
            );
        });
    }

    #[test]
    fn test_datum_comment_error_commenting_dot() {
        with_cx(|cx| {
            // (a #;. b) -> error: dot is not a datum
            let mut parser = Parser::new("(a #;. b)").unwrap();
            let result = parser.parse(cx);
            assert!(
                result.is_err(),
                "Expected error for (a #;. b), got {:?}",
                result
            );
        });
    }

    #[test]
    fn test_datum_comment_error_multiple_before_dot() {
        with_cx(|cx| {
            // (#;x #;y . z) -> error: no datum before dot
            let mut parser = Parser::new("(#;x #;y . z)").unwrap();
            let result = parser.parse(cx);
            assert!(
                result.is_err(),
                "Expected error for (#;x #;y . z), got {:?}",
                result
            );
        });
    }

    #[test]
    fn test_datum_comment_error_nested_before_dot() {
        with_cx(|cx| {
            // (#; #;x #;y . z) -> error: no datum before dot
            let mut parser = Parser::new("(#; #;x #;y . z)").unwrap();
            let result = parser.parse(cx);
            assert!(
                result.is_err(),
                "Expected error for (#; #;x #;y . z), got {:?}",
                result
            );
        });
    }

    #[test]
    fn test_datum_comment_error_nested_dotted() {
        with_cx(|cx| {
            // (#; #;x . z) -> error: missing datum after nested #;
            let mut parser = Parser::new("(#; #;x . z)").unwrap();
            let result = parser.parse(cx);
            assert!(
                result.is_err(),
                "Expected error for (#; #;x . z), got {:?}",
                result
            );
        });
    }

    // ========== Alternate Exponent Marker Tests ==========

    #[test]
    fn test_alternate_exponent_markers() {
        with_cx(|cx| {
            // R7RS allows s, f, d, l as exponent markers (all treated as f64)
            let test_cases = [
                ("1s2", 100.0),
                ("1S2", 100.0),
                ("1f2", 100.0),
                ("1F2", 100.0),
                ("1d2", 100.0),
                ("1D2", 100.0),
                ("1l2", 100.0),
                ("1L2", 100.0),
                ("1.5s1", 15.0),
                ("2.5f-1", 0.25),
            ];

            for (input, expected) in test_cases {
                let mut parser = Parser::new(input).unwrap();
                let result = parser.parse(cx).unwrap();
                let f = cx
                    .get_real(result)
                    .unwrap_or_else(|| panic!("For input '{}': expected Real", input));
                assert!(
                    (f - expected).abs() < 1e-10,
                    "For input '{}': expected {}, got {}",
                    input,
                    expected,
                    f
                );
            }
        });
    }

    #[test]
    fn test_standard_exponent_still_works() {
        with_cx(|cx| {
            // Make sure we didn't break standard 'e' exponent
            let mut parser = Parser::new("1e2").unwrap();
            let result = parser.parse(cx).unwrap();
            let f = cx.get_real(result).expect("Expected Real");
            assert!((f - 100.0).abs() < 1e-10);
        });
    }

    // ========== Non-Decimal Radix Rational Tests ==========

    #[test]
    fn test_hex_rational() {
        with_cx(|cx| {
            // #x1/10 = 1/16 in decimal
            let mut parser = Parser::new("#x1/10").unwrap();
            let result = parser.parse(cx).unwrap();
            let r = cx.get_rational(result).expect("Expected Rational");
            assert_eq!(r.numer().to_i64(), Some(1));
            assert_eq!(r.denom().to_i64(), Some(16));
        });
    }

    #[test]
    fn test_hex_rational_simplifies_to_integer() {
        with_cx(|cx| {
            // #x10/2 = 16/2 = 8
            let mut parser = Parser::new("#x10/2").unwrap();
            let result = parser.parse(cx).unwrap();
            assert!(result.is_fixnum());
            assert_eq!(result.as_fixnum_unchecked(), 8);
        });
    }

    #[test]
    fn test_hex_rational_simplifies() {
        with_cx(|cx| {
            // #x11/2 = 17/2
            let mut parser = Parser::new("#x11/2").unwrap();
            let result = parser.parse(cx).unwrap();
            let r = cx.get_rational(result).expect("Expected Rational");
            assert_eq!(r.numer().to_i64(), Some(17));
            assert_eq!(r.denom().to_i64(), Some(2));
        });
    }

    #[test]
    fn test_octal_rational() {
        with_cx(|cx| {
            // #o11/2 = 9/2 in decimal
            let mut parser = Parser::new("#o11/2").unwrap();
            let result = parser.parse(cx).unwrap();
            let r = cx.get_rational(result).expect("Expected Rational");
            assert_eq!(r.numer().to_i64(), Some(9));
            assert_eq!(r.denom().to_i64(), Some(2));
        });
    }

    #[test]
    fn test_binary_rational() {
        with_cx(|cx| {
            // #b11/10 = 3/2 in decimal
            let mut parser = Parser::new("#b11/10").unwrap();
            let result = parser.parse(cx).unwrap();
            let r = cx.get_rational(result).expect("Expected Rational");
            assert_eq!(r.numer().to_i64(), Some(3));
            assert_eq!(r.denom().to_i64(), Some(2));
        });
    }

    #[test]
    fn test_hex_rational_with_letters() {
        with_cx(|cx| {
            // #xa/b = 10/11
            let mut parser = Parser::new("#xa/b").unwrap();
            let result = parser.parse(cx).unwrap();
            let r = cx.get_rational(result).expect("Expected Rational");
            assert_eq!(r.numer().to_i64(), Some(10));
            assert_eq!(r.denom().to_i64(), Some(11));
        });
    }

    #[test]
    fn test_hex_rational_zero_denominator_error() {
        with_cx(|cx| {
            // #x1/0 should error
            let mut parser = Parser::new("#x1/0").unwrap();
            let result = parser.parse(cx);
            assert!(result.is_err());
        });
    }

    // ========== Complex Numbers with Alternate Exponent Markers ==========

    #[test]
    fn test_complex_with_short_exponent_marker() {
        with_cx(|cx| {
            // 1s2+1.0i = 100.0+1.0i
            let mut parser = Parser::new("1s2+1.0i").unwrap();
            let result = parser.parse(cx).unwrap();
            let (real_tv, imag_tv) = cx.get_complex(result).expect("Expected Complex");
            let real = cx.get_real(real_tv).expect("Expected real part");
            let imag = cx.get_real(imag_tv).expect("Expected imag part");
            assert!(
                (real - 100.0).abs() < 1e-10,
                "Expected real 100.0, got {}",
                real
            );
            assert!(
                (imag - 1.0).abs() < 1e-10,
                "Expected imag 1.0, got {}",
                imag
            );
        });
    }

    #[test]
    fn test_complex_with_exponent_in_imag() {
        with_cx(|cx| {
            // 1.0+1s2i = 1.0+100.0i
            let mut parser = Parser::new("1.0+1s2i").unwrap();
            let result = parser.parse(cx).unwrap();
            let (real_tv, imag_tv) = cx.get_complex(result).expect("Expected Complex");
            let real = cx.get_real(real_tv).expect("Expected real part");
            let imag = cx.get_real(imag_tv).expect("Expected imag part");
            assert!(
                (real - 1.0).abs() < 1e-10,
                "Expected real 1.0, got {}",
                real
            );
            assert!(
                (imag - 100.0).abs() < 1e-10,
                "Expected imag 100.0, got {}",
                imag
            );
        });
    }

    #[test]
    fn test_complex_with_double_exponent_marker() {
        with_cx(|cx| {
            // 1d2+1.0i = 100.0+1.0i (double precision marker)
            let mut parser = Parser::new("1d2+1.0i").unwrap();
            let result = parser.parse(cx).unwrap();
            let (real_tv, imag_tv) = cx.get_complex(result).expect("Expected Complex");
            let real = cx.get_real(real_tv).expect("Expected real part");
            let imag = cx.get_real(imag_tv).expect("Expected imag part");
            assert!(
                (real - 100.0).abs() < 1e-10,
                "Expected real 100.0, got {}",
                real
            );
            assert!(
                (imag - 1.0).abs() < 1e-10,
                "Expected imag 1.0, got {}",
                imag
            );
        });
    }

    #[test]
    fn test_complex_with_long_exponent_marker() {
        with_cx(|cx| {
            // 1l2+3.0i = 100.0+3.0i (long precision marker)
            let mut parser = Parser::new("1l2+3.0i").unwrap();
            let result = parser.parse(cx).unwrap();
            let (real_tv, imag_tv) = cx.get_complex(result).expect("Expected Complex");
            let real = cx.get_real(real_tv).expect("Expected real part");
            let imag = cx.get_real(imag_tv).expect("Expected imag part");
            assert!(
                (real - 100.0).abs() < 1e-10,
                "Expected real 100.0, got {}",
                real
            );
            assert!(
                (imag - 3.0).abs() < 1e-10,
                "Expected imag 3.0, got {}",
                imag
            );
        });
    }

    #[test]
    fn test_polar_with_exponent_marker() {
        with_cx(|cx| {
            // An exact zero angle preserves the magnitude, including inexactness.
            let mut parser = Parser::new("1s2@0").unwrap();
            let result = parser.parse(cx).unwrap();
            assert_eq!(cx.get_real(result), Some(100.0));
        });
    }

    // ========== Datum Label Tests (R7RS Section 2.4) ==========

    #[test]
    fn test_datum_label_simple() {
        with_cx(|cx| {
            // #0=42 -> just returns 42, label is stored but not needed
            let mut parser = Parser::new("#0=42").unwrap();
            let result = parser.parse(cx).unwrap();
            assert!(result.is_fixnum());
            assert_eq!(result.as_fixnum_unchecked(), 42);
        });
    }

    #[test]
    fn test_datum_label_shared_structure() {
        with_cx(|cx| {
            // (#0=(a b) #0#) -> two references to the same list
            let mut parser = Parser::new("(#0=(a b) #0#)").unwrap();
            let result = parser.parse(cx).unwrap();

            // HeapPair structure - shared structure via same TaggedValue (raw pointer)
            assert!(result.is_pair());
            let first = cx.car(result);
            let second_cell = cx.cdr(result);

            // First element should be (a b)
            assert!(first.is_pair());
            assert_eq!(cx.get_symbol_name(cx.car(first)).as_deref(), Some("a"));

            // Second element of outer list should also be the same (a b)
            assert!(second_cell.is_pair());
            let second = cx.car(second_cell);
            assert!(second.is_pair());

            // Check pointer equality - both should be the same TaggedValue
            assert_eq!(
                first.raw(),
                second.raw(),
                "Shared structure should use same heap pointer"
            );
        });
    }

    #[test]
    fn test_datum_label_cyclic() {
        with_cx(|cx| {
            // #0=(1 . #0#) -> creates a cycle
            let mut parser = Parser::new("#0=(1 . #0#)").unwrap();
            let result = parser.parse(cx).unwrap();

            assert!(result.is_pair());
            let car = cx.car(result);
            let cdr = cx.cdr(result);

            assert!(car.is_fixnum());
            assert_eq!(car.as_fixnum_unchecked(), 1);

            // The cdr should be eq? to the pair itself (cycle)
            assert!(cdr.is_pair());
            assert_eq!(
                result.raw(),
                cdr.raw(),
                "Cyclic structure: cdr should point to self"
            );
        });
    }

    #[test]
    fn test_datum_label_forward_reference() {
        with_cx(|cx| {
            // (#0# #0=(a b)) -> forward reference resolved
            let mut parser = Parser::new("(#0# #0=(a b))").unwrap();
            let result = parser.parse(cx).unwrap();

            assert!(result.is_pair());
            let first = cx.car(result);
            let rest = cx.cdr(result);

            // Both elements should be the same (a b) list
            assert!(first.is_pair());
            assert!(rest.is_pair());
            let second = cx.car(rest);
            assert!(second.is_pair());

            assert_eq!(
                first.raw(),
                second.raw(),
                "Forward reference should resolve to same object"
            );
        });
    }

    #[test]
    fn label_patching_preserves_a_shared_pair_vector_cycle() {
        with_cx(|cx| {
            // Exercise a placeholder in every kind of slot, with sharing that
            // reaches the same containers along several paths through the worklist.
            let mut parser = Parser::new("(#0# . #0=#(#1# #1=(x . #0#)))").unwrap();
            let root = parser.parse(cx).unwrap();
            let vector = cx.car(root);
            assert!(vector.is_vector());
            assert_eq!(cx.cdr(root), vector);
            let pair = cx.vector_ref(vector, 0);
            assert!(pair.is_pair());
            assert_eq!(cx.vector_ref(vector, 1), pair);
            assert_eq!(cx.get_symbol_name(cx.car(pair)).as_deref(), Some("x"));
            assert_eq!(cx.cdr(pair), vector);
        });
    }

    #[test]
    fn test_datum_label_multiple() {
        with_cx(|cx| {
            // (#0=a #1=b #0# #1#) -> (a b a b)
            let mut parser = Parser::new("(#0=a #1=b #0# #1#)").unwrap();
            let result = parser.parse(cx).unwrap();

            // Collect all symbols from the list
            let mut symbols = Vec::new();
            let mut current = result;
            while current.is_pair() {
                if let Some(name) = cx.get_symbol_name(cx.car(current)).as_deref() {
                    symbols.push(name.to_string());
                }
                current = cx.cdr(current);
            }

            assert_eq!(symbols, vec!["a", "b", "a", "b"]);
        });
    }

    #[test]
    fn test_datum_label_duplicate_error() {
        with_cx(|cx| {
            // #0=a #0=b -> error: duplicate label
            let mut parser = Parser::new("(#0=a #0=b)").unwrap();
            let result = parser.parse(cx);
            assert!(
                matches!(
                    result.as_ref().map_err(ParseError::kind),
                    Err(ParseError::DuplicateLabel(0))
                ),
                "Expected DuplicateLabel error, got {:?}",
                result
            );
        });
    }

    #[test]
    fn test_datum_label_undefined() {
        with_cx(|cx| {
            // #99# with no #99= anywhere in the datum: R7RS 2.4 scopes a label to
            // its outermost datum, so nothing later can define it, and a
            // placeholder object left in the data would be garbage to every
            // consumer. It is an error at the end of the datum.
            let mut parser = Parser::new("(a #99# b)").unwrap();
            match parser.parse(cx).as_ref().map_err(ParseError::kind) {
                Err(ParseError::UndefinedLabel(99)) => {}
                other => panic!("expected UndefinedLabel(99), got {other:?}"),
            }
            // A label defined *after* its first reference in the same datum is
            // fine — that is what placeholders are for.
            let mut parser = Parser::new("(#0# . #0=(x))").unwrap();
            assert!(parser.parse(cx).is_ok());
        });
    }

    #[test]
    fn test_datum_label_in_vector() {
        with_cx(|cx| {
            // #(#0=1 #0# #0#) -> vector with shared elements
            let mut parser = Parser::new("#(#0=1 #0# #0#)").unwrap();
            let result = parser.parse(cx).unwrap();

            assert!(result.is_vector());
            let len = cx.vector_len(result);
            assert_eq!(len, 3);
            for i in 0..len {
                let elem = cx.vector_ref(result, i);
                assert!(elem.is_fixnum());
                assert_eq!(elem.as_fixnum_unchecked(), 1);
            }
        });
    }

    #[test]
    fn test_datum_label_nested() {
        with_cx(|cx| {
            // #0=(a #1=(b c) #1#) -> nested shared structure
            let mut parser = Parser::new("#0=(a #1=(b c) #1#)").unwrap();
            let result = parser.parse(cx).unwrap();

            // Verify the nested list (b c) is shared
            assert!(result.is_pair());
            let rest1 = cx.cdr(result);

            // Get second element (should be (b c))
            assert!(rest1.is_pair());
            let inner1 = cx.car(rest1);
            assert!(inner1.is_pair());

            // Get third element (should also be (b c))
            let rest2 = cx.cdr(rest1);
            assert!(rest2.is_pair());
            let inner2 = cx.car(rest2);
            assert!(inner2.is_pair());

            assert_eq!(
                inner1.raw(),
                inner2.raw(),
                "Nested shared structure should use same heap pointer"
            );
        });
    }

    #[test]
    fn comments_inside_prefixes_do_not_define_or_reference_live_labels() {
        with_cx(|cx| {
            let mut parser =
                Parser::new("#(#0=#;#0=ignored '#;#99# (a . #;discard b) #0# #u8(1 #;#0# 255))")
                    .unwrap();
            let value = parser.parse(cx).unwrap();
            let quoted = cx.vector_ref(value, 0);
            assert_eq!(cx.vector_ref(value, 1), quoted);
            assert_eq!(cx.get_symbol_name(cx.car(quoted)).as_deref(), Some("quote"));
            let pair = cx.car(cx.cdr(quoted));
            assert_eq!(cx.get_symbol_name(cx.car(pair)).as_deref(), Some("a"));
            assert_eq!(cx.get_symbol_name(cx.cdr(pair)).as_deref(), Some("b"));
            assert!(cx.is_bytevector(cx.vector_ref(value, 2)));
        });
    }

    #[test]
    fn every_pending_prefix_requires_a_datum_before_a_closer() {
        with_cx(|cx| {
            for source in [
                "(')",
                "(`)",
                "(,)",
                "(,@)",
                "(#0=)",
                "(#;)",
                "(1 . #;2)",
                "#(1 . 2)",
            ] {
                let mut parser = Parser::new(source).unwrap();
                assert!(
                    matches!(
                        parser.parse(cx).as_ref().map_err(ParseError::kind),
                        Err(ParseError::UnexpectedToken(_))
                    ),
                    "{source}"
                );
            }
        });
    }

    #[test]
    fn skipped_nested_data_leaves_the_next_datum_unconsumed() {
        with_cx(|cx| {
            let source = format!("#;{}a{} 42", "'(#(".repeat(10_000), "))".repeat(10_000));
            let mut parser = Parser::new(&source).unwrap();
            assert_eq!(parser.parse(cx).unwrap().as_fixnum(), Some(42));
            assert!(parser.parse_next(cx).unwrap().is_none());
            let mut parser = Parser::new(&source).unwrap();
            parser.skip_rest(cx).unwrap();
            assert!(parser.parse_next(cx).unwrap().is_none());
        });
    }

    #[test]
    fn test_source_map_list() {
        with_cx(|cx| {
            let sm = Rc::new(RefCell::new(crate::source_map::SourceMap::new()));
            let mut parser =
                Parser::new_with_source_map("(+ 1 2)", Rc::from("test.scm"), sm.clone()).unwrap();
            let result = parser.parse(cx).unwrap();
            let loc = cx.source(result);
            assert!(loc.is_some(), "list form should have source location");
            let loc = loc.unwrap();
            assert_eq!(loc.line, 1);
            assert_eq!(loc.column, 1);
            assert_eq!(&*loc.source, "test.scm");
        });
    }

    #[test]
    fn test_source_map_nested_lists() {
        with_cx(|cx| {
            let sm = Rc::new(RefCell::new(crate::source_map::SourceMap::new()));
            let mut parser = Parser::new_with_source_map(
                "(define (f x) (+ x 1))",
                Rc::from("test.scm"),
                sm.clone(),
            )
            .unwrap();
            let result = parser.parse(cx).unwrap();
            // Outer list starts at column 1
            let loc = cx.source(result).unwrap();
            assert_eq!(loc.line, 1);
            assert_eq!(loc.column, 1);

            // Find inner (+ x 1) — it's the third element: (define (f x) (+ x 1))
            let cdr1 = cx.cdr(result); // ((f x) (+ x 1))
            let cdr2 = cx.cdr(cdr1); // ((+ x 1))
            let inner = cx.car(cdr2); // (+ x 1)
            let inner_loc = cx.source(inner).unwrap();
            assert_eq!(inner_loc.line, 1);
            assert_eq!(inner_loc.column, 15); // position of `(` in `(+ x 1)`
        });
    }

    #[test]
    fn test_source_map_quote_abbreviation() {
        with_cx(|cx| {
            let sm = Rc::new(RefCell::new(crate::source_map::SourceMap::new()));
            let mut parser =
                Parser::new_with_source_map("'(1 2 3)", Rc::from("test.scm"), sm.clone()).unwrap();
            let result = parser.parse(cx).unwrap();
            // The quote abbreviation should record position of the `'`
            let loc = cx.source(result);
            assert!(loc.is_some(), "quote form should have source location");
            let loc = loc.unwrap();
            assert_eq!(loc.line, 1);
            assert_eq!(loc.column, 1);
        });
    }

    #[test]
    fn test_source_map_vector() {
        with_cx(|cx| {
            let sm = Rc::new(RefCell::new(crate::source_map::SourceMap::new()));
            let mut parser =
                Parser::new_with_source_map("#(1 2 3)", Rc::from("test.scm"), sm.clone()).unwrap();
            let result = parser.parse(cx).unwrap();
            let loc = cx.source(result);
            assert!(loc.is_some(), "vector should have source location");
            let loc = loc.unwrap();
            assert_eq!(loc.line, 1);
            assert_eq!(loc.column, 1);
        });
    }

    #[test]
    fn test_source_map_multiline() {
        with_cx(|cx| {
            let sm = Rc::new(RefCell::new(crate::source_map::SourceMap::new()));
            let mut parser = Parser::new_with_source_map(
                "(define x 1)\n(+ x 2)",
                Rc::from("test.scm"),
                sm.clone(),
            )
            .unwrap();
            let expr1 = parser.parse(cx).unwrap();
            let expr2 = parser.parse(cx).unwrap();
            let loc1 = cx.source(expr1).unwrap();
            let loc2 = cx.source(expr2).unwrap();
            assert_eq!(loc1.line, 1);
            assert_eq!(loc1.column, 1);
            assert_eq!(loc2.line, 2);
            assert_eq!(loc2.column, 1);
        });
    }

    #[test]
    fn test_no_source_map_backward_compat() {
        with_cx(|cx| {
            // Without source map, everything still works
            let mut parser = Parser::new("(+ 1 2)").unwrap();
            let result = parser.parse(cx).unwrap();
            assert!(result.is_pair());
        });
    }
}

#[cfg(test)]
mod occurrence_span_tests {
    use super::*;

    /// Run `f` in a context over a fresh cx.
    fn with_cx<R>(f: impl for<'gc> FnOnce(&mut Cx<'gc>) -> R) -> R {
        patina_core::NoGcScope::with_cx(&patina_core::new_shared_heap(), f)
    }

    #[test]
    fn program_identifiers_are_occurrences() {
        with_cx(|cx| {
            let mut parser = Parser::new_program("(same 42 same) next", "test.scm", false).unwrap();
            let form = parser.parse(cx).unwrap();
            let first = cx.car(form);
            let rest = cx.cdr(form);
            let last = cx.car(cx.cdr(rest));
            assert_ne!(first, last);
            assert_eq!(cx.get_identifier_data(first).unwrap().0.as_ref(), "same");
            assert_eq!(cx.source(first).unwrap().column, 2);
            assert_eq!(cx.source(last).unwrap().column, 10);
            assert_eq!(cx.source(last).unwrap().length, Some(4));
            let outer = cx.source(form).unwrap();
            assert_eq!(outer.span.as_ref().unwrap().end_column, 15);
            let mut datum_reader = Parser::new("(same same)").unwrap();
            let datum = datum_reader.parse(cx).unwrap();
            assert_eq!(cx.car(datum), cx.car(cx.cdr(datum)));
            assert_eq!(cx.get_symbol_name(cx.car(datum)).as_deref(), Some("same"));
            assert!(cx.source(datum).is_none());
        });
    }
}
