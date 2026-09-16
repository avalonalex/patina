use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Delimiters
    LeftParen,
    RightParen,
    VectorOpen,     // #(
    BytevectorOpen, // #u8(

    // Data
    Boolean(bool),
    Number(String), // Parse into actual number later
    Character(char),
    String(String),
    Identifier(String),

    // Special syntax
    Quote,             // '
    Quasiquote,        // `
    Unquote,           // ,
    UnquoteSplicing,   // ,@
    Dot,               // .
    DatumComment,      // #; (parser should skip next datum)
    DatumLabel(usize), // #n= (label definition)
    DatumRef(usize),   // #n# (label reference)

    // End of input
    Eof,
}

#[derive(Error, Debug)]
pub enum LexError {
    /// Formatted through [`describe_char`], because the characters that reach
    /// here are largely ones with no visible glyph: `'\0'` is also the
    /// end-of-input sentinel `current_char` returns past the end, and above
    /// ASCII only whitespace is left, everything else now being an identifier.
    #[error("Unexpected character: {}", describe_char(*.0))]
    UnexpectedChar(char),

    #[error("Unterminated string")]
    UnterminatedString,

    #[error("Invalid character literal")]
    InvalidCharacter,

    #[error("Reserved character (R7RS): {0}. Reserved for future extensions")]
    ReservedCharacter(char),

    /// R6RS syntax refused by the R7RS default. Names the switch, because the
    /// reader is refusing something it can perfectly well read and saying so
    /// is the difference between a dead end and a one-flag fix.
    #[error("{syntax} is R6RS syntax, not R7RS (pass --allow-r6rs to read it)")]
    R6rsSyntax { syntax: &'static str },

    /// A closer whose shape disagrees with the opener it would close —
    /// `(let ([x 1)]) …)`. Naming both spellings is the whole value of the
    /// message: the reader's complaint is about a `)` several characters
    /// before the place a human would look for the missing `]`.
    #[error("Mismatched delimiter: expected {expected}, got {closed}")]
    MismatchedDelimiter { expected: char, closed: char },

    #[error("Unterminated vertical bar identifier")]
    UnterminatedVerticalBarIdentifier,

    #[error("Invalid escape sequence in identifier: \\{0}")]
    InvalidEscapeInIdentifier(String),

    #[error("Invalid escape sequence in string: {0}")]
    InvalidEscapeInString(String),

    #[error("Unterminated block comment")]
    UnterminatedBlockComment,

    #[allow(dead_code)]
    #[error("Invalid number format: {0}")]
    InvalidNumber(String),
}

/// Render a character for an error message, naming it by code point when the
/// glyph would show the reader nothing — whitespace and controls.
///
/// Without this, a stray U+00A0 between two identifiers reports
/// `Unexpected character: ` and the reader is left hunting for an invisible
/// difference; with it, the message says `U+00A0`.
fn describe_char(ch: char) -> String {
    if ch.is_whitespace() || ch.is_control() {
        format!("U+{:04X}", ch as u32)
    } else {
        ch.to_string()
    }
}

/// Where a reader stands in its text: a character offset with the line and
/// column it falls on, and the one piece of reader state that outlives a
/// token, whether `#!fold-case` is in effect.
///
/// A reader that stops and later carries on — a program read as it arrives,
/// which cannot parse a form until the lines holding it are in — resumes from
/// one of these with [`Lexer::resume_at`], so positions stay the source's own
/// and a directive read earlier still holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReaderState {
    /// Characters of the text before this point.
    pub offset: usize,
    pub line: u32,
    pub column: u32,
    pub fold_case: bool,
}

impl ReaderState {
    /// The start of a source.
    pub const START: ReaderState = ReaderState {
        offset: 0,
        line: 1,
        column: 1,
        fold_case: false,
    };
}

/// A token with its source position
#[derive(Debug, Clone)]
pub struct Spanned {
    pub token: Token,
    pub line: u32,
    pub column: u32,
    /// Where the token begins, with the reader state there.
    pub start: ReaderState,
    /// Where the text after the token begins, with the reader state there, so
    /// a reader replaying tokens can say how much text they consumed without
    /// the lexer that produced them.
    pub end: ReaderState,
}

pub struct Lexer {
    input: Vec<char>,
    position: usize,
    /// Whether to fold identifiers to lowercase (R7RS #!fold-case directive)
    fold_case: bool,
    /// Current line number (1-based)
    line: u32,
    /// Current column number (1-based)
    column: u32,
    /// Where the token returned by the previous `next_token` call ended (the
    /// start of the input before any token is returned). Its offset does not
    /// count a dropped byte order mark; the accessors add it back.
    prev_token_end: ReaderState,
    /// 1 when a leading U+FEFF was dropped, so offsets reported outward can
    /// be put back into the caller's own input.
    bom_offset: usize,
    /// Whether the R6RS surface syntax R7RS reserves is read — resolved once
    /// here rather than per token. See [`crate::dialect`].
    allow_r6rs: bool,
    /// The closer each currently-open delimiter expects, innermost last, so a
    /// closer can be checked against the opener it actually closes.
    ///
    /// `[` and `]` are read as parentheses (see [`Lexer::lex_token`]), which
    /// on its own would let `(let ([x 1)]) …)` through as if it were written
    /// with matched pairs. Every implementation that accepts brackets —
    /// Gauche, Chez, Racket, Guile — rejects the mismatch instead, and it is
    /// a typo worth catching rather than a dialect to accept. Unbalanced
    /// *input* is not this stack's business: a closer with nothing open pops
    /// nothing and reaches the parser as a plain `)`, which is what the REPL
    /// needs while a form is still being typed.
    open_delimiters: Vec<char>,
    /// Whether more text may still arrive ([`Lexer::feed`]), which makes a
    /// token that runs to the end of what is here provisional. See
    /// [`Lexer::next_fed_token`].
    more_may_come: bool,
    /// Whether nothing has been read yet from the start of a source, so the
    /// first text fed may begin with a byte order mark to drop. A lexer
    /// resuming part-way through a source ([`Lexer::resume_at`]) is not at one,
    /// and U+FEFF there is the character it is.
    at_source_start: bool,
    /// Where the token being read begins: before the whitespace and comments
    /// it follows have been skipped, and again after. A token the text runs
    /// out inside is read again from here, so the whitespace before it is read
    /// once ([`Lexer::next_fed_token`]).
    token_start: ReaderState,
    /// The token the text ran out inside, when it is one that may span lines.
    /// See [`Partial`].
    partial: Option<Partial>,
}

impl LexError {
    /// Whether the text ran out part-way through a token, as opposed to text
    /// that stays wrong however much more follows.
    ///
    /// A string, a `|symbol|` and a block comment may each span lines, so an
    /// unterminated one at the end of what has arrived says only that it is
    /// not finished yet. `#\bogus` is wrong whatever follows it.
    pub fn is_incomplete(&self) -> bool {
        Looking::of(self).is_some()
    }
}

/// A token the text ran out inside, and how far the scan for its end has got.
///
/// A string, a `|symbol|` and a block comment may each span any number of
/// lines. Reading such a token again whenever a line arrives costs time
/// proportional to the square of its length, so the lexer scans on for its end
/// instead, carrying on from where the scan stopped. When the end is here the
/// token is read once, from its start (#341).
#[derive(Debug, Clone, Copy)]
struct Partial {
    /// Where the token begins, so it is read with its own position.
    start: ReaderState,
    looking_for: Looking,
    /// How far the scan has got.
    scanned_to: usize,
}

/// What the end of an unfinished token looks like, and what the scan carries
/// for it.
#[derive(Debug, Clone, Copy)]
enum Looking {
    /// The `"` ending a string or the `|` ending a `|symbol|`, with whether
    /// the scan stopped just after a backslash, which escapes what follows.
    Closer { closer: char, escaped: bool },
    /// The `|#` ending a block comment, and how deep it is, since they nest.
    BlockComment { depth: usize },
    /// The line ending that closes a `;` comment. There is no error for one
    /// that runs to the end of the text, so the skip below builds this itself.
    LineComment,
}

impl Looking {
    /// What the scanner was reading, from the error it gave when the text ran
    /// out inside it. Every other error is one more text cannot finish.
    fn of(error: &LexError) -> Option<Looking> {
        match error {
            LexError::UnterminatedString => Some(Looking::Closer {
                closer: '"',
                escaped: false,
            }),
            LexError::UnterminatedVerticalBarIdentifier => Some(Looking::Closer {
                closer: '|',
                escaped: false,
            }),
            LexError::UnterminatedBlockComment => Some(Looking::BlockComment { depth: 1 }),
            _ => None,
        }
    }

    /// How many characters open it: `"`, `|`, `#|`.
    fn opener(self) -> usize {
        match self {
            Looking::BlockComment { .. } => 2,
            Looking::Closer { .. } | Looking::LineComment => 1,
        }
    }
}

/// Where a lexer stood, for undoing a trial read. See [`Lexer::mark`].
pub struct LexerMark {
    at: ReaderState,
    prev_token_end: ReaderState,
    open_delimiters: Vec<char>,
    more_may_come: bool,
    token_start: ReaderState,
    partial: Option<Partial>,
}

