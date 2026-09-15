//! A quick test for text that certainly cannot yet hold only complete data.
//!
//! A reader fed a line at a time asks after every line whether what it has is
//! finished. The parser answers exactly, but only by reading all of it, so
//! asking after each line of one long form takes time proportional to the
//! square of the form's length. [`DatumScan`] answers a weaker question in
//! constant time per character: it follows the nesting depth and the lexical
//! states that hide delimiters (strings, `|symbols|`, comments, character
//! literals), and says when the text certainly is not finished. When it cannot
//! say that, the parser decides.
//!
//! It must never call finished, well-formed text unfinished, since that would
//! hold back a form whose input has all arrived; a test checks it against the
//! parser. It may call text with a mistake in it unfinished, which only delays
//! the report of a mistake inside a form that is still open.

use crate::dialect;
use crate::lexer::Lexer;

/// Where a scan stands, as far as delimiters are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Between tokens.
    Boundary,
    /// Inside an identifier, number or other atom, which runs to a delimiter.
    Atom,
    /// After `#` between tokens.
    Hash,
    /// After `#\`: the next character belongs to the literal, whatever it is.
    CharFirst,
    /// After `#!`: a shebang line if `/` or a space follows.
    Directive,
    String,
    StringEscape,
    Bar,
    BarEscape,
    LineComment,
    /// Inside `#| |#` comments nested this deep.
    Block(usize),
    /// Inside a block comment, after a `#` that may open another.
    BlockHash(usize),
    /// Inside a block comment, after a `|` that may close one.
    BlockBar(usize),
}

/// Scan state for text arriving in pieces; see the module documentation.
#[derive(Debug, Clone)]
pub struct DatumScan {
    state: State,
    /// Openers not yet closed.
    depth: usize,
    /// A closer arrived with nothing open. That is a mistake the parser reports
    /// at once, so the scan stops claiming anything.
    unbalanced: bool,
    /// Whether brackets are read as parentheses, as the lexer decides.
    brackets: bool,
}

impl Default for DatumScan {
    fn default() -> Self {
        Self::new()
    }
}

impl DatumScan {
    pub fn new() -> Self {
        DatumScan {
            state: State::Boundary,
            depth: 0,
            unbalanced: false,
            brackets: dialect::allow_r6rs(),
        }
    }

    /// Scan more of the text.
    pub fn feed(&mut self, text: &str) {
        for c in text.chars() {
            self.state = self.step(c);
        }
    }

    /// Whether the text scanned so far certainly does not hold only complete
    /// data. `false` means the parser has to decide.
    pub fn certainly_unfinished(&self) -> bool {
        !self.unbalanced
            && (self.depth > 0
                || matches!(
                    self.state,
                    State::String
                        | State::StringEscape
                        | State::Bar
                        | State::BarEscape
                        | State::Block(_)
                        | State::BlockHash(_)
                        | State::BlockBar(_)
                ))
    }

    fn step(&mut self, c: char) -> State {
        use State::*;
        match self.state {
            Boundary | Atom => self.code(c),
            Hash => match c {
                '|' => Block(1),
                '\\' => CharFirst,
                '!' => Directive,
                '(' => {
                    self.depth += 1;
                    Boundary
                }
                ';' => Boundary,
                c if Lexer::is_delimiter(c) => self.code(c),
                _ => Atom,
            },
            CharFirst if Lexer::is_delimiter(c) => Boundary,
            CharFirst => Atom,
            Directive if c == '/' || c == ' ' => LineComment,
            Directive if Lexer::is_delimiter(c) => self.code(c),
            Directive => Atom,
            String => match c {
                '\\' => StringEscape,
                '"' => Boundary,
                _ => String,
            },
            StringEscape => String,
            Bar => match c {
                '\\' => BarEscape,
                '|' => Boundary,
                _ => Bar,
            },
            BarEscape => Bar,
            LineComment if c == '\n' || c == '\r' => Boundary,
            LineComment => LineComment,
            Block(depth) => Self::block(depth, c),
            BlockHash(depth) if c == '|' => Block(depth + 1),
            BlockHash(depth) => Self::block(depth, c),
            BlockBar(1) if c == '#' => Boundary,
            BlockBar(depth) if c == '#' => Block(depth - 1),
            BlockBar(depth) => Self::block(depth, c),
        }
    }

    /// A character between tokens or inside an atom, mirroring `lex_token`.
    fn code(&mut self, c: char) -> State {
        let in_atom = self.state == State::Atom;
        match c {
            '(' => {
                self.depth += 1;
                State::Boundary
            }
            ')' => {
                self.close();
                State::Boundary
            }
            '[' if self.brackets => {
                self.depth += 1;
                State::Boundary
            }
            ']' if self.brackets => {
                self.close();
                State::Boundary
            }
            '"' => State::String,
            ';' => State::LineComment,
            c if Lexer::is_delimiter(c) => State::Boundary,
            // `#` and `|` open something only where a token starts: inside an
            // atom they are part of it, as `a|b` and `a#|b` are to the lexer.
            _ if in_atom => State::Atom,
            '#' => State::Hash,
            '|' => State::Bar,
            '\'' | '`' | ',' => State::Boundary,
            _ => State::Atom,
        }
    }

