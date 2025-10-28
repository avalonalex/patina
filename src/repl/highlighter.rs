use nu_ansi_term::{Color, Style};
use rustyline::highlight::Highlighter;
use std::borrow::Cow;

pub struct SchemeHighlighter {
    keyword_style: Style,
    builtin_style: Style,
    string_style: Style,
    comment_style: Style,
    number_style: Style,
    boolean_style: Style,
    paren_style: Style,
}

impl SchemeHighlighter {
    pub fn new() -> Self {
        SchemeHighlighter {
            keyword_style: Color::Cyan.bold(),
            builtin_style: Color::Yellow.normal(),
            string_style: Color::Green.normal(),
            comment_style: Color::LightGray.dimmed(),
            number_style: Color::Magenta.normal(),
            boolean_style: Color::Blue.bold(),
            paren_style: Color::White.dimmed(),
        }
    }

    fn is_keyword(word: &str) -> bool {
        matches!(
            word,
            "define"
                | "lambda"
                | "if"
                | "cond"
                | "case"
                | "else"
                | "when"
                | "unless"
                | "let"
                | "let*"
                | "letrec"
                | "let-values"
                | "let*-values"
                | "do"
                | "delay"
                | "force"
                | "begin"
                | "set!"
                | "quote"
                | "quasiquote"
                | "unquote"
                | "unquote-splicing"
                | "and"
                | "or"
                | "define-syntax"
                | "let-syntax"
                | "letrec-syntax"
                | "syntax-rules"
                | "define-record-type"
                | "guard"
                | "raise"
                | "raise-continuity-error"
                | "call-with-current-continuation"
                | "call/cc"
                | "dynamic-wind"
                | "define-library"
                | "import"
                | "export"
                | "include"
        )
    }

    fn is_builtin(word: &str) -> bool {
        matches!(
            word,
            // Arithmetic
            "+" | "-" | "*" | "/" | "quotient" | "remainder" | "modulo"
                | "=" | "<" | ">" | "<=" | ">=" | "abs" | "floor" | "ceiling"
                | "truncate" | "round" | "sqrt" | "expt" | "sin" | "cos" | "tan"
                | "exact?" | "inexact?" | "exact" | "inexact"
                | "number?" | "complex?" | "real?" | "rational?" | "integer?"
                | "even?" | "odd?" | "positive?" | "negative?" | "zero?"
                // Lists
                | "cons" | "car" | "cdr" | "caar" | "cadr" | "cdar" | "cddr"
                | "list" | "list?" | "length" | "append" | "reverse" | "null?"
                | "pair?" | "list-ref" | "list-tail" | "map" | "for-each"
                | "filter" | "fold" | "foldl" | "foldr"
                // Strings
                | "string?" | "string-length" | "string-ref" | "substring"
                | "string-append" | "string->list" | "list->string"
                | "string=?" | "string<?" | "string>?"
                // Characters
                | "char?" | "char=?" | "char<?" | "char>?" | "char->integer"
                | "integer->char"
                // Vectors
                | "vector" | "vector?" | "make-vector" | "vector-length"
                | "vector-ref" | "vector-set!" | "vector->list" | "list->vector"
                // I/O
                | "display" | "write" | "read" | "newline" | "open-input-file"
                | "open-output-file" | "close-input-port" | "close-output-port"
                // Type predicates
                | "boolean?" | "symbol?" | "procedure?" | "eq?" | "eqv?" | "equal?"
                // Other
                | "apply" | "eval" | "load" | "error" | "not"
        )
    }
}

impl Highlighter for SchemeHighlighter {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        let mut result = String::new();
        let mut current_token = String::new();
        let mut in_string = false;
        let mut in_comment = false;
        let mut escape_next = false;

        for ch in line.chars() {
            if in_comment {
                result.push_str(&self.comment_style.paint(ch.to_string()).to_string());
                continue;
            }

            if escape_next {
                result.push_str(&self.string_style.paint(ch.to_string()).to_string());
                escape_next = false;
                continue;
            }

            if in_string {
                if ch == '\\' {
                    escape_next = true;
                    result.push_str(&self.string_style.paint(ch.to_string()).to_string());
                } else if ch == '"' {
                    result.push_str(&self.string_style.paint(ch.to_string()).to_string());
                    in_string = false;
                } else {
                    result.push_str(&self.string_style.paint(ch.to_string()).to_string());
                }
                continue;
            }

            match ch {
                '"' => {
                    self.flush_token(&mut result, &current_token);
                    current_token.clear();
                    result.push_str(&self.string_style.paint("\"").to_string());
                    in_string = true;
                }
                ';' => {
                    self.flush_token(&mut result, &current_token);
                    current_token.clear();
                    in_comment = true;
                    result.push_str(&self.comment_style.paint(";").to_string());
                }
                '(' | ')' | '[' | ']' => {
                    self.flush_token(&mut result, &current_token);
                    current_token.clear();
                    result.push_str(&self.paren_style.paint(ch.to_string()).to_string());
                }
                ' ' | '\t' | '\n' => {
                    self.flush_token(&mut result, &current_token);
                    current_token.clear();
                    result.push(ch);
                }
                _ => {
                    current_token.push(ch);
                }
            }
        }

        self.flush_token(&mut result, &current_token);
        Cow::Owned(result)
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _forced: bool) -> bool {
        true
    }
}

impl SchemeHighlighter {
    fn flush_token(&self, result: &mut String, token: &str) {
        if token.is_empty() {
            return;
        }

        if token.starts_with("#t") || token.starts_with("#f") {
            result.push_str(&self.boolean_style.paint(token).to_string());
        } else if token.starts_with('#') {
            // Characters, vectors, bytevectors
            result.push_str(&self.number_style.paint(token).to_string());
        } else if Self::is_keyword(token) {
            result.push_str(&self.keyword_style.paint(token).to_string());
        } else if Self::is_builtin(token) {
            result.push_str(&self.builtin_style.paint(token).to_string());
        } else if token.chars().next().map_or(false, |c| c.is_numeric())
            || (token.starts_with('-') || token.starts_with('+'))
                && token.len() > 1
                && token.chars().nth(1).map_or(false, |c| c.is_numeric())
        {
            result.push_str(&self.number_style.paint(token).to_string());
        } else {
            // Regular identifiers
            result.push_str(token);
        }
    }
}
