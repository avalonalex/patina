//! Reading data from text that arrives in pieces.
//!
//! A program on standard input, a `read` from a line-oriented port and an
//! interactive session are all fed text a line at a time, and each must decide
//! after every line whether a datum has finished. Reading everything since the
//! datum began again after each line costs time proportional to the square of
//! its length, which is what all three did (#341).
//!
//! A [`Reader`] keeps one lexer and feeds it each piece of text as it arrives.
//! The lexer stops at a token the next text could still continue
//! ([`Lexer::next_fed_token`]), so what it has produced are finished tokens,
//! and counting *those* says whether a datum has finished: openers not yet
//! closed, and prefixes — `'`, `` ` ``, `,`, `,@`, `#;`, `#n=` — still waiting
//! for their datum. That is structure rather than lexical rules: the tokens
//! counted are the lexer's own, where the scan this replaces had to decide for
//! itself what a token was. One lexical rule is still written twice, inside the
//! lexer — where a token that spans lines ends, which `Lexer::next_fed_token`
//! scans for and the three scanners that read those tokens also decide — and
//! `feeding_text_in_pieces_reads_what_parsing_it_whole_does` at the foot of
//! this file is what holds those two together.
//!
//! The parser runs only when the count says a datum is there, over the tokens
//! already lexed ([`Parser::from_tokens`]), and those tokens are then dropped.
//! Each piece of text is lexed once and parsed once, however many times the
//! reader was asked.

use crate::lexer::{LexError, Lexer, ReaderState, Spanned, Token};
use crate::parser::{ParseError, Parser};
use patina_core::{SharedHeap, TaggedValue};
use std::collections::VecDeque;

/// What a prefix waiting for a datum does with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prefix {
    /// `'`, `` ` ``, `,`, `,@` and `#n=` make a datum of it.
    Wraps,
    /// `#;` drops it.
    Drops,
}

/// A reader fed text as it arrives. See the module documentation.
pub struct Reader {
    lexer: Lexer,
    /// Finished tokens no datum has taken yet.
    tokens: Vec<Spanned>,
    /// Where each datum that has finished ends, as an index into `tokens`.
    datum_ends: VecDeque<usize>,
    /// Openers not yet closed.
    depth: usize,
    /// Prefixes at the top level still waiting for their datum, innermost
    /// last. One inside a list does not matter: the list is open anyway.
    prefixes: Vec<Prefix>,
    /// Where the text handed out as data ends.
    consumed: ReaderState,
    /// How much of that a caller has been told about ([`Reader::take_consumed`]).
    reported: usize,
    /// A lexical error met while reading ahead.
    ///
    /// Kept rather than reported at once, because the data before it have
    /// finished and are read first: a program whose last token is bad still
    /// runs the forms before it, as chibi and Gauche run them (#339).
    error: Option<LexError>,
    /// Whether the text has ended.
    ended: bool,
    /// Whether a trial read is running, which must leave nothing behind: it
    /// ends the text to ask what would be finished, and an unfinished datum is
    /// the answer rather than something to report. See [`Reader::inside_datum`].
    trial: bool,
}

impl Reader {
    /// A reader of text in the dialect `r6rs` selects, resolved once by the
    /// caller rather than once per lexer.
    pub fn new(r6rs: bool) -> Self {
        Reader {
            lexer: Lexer::feedable(r6rs),
            tokens: Vec::new(),
            datum_ends: VecDeque::new(),
            depth: 0,
            prefixes: Vec::new(),
            consumed: ReaderState::START,
            reported: 0,
            error: None,
            ended: false,
            trial: false,
        }
    }

    /// A reader carrying on from `at`, a point an earlier reader of the same
    /// source stopped at: line, column and `#!fold-case` continue from there,
    /// so what a diagnostic reports is the source's own position.
    ///
    /// A program on standard input needs this when it reads from that input
    /// itself, taking text the reader had been given but not yet run.
    pub fn resuming(r6rs: bool, at: ReaderState) -> Self {
        let mut reader = Reader::new(r6rs);
        reader.lexer.resume_at(at);
        // The line and column carry on; the offsets do not, since the text
        // this reader is given starts empty.
        reader.consumed = ReaderState { offset: 0, ..at };
        reader
    }