    /// A character inside a block comment, mirroring `skip_block_comment`.
    fn block(depth: usize, c: char) -> State {
        match c {
            '#' => State::BlockHash(depth),
            '|' => State::BlockBar(depth),
            _ => State::Block(depth),
        }
    }

    fn close(&mut self) {
        match self.depth.checked_sub(1) {
            Some(depth) => self.depth = depth,
            None => self.unbalanced = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    /// What the reader decides: finished and well formed, unfinished, or a
    /// mistake that is not about running out of input.
    #[derive(Debug, PartialEq)]
    enum Reader {
        Finished,
        Unfinished,
        Mistake,
    }

    fn reader(text: &str) -> Reader {
        let judge = |e: crate::ParseError| {
            if e.is_incomplete() {
                Reader::Unfinished
            } else {
                Reader::Mistake
            }
        };
        match Parser::new(text) {
            Err(e) => judge(e),
            Ok(mut parser) => loop {
                match parser.parse_next() {
                    Ok(Some(_)) => continue,
                    Ok(None) => return Reader::Finished,
                    Err(e) => return judge(e),
                }
            },
        }
    }

    fn scan(text: &str) -> DatumScan {
        let mut scan = DatumScan::new();
        scan.feed(text);
        scan
    }

    /// The one claim the scan makes must hold: it never calls finished,
    /// well-formed text unfinished. Checked against the reader over every
    /// combination of up to four of the pieces that open, close or hide a
    /// delimiter.
    #[test]
    fn it_never_calls_finished_text_unfinished() {
        let pieces = [
            "(", ")", "\"", "\\", "|", "#", ";", "\n", " ", "a", "'", "!", "/", "u8", "x", "#|",
            "|#",
        ];
        let mut finished = 0;
        let mut stack = vec![(String::new(), 0)];
        while let Some((text, used)) = stack.pop() {
            if reader(&text) == Reader::Finished {
                assert!(!scan(&text).certainly_unfinished(), "{text:?}");
                finished += 1;
            }
            if used < 4 {
                for piece in pieces {
                    stack.push((format!("{text}{piece}"), used + 1));
                }
            }
        }
        assert!(
            finished > 1000,
            "only {finished} finished texts were checked"
        );
    }

    #[test]
    fn delimiters_hidden_in_strings_symbols_comments_and_characters_do_not_count() {
        for text in [
            "(display \"a (paren\")",
            "(write #\\()",
            "(write #\\))",
            "(write #\\[)",
            "(quote |a (b|)",
            "(+ 1 #| ( |# 2)",
            "(+ 1 #| #| ( |# ( |# 2)",
            "#!/usr/bin/env patina (\n(a)",
            "; (\n(a)",
            "(a)\n#;(b)\n",
        ] {
            assert_eq!(reader(text), Reader::Finished, "{text:?}");
            assert!(!scan(text).certainly_unfinished(), "{text:?}");
        }
    }

    #[test]
    fn open_forms_strings_symbols_and_comments_are_certainly_unfinished() {
        for text in [
            "(a",
            "#(1 2",
            "#u8(1",
            "\"abc",
            "\"abc\\",
            "|abc",
            "#| x",
            "#| #| x |#",
            "(a \"b)\" (c",
            "(define s \"one\ntwo",
        ] {
            assert_eq!(reader(text), Reader::Unfinished, "{text:?}");
            assert!(scan(text).certainly_unfinished(), "{text:?}");
        }
    }

    /// `#` and `|` open a comment or a symbol only where a token starts.
    #[test]
    fn a_bar_or_hash_inside_an_atom_opens_nothing() {
        assert!(!scan("a|b").certainly_unfinished());
        assert!(!scan("(a#|b)").certainly_unfinished());
    }

    /// A closer with nothing open is a mistake the parser reports at once, so
    /// the scan never holds that text back, whatever follows.
    #[test]
    fn a_stray_closer_ends_every_claim() {
        assert!(!scan("(a))(b").certainly_unfinished());
    }

    #[test]
    fn scanning_a_line_at_a_time_matches_scanning_at_once() {
        let text = "(define s \"one\ntwo\")\n#| a\n#| b |#\nc |#\n(write #\\()\n'|x\ny|\n";
        let mut incremental = DatumScan::new();
        let mut so_far = String::new();
        for line in text.split_inclusive('\n') {
            incremental.feed(line);
            so_far.push_str(line);
            assert_eq!(
                incremental.certainly_unfinished(),
                scan(&so_far).certainly_unfinished(),
                "{so_far:?}"
            );
        }
    }
}
