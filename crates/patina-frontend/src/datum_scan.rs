//! A quick test for text in which no datum can have finished yet.
//!
//! A reader fed a line at a time asks after every line whether a datum has
//! finished. The parser answers exactly, but only by reading everything since
//! the datum began, so asking after each line of one long form takes time
//! proportional to the square of its length. [`DatumScan`] answers a weaker
//! question in constant time per character. It follows the lexer's tokens far
//! enough to know the nesting depth; whether it is inside a string, a
//! `|symbol|` or a block comment; and whether a prefix — `'`, `` ` ``, `,`,
//! `,@`, `#;` or a `#n=` label — is still waiting for its datum. From those it
//! says when no datum can have finished. When it cannot say that, the parser
//! decides.
//!
//! It must never say so of text in which a datum has finished, since that
//! would hold back a form whose input has all arrived; tests check it against
//! the parser over every short combination of the characters that matter to
//! it. It may fail to say so of text with a mistake in it, or of text whose
//! datum is unfinished in a way it does not follow, which costs only a read
//! that finds nothing finished yet.
//!
//! Tokens end where [`Lexer`] ends them, which is not always at a delimiter:
//! `#t`, `#false`, `,@`, `#0=` and `#0#` end on their own last character, so
//! what follows them starts a token of its own. A `|` there opens a symbol and
//! a `#|` a comment, as they would after a space.

use crate::lexer::Lexer;

/// Where a scan stands, as far as tokens are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Between tokens.
    Boundary,
    /// Inside a token that runs to the next delimiter — an identifier, a
    /// number, a character's name — and is a datum.
    Atom,
    /// Inside a `#!name` directive, which runs to the next delimiter and is
    /// not a datum.
    DirectiveName,
    /// After `#` where a token starts.
    Hash,
    /// After `#!`: a shebang line if `/` or a space follows.
    Directive,
    /// After `,`, which an `@` would make `,@`.
    Comma,
    /// After `#\`: the next character belongs to the literal, whatever it is.
    CharFirst,
    /// After `#t` or `#f`, having seen `matched` characters of `rest`, the
    /// remainder of `#true` or `#false`. The lexer takes the long form only
    /// when all of it is there.
    Boolean {
        rest: &'static str,
        matched: usize,
    },
    /// After `#` and a digit: more digits, then `=` for a label or `#` for a
    /// reference.
    Label,
    /// Part-way through `#u8(` or `#vu8(`, with `expect` still to come.
    Bytevector {
        expect: &'static str,
    },
    /// Inside a string or a `|symbol|`, which `close` ends.
    Quoted {
        close: char,
        escaped: bool,
    },
    LineComment,
    /// Inside `#| |#` comments nested `depth` deep. `after` is the previous
    /// character when it was a `#` or `|` that could pair with this one.
    Block {
        depth: usize,
        after: Option<char>,
    },
}

/// What a prefix does with the datum it is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prefix {
    /// `'`, `` ` ``, `,`, `,@` and `#n=` make a datum of it.
    Wraps,
    /// `#;` drops it.
    Drops,
}

/// Scan state for text arriving in pieces; see the module documentation.
#[derive(Debug, Clone)]
pub struct DatumScan {
    state: State,
    /// Openers not yet closed.
    depth: usize,
    /// Prefixes at the top level still waiting for their datum, innermost
    /// last. One inside a list does not matter: the list is open anyway.
    prefixes: Vec<Prefix>,
    /// Whether a datum has finished at the top level.
    finished: bool,
    /// Whether the R6RS syntax R7RS reserves is read, as the lexer decides.
    r6rs: bool,
}

impl DatumScan {
    /// A scan of text read with the R6RS syntax R7RS reserves (`[ ]`,
    /// `#vu8(`) or without it.
    pub fn new(r6rs: bool) -> Self {
        DatumScan {
            state: State::Boundary,
            depth: 0,
            prefixes: Vec::new(),
            finished: false,
            r6rs,
        }
    }