    /// Where the text handed out as data ends, which is where a caller
    /// keeping the text itself now stands in it.
    ///
    /// Not where the lexer stands: it reads ahead of the data handed out, as
    /// far as the text it has been given goes.
    pub fn position(&self) -> ReaderState {
        self.consumed
    }

    /// Read `text`, which follows whatever has been fed already.
    pub fn feed(&mut self, text: &str) {
        self.lexer.feed(text);
        self.lex_what_is_here();
    }

    /// No more text is coming: the last token ends where the text does, and a
    /// datum left unfinished is read as it stands, so it is reported as
    /// unfinished rather than waited for.
    pub fn no_more_text(&mut self) {
        self.lexer.no_more_text();
        self.lex_what_is_here();
    }

    /// How many characters of text the data handed out have used since this
    /// was last asked, for a caller that keeps the text itself — a port whose
    /// unread text the running program reads from too.
    ///
    /// The reader drops the text it has finished with when it can, so what it
    /// holds follows the datum being read rather than everything that has
    /// arrived (#333). Offsets then start again from there, which is why this
    /// is a count since the last call rather than a position.
    pub fn take_consumed(&mut self) -> usize {
        let consumed = self.consumed.offset - self.reported;
        self.reported = self.consumed.offset;
        // Only with no token waiting: a token the reader is holding carries
        // offsets into the text, which dropping it would leave behind.
        if self.tokens.is_empty() && !self.lexer.inside_token() {
            self.lexer.forget_read_text(self.consumed.offset);
            self.consumed.offset = 0;
            self.reported = 0;
        }
        consumed
    }

    /// Whether a datum would be left unfinished if the text ended where it
    /// does now, so a session should take another line.
    ///
    /// It asks about the text as it stands, which is not the question feeding
    /// asks: a reader fed more text holds back the token at the end, since
    /// what arrives next could continue it, and `(a)` typed at a prompt is
    /// finished all the same. So the reader tries ending the text, and puts
    /// back everything the trial read.
    ///
    /// Being inside a *token* counts — an unterminated string is unfinished
    /// however balanced the parentheses around it are. A lexical error does
    /// not: no further text finishes it, so it is reported as it stands, as a
    /// stray `)` is.
    pub fn inside_datum(&mut self) -> bool {
        if self.error.is_some() {
            return false;
        }
        let mark = self.lexer.mark();
        let tokens = self.tokens.len();
        let ends = self.datum_ends.len();
        let depth = self.depth;
        let prefixes = self.prefixes.clone();
        let ended = self.ended;

        self.trial = true;
        self.lexer.no_more_text();
        self.lex_what_is_here();
        // An error the trial meets is the text running out inside a token —
        // `"abc` is unfinished rather than wrong — but only for the tokens
        // that more text could finish. Ending the text also reveals errors
        // that stay errors, and `#\bogus` is one: a session reports that
        // rather than waiting for a line that cannot mend it.
        let ran_out = self.error.as_ref().is_some_and(LexError::is_incomplete);
        let unfinished = ran_out || self.unfinished_start().is_some();
        self.trial = false;

        self.lexer.restore(mark);
        self.tokens.truncate(tokens);
        self.datum_ends.truncate(ends);
        self.depth = depth;
        self.prefixes = prefixes;
        self.ended = ended;
        // A trial that ran into the end of the text found an error only
        // because it ended it; more text may yet finish that token.
        self.error = None;

        unfinished
    }

    /// Where the datum the reader is part-way through begins, if it is inside
    /// one: what a session cut off mid-form reports.
    fn unfinished_start(&self) -> Option<ReaderState> {
        let finished = self.datum_ends.back().copied().unwrap_or(0);
        self.tokens.get(finished).map(|token| token.start)
    }

