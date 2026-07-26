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
    #[error("Unexpected character: {0}")]
    UnexpectedChar(char),

    #[error("Unterminated string")]
    UnterminatedString,

    #[error("Invalid character literal")]
    InvalidCharacter,

    #[error(
        "Reserved character (R7RS): {0}. Square brackets [ ] and curly braces {{ }} are reserved for future extensions"
    )]
    ReservedCharacter(char),

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

/// A token with its source position
#[derive(Debug, Clone)]
pub struct Spanned {
    pub token: Token,
    pub line: u32,
    pub column: u32,
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
    /// Char offset just past the end of the token returned by the
    /// previous `next_token` call (0 before any token is returned)
    prev_token_end: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),
            position: 0,
            fold_case: false,
            line: 1,
            column: 1,
            prev_token_end: 0,
        }
    }

    /// Create a lexer with case-folding enabled from the start.
    ///
    /// This is used for `include-ci` which reads files in case-insensitive mode.
    /// Identifiers will be folded to lowercase, matching R7RS `#!fold-case` behavior.
    pub fn new_case_insensitive(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),
            position: 0,
            fold_case: true,
            line: 1,
            column: 1,
            prev_token_end: 0,
        }
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
        self.prev_token_end = self.position;
        self.skip_whitespace_and_comments()?;

        let line = self.line;
        let column = self.column;

        let token = self.lex_token()?;
        Ok(Spanned {
            token,
            line,
            column,
        })
    }

    /// Char offset just past the end of the token returned by the previous
    /// `next_token` call. Used to determine exactly how much input a parse
    /// consumed.
    pub fn prev_token_end(&self) -> usize {
        self.prev_token_end
    }

    fn lex_token(&mut self) -> Result<Token, LexError> {
        if self.is_at_end() {
            return Ok(Token::Eof);
        }

        let ch = self.current_char();

        match ch {
            '(' => {
                self.advance();
                Ok(Token::LeftParen)
            }
            ')' => {
                self.advance();
                Ok(Token::RightParen)
            }
            // R7RS reserves [ ] { } for future extensions
            '[' | ']' | '{' | '}' => Err(LexError::ReservedCharacter(ch)),
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
            _ if ch.is_numeric()
                || (ch == '-' || ch == '+')
                    && (self.peek_is_numeric()
                        || self.peek_is_decimal_start()
                        || self.peek_is_imaginary()
                        || self.is_special_float_literal()) =>
            {
                self.read_number()
            }
            _ if self.is_identifier_start(ch) => self.read_identifier(),
            _ => Err(LexError::UnexpectedChar(ch)),
        }
    }

    fn current_char(&self) -> char {
        self.input.get(self.position).copied().unwrap_or('\0')
    }

    fn advance(&mut self) {
        if self.position < self.input.len() {
            if self.input[self.position] == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
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
            match self.current_char() {
                ' ' | '\t' | '\n' | '\r' | '\x0C' => self.advance(),
                ';' => {
                    // Line comment: skip until end of line
                    while !self.is_at_end() && self.current_char() != '\n' {
                        self.advance();
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

    fn is_delimiter_next(&self) -> bool {
        if self.position + 1 >= self.input.len() {
            return true;
        }
        let next = self.input[self.position + 1];
        next.is_whitespace() || matches!(next, '(' | ')' | '"' | ';')
    }

    fn peek_is_numeric(&self) -> bool {
        if self.position + 1 >= self.input.len() {
            return false;
        }
        self.input[self.position + 1].is_numeric()
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
        next == '.' && after.is_numeric()
    }

    fn is_special_float_literal(&self) -> bool {
        // Check if we're at the start of +inf.0, -inf.0, +nan.0, or -nan.0 (case-insensitive)
        let remaining: String = self.input[self.position..].iter().collect();
        let lower = remaining.to_lowercase();
        lower.starts_with("+inf.0")
            || lower.starts_with("-inf.0")
            || lower.starts_with("+nan.0")
            || lower.starts_with("-nan.0")
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
                Ok(Token::VectorOpen)
            }
            'u' => {
                self.advance();
                if self.current_char() == '8' {
                    self.advance();
                    if self.current_char() == '(' {
                        self.advance();
                        Ok(Token::BytevectorOpen)
                    } else {
                        Err(LexError::UnexpectedChar(self.current_char()))
                    }
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

    /// If the upcoming characters spell `suffix` (ASCII case-insensitive),
    /// consume them. Used for the long boolean forms #true and #false, whose
    /// suffixes must be consumed with the token rather than left in the
    /// input as a stray identifier.
    fn consume_ascii_suffix(&mut self, suffix: &str) {
        let end = self.position + suffix.len();
        if end > self.input.len() {
            return;
        }
        let matches = self.input[self.position..end]
            .iter()
            .zip(suffix.chars())
            .all(|(c, s)| c.eq_ignore_ascii_case(&s));
        if matches {
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

    /// Read a reader directive like #!fold-case or #!no-fold-case
    /// These directives affect subsequent lexing but don't produce tokens themselves
    fn read_reader_directive(&mut self) -> Result<Token, LexError> {
        // Read the directive name (until whitespace or delimiter)
        let start = self.position;
        while !self.is_at_end()
            && !self.current_char().is_whitespace()
            && !matches!(self.current_char(), '(' | ')' | '"' | ';')
        {
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
        if !first.is_whitespace() && !matches!(first, '(' | ')' | '"' | ';') {
            while !self.is_at_end()
                && !self.current_char().is_whitespace()
                && !matches!(self.current_char(), '(' | ')' | '"' | ';')
            {
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

        while !self.is_at_end()
            && !self.current_char().is_whitespace()
            && !matches!(self.current_char(), '(' | ')' | '"' | ';')
        {
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
        while !self.is_at_end()
            && !self.current_char().is_whitespace()
            && !matches!(self.current_char(), '(' | ')' | '"' | ';')
        {
            self.advance();
        }

        let num_str: String = self.input[start..self.position].iter().collect();
        Ok(Token::Number(num_str))
    }

    fn is_identifier_start(&self, ch: char) -> bool {
        ch.is_alphabetic()
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
            )
    }

    fn read_identifier(&mut self) -> Result<Token, LexError> {
        let start = self.position;

        while !self.is_at_end()
            && !self.current_char().is_whitespace()
            && !matches!(self.current_char(), '(' | ')' | '"' | ';')
        {
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
    fn test_reject_reserved_characters() {
        // R7RS reserves [ ] { } for future extensions
        let mut lexer = Lexer::new("[");
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::ReservedCharacter('['))
        ));

        let mut lexer = Lexer::new("]");
        assert!(matches!(
            lexer.next_token_kind(),
            Err(LexError::ReservedCharacter(']'))
        ));

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