    /// Scan more of the text.
    pub fn feed(&mut self, text: &str) {
        for c in text.chars() {
            self.state = self.step(c);
        }
    }

    /// Whether no datum can have finished in the text scanned so far, so a
    /// parser reading it would find nothing to return yet. `false` means the
    /// parser has to decide.
    pub fn nothing_finished(&self) -> bool {
        !self.finished
            && (self.depth > 0
                || !self.prefixes.is_empty()
                || matches!(self.state, State::Quoted { .. } | State::Block { .. }))
    }

    fn step(&mut self, c: char) -> State {
        use State::*;
        match self.state {
            Boundary => self.token_start(c),
            Atom => self.in_atom(c),
            DirectiveName if Lexer::is_delimiter(c) => self.token_start(c),
            DirectiveName => DirectiveName,
            Hash => match c {
                // `skip_whitespace_and_comments` takes `#|` before a token.
                '|' => Block {
                    depth: 1,
                    after: None,
                },
                't' | 'T' => Boolean {
                    rest: "rue",
                    matched: 0,
                },
                'f' | 'F' => Boolean {
                    rest: "alse",
                    matched: 0,
                },
                '\\' => CharFirst,
                '(' => {
                    self.depth += 1;
                    Boundary
                }
                'u' => Bytevector { expect: "8(" },
                'v' if self.r6rs => Bytevector { expect: "u8(" },
                ';' => {
                    self.prefix(Prefix::Drops);
                    Boundary
                }
                '!' => Directive,
                '0'..='9' => Label,
                // A numeric prefix: the number runs to a delimiter.
                'e' | 'E' | 'i' | 'I' | 'b' | 'B' | 'o' | 'O' | 'd' | 'D' | 'x' | 'X' => Atom,
                // Anything else is a mistake the lexer reports.
                _ => self.token_start(c),
            },
            Directive => match c {
                '/' | ' ' => LineComment,
                // An empty name: an unknown directive, which is ignored.
                c if Lexer::is_delimiter(c) => self.token_start(c),
                _ => DirectiveName,
            },
            Comma => {
                self.prefix(Prefix::Wraps);
                if c == '@' {
                    Boundary
                } else {
                    self.token_start(c)
                }
            }
            CharFirst if Lexer::is_delimiter(c) => {
                self.datum();
                Boundary
            }
            CharFirst => Atom,
            Boolean { rest, matched } => {
                let next = rest[matched..].chars().next();
                if next.is_some_and(|r| r.eq_ignore_ascii_case(&c)) {
                    if matched + 1 == rest.len() {
                        self.datum();
                        Boundary
                    } else {
                        Boolean {
                            rest,
                            matched: matched + 1,
                        }
                    }
                } else {
                    // Not the long form: the boolean ended at its letter, and
                    // what looked like the rest of it starts an identifier.
                    self.datum();
                    if matched == 0 {
                        self.token_start(c)
                    } else {
                        self.in_atom(c)
                    }
                }
            }
            Label => match c {
                '0'..='9' => Label,
                '=' => {
                    self.prefix(Prefix::Wraps);
                    Boundary
                }
                '#' => {
                    self.datum();
                    Boundary
                }
                _ => self.token_start(c),
            },
            Bytevector { expect } if expect.starts_with(c) => {
                let expect = &expect[c.len_utf8()..];
                if expect.is_empty() {
                    self.depth += 1;
                    Boundary
                } else {
                    Bytevector { expect }
                }
            }
            Bytevector { .. } => self.token_start(c),
            Quoted {
                close,
                escaped: true,
            } => Quoted {
                close,
                escaped: false,
            },
            Quoted { close, .. } if c == '\\' => Quoted {
                close,
                escaped: true,
            },
            Quoted { close, .. } if c == close => {
                self.datum();
                Boundary
            }
            Quoted { .. } => self.state,
            LineComment if c == '\n' || c == '\r' => Boundary,
            LineComment => LineComment,
            // Mirrors `skip_block_comment`, which takes `#|` before `|#`.
            Block {
                depth,
                after: Some('#'),
            } if c == '|' => Block {
                depth: depth + 1,
                after: None,
            },
            Block {
                depth: 1,
                after: Some('|'),
            } if c == '#' => Boundary,
            Block {
                depth,
                after: Some('|'),
            } if c == '#' => Block {
                depth: depth - 1,
                after: None,
            },
            Block { depth, .. } => Block {
                depth,
                after: matches!(c, '#' | '|').then_some(c),
            },
        }
    }