/// Drop a leading U+FEFF.
///
/// A byte-order mark is what a Windows editor puts at the head of a UTF-8
/// file, and it is not whitespace and not a delimiter, so it started an
/// identifier: a file that differs from a working one only by its first three
/// bytes failed with `unbound variable: ` and an empty-looking name. R7RS has
/// nothing to say about it and neither does chibi, which reads it the same
/// way, but a mark that means "this is UTF-8" is not program text under any
/// reading. Only a *leading* one is dropped — anywhere else U+FEFF is a real
/// (if inadvisable) character and stays.
fn strip_byte_order_mark(input: &str) -> Vec<char> {
    input
        .strip_prefix('\u{feff}')
        .unwrap_or(input)
        .chars()
        .collect()
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: strip_byte_order_mark(input),
            position: 0,
            fold_case: false,
            line: 1,
            column: 1,
            prev_token_end: ReaderState::START,
            bom_offset: usize::from(input.starts_with('\u{feff}')),
            allow_r6rs: crate::dialect::allow_r6rs(),
            open_delimiters: Vec::new(),
            more_may_come: false,
            at_source_start: input.is_empty(),
            token_start: ReaderState::START,
            partial: None,
        }
    }

    /// A lexer with no text yet, fed with [`Lexer::feed`] as it arrives: a
    /// program on standard input, a `read` from a line-oriented port, a
    /// session deciding whether to take another line.
    ///
    /// `r6rs` is the dialect, resolved once by the caller rather than once per
    /// lexer, as [`Lexer::reading_r6rs`] takes it.
    pub fn feedable(r6rs: bool) -> Self {
        Lexer {
            more_may_come: true,
            ..Lexer::new("").reading_r6rs(r6rs)
        }
    }

    /// Add `text` to the end of what this lexer reads.
    ///
    /// A byte order mark is dropped only from the first text of a source, as
    /// [`Lexer::new`] drops it; anywhere else U+FEFF is a character.
    pub fn feed(&mut self, text: &str) {
        // Empty text is not the start being read: a `read` feeds a port's
        // pushback first, which is usually empty, and the mark is in the line
        // that follows it.
        if self.at_source_start && !text.is_empty() {
            self.bom_offset = usize::from(text.starts_with('\u{feff}'));
            self.input.extend(strip_byte_order_mark(text));
            self.at_source_start = false;
        } else {
            self.input.extend(text.chars());
        }
    }

    /// No more text is coming, so a token reaching the end of what is here is
    /// finished rather than provisional.
    pub fn no_more_text(&mut self) {
        self.more_may_come = false;
    }

    /// The next token, or `None` when the text that has arrived ends inside
    /// one and more may still come.
    ///
    /// A token that runs to the end of that text is provisional, because what
    /// arrives next could continue it: `foo` may become `foobar`, `,` may
    /// become `,@`, and `#t` may become `#true`. The lexer rewinds to where
    /// such a token began and waits for more.
    ///
    /// That is also the answer to "can a datum have finished yet", with no
    /// second reading of the lexical rules: until a token is finished, no
    /// datum containing it has. Only the text running out stops the lexer —
    /// a mistake in text that is all here is still an error, reported where
    /// it is.
    pub fn next_fed_token(&mut self) -> Result<Option<Spanned>, LexError> {
        let delimiters = (
            self.open_delimiters.len(),
            self.open_delimiters.last().copied(),
        );
        if self.partial.is_some() {
            match self.finish_partial() {
                // The end of the token is here: read it from its start, in one
                // pass.
                Some(start) => self.rewind_to(start, delimiters),
                None => return Ok(None),
            }
        }
        // After that rewind, not before it: a token whose end has just arrived
        // is read from its start, and rewinding it to where the lexer stood
        // before would step over the token altogether.
        let resume = self.raw_state();
        let token = self.next_token();
        if self.more_may_come && self.is_at_end() {
            if let Err(error) = &token
                && let Some(looking_for) = Looking::of(error)
            {
                // A token that may span any number of lines. Rather than
                // reading it again whenever a line arrives, scan on for its
                // end as the text comes (`Partial`).
                self.partial = Some(Partial {
                    start: self.token_start,
                    looking_for,
                    scanned_to: self.token_start.offset + looking_for.opener(),
                });
                return Ok(None);
            }
            // Either a token that ran to the end of the text, or the error of
            // one the text ran out inside: a `#\` with nothing after it, an
            // identifier the next text may continue.
            //
            // Back to where the token began, which is past the whitespace and
            // comments before it: those are finished, and reading them again
            // on every line that arrives is what made a form spanning many of
            // them cost time proportional to its square.
            let back_to = if self.token_start.offset > resume.offset {
                self.token_start
            } else {
                resume
            };
            self.rewind_to(back_to, delimiters);
            return Ok(None);
        }
        token.map(Some)
    }

    /// Carry on from `at`, a point an earlier lexer of the same source stopped
    /// at, before any text has been fed: positions continue from there, and so
    /// does `#!fold-case`.
    pub fn resume_at(&mut self, at: ReaderState) {
        self.at_source_start = false;
        self.position = 0;
        self.line = at.line;
        self.column = at.column;
        self.fold_case = at.fold_case;
        self.prev_token_end = ReaderState { offset: 0, ..at };
        self.token_start = ReaderState::START;
    }

    /// Drop the text already read, so what a fed lexer holds follows what it
    /// is still reading rather than everything that has arrived (#333).
    ///
    /// Offsets start again from it: only a caller that has finished with the
    /// text behind the lexer calls this, and the line and column carry on, so
    /// what a diagnostic reports is unaffected.
    pub fn forget_read_text(&mut self, up_to: usize) {
        debug_assert!(
            self.partial.is_none(),
            "dropping text under a token being scanned would leave its offsets behind",
        );
        let up_to = up_to.min(self.position);
        self.input.drain(..up_to);
        self.position -= up_to;
        self.token_start = ReaderState {
            offset: self.token_start.offset.saturating_sub(up_to),
            ..self.token_start
        };
        self.prev_token_end = ReaderState {
            offset: self.prev_token_end.offset.saturating_sub(up_to),
            ..self.prev_token_end
        };
        self.bom_offset = 0;
    }

    /// Where the lexer stands, for a trial read that is to be undone
    /// ([`Lexer::restore`]).
    pub fn mark(&self) -> LexerMark {
        LexerMark {
            at: self.raw_state(),
            prev_token_end: self.prev_token_end,
            open_delimiters: self.open_delimiters.clone(),
            more_may_come: self.more_may_come,
            token_start: self.token_start,
            partial: self.partial,
        }
    }

    /// Put the lexer back where `mark` was taken, undoing a trial read. The
    /// text itself is kept: only where the lexer stands in it is restored.
    pub fn restore(&mut self, mark: LexerMark) {
        self.position = mark.at.offset;
        self.line = mark.at.line;
        self.column = mark.at.column;
        self.fold_case = mark.at.fold_case;
        self.prev_token_end = mark.prev_token_end;
        self.open_delimiters = mark.open_delimiters;
        self.more_may_come = mark.more_may_come;
        self.token_start = mark.token_start;
        self.partial = mark.partial;
    }

    /// Look for the end of the token the text ran out inside, carrying on from
    /// where the last scan stopped: `Some` with where that token begins once
    /// its end is here, and `None` while it is not.
    ///
    /// Nothing more is coming, so the token is read as it stands and reported
    /// unterminated.
    fn finish_partial(&mut self) -> Option<ReaderState> {
        let mut partial = self.partial?;
        if !self.more_may_come {
            self.partial = None;
            return Some(partial.start);
        }
        let mut at = partial.scanned_to;
        while at < self.input.len() {
            let ch = self.input[at];
            match &mut partial.looking_for {
                Looking::Closer { closer, escaped } => {
                    if *escaped {
                        *escaped = false;
                    } else if ch == '\\' {
                        *escaped = true;
                    } else if ch == *closer {
                        self.partial = None;
                        return Some(partial.start);
                    }
                    at += 1;
                }
                Looking::LineComment => {
                    if ch == '\n' {
                        self.partial = None;
                        return Some(partial.start);
                    }
                    at += 1;
                }
                Looking::BlockComment { depth } => {
                    let next = self.input.get(at + 1).copied();
                    if matches!(ch, '#' | '|') && next.is_none() {
                        // The pair may be split between what has arrived and
                        // what has not.
                        break;
                    }
                    match (ch, next) {
                        ('#', Some('|')) => {
                            *depth += 1;
                            at += 2;
                        }
                        ('|', Some('#')) => {
                            *depth -= 1;
                            at += 2;
                            if *depth == 0 {
                                self.partial = None;
                                return Some(partial.start);
                            }
                        }
                        _ => at += 1,
                    }
                }
            }
        }
        partial.scanned_to = at;
        self.partial = Some(partial);
        None
    }

    /// Whether the text so far ends inside a token that may span lines, whose
    /// text the lexer is therefore still holding.
    pub fn inside_token(&self) -> bool {
        self.partial.is_some()
    }

    /// Put the lexer back where it stood before the token just read, so that
    /// token is read again once more text has arrived.
    ///
    /// `#!fold-case` travels in the state. The open-delimiter stack is put
    /// back by hand, since the token may have pushed one (`(`) or popped one
    /// (`)`).
    fn rewind_to(&mut self, at: ReaderState, delimiters: (usize, Option<char>)) {
        self.position = at.offset;
        self.line = at.line;
        self.column = at.column;
        self.fold_case = at.fold_case;
        self.prev_token_end = at;
        let (depth, innermost) = delimiters;
        if self.open_delimiters.len() > depth {
            self.open_delimiters.truncate(depth);
        } else if self.open_delimiters.len() < depth
            && let Some(closer) = innermost
        {
            self.open_delimiters.push(closer);
        }
    }

    /// Create a lexer with case-folding enabled from the start.
    ///
    /// This is used for `include-ci` which reads files in case-insensitive mode.
    /// Identifiers will be folded to lowercase, matching R7RS `#!fold-case` behavior.
    pub fn new_case_insensitive(input: &str) -> Self {
        Lexer {
            fold_case: true,
            ..Lexer::new(input)
        }
    }

    /// Read the R6RS syntax R7RS reserves or not, whatever the ambient
    /// setting says.
    ///
    /// The setting is an environment variable and therefore process-wide,
    /// which is right for a person choosing how their program is read and
    /// wrong for a caller that knows: a test asserting bracket behaviour would
    /// otherwise have to set a variable every other test in the binary can
    /// see, and a reader that builds a lexer per line has resolved the
    /// setting once already.
    pub fn reading_r6rs(mut self, allow: bool) -> Self {
        self.allow_r6rs = allow;
        self
    }

    /// Lex the next token, returning just the Token without position info.
    /// Convenience method for tests and callers that don't need source positions.
    pub fn next_token_kind(&mut self) -> Result<Token, LexError> {
        self.next_token().map(|s| s.token)
    }

    pub fn next_token(&mut self) -> Result<Spanned, LexError> {
        // The current position is exactly the end of the previously
        // returned token — record it before skipping whitespace so callers
        // can tell how much input the previous tokens consumed
        self.prev_token_end = self.raw_state();
        // Where what is being read begins: the comment or whitespace first,
        // since running out inside one of those is also running out inside
        // something, and then the token itself. See `next_fed_token`.
        self.token_start = self.raw_state();
        self.skip_whitespace_and_comments()?;

        let start = self.state();
        self.token_start = self.raw_state();
        let token = self.lex_token()?;
        Ok(Spanned {
            token,
            line: start.line,
            column: start.column,
            start,
            end: self.state(),
        })
    }

    fn raw_state(&self) -> ReaderState {
        ReaderState {
            offset: self.position,
            line: self.line,
            column: self.column,
            fold_case: self.fold_case,
        }
    }

    /// Where the lexer stands, its offset counted in the caller's input.
    pub fn state(&self) -> ReaderState {
        ReaderState {
            offset: self.position + self.bom_offset,
            ..self.raw_state()
        }
    }

    /// Where the token returned by the previous `next_token` call ended, its
    /// offset counted in the caller's input. See [`Lexer::prev_token_end`].
    pub fn prev_token_end_state(&self) -> ReaderState {
        ReaderState {
            offset: self.prev_token_end.offset + self.bom_offset,
            ..self.prev_token_end
        }
    }

    /// Char offset just past the end of the token returned by the previous
    /// `next_token` call. Used to determine exactly how much input a parse
    /// consumed.
    ///
    /// Counted in the input the caller handed over, byte order mark included:
    /// the lexer drops a leading one, but `read` maps this offset back onto
    /// its own buffer, and an offset one character short there left the port
    /// re-reading the last character of every datum.
    pub fn prev_token_end(&self) -> usize {
        self.prev_token_end_state().offset
    }

    /// Read the `u8(` that both `#u8(` and `#vu8(` end with, positioned on
    /// the `u`.
    fn read_bytevector_open(&mut self) -> Result<Token, LexError> {
        self.advance(); // consume u
        if self.current_char() != '8' {
            return Err(LexError::UnexpectedChar(self.current_char()));
        }
        self.advance();
        if self.current_char() != '(' {
            return Err(LexError::UnexpectedChar(self.current_char()));
        }
        self.advance();
        self.open_delimiters.push(')');
        Ok(Token::BytevectorOpen)
    }

    /// Pop the innermost open delimiter, requiring `closed` to be its match.
    ///
    /// A closer with nothing open is left alone: the parser reports it, with
    /// the surrounding datum for context, and the REPL's incomplete-input
    /// path depends on partial text lexing without complaint.
    fn close_delimiter(&mut self, closed: char) -> Result<(), LexError> {
        match self.open_delimiters.pop() {
            Some(expected) if expected != closed => {
                Err(LexError::MismatchedDelimiter { expected, closed })
            }
            _ => Ok(()),
        }
    }

    fn lex_token(&mut self) -> Result<Token, LexError> {
        if self.is_at_end() {
            return Ok(Token::Eof);
        }

        let ch = self.current_char();

        match ch {
            // `[` and `]` are R6RS list delimiters, and the house style of
            // Chez, Racket, Guile and Gauche. R7RS 7.1.1 reserves them, so no
            // conforming R7RS program contains one and reading them only
            // widens the accepted language — the same trade taken for the
            // bare `@` token. Their shape is checked against the opener, so
            // the pairing is still enforced.
            '[' | ']' if !self.allow_r6rs => Err(LexError::R6rsSyntax {
                syntax: "square bracket",
            }),
            '(' | '[' => {
                self.advance();
                self.open_delimiters.push(if ch == '[' { ']' } else { ')' });
                Ok(Token::LeftParen)
            }
            ')' | ']' => {
                self.advance();
                self.close_delimiter(ch)?;
                Ok(Token::RightParen)
            }
            // R7RS reserves { } for future extensions
            '{' | '}' => Err(LexError::ReservedCharacter(ch)),
            '\'' => {
                self.advance();
                Ok(Token::Quote)
            }
            '`' => {
                self.advance();
                Ok(Token::Quasiquote)
            }
            ',' => {
                self.advance();
                if self.current_char() == '@' {
                    self.advance();
                    Ok(Token::UnquoteSplicing)
                } else {
                    Ok(Token::Unquote)
                }
            }
            '.' if self.is_delimiter_next() => {
                self.advance();
                Ok(Token::Dot)
            }
            // R7RS: Numbers can start with a decimal point (e.g., .3 is 0.3)
            '.' if self.peek_is_numeric() => self.read_number(),
            '"' => self.read_string(),
            '|' => self.read_vertical_bar_identifier(),
            '#' => self.read_hash_syntax(),
            // `is_ascii_digit`, not `is_numeric`: R7RS number literals are
            // ASCII, while `char::is_numeric` also covers Nd/Nl/No above it,
            // so `₁` (U+2081) was claimed here and rejected as a bad number
            // rather than reaching `is_identifier_start` at all. The two
            // `peek_is_*` helpers hold the same boundary.
            _ if ch.is_ascii_digit()
                || (ch == '-' || ch == '+')
                    && (self.peek_is_numeric()
                        || self.peek_is_decimal_start()
                        || self.peek_is_imaginary()
                        || self.is_special_float_literal()) =>
            {
                self.read_number()
            }
            _ if Self::is_identifier_start(ch) => self.read_identifier(),
            _ => Err(LexError::UnexpectedChar(ch)),
        }
    }

    fn current_char(&self) -> char {
        self.input.get(self.position).copied().unwrap_or('\0')
    }

    fn advance(&mut self) {
        if self.position < self.input.len() {
            // Saturating: a program read as it arrives has no length, and a
            // position that stops counting beats one that wraps or panics.
            if self.input[self.position] == '\n' {
                self.line = self.line.saturating_add(1);
                self.column = 1;
            } else {
                self.column = self.column.saturating_add(1);
            }
        }
        self.position += 1;
    }

    /// Current line number (1-based)
    pub fn current_line(&self) -> u32 {
        self.line
    }

    /// Current column number (1-based)
    pub fn current_column(&self) -> u32 {
        self.column
    }

    fn is_at_end(&self) -> bool {
        self.position >= self.input.len()
    }

    fn skip_whitespace_and_comments(&mut self) -> Result<(), LexError> {
        while !self.is_at_end() {
            // Where what is being skipped begins, so a block comment the text
            // runs out inside is scanned from its own start rather than from
            // the end of the last token. See [`Lexer::next_fed_token`].
            self.token_start = self.raw_state();
            match self.current_char() {
                // Exactly R7RS 7.1.1's <whitespace>, deliberately *not*
                // `char::is_whitespace`: widening it here would silently turn
                // a stray U+00A0 into a space, where `is_identifier_start`
                // keeps it a visible error. See that function.
                ' ' | '\t' | '\n' | '\r' | '\x0C' => self.advance(),
                ';' => {
                    let start = self.raw_state();
                    self.skip_to_line_ending();
                    if self.more_may_come && self.is_at_end() {
                        // No line ending yet, so the text that follows is
                        // still comment. Scan on for one rather than treating
                        // the comment as read (`Partial`).
                        self.partial = Some(Partial {
                            start,
                            looking_for: Looking::LineComment,
                            scanned_to: self.position,
                        });
                        return Ok(());
                    }
                }
                '#' if self.peek_char() == Some('|') => {
                    // Block comment: skip nested block comment
                    self.skip_block_comment()?;
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn peek_char(&self) -> Option<char> {
        self.input.get(self.position + 1).copied()
    }

    fn skip_block_comment(&mut self) -> Result<(), LexError> {
        // R7RS: Block comments can be nested
        // #| ... |# where ... can contain more #| ... |#

        self.advance(); // consume #
        self.advance(); // consume |

        let mut depth = 1;

        while !self.is_at_end() && depth > 0 {
            if self.current_char() == '#' && self.peek_char() == Some('|') {
                // Nested block comment start
                depth += 1;
                self.advance(); // consume #
                self.advance(); // consume |
            } else if self.current_char() == '|' && self.peek_char() == Some('#') {
                // Block comment end
                depth -= 1;
                self.advance(); // consume |
                self.advance(); // consume #
            } else {
                self.advance();
            }
        }

        if depth > 0 {
            return Err(LexError::UnterminatedBlockComment);
        }

        Ok(())
    }

    /// Where a token ends.
    ///
    /// Narrower than R7RS 7.1.1's <delimiter>, which also lists `|`, and
    /// narrower than what Gauche and chibi stop at (`'`, `` ` ``, `,`). So
    /// `a'b` and `a,b` still read as one symbol here and as two tokens there.
    /// That divergence is deliberately left alone — widening this set can only
    /// *split* tokens that used to be whole, which is the one kind of lexer
    /// change that can alter an existing program's meaning, so it wants its
    /// own decision and its own cross-check.
    ///
    /// **`[` and `]` are the one such widening taken so far**, forced by
    /// reading them as list delimiters: without it `[x 1]` ends at the `1]`,
    /// which the number reader then rejects. It is safe in the direction the
    /// warning above is about, because a bracket is not an `<initial>` or
    /// `<subsequent>` in R7RS 7.1.1 — no conforming identifier contains one,
    /// so nothing conforming is being split. Cross-checked against the whole
    /// `compat/vendor/` corpus and `lib/`, where every occurrence outside a
    /// string or comment is `#\[` or `#\]`, and those are unaffected:
    /// `read_character` takes a delimiter first character as a complete
    /// one-character literal, which is already how `#\(` is read. A symbol
    /// that genuinely needs a bracket can still be written `|a[b]|`.
    ///
    /// What this function is for is making that decision live in one place:
    /// the set was written out seven times before, which is why the question
    /// had no home.
    pub(crate) fn is_delimiter(ch: char) -> bool {
        ch.is_whitespace() || matches!(ch, '(' | ')' | '[' | ']' | '"' | ';')
    }

    fn is_delimiter_next(&self) -> bool {
        if self.position + 1 >= self.input.len() {
            return true;
        }
        Self::is_delimiter(self.input[self.position + 1])
    }

    /// ASCII-only; see the number dispatch in `lex_token` for why.
    fn peek_is_numeric(&self) -> bool {
        if self.position + 1 >= self.input.len() {
            return false;
        }
        self.input[self.position + 1].is_ascii_digit()
    }

    fn peek_is_imaginary(&self) -> bool {
        if self.position + 1 >= self.input.len() {
            return false;
        }
        let next = self.input[self.position + 1];
        next == 'i' || next == 'I'
    }

    /// Check if the next character is a decimal point followed by a digit
    /// This handles cases like `-.1` which should parse as `-0.1`
    fn peek_is_decimal_start(&self) -> bool {
        if self.position + 2 >= self.input.len() {
            return false;
        }
        let next = self.input[self.position + 1];
        let after = self.input[self.position + 2];
        next == '.' && after.is_ascii_digit()
    }

    /// Are we at `+nan.0` or `-nan.0` — or, in principle, `+inf.0`/`-inf.0`?
    ///
    /// The `inf` spellings never actually arrive: `peek_is_imaginary` runs
    /// first in the dispatch and claims any `+`/`-` followed by `i`, so they
    /// reach `read_number` by that route. They are still named here because
    /// this predicate is where a reader looks for the set of special floats,
    /// and because the two orderings should not have to be read together to
    /// know the answer is right.
    ///
    /// Kept allocation-free deliberately. Collecting the rest of the input
    /// into a `String` to test a prefix — the obvious spelling, and the one
    /// this replaced — made lexing quadratic in file size, because the
    /// dispatch reaches here for every `+`/`-` token that is not a number:
    /// every `(- a b)`, every `->name`. See `PRD/TRACK_P_PERFORMANCE_PRD.md`.
    ///
    /// The ASCII-only case folding is not a narrowing: no non-ASCII character
    /// lowercases to a bare `i`, `n`, `f` or `a` (checked over every code
    /// point), and `İ` (U+0130), the near miss, folds to *two* chars — which
    /// the old prefix test rejected too.
    fn is_special_float_literal(&self) -> bool {
        matches!(self.current_char(), '+' | '-')
            && (self.matches_ascii_at(1, "nan.0") || self.matches_ascii_at(1, "inf.0"))
    }

    fn read_string(&mut self) -> Result<Token, LexError> {
        self.advance(); // consume opening "
        let mut result = String::new();

        while !self.is_at_end() && self.current_char() != '"' {
            if self.current_char() == '\\' {
                self.advance();
                if self.is_at_end() {
                    return Err(LexError::UnterminatedString);
                }
                let escaped = match self.current_char() {
                    // R7RS mnemonic escapes
                    'a' => '\u{0007}', // alarm
                    'b' => '\u{0008}', // backspace
                    't' => '\t',       // tab
                    'n' => '\n',       // newline
                    'r' => '\r',       // carriage return
                    '\\' => '\\',      // backslash
                    '"' => '"',        // double quote
                    '|' => '|',        // vertical bar
                    // R7RS inline hex escape: \x<hex>;
                    'x' => {
                        self.advance();
                        let mut hex_str = String::new();
                        while !self.is_at_end() && self.current_char() != ';' {
                            hex_str.push(self.current_char());
                            self.advance();
                        }
                        if self.is_at_end() || self.current_char() != ';' {
                            return Err(LexError::InvalidEscapeInString(format!(
                                "\\x{} (missing semicolon)",
                                hex_str
                            )));
                        }
                        // Don't advance past ';' here - done at end of loop
                        match u32::from_str_radix(&hex_str, 16) {
                            Ok(code) => match char::from_u32(code) {
                                Some(ch) => ch,
                                None => {
                                    return Err(LexError::InvalidEscapeInString(format!(
                                        "\\x{}; (invalid Unicode code point)",
                                        hex_str
                                    )));
                                }
                            },
                            Err(_) => {
                                return Err(LexError::InvalidEscapeInString(format!(
                                    "\\x{}; (invalid hex)",
                                    hex_str
                                )));
                            }
                        }
                    }
                    // R7RS: Line ending escape - backslash followed by intraline whitespace
                    // and line ending is ignored along with any leading intraline whitespace
                    // on the next line
                    '\n' | '\r' => {
                        // Skip the line ending
                        if self.current_char() == '\r' && self.peek_char() == Some('\n') {
                            self.advance();
                        }
                        self.advance();
                        // Skip leading whitespace on next line
                        while !self.is_at_end()
                            && (self.current_char() == ' ' || self.current_char() == '\t')
                        {
                            self.advance();
                        }
                        continue; // Don't push anything, don't advance again
                    }
                    ' ' | '\t' => {
                        // Skip intraline whitespace before line ending
                        while !self.is_at_end()
                            && (self.current_char() == ' ' || self.current_char() == '\t')
                        {
                            self.advance();
                        }
                        if self.current_char() == '\n' || self.current_char() == '\r' {
                            if self.current_char() == '\r' && self.peek_char() == Some('\n') {
                                self.advance();
                            }
                            self.advance();
                            // Skip leading whitespace on next line
                            while !self.is_at_end()
                                && (self.current_char() == ' ' || self.current_char() == '\t')
                            {
                                self.advance();
                            }
                            continue; // Don't push anything
                        } else {
                            return Err(LexError::InvalidEscapeInString(
                                "backslash-space not followed by line ending".to_string(),
                            ));
                        }
                    }
                    c => {
                        return Err(LexError::InvalidEscapeInString(format!("\\{}", c)));
                    }
                };
                result.push(escaped);
            } else {
                result.push(self.current_char());
            }
            self.advance();
        }

        if self.is_at_end() {
            return Err(LexError::UnterminatedString);
        }

        self.advance(); // consume closing "
        Ok(Token::String(result))
    }

    fn read_vertical_bar_identifier(&mut self) -> Result<Token, LexError> {
        self.advance(); // consume opening |
        let mut result = String::new();

        while !self.is_at_end() && self.current_char() != '|' {
            if self.current_char() == '\\' {
                self.advance();
                if self.is_at_end() {
                    return Err(LexError::UnterminatedVerticalBarIdentifier);
                }
                let escaped = match self.current_char() {
                    // R7RS mnemonic escapes
                    'a' => '\u{0007}', // alarm
                    'b' => '\u{0008}', // backspace
                    't' => '\t',       // tab
                    'n' => '\n',       // newline
                    'r' => '\r',       // return
                    '\\' => '\\',      // backslash
                    '|' => '|',        // vertical bar
                    '"' => '"',        // double quote
                    // Inline hex escape: \x<hex>;
                    'x' => {
                        self.advance();
                        let mut hex_str = String::new();
                        while !self.is_at_end() && self.current_char() != ';' {
                            hex_str.push(self.current_char());
                            self.advance();
                        }
                        if self.current_char() != ';' {
                            return Err(LexError::InvalidEscapeInIdentifier(format!(
                                "x{} (missing semicolon)",
                                hex_str
                            )));
                        }
                        match u32::from_str_radix(&hex_str, 16) {
                            Ok(code) => match char::from_u32(code) {
                                Some(ch) => ch,
                                None => {
                                    return Err(LexError::InvalidEscapeInIdentifier(format!(
                                        "x{};",
                                        hex_str
                                    )));
                                }
                            },
                            Err(_) => {
                                return Err(LexError::InvalidEscapeInIdentifier(format!(
                                    "x{};",
                                    hex_str
                                )));
                            }
                        }
                    }
                    c => {
                        return Err(LexError::InvalidEscapeInIdentifier(c.to_string()));
                    }
                };
                result.push(escaped);
            } else {
                result.push(self.current_char());
            }
            self.advance();
        }

        if self.is_at_end() {
            return Err(LexError::UnterminatedVerticalBarIdentifier);
        }

        self.advance(); // consume closing |
        Ok(Token::Identifier(result))
    }

    fn read_hash_syntax(&mut self) -> Result<Token, LexError> {
        self.advance(); // consume #

        match self.current_char() {
            't' | 'T' => {
                self.advance();
                // R7RS long form #true
                self.consume_ascii_suffix("rue");
                Ok(Token::Boolean(true))
            }
            'f' | 'F' => {
                self.advance();
                // R7RS long form #false
                self.consume_ascii_suffix("alse");
                Ok(Token::Boolean(false))
            }
            '\\' => self.read_character(),
            '(' => {
                self.advance();
                self.open_delimiters.push(')');
                Ok(Token::VectorOpen)
            }
            // `#u8(` is R7RS; `#vu8(` is R6RS's spelling of the same thing.
            // Reading both costs one character of lookahead and is not
            // ambiguous with anything R7RS admits.
            'u' => self.read_bytevector_open(),
            'v' if !self.allow_r6rs => Err(LexError::R6rsSyntax { syntax: "#vu8(" }),
            'v' => {
                self.advance();
                if self.current_char() == 'u' {
                    self.read_bytevector_open()
                } else {
                    Err(LexError::UnexpectedChar(self.current_char()))
                }
            }
            // R7RS numeric prefixes: #e #i #b #o #d #x
            'e' | 'E' | 'i' | 'I' | 'b' | 'B' | 'o' | 'O' | 'd' | 'D' | 'x' | 'X' => {
                self.read_number_with_prefix()
            }
            // R7RS datum comment: #; comments out the next datum
            ';' => {
                self.advance(); // consume ;
                Ok(Token::DatumComment)
            }
            // R7RS reader directives: #!fold-case, #!no-fold-case
            '!' => {
                self.advance(); // consume !
                self.read_reader_directive()
            }
            // R7RS datum labels: #n= (definition) and #n# (reference)
            '0'..='9' => self.read_datum_label(),
            _ => Err(LexError::UnexpectedChar(self.current_char())),
        }
    }

    /// Do the characters at `offset` from here spell `text`, ASCII
    /// case-insensitively? Never allocates and never reads past the end.
    fn matches_ascii_at(&self, offset: usize, text: &str) -> bool {
        let start = self.position + offset;
        let Some(window) = self.input.get(start..start + text.len()) else {
            return false;
        };
        window
            .iter()
            .zip(text.chars())
            .all(|(c, t)| c.eq_ignore_ascii_case(&t))
    }

    /// If the upcoming characters spell `suffix` (ASCII case-insensitive),
    /// consume them. Used for the long boolean forms #true and #false, whose
    /// suffixes must be consumed with the token rather than left in the
    /// input as a stray identifier.
    fn consume_ascii_suffix(&mut self, suffix: &str) {
        if self.matches_ascii_at(0, suffix) {
            for _ in 0..suffix.len() {
                self.advance();
            }
        }
    }

    /// Read a datum label (#n=) or datum reference (#n#)
    /// R7RS Section 2.4: Datum labels for shared/cyclic structures
    fn read_datum_label(&mut self) -> Result<Token, LexError> {
        // We're positioned at the first digit after #
        let start = self.position;

        // Read all digits
        while !self.is_at_end() && self.current_char().is_ascii_digit() {
            self.advance();
        }

        // Parse the label number
        let label_str: String = self.input[start..self.position].iter().collect();
        let label: usize = label_str
            .parse()
            .map_err(|_| LexError::InvalidNumber(format!("Invalid datum label: {}", label_str)))?;

        // Check what follows: = for definition, # for reference
        if self.is_at_end() {
            return Err(LexError::UnexpectedChar('\0'));
        }

        match self.current_char() {
            '=' => {
                self.advance(); // consume =
                Ok(Token::DatumLabel(label))
            }
            '#' => {
                self.advance(); // consume #
                Ok(Token::DatumRef(label))
            }
            ch => Err(LexError::UnexpectedChar(ch)),
        }
    }

    /// Skip to the end of the current line — the rest of a `;` comment or
    /// a shebang line. R7RS 7.1.1's line endings are newline, return, and
    /// return+newline, so a bare return ends the line too: `;c\rnot` is a
    /// comment and then a datum. The ending itself is left for the
    /// whitespace loop.
    fn skip_to_line_ending(&mut self) {
        while !self.is_at_end() && !matches!(self.current_char(), '\n' | '\r') {
            self.advance();
        }
    }

    /// Read a reader directive like #!fold-case or #!no-fold-case
    /// These directives affect subsequent lexing but don't produce tokens themselves
    fn read_reader_directive(&mut self) -> Result<Token, LexError> {
        // A shebang (`#!/usr/bin/env patina`) is not a reader directive:
        // `#!` followed by `/` or a space comments out the rest of the line,
        // so an installed script runs. `#!fold-case` is unaffected — a
        // directive name follows its `#!` immediately.
        if !self.is_at_end() && matches!(self.current_char(), '/' | ' ') {
            self.skip_to_line_ending();
            self.skip_whitespace_and_comments()?;
            return self.lex_token();
        }

        // Read the directive name (until whitespace or delimiter)
        let start = self.position;
        while !self.is_at_end() && !Self::is_delimiter(self.current_char()) {
            self.advance();
        }

        let directive: String = self.input[start..self.position].iter().collect();

        match directive.to_lowercase().as_str() {
            "fold-case" => {
                self.fold_case = true;
                // Directive consumed, skip whitespace and get next token
                self.skip_whitespace_and_comments()?;
                self.lex_token()
            }
            "no-fold-case" => {
                self.fold_case = false;
                // Directive consumed, skip whitespace and get next token
                self.skip_whitespace_and_comments()?;
                self.lex_token()
            }
            _ => {
                // Unknown directive - R7RS says implementations may support others
                // For now, just ignore unknown directives and continue
                self.skip_whitespace_and_comments()?;
                self.lex_token()
            }
        }
    }

    fn read_character(&mut self) -> Result<Token, LexError> {
        self.advance(); // consume \

        if self.is_at_end() {
            return Err(LexError::InvalidCharacter);
        }

        // R7RS: <character> -> #\<any character>, so the first character
        // after #\ is always part of the literal even when it is itself a
        // delimiter (#\(, #\), #\ , #\;). No named (#\space) or hex (#\x41)
        // literal starts with a delimiter, so a delimiter first character is
        // always a single-character literal; otherwise keep scanning to
        // capture a possible multi-character name.
        let start = self.position;
        let first = self.current_char();
        self.advance();
        if !Self::is_delimiter(first) {
            while !self.is_at_end() && !Self::is_delimiter(self.current_char()) {
                self.advance();
            }
        }

        let char_str: String = self.input[start..self.position].iter().collect();

        let ch = match char_str.as_str() {
            "space" => ' ',
            "newline" => '\n',
            "tab" => '\t',
            // R7RS named characters
            "alarm" => '\u{0007}',
            "backspace" => '\u{0008}',
            "delete" => '\u{007F}',
            "escape" => '\u{001B}',
            "null" => '\u{0000}',
            "return" => '\r',
            // Check for single character (use char count, not byte length!)
            s if s.chars().count() == 1 => s.chars().next().unwrap(),
            // Check for hex scalar value: #\x03BB (lambda)
            s if s.starts_with('x') => {
                let hex_str = &s[1..];
                match u32::from_str_radix(hex_str, 16) {
                    Ok(code) => match char::from_u32(code) {
                        Some(ch) => ch,
                        None => return Err(LexError::InvalidCharacter),
                    },
                    Err(_) => return Err(LexError::InvalidCharacter),
                }
            }
            _ => return Err(LexError::InvalidCharacter),
        };

        Ok(Token::Character(ch))
    }

    fn read_number(&mut self) -> Result<Token, LexError> {
        let start = self.position;

        while !self.is_at_end() && !Self::is_delimiter(self.current_char()) {
            self.advance();
        }

        let num_str: String = self.input[start..self.position].iter().collect();
        Ok(Token::Number(num_str))
    }

    fn read_number_with_prefix(&mut self) -> Result<Token, LexError> {
        // We're at the first prefix character (e, i, b, o, d, or x)
        // We need to go back to include the # that was consumed in read_hash_syntax
        let start = self.position - 1; // -1 to include the #

        // Read through all prefixes and the number
        // R7RS allows combinations like #e#x10, #i#b1010, etc.
        while !self.is_at_end() && !Self::is_delimiter(self.current_char()) {
            self.advance();
        }

        let num_str: String = self.input[start..self.position].iter().collect();
        Ok(Token::Number(num_str))
    }

    /// Whether `ch` can begin an identifier.
    ///
    /// Wider than R7RS 7.1.1's <initial> in three deliberate places: any
    /// non-ASCII character that is not whitespace (7.1.1 admits only a-zA-Z),
    /// `.`, and `@`. All three are pure widenings — no conforming program
    /// contains a token they admit — so none can change an existing program's
    /// meaning. The reasoning for the non-ASCII rule, and what the three
    /// reference implementations do, is in `PRD/ARCHIVE/TRACK_L_FIXED_
    /// DEFECTS.md`; `@` is in `PRD/ARCHIVE/TRACK_L_FIXED_DEFECTS.md`, with
    /// cases in `crates/patina-tests/tests/scheme/reader/at-identifiers.scm`.
    ///
    /// Whitespace is the one exclusion, and the one place Patina is *stricter*
    /// than every reference: chibi welds `a<U+00A0>b` into one symbol, Gauche
    /// and Chez both split it, and we reject it. `char::is_whitespace` (Unicode
    /// White_Space) keeps it out of identifiers while
    /// [`Self::skip_whitespace_and_comments`] skips only R7RS's five ASCII
    /// spaces, so it can be neither, and a stray non-breaking space is a typo
    /// worth seeing rather than silently welding or splitting a token.
    ///
    /// The writer deliberately does *not* track this set:
    /// `symbol_needs_vertical_bars` (patina-primitives) stays strict, so `@`
    /// writes back as `|@|`. It answers the other question — "would every R7RS
    /// reader accept this bare?", not "will we read it?" — and erring toward
    /// bars is always safe. Widening here must not widen it.
    fn is_identifier_start(ch: char) -> bool {
        if !ch.is_ascii() {
            return !ch.is_whitespace();
        }
        ch.is_ascii_alphabetic()
            || matches!(
                ch,
                '!' | '$'
                    | '%'
                    | '&'
                    | '*'
                    | '/'
                    | ':'
                    | '<'
                    | '='
                    | '>'
                    | '?'
                    | '^'
                    | '_'
                    | '~'
                    | '+'
                    | '-'
                    | '.'
                    | '@'
            )
    }

    fn read_identifier(&mut self) -> Result<Token, LexError> {
        let start = self.position;

        while !self.is_at_end() && !Self::is_delimiter(self.current_char()) {
            self.advance();
        }

        let ident: String = self.input[start..self.position].iter().collect();

        // Apply case folding if #!fold-case directive is active
        let ident = if self.fold_case {
            ident.to_lowercase()
        } else {
            ident
        };

        Ok(Token::Identifier(ident))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The token kinds a fed lexer produces, once it has them.
    fn fed(pieces: &[&str]) -> Vec<Token> {
        let mut lexer = Lexer::feedable(false);
        let mut tokens = Vec::new();
        for piece in pieces {
            lexer.feed(piece);
            while let Some(spanned) = lexer.next_fed_token().expect("lexes") {
                tokens.push(spanned.token);
            }
        }
        tokens
    }

    /// A token running to the end of the text that has arrived may be
    /// continued by the text that has not, so the lexer waits for it rather
    /// than splitting it in two.
    #[test]
    fn a_fed_lexer_waits_at_the_end_of_the_text() {
        assert_eq!(fed(&["foo"]), vec![], "foo may still become foobar");
        assert_eq!(
            fed(&["foo", "bar "]),
            vec![Token::Identifier("foobar".into())]
        );
        assert_eq!(
            fed(&[",", "@x "]),
            vec![Token::UnquoteSplicing, Token::Identifier("x".into())]
        );
        assert_eq!(fed(&["#t", "rue "]), vec![Token::Boolean(true)]);
        assert_eq!(fed(&["\"ab", "cd\" "]), vec![Token::String("abcd".into())]);
        assert_eq!(
            fed(&["#| a ", "|# x "]),
            vec![Token::Identifier("x".into())]
        );
        assert_eq!(
            fed(&["(display ", "1)\n"]),
            vec![
                Token::LeftParen,
                Token::Identifier("display".into()),
                Token::Number("1".into()),
                Token::RightParen
            ],
            "a form finished before the end of its line is read at once"
        );
    }

    /// A token that may span any number of lines is scanned on for its end as
    /// the text arrives, and read once when the end is here — including when
    /// what marks the end is split between one piece of text and the next.
    #[test]
    fn a_token_that_spans_the_text_is_scanned_on_for_its_end() {
        assert_eq!(
            fed(&["\"a", "b", "c\" "]),
            vec![Token::String("abc".into())],
            "a string over three pieces"
        );
        assert_eq!(
            fed(&["\"a\\", "\" b\" "]),
            vec![Token::String("a\" b".into())],
            "an escape split from the quote it escapes"
        );
        assert_eq!(
            fed(&["|a", "b| "]),
            vec![Token::Identifier("ab".into())],
            "a |symbol| over two pieces"
        );
        assert_eq!(
            fed(&["#| x |", "# y "]),
            vec![Token::Identifier("y".into())],
            "a block comment whose closing pair is split"
        );
        assert_eq!(
            fed(&["#| a #", "| b |", "# c |# d "]),
            vec![Token::Identifier("d".into())],
            "a nested block comment, both pairs split"
        );
    }

    /// Nothing more is coming, so the last token is finished where the text
    /// is.
    #[test]
    fn the_end_of_the_text_finishes_the_last_token() {
        let mut lexer = Lexer::feedable(false);
        lexer.feed("foo");
        assert!(lexer.next_fed_token().unwrap().is_none());
        lexer.no_more_text();
        assert_eq!(
            lexer.next_fed_token().unwrap().map(|s| s.token),
            Some(Token::Identifier("foo".into()))
        );
        assert_eq!(
            lexer.next_fed_token().unwrap().map(|s| s.token),
            Some(Token::Eof)
        );
    }

    /// A rewound token puts back what reading it changed: `#!fold-case` for
    /// the text that follows, and the stack a closer is checked against.
    #[test]
    fn rewinding_a_token_puts_back_what_it_changed() {
        assert_eq!(
            fed(&["#!fold-case ABC", " "]),
            vec![Token::Identifier("abc".into())],
            "the directive applies to the identifier it was rewound with"
        );

        let mut lexer = Lexer::feedable(true);
        lexer.feed("(a]");
        assert_eq!(
            lexer.next_fed_token().unwrap().map(|s| s.token),
            Some(Token::LeftParen)
        );
        assert_eq!(
            lexer.next_fed_token().unwrap().map(|s| s.token),
            Some(Token::Identifier("a".into()))
        );
        assert!(
            lexer.next_fed_token().unwrap().is_none(),
            "] may not be its end"
        );
        lexer.feed(" ");
        assert!(
            lexer.next_fed_token().is_err(),
            "the ] still closes the ( it was rewound over"
        );
    }

    /// `is_special_float_literal` decides whether `+`/`-` starts a number or
    /// an identifier. It had no test of its own, which is how it kept a
    /// quadratic implementation — the behaviour was covered only indirectly,
    /// through programs that happened to contain `+inf.0`.
    ///
    /// The `inf` spellings are here for completeness but do not exercise this
    /// predicate: `peek_is_imaginary` claims any `+`/`-` followed by `i`
    /// first. `parser::tests::test_parse_special_floats_case_insensitive`
    /// covers a superset of these end to end; this test localizes a failure
    /// to the lexer.
    #[test]
    fn test_special_float_literals_lex_as_numbers() {
        for src in ["+nan.0", "-nan.0", "+NaN.0", "-nAn.0", "+inf.0", "-Inf.0"] {
            assert_eq!(
                Lexer::new(src).next_token_kind().unwrap(),
                Token::Number(src.to_string()),
                "{src} should lex as a number"
            );
        }
    }

    /// The near-misses have to stay identifiers. These are the cases that
    /// actually reach the predicate and fail it: on the last character
    /// (`-nan.1`), on the letters (`+na`, `->foo`), and on running out of
    /// input mid-pattern (`-nan`, `-`, `+`) — which is also the bounds check,
    /// since a short buffer must return false rather than read past the end.
    #[test]
    fn test_near_miss_special_floats_lex_as_identifiers() {
        for src in ["-nan", "+na", "-nan.1", "-", "+", "->foo", "-x"] {
            assert_eq!(
                Lexer::new(src).next_token_kind().unwrap(),
                Token::Identifier(src.to_string()),
                "{src} should lex as an identifier"
            );
        }
    }

    /// Lexing must stay linear in input size.
    ///
    /// Nothing else here pins the *complexity*, and this file has already
    /// carried one accidental quadratic: `is_special_float_literal` copied the
    /// rest of the input on every `+`/`-` token that was not a number, and
    /// every behavioural test passed throughout. Measured on this input:
    /// ~10 ms linear against 5.9 s with that version restored, so the 2 s
    /// bound sits ~200× above a healthy run and ~3× below a regressed one.
    /// An absolute bound rather than a ratio, following `gc_tree_walker.rs`'s
    /// idiom — it fires only on a complexity change, never on a slow machine.
    #[test]
    fn test_lexing_is_linear_in_input_size() {
        let source = "(define (f a b) (- a b))\n".repeat(16_000);
        let start = std::time::Instant::now();
        let mut lexer = Lexer::new(&source);
        let mut tokens = 0usize;
        while !matches!(lexer.next_token_kind().unwrap(), Token::Eof) {
            tokens += 1;
        }
        let elapsed = start.elapsed();
        // 13 tokens per form: ( define ( f a b ) ( - a b ) )
        assert_eq!(tokens, 16_000 * 13);
        assert!(
            elapsed.as_millis() < 2_000,
            "lexing {} forms took {elapsed:?} — the lexer is likely quadratic again",
            16_000
        );
    }

    #[test]
    fn test_basic_tokens() {
        let mut lexer = Lexer::new("(+ 1 2)");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("+".to_string())
        );
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("1".to_string())
        );
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("2".to_string())
        );
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_leading_at_sign_starts_an_identifier() {
        // Extension beyond R7RS 7.1.1 — see `is_identifier_start`.
        let mut lexer = Lexer::new("(@ @raw x@y)");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Identifier(s) if s == "@"));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Identifier(s) if s == "@raw"));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Identifier(s) if s == "x@y"));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_unquote_splicing_still_wins_over_at_identifier() {
        // `,@` is one token, so an `@` identifier can never be read out of it.
        let mut lexer = Lexer::new(",@x ,@ , @");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::UnquoteSplicing);
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Identifier(s) if s == "x"));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::UnquoteSplicing);
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Unquote);
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Identifier(s) if s == "@"));
    }

    #[test]
    fn test_polar_complex_is_still_a_number() {
        // `@` is only an identifier *start*; a leading digit still reads as
        // the R7RS <real> @ <real> polar notation.
        let mut lexer = Lexer::new("1@2");
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == "1@2"));
    }

    #[test]
    fn test_character_literal_paren_chars() {
        // R7RS <character> -> #\<any character>: the character may itself
        // be a delimiter
        let mut lexer = Lexer::new("#\\(");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('('));

        let mut lexer = Lexer::new("#\\)");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character(')'));

        // In call position, followed by a closing paren
        let mut lexer = Lexer::new("(list #\\( #\\))");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Identifier(s) if s == "list"));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('('));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character(')'));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_character_literal_whitespace_chars() {
        // #\ followed by a literal space is the space character
        let mut lexer = Lexer::new("#\\ ");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character(' '));

        // #\ followed by a literal newline is the newline character
        let mut lexer = Lexer::new("#\\\n42");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('\n'));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == "42"));

        // Maze-benchmark style: a list of drawing characters
        let mut lexer = Lexer::new("(#\\  #\\_ #\\/ #\\\\)");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character(' '));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('_'));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('/'));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('\\'));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_character_literal_other_delimiter_chars() {
        let mut lexer = Lexer::new("#\\;");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character(';'));

        let mut lexer = Lexer::new("#\\\"");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('"'));
    }

    #[test]
    fn test_character_literal_terminated_by_comment_or_string() {
        // A comment or string directly after the literal delimits it
        let mut lexer = Lexer::new("#\\a; comment");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('a'));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Eof);

        let mut lexer = Lexer::new("#\\space; comment");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character(' '));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Eof);

        let mut lexer = Lexer::new("#\\a\"s\"");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('a'));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::String(s) if s == "s"));
    }

    #[test]
    fn test_character_literal_named_and_hex_still_work() {
        let mut lexer = Lexer::new("#\\space #\\newline #\\tab #\\x41 #\\x3BB");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character(' '));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('\n'));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('\t'));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('A'));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('λ'));
    }

    #[test]
    fn test_brackets_are_refused_by_default() {
        // `Lexer::new` follows the ambient setting, which is R7RS unless a
        // person asked otherwise; `reading_r6rs(true)` is what the rest of these
        // cases use to opt in without touching a process-wide variable.
        let mut lexer = Lexer::new("[a]");
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::R6rsSyntax { .. })
        ));

        let mut lexer = Lexer::new("#vu8(1)");
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::R6rsSyntax { .. })
        ));
    }

    #[test]
    fn test_square_brackets_read_as_parentheses() {
        let mut lexer = Lexer::new("[a]").reading_r6rs(true);
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("a".to_string())
        );
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Eof);
    }

    #[test]
    fn test_brackets_delimit_the_token_before_them() {
        // The reason `is_delimiter` had to widen: without `]` in the set the
        // number reader swallows it and rejects `1]` as a malformed number.
        let mut lexer = Lexer::new("[x 1][y 2]").reading_r6rs(true);
        for expected in [
            Token::LeftParen,
            Token::Identifier("x".to_string()),
            Token::Number("1".to_string()),
            Token::RightParen,
            Token::LeftParen,
            Token::Identifier("y".to_string()),
            Token::Number("2".to_string()),
            Token::RightParen,
        ] {
            assert_eq!(lexer.next_token_kind().unwrap(), expected);
        }
    }

    #[test]
    fn test_mismatched_delimiters_are_rejected() {
        let mut lexer = Lexer::new("[a)").reading_r6rs(true);
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("a".to_string())
        );
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::MismatchedDelimiter {
                expected: ']',
                closed: ')'
            })
        ));

        let mut lexer = Lexer::new("(a]").reading_r6rs(true);
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("a".to_string())
        );
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::MismatchedDelimiter {
                expected: ')',
                closed: ']'
            })
        ));
    }

    #[test]
    fn test_vector_and_bytevector_openers_close_with_a_paren() {
        // `#(` and `#u8(` open with a parenthesis whatever precedes it, so a
        // `]` must not close them.
        for src in ["#(1]", "#u8(1]"] {
            let mut lexer = Lexer::new(src).reading_r6rs(true);
            lexer.next_token_kind().unwrap();
            lexer.next_token_kind().unwrap();
            assert!(
                matches!(
                    lexer.next_token_kind(),
                    Err(LexError::MismatchedDelimiter { closed: ']', .. })
                ),
                "{src} should not accept a bracket as its closer"
            );
        }
    }

    #[test]
    fn test_unmatched_closer_is_left_to_the_parser() {
        // The REPL lexes incomplete input while a form is still being typed,
        // so a closer with nothing open must not be a lexer error.
        let mut lexer = Lexer::new(")]").reading_r6rs(true);
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_r6rs_bytevector_syntax() {
        let mut lexer = Lexer::new("#vu8(1 2)").reading_r6rs(true);
        assert_eq!(lexer.next_token_kind().unwrap(), Token::BytevectorOpen);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("1".to_string())
        );
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("2".to_string())
        );
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);

        // `#v` not followed by `u8(` is still an error.
        let mut lexer = Lexer::new("#vx").reading_r6rs(true);
        assert!(lexer.next_token_kind().is_err());
    }

    #[test]
    fn test_bracket_character_literals_are_unaffected() {
        // `read_character` takes a delimiter first character as a complete
        // one-character literal, so widening `is_delimiter` leaves these be.
        let mut lexer = Lexer::new(r"#\[ #\]");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character('['));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Character(']'));
    }

    #[test]
    fn test_reject_reserved_characters() {
        // R7RS reserves { } for future extensions; [ ] are read as
        // parentheses — see test_square_brackets_read_as_parentheses.
        let mut lexer = Lexer::new("{");
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::ReservedCharacter('{'))
        ));

        let mut lexer = Lexer::new("}");
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::ReservedCharacter('}'))
        ));
    }

    #[test]
    fn test_long_boolean_forms() {
        // R7RS: #true and #false are the long forms of #t and #f; the whole
        // spelling must be consumed, not just the first two characters
        let mut lexer = Lexer::new("#true 6");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Boolean(true));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == "6"));

        let mut lexer = Lexer::new("#false\"8\"");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Boolean(false));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::String(s) if s == "8"));

        // Short forms unchanged
        let mut lexer = Lexer::new("#t #f");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Boolean(true));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Boolean(false));
    }

    #[test]
    fn test_vertical_bar_identifier_basic() {
        let mut lexer = Lexer::new("|hello world|");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("hello world".to_string())
        );
    }

    #[test]
    fn test_vertical_bar_identifier_empty() {
        // R7RS: || is a valid identifier
        let mut lexer = Lexer::new("||");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("".to_string())
        );
    }

    #[test]
    fn test_vertical_bar_identifier_with_special_chars() {
        let mut lexer = Lexer::new("|(hello world!)|");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("(hello world!)".to_string())
        );
    }

    #[test]
    fn test_vertical_bar_identifier_with_escapes() {
        // Test \| escape
        let mut lexer = Lexer::new("|foo\\|bar|");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("foo|bar".to_string())
        );

        // Test \t escape
        let mut lexer = Lexer::new("|\\t\\t|");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("\t\t".to_string())
        );

        // Test \n escape
        let mut lexer = Lexer::new("|foo\\nbar|");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("foo\nbar".to_string())
        );

        // Test \a (alarm) escape
        let mut lexer = Lexer::new("|\\a|");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("\u{0007}".to_string())
        );

        // Test \b (backspace) escape
        let mut lexer = Lexer::new("|\\b|");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("\u{0008}".to_string())
        );
    }

    #[test]
    fn test_vertical_bar_identifier_with_hex_escape() {
        // R7RS example: |H\x65;llo| == Hello
        let mut lexer = Lexer::new("|H\\x65;llo|");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("Hello".to_string())
        );

        // R7RS example: |\x3BB;| == λ
        let mut lexer = Lexer::new("|\\x3BB;|");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("λ".to_string())
        );

        // R7RS example: |\x9;\x9;| == two tabs
        let mut lexer = Lexer::new("|\\x9;\\x9;|");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("\t\t".to_string())
        );
    }

    #[test]
    fn test_vertical_bar_identifier_unterminated() {
        let mut lexer = Lexer::new("|hello");
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::UnterminatedVerticalBarIdentifier)
        ));
    }

    #[test]
    fn test_vertical_bar_identifier_invalid_escape() {
        let mut lexer = Lexer::new("|foo\\q|");
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::InvalidEscapeInIdentifier(_))
        ));
    }

    #[test]
    fn test_vertical_bar_identifier_in_expression() {
        let mut lexer = Lexer::new("(|foo bar| |hello world|)");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("foo bar".to_string())
        );
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("hello world".to_string())
        );
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_block_comment_basic() {
        let mut lexer = Lexer::new("#| this is a comment |# 42");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("42".to_string())
        );
    }

    #[test]
    fn test_block_comment_nested() {
        let mut lexer = Lexer::new("#| outer #| inner |# outer |# 42");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("42".to_string())
        );
    }

    #[test]
    fn test_block_comment_with_code() {
        let mut lexer = Lexer::new("(+ #| comment |# 1 2)");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("+".to_string())
        );
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("1".to_string())
        );
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("2".to_string())
        );
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_block_comment_multiline() {
        let mut lexer = Lexer::new("#|\nline 1\nline 2\n|# 42");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("42".to_string())
        );
    }

    #[test]
    fn test_block_comment_unterminated() {
        let mut lexer = Lexer::new("#| this is unterminated");
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::UnterminatedBlockComment)
        ));
    }

    #[test]
    fn test_block_comment_unterminated_nested() {
        let mut lexer = Lexer::new("#| outer #| inner |# outer");
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::UnterminatedBlockComment)
        ));
    }

    #[test]
    fn test_decimal_point_number() {
        // R7RS: Numbers can start with a decimal point (e.g., .3 is 0.3)
        let mut lexer = Lexer::new(".3");
        let token = lexer.next_token_kind().unwrap();
        assert!(
            matches!(token, Token::Number(ref s) if s == ".3"),
            "Expected Number('.3'), got {:?}",
            token
        );
    }

    #[test]
    fn test_decimal_point_number_in_expression() {
        // Test that .1, .2, .3 are all parsed as numbers in an expression
        let mut lexer = Lexer::new("(+ .1 .2 .3)");
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::LeftParen));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Identifier(s) if s == "+"));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == ".1"));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == ".2"));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == ".3"));
        assert!(matches!(
            lexer.next_token_kind().unwrap(),
            Token::RightParen
        ));
    }

    #[test]
    fn test_dot_vs_decimal_number() {
        // A lone dot followed by whitespace/delimiter is Token::Dot
        let mut lexer = Lexer::new("(a . b)");
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::LeftParen));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Identifier(s) if s == "a"));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Dot));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Identifier(s) if s == "b"));
        assert!(matches!(
            lexer.next_token_kind().unwrap(),
            Token::RightParen
        ));
    }

    #[test]
    fn test_negative_decimal_point_number() {
        // R7RS: -.1 should parse as -0.1, not as a symbol
        let mut lexer = Lexer::new("-.1");
        let token = lexer.next_token_kind().unwrap();
        assert!(
            matches!(token, Token::Number(ref s) if s == "-.1"),
            "Expected Number('-.1'), got {:?}",
            token
        );
    }

    #[test]
    fn test_positive_decimal_point_number() {
        // +.5 should parse as +0.5
        let mut lexer = Lexer::new("+.5");
        let token = lexer.next_token_kind().unwrap();
        assert!(
            matches!(token, Token::Number(ref s) if s == "+.5"),
            "Expected Number('+.5'), got {:?}",
            token
        );
    }

    #[test]
    fn test_negative_decimal_in_expression() {
        // Test -.1 in an expression
        let mut lexer = Lexer::new("(+ -.1 -.2)");
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::LeftParen));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Identifier(s) if s == "+"));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == "-.1"));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == "-.2"));
        assert!(matches!(
            lexer.next_token_kind().unwrap(),
            Token::RightParen
        ));
    }

    #[test]
    fn test_number_terminated_by_line_comment() {
        // R7RS: `;` is a delimiter — a comment directly after a number must
        // not be absorbed into the number token (seen in the wild as `0.5714;;`)
        let mut lexer = Lexer::new("0.5714;; comment\n42");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("0.5714".to_string())
        );
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("42".to_string())
        );
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Eof);
    }

    #[test]
    fn test_integer_terminated_by_line_comment_in_list() {
        let mut lexer = Lexer::new("(maker 0;; coef-zero\n 1)");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Identifier(s) if s == "maker"));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == "0"));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == "1"));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_number_terminated_by_string() {
        // `"` is also a delimiter, matching the identifier lexing rules
        let mut lexer = Lexer::new("1\"a\"");
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == "1"));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::String(s) if s == "a"));
    }

    #[test]
    fn test_prefixed_number_terminated_by_line_comment() {
        let mut lexer = Lexer::new("#x10; comment\n#b101;");
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == "#x10"));
        assert!(matches!(lexer.next_token_kind().unwrap(), Token::Number(s) if s == "#b101"));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Eof);
    }

    // ========== String Escape Sequence Tests ==========

    #[test]
    fn test_string_basic_escapes() {
        let mut lexer = Lexer::new(r#""hello\nworld""#);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::String("hello\nworld".to_string())
        );
    }

    #[test]
    fn test_string_all_mnemonic_escapes() {
        // R7RS mnemonic escapes: \a \b \t \n \r \\ \" \|
        let mut lexer = Lexer::new(r#""\a\b\t\n\r\\\""|""#);
        let token = lexer.next_token_kind().unwrap();
        match token {
            Token::String(s) => {
                assert_eq!(s, "\u{0007}\u{0008}\t\n\r\\\"");
            }
            _ => panic!("Expected String token"),
        }
    }

    #[test]
    fn test_string_hex_escape_basic() {
        // \x41; is 'A' (ASCII 65)
        let mut lexer = Lexer::new(r#""\x41;""#);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::String("A".to_string())
        );
    }

    #[test]
    fn test_string_hex_escape_in_context() {
        // "a\x41;b" should be "aAb"
        let mut lexer = Lexer::new(r#""a\x41;b""#);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::String("aAb".to_string())
        );
    }

    #[test]
    fn test_string_hex_escape_unicode() {
        // \x3BB; is Greek lowercase lambda (λ)
        let mut lexer = Lexer::new(r#""\x3BB;""#);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::String("λ".to_string())
        );
    }

    #[test]
    fn test_string_hex_escape_combining_char() {
        // \x0307; is combining dot above - used in İ case folding
        let mut lexer = Lexer::new(r#""i\x0307;""#);
        let token = lexer.next_token_kind().unwrap();
        match token {
            Token::String(s) => {
                assert_eq!(s.len(), 3); // 'i' (1 byte) + combining dot (2 bytes in UTF-8)
                assert_eq!(s.chars().count(), 2); // 2 Unicode characters
                let chars: Vec<char> = s.chars().collect();
                assert_eq!(chars[0], 'i');
                assert_eq!(chars[1], '\u{0307}');
            }
            _ => panic!("Expected String token"),
        }
    }

    #[test]
    fn test_string_hex_escape_missing_semicolon() {
        let mut lexer = Lexer::new(r#""\x41""#);
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::InvalidEscapeInString(_))
        ));
    }

    #[test]
    fn test_string_hex_escape_invalid_hex() {
        let mut lexer = Lexer::new(r#""\xGG;""#);
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::InvalidEscapeInString(_))
        ));
    }

    #[test]
    fn test_string_hex_escape_invalid_codepoint() {
        // U+D800 is a surrogate, not a valid Unicode scalar value
        let mut lexer = Lexer::new(r#""\xD800;""#);
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::InvalidEscapeInString(_))
        ));
    }

    #[test]
    fn test_string_line_continuation() {
        // R7RS: backslash-newline and any leading whitespace on next line is ignored
        let input = "\"hello\\\n    world\"";
        let mut lexer = Lexer::new(input);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::String("helloworld".to_string())
        );
    }

    #[test]
    fn test_string_invalid_escape() {
        // \q is not a valid escape sequence
        let mut lexer = Lexer::new(r#""\q""#);
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::InvalidEscapeInString(_))
        ));
    }

    // ========== Reader Directive Tests ==========

    #[test]
    fn test_fold_case_directive() {
        // #!fold-case causes identifiers to be lowercased
        let mut lexer = Lexer::new("#!fold-case ABC");
        let token = lexer.next_token_kind().unwrap();
        assert_eq!(token, Token::Identifier("abc".to_string()));
    }

    #[test]
    fn test_no_fold_case_directive() {
        // #!no-fold-case preserves case (this is the default)
        let mut lexer = Lexer::new("#!no-fold-case ABC");
        let token = lexer.next_token_kind().unwrap();
        assert_eq!(token, Token::Identifier("ABC".to_string()));
    }

    #[test]
    fn test_shebang_line_skipped() {
        // A leading shebang comments out its whole line, including arguments.
        let mut lexer = Lexer::new("#!/usr/bin/env patina\n(display 1)");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("display".to_string())
        );
    }

    #[test]
    fn test_shebang_with_space_skipped() {
        // `#! /usr/bin/env patina` — the space form some systems use.
        let mut lexer = Lexer::new("#! /usr/bin/env patina\n42");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("42".to_string())
        );
    }

    #[test]
    fn test_shebang_only_file_is_eof() {
        let mut lexer = Lexer::new("#!/usr/bin/env patina\n");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Eof);
    }

    #[test]
    fn test_fold_case_then_no_fold_case() {
        // #!fold-case followed by #!no-fold-case should preserve case
        let mut lexer = Lexer::new("#!fold-case #!no-fold-case ABC");
        let token = lexer.next_token_kind().unwrap();
        assert_eq!(token, Token::Identifier("ABC".to_string()));
    }

    #[test]
    fn test_fold_case_multiple_identifiers() {
        // #!fold-case affects all subsequent identifiers
        let mut lexer = Lexer::new("#!fold-case ABC DEF");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("abc".to_string())
        );
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("def".to_string())
        );
    }

    #[test]
    fn test_fold_case_in_list() {
        // #!fold-case works inside lists
        let mut lexer = Lexer::new("(#!fold-case ABC)");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("abc".to_string())
        );
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_fold_case_preserves_numbers() {
        // Numbers are not affected by fold-case (they're not identifiers)
        let mut lexer = Lexer::new("#!fold-case 42");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Number("42".to_string())
        );
    }

    #[test]
    fn test_fold_case_preserves_strings() {
        // Strings are not affected by fold-case
        let mut lexer = Lexer::new("#!fold-case \"ABC\"");
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::String("ABC".to_string())
        );
    }

    // Datum label tests
    #[test]
    fn test_datum_label_basic() {
        let mut lexer = Lexer::new("#0=");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::DatumLabel(0));
    }

    #[test]
    fn test_datum_ref_basic() {
        let mut lexer = Lexer::new("#0#");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::DatumRef(0));
    }

    #[test]
    fn test_datum_label_multi_digit() {
        let mut lexer = Lexer::new("#123=");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::DatumLabel(123));
    }

    #[test]
    fn test_datum_ref_multi_digit() {
        let mut lexer = Lexer::new("#42#");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::DatumRef(42));
    }

    #[test]
    fn test_datum_label_in_list() {
        let mut lexer = Lexer::new("(#0=(a b) #0#)");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert_eq!(lexer.next_token_kind().unwrap(), Token::DatumLabel(0));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("a".to_string())
        );
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("b".to_string())
        );
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
        assert_eq!(lexer.next_token_kind().unwrap(), Token::DatumRef(0));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_datum_label_cyclic() {
        // #0=(a b c . #0#)
        let mut lexer = Lexer::new("#0=(a . #0#)");
        assert_eq!(lexer.next_token_kind().unwrap(), Token::DatumLabel(0));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token_kind().unwrap(),
            Token::Identifier("a".to_string())
        );
        assert_eq!(lexer.next_token_kind().unwrap(), Token::Dot);
        assert_eq!(lexer.next_token_kind().unwrap(), Token::DatumRef(0));
        assert_eq!(lexer.next_token_kind().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_datum_label_invalid_no_terminator() {
        // #0 without = or # is invalid
        let mut lexer = Lexer::new("#0 ");
        assert!(lexer.next_token_kind().is_err());
    }
}