    /// The next datum that has finished, parsed into `heap`, or `None` when
    /// none has and more text is needed.
    ///
    /// `configure` is handed the parser of that datum's own tokens, for a
    /// caller that records source positions ([`Parser::recording_into`]).
    /// A lexical error met while reading ahead is reported here, after the
    /// data that were finished before it.
    pub fn next_datum(
        &mut self,
        heap: &SharedHeap,
        configure: impl Fn(Parser) -> Parser,
    ) -> Option<Result<TaggedValue, ParseError>> {
        loop {
            let Some(end) = self.datum_ends.pop_front() else {
                return self.error.take().map(|error| Err(error.into()));
            };
            let tokens: Vec<Spanned> = self.tokens.drain(..end).collect();
            for later in &mut self.datum_ends {
                *later -= end;
            }
            let at = self.consumed;
            if let Some(last) = tokens.last() {
                self.consumed = last.end;
            }
            let parser = match Parser::from_tokens(tokens, heap.clone(), at) {
                Ok(parser) => configure(parser),
                Err(error) => return Some(Err(error)),
            };
            let mut parser = parser;
            match parser.parse_next() {
                Ok(Some(value)) => return Some(Ok(value)),
                // Only comments: a `#;` that dropped what followed it, or the
                // text ending in them. Read on.
                Ok(None) => continue,
                Err(error) => return Some(Err(error)),
            }
        }
    }

    /// Lex as much of the text as is here, stopping at a token more text could
    /// continue.
    fn lex_what_is_here(&mut self) {
        if self.error.is_some() {
            return;
        }
        loop {
            match self.lexer.next_fed_token() {
                Ok(Some(spanned)) if spanned.token == Token::Eof => {
                    self.end_of_text();
                    return;
                }
                Ok(Some(spanned)) => self.push(spanned),
                Ok(None) => return,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            }
        }
    }

    /// Take a finished token, and say whether it finished a datum.
    fn push(&mut self, spanned: Spanned) {
        let depth = self.depth;
        match &spanned.token {
            Token::LeftParen | Token::VectorOpen | Token::BytevectorOpen => {
                self.depth += 1;
                self.tokens.push(spanned);
            }
            Token::RightParen if depth > 0 => {
                self.depth -= 1;
                self.tokens.push(spanned);
                if self.depth == 0 {
                    self.datum_finished();
                }
            }
            Token::Quote
            | Token::Quasiquote
            | Token::Unquote
            | Token::UnquoteSplicing
            | Token::DatumLabel(_)
                if depth == 0 =>
            {
                self.prefixes.push(Prefix::Wraps);
                self.tokens.push(spanned);
            }
            Token::DatumComment if depth == 0 => {
                self.prefixes.push(Prefix::Drops);
                self.tokens.push(spanned);
            }
            _ if depth == 0 => {
                // A datum of its own: an atom or a `#n#` reference — and also
                // a `.` or a closer with nothing open, which finish nothing
                // but are mistakes to report now rather than text to wait on.
                self.tokens.push(spanned);
                self.datum_finished();
            }
            _ => self.tokens.push(spanned),
        }
    }

    /// A datum has finished at the top level.
    fn datum_finished(&mut self) {
        // It satisfies the innermost prefix waiting for one. A prefix that
        // wraps it makes a datum of itself, which satisfies the next; one that
        // drops it leaves nothing finished.
        while let Some(prefix) = self.prefixes.pop() {
            if prefix == Prefix::Drops {
                return;
            }
        }
        self.datum_ends.push_back(self.tokens.len());
    }