    /// A character where a token may start, mirroring `lex_token`.
    fn token_start(&mut self, c: char) -> State {
        match c {
            '(' => {
                self.depth += 1;
                State::Boundary
            }
            '[' if self.r6rs => {
                self.depth += 1;
                State::Boundary
            }
            ')' => self.close(),
            ']' if self.r6rs => self.close(),
            '"' => State::Quoted {
                close: '"',
                escaped: false,
            },
            '|' => State::Quoted {
                close: '|',
                escaped: false,
            },
            ';' => State::LineComment,
            '#' => State::Hash,
            '\'' | '`' => {
                self.prefix(Prefix::Wraps);
                State::Boundary
            }
            ',' => State::Comma,
            // Whitespace, and characters the lexer refuses outright.
            c if Lexer::is_delimiter(c) || matches!(c, '{' | '}') => State::Boundary,
            _ => State::Atom,
        }
    }

    /// A character inside a token that runs to the next delimiter.
    fn in_atom(&mut self, c: char) -> State {
        if Lexer::is_delimiter(c) {
            self.datum();
            self.token_start(c)
        } else {
            State::Atom
        }
    }

    fn close(&mut self) -> State {
        match self.depth {
            // A closer with nothing open is a mistake the parser reports.
            0 => {}
            1 => {
                self.depth = 0;
                self.datum();
            }
            _ => self.depth -= 1,
        }
        State::Boundary
    }

    fn prefix(&mut self, prefix: Prefix) {
        if self.depth == 0 {
            self.prefixes.push(prefix);
        }
    }

    /// A datum has finished. At the top level it satisfies the innermost
    /// waiting prefix, and a prefix that wraps it is a finished datum in turn.
    fn datum(&mut self) {
        if self.depth > 0 {
            return;
        }
        loop {
            match self.prefixes.pop() {
                None => {
                    self.finished = true;
                    return;
                }
                Some(Prefix::Wraps) => continue,
                Some(Prefix::Drops) => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parser, ReaderState};
    use patina_core::SharedHeap;

    /// Whether a parser's first read of `text` makes progress: returns a
    /// datum, or finds the clean end.
    fn reader_progresses(text: &str, r6rs: bool, heap: &SharedHeap) -> bool {
        Parser::resuming(text, ReaderState::START, heap.clone(), r6rs)
            .and_then(|mut parser| parser.parse_next())
            .is_ok()
    }

    fn scan(text: &str, r6rs: bool) -> DatumScan {
        let mut scan = DatumScan::new(r6rs);
        scan.feed(text);
        scan
    }

    /// Check the scan's one claim against the parser for `text`, returning
    /// whether the scan made it. Texts end in a newline, as the lines a
    /// reader feeds it do.
    fn check(text: &str, heap: &SharedHeap) -> bool {
        let brackets = text.contains(['[', ']']);
        let mut held = false;
        for r6rs in [false, true] {
            if r6rs && !brackets {
                continue;
            }
            if scan(text, r6rs).nothing_finished() {
                held = true;
                assert!(
                    !reader_progresses(text, r6rs, heap),
                    "held back {text:?}, where the reader makes progress (r6rs: {r6rs})"
                );
            }
        }
        held
    }

    /// Every combination of up to four of the pieces that open, close, hide
    /// or end a token.
    #[test]
    fn it_never_holds_back_text_a_reader_would_make_progress_on() {
        const PIECES: [&str; 24] = [
            "(", ")", "\"", "\\", "|", "#", ";", " ", "\n", "a", "'", ",", "@", "!", "/", "u8",
            "t", "rue", "0", "=", "#|", "|#", "[", "]",
        ];
        let heap = patina_core::new_shared_heap();
        let mut held = 0;
        let mut stack = vec![(String::new(), 0)];
        while let Some((text, used)) = stack.pop() {
            if check(&format!("{text}\n"), &heap) {
                held += 1;
            }
            if used < 4 {
                for piece in PIECES {
                    stack.push((format!("{text}{piece}"), used + 1));
                }
            }
        }
        assert!(held > 10_000, "only {held} texts were held back");
    }

    /// Longer texts, drawn at random from a wider set of pieces.
    #[test]
    fn longer_texts_are_never_held_back_either() {
        const PIECES: [&str; 34] = [
            "(",
            ")",
            "\"",
            "\\",
            "|",
            "#",
            ";",
            " ",
            "\n",
            "a",
            "'",
            "`",
            ",",
            ",@",
            "#;",
            "#t",
            "#f",
            "#true",
            "alse",
            "#\\",
            "x",
            "#0=",
            "#0#",
            "#(",
            "#u8(",
            "#vu8(",
            "\\x41;",
            "#!fold-case",
            "#! ",
            "#|",
            "|#",
            "1",
            "[",
            "]",
        ];
        let heap = patina_core::new_shared_heap();
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut held = 0;
        for _ in 0..100_000 {
            let pieces = 5 + next() % 8;
            let mut text = String::new();
            for _ in 0..pieces {
                text.push_str(PIECES[(next() % PIECES.len() as u64) as usize]);
            }
            text.push('\n');
            if check(&text, &heap) {
                held += 1;
            }
        }
        assert!(held > 10_000, "only {held} texts were held back");
    }

    #[test]
    fn text_in_which_a_datum_has_finished_is_not_held() {
        let heap = patina_core::new_shared_heap();
        for text in [
            "(display \"a (paren\")\n",
            "(write #\\()\n",
            "(write #\\))\n",
            "(quote |a (b|)\n",
            "(+ 1 #| ( |# 2)\n",
            "#!/usr/bin/env patina (\n(a)\n",
            "; (\n(a)\n",
            "(a)\n#;(b)\n",
            "(a))(b\n",
            // Tokens the lexer ends without a delimiter, so that what follows
            // them starts a token of its own.
            "`(,@|a (b|)\n",
            "'(#t#| ( |#)\n",
            "(#0=|(| #0#)\n",
            "(write (list #t#\\() p)\n",
            "#true|(|\n",
            // A finished form before one still open.
            "(a) (b\n",
            "(a) \"unterminated\n",
        ] {
            assert!(reader_progresses(text, false, &heap), "{text:?}");
            assert!(!scan(text, false).nothing_finished(), "{text:?}");
        }
    }

    #[test]
    fn text_in_which_nothing_can_have_finished_is_held() {
        let heap = patina_core::new_shared_heap();
        for text in [
            "(a\n",
            "#(1 2\n",
            "#u8(1\n",
            "\"abc\n",
            "|abc\n",
            "#| x\n",
            "#| #| x |#\n",
            "(a \"b)\" (c\n",
            "(define s \"one\ntwo\n",
            // A prefix still waiting for its datum, however many lines of
            // nothing follow it.
            "'\n",
            "#;\n; comment\n\n",
            "`(a ,\n",
            "#0=\n",
            "' #;a\n",
            "#!fold-case '\n",
        ] {
            assert!(!reader_progresses(text, false, &heap), "{text:?}");
            assert!(scan(text, false).nothing_finished(), "{text:?}");
        }
    }

    /// Brackets are parentheses only when the R6RS syntax is read.
    #[test]
    fn brackets_open_a_datum_only_in_r6rs_mode() {
        assert!(scan("(let ([x 1]\n", true).nothing_finished());
        assert!(!scan("[x\n", false).nothing_finished());
    }
}