    /// The text has ended. Whatever is left is read as it stands, so an
    /// unfinished datum is reported rather than waited for.
    fn end_of_text(&mut self) {
        // A trial read ends the text only to ask what that would finish, and
        // reads nothing: an unfinished datum stays unfinished.
        if self.ended || self.trial {
            return;
        }
        self.ended = true;
        let finished = self.datum_ends.back().copied().unwrap_or(0);
        if self.tokens.len() > finished {
            self.datum_ends.push_back(self.tokens.len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_core::debug_format::format_tagged;

    /// Every datum a reader finds in `pieces`, fed one at a time, written out;
    /// an error is `error`, and what is still unfinished at the end of the
    /// last piece is left unread.
    fn read(pieces: &[&str]) -> Vec<String> {
        let heap = patina_core::new_shared_heap();
        let mut reader = Reader::new(false);
        let mut data = Vec::new();
        for piece in pieces {
            reader.feed(piece);
            while let Some(result) = reader.next_datum(&heap, |parser| parser) {
                data.push(match result {
                    Ok(value) => format_tagged(value, &heap.borrow()),
                    Err(_) => "error".to_string(),
                });
            }
        }
        data
    }

    /// A datum is read as soon as the text that finishes it has arrived, and
    /// not before.
    #[test]
    fn a_datum_is_read_once_its_text_is_all_here() {
        assert_eq!(read(&["(display 1)\n"]), vec!["(display 1)"]);
        assert_eq!(read(&["(list 1"]), Vec::<String>::new());
        assert_eq!(read(&["(list 1", " 2)\n"]), vec!["(list 1 2)"]);
        assert_eq!(read(&["(a)(b)\n"]), vec!["(a)", "(b)"]);
        // A form finished before the end of its line runs as soon as that
        // line arrives, which a producer waiting on its output depends on.
        assert_eq!(read(&["(a) (b"]), vec!["(a)"]);
    }

    /// A prefix waits for the datum it applies to; `#;` takes one away.
    #[test]
    fn a_prefix_waits_for_its_datum() {
        assert_eq!(read(&["'"]), Vec::<String>::new());
        assert_eq!(read(&["'", "a "]), vec!["(quote a)"]);
        assert_eq!(read(&["''a "]), vec!["(quote (quote a))"]);
        assert_eq!(read(&["#;a "]), Vec::<String>::new());
        assert_eq!(read(&["#;a b "]), vec!["b"]);
        assert_eq!(read(&["#;(1 2) b "]), vec!["b"]);
    }

    /// A mistake is reported where it is, rather than waited on — and the data
    /// before it are read first, so a program whose last token is bad still
    /// runs the forms before it.
    #[test]
    fn a_mistake_is_reported_after_the_data_before_it() {
        assert_eq!(read(&[") "]), vec!["error"]);
        assert_eq!(read(&["(a) { "]), vec!["(a)", "error"]);
    }

    /// The end of the text finishes the last datum, or reports it unfinished.
    #[test]
    fn the_end_of_the_text_finishes_or_reports_what_is_left() {
        let heap = patina_core::new_shared_heap();
        let mut reader = Reader::new(false);
        reader.feed("(a) (b");
        assert!(reader.next_datum(&heap, |p| p).is_some(), "(a) is read");
        assert!(
            reader.next_datum(&heap, |p| p).is_none(),
            "(b is unfinished"
        );
        assert!(reader.inside_datum());
        reader.no_more_text();
        assert!(
            matches!(reader.next_datum(&heap, |p| p), Some(Err(error)) if error.is_incomplete()),
            "the unfinished datum is reported, not waited for"
        );
    }

    /// A reader fed text in pieces reads what a parser handed the whole text
    /// at once reads, however the pieces fall.
    ///
    /// Where a token that spans lines ends is decided twice inside the lexer —
    /// by the scan that looks for it as text arrives, and by the scanner that
    /// reads the token — so this is what keeps the two together. The scan this
    /// reader replaced had drifted from the lexer exactly so (#341), and the
    /// test that caught it went with it.
    #[test]
    fn feeding_text_in_pieces_reads_what_parsing_it_whole_does() {
        let texts = [
            "(a b) 'c #;(d) e",
            "\"a b\" \"c\\\"d\" |a b| |c\\|d|",
            "#| a #| b |# c |# (d)",
            "#\\a #\\space #\\( 42 #xff",
            "#(1 2) #u8(1 2) `(a ,b ,@c)",
            "#0=(1 2) #0# (a . b)",
            "; a comment\n(after)\n",
            "#!fold-case ABC (DEF)",
            "\"a\nb\" |c\nd| #| e\nf |# g",
            "(a) #\\bogus",
            "(unfinished",
        ];
        for text in texts {
            let whole = {
                let heap = patina_core::new_shared_heap();
                let mut parser = Parser::new_with_heap(text, heap.clone()).expect("a parser");
                let mut data = Vec::new();
                while let Ok(Some(value)) = parser.parse_next() {
                    data.push(format_tagged(value, &heap.borrow()));
                }
                data
            };
            for piece in 1..=4 {
                let heap = patina_core::new_shared_heap();
                let mut reader = Reader::new(false);
                let mut data = Vec::new();
                let mut stopped = false;
                // Stopping at the first error, as the whole-text read above
                // does, so the two are compared on the same data.
                let take = |reader: &mut Reader, data: &mut Vec<String>| {
                    while let Some(result) = reader.next_datum(&heap, |parser| parser) {
                        match result {
                            Ok(value) => data.push(format_tagged(value, &heap.borrow())),
                            Err(_) => return true,
                        }
                    }
                    false
                };
                let characters: Vec<char> = text.chars().collect();
                for chunk in characters.chunks(piece) {
                    reader.feed(&chunk.iter().collect::<String>());
                    stopped = take(&mut reader, &mut data);
                    if stopped {
                        break;
                    }
                }
                if !stopped {
                    reader.no_more_text();
                    take(&mut reader, &mut data);
                }
                assert_eq!(data, whole, "{text:?}, fed {piece} characters at a time");
            }
        }
    }

    /// A reader carrying on part-way through a source reports that source's
    /// own positions, and keeps a `#!fold-case` read before it.
    #[test]
    fn a_resuming_reader_keeps_the_source_s_positions_and_directives() {
        let heap = patina_core::new_shared_heap();
        let source_map =
            std::rc::Rc::new(std::cell::RefCell::new(crate::source_map::SourceMap::new()));
        let at = ReaderState {
            offset: 0,
            line: 7,
            column: 5,
            fold_case: true,
        };
        let mut reader = Reader::resuming(false, at);
        reader.feed("(A b)\n");
        let value = reader
            .next_datum(&heap, |parser| {
                parser.recording_into(std::rc::Rc::from("<stdin>"), source_map.clone())
            })
            .expect("a datum has finished")
            .expect("it reads");
        let at = source_map.borrow().get(value).map(|l| (l.line, l.column));
        assert_eq!(at, Some((7, 5)), "the position is the source's own");
        assert_eq!(
            format_tagged(value, &heap.borrow()),
            "(a b)",
            "the directive read before it still holds"
        );
    }

    /// The first text of a source may begin with a byte order mark, which is
    /// dropped. A reader carrying on part-way through one is at no such start,
    /// and U+FEFF there is the character it is.
    #[test]
    fn only_the_start_of_a_source_drops_a_byte_order_mark() {
        assert_eq!(read(&["\u{feff}42 "]), vec!["42"]);

        let heap = patina_core::new_shared_heap();
        let mut reader = Reader::resuming(false, ReaderState::START);
        reader.feed("\u{feff}42 ");
        let value = reader
            .next_datum(&heap, |parser| parser)
            .expect("a datum has finished")
            .expect("it reads");
        assert_ne!(format_tagged(value, &heap.borrow()), "42");
    }

    /// A directive read in one piece of text holds for the text after it.
    #[test]
    fn a_directive_holds_for_the_text_that_follows_it() {
        assert_eq!(read(&["#!fold-case\n", "HELLO\n"]), vec!["hello"]);
    }

    /// Text stopping inside a token is unfinished however balanced it is; a
    /// mistake in it is not.
    #[test]
    fn being_inside_a_token_is_being_inside_a_datum() {
        let inside = |text: &str| {
            let mut reader = Reader::new(false);
            reader.feed(text);
            reader.inside_datum()
        };
        assert!(inside("\"abc"), "an unterminated string");
        assert!(inside("#| a "), "a block comment still open");
        assert!(inside("(a"), "a list still open");
        assert!(inside("'"), "a prefix with nothing after it");
        assert!(
            !inside("(display 1) #\\bogus"),
            "a bad character literal stays wrong however much follows it"
        );
        assert!(!inside("(a) "), "whitespace after a finished datum");
        assert!(!inside("(a)"), "a finished datum");
        assert!(!inside(") "), "a mistake is reported, not waited on");
    }
}
