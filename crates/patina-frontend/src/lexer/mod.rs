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
    Quote,           // '
    Quasiquote,      // `
    Unquote,         // ,
    UnquoteSplicing, // ,@
    Dot,             // .

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

    #[error("Unterminated block comment")]
    UnterminatedBlockComment,

    #[allow(dead_code)]
    #[error("Invalid number format: {0}")]
    InvalidNumber(String),
}

pub struct Lexer {
    input: Vec<char>,
    position: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),
            position: 0,
        }
    }

    pub fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_whitespace_and_comments()?;

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
            '"' => self.read_string(),
            '|' => self.read_vertical_bar_identifier(),
            '#' => self.read_hash_syntax(),
            _ if ch.is_numeric()
                || (ch == '-' || ch == '+')
                    && (self.peek_is_numeric()
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
        self.position += 1;
    }

    fn is_at_end(&self) -> bool {
        self.position >= self.input.len()
    }

    fn skip_whitespace_and_comments(&mut self) -> Result<(), LexError> {
        while !self.is_at_end() {
            match self.current_char() {
                ' ' | '\t' | '\n' | '\r' => self.advance(),
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

    fn is_special_float_literal(&self) -> bool {
        // Check if we're at the start of +inf.0, -inf.0, or +nan.0
        let remaining: String = self.input[self.position..].iter().collect();
        remaining.starts_with("+inf.0")
            || remaining.starts_with("-inf.0")
            || remaining.starts_with("+nan.0")
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
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    '\\' => '\\',
                    '"' => '"',
                    c => c, // For simplicity, just accept the character
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
                Ok(Token::Boolean(true))
            }
            'f' | 'F' => {
                self.advance();
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
            _ => Err(LexError::UnexpectedChar(self.current_char())),
        }
    }

    fn read_character(&mut self) -> Result<Token, LexError> {
        self.advance(); // consume \

        if self.is_at_end() {
            return Err(LexError::InvalidCharacter);
        }

        let start = self.position;
        while !self.is_at_end()
            && !self.current_char().is_whitespace()
            && !matches!(self.current_char(), '(' | ')')
        {
            self.advance();
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
            && !matches!(self.current_char(), '(' | ')')
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
            && !matches!(self.current_char(), '(' | ')')
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
        Ok(Token::Identifier(ident))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let mut lexer = Lexer::new("(+ 1 2)");
        assert_eq!(lexer.next_token().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("+".to_string())
        );
        assert_eq!(lexer.next_token().unwrap(), Token::Number("1".to_string()));
        assert_eq!(lexer.next_token().unwrap(), Token::Number("2".to_string()));
        assert_eq!(lexer.next_token().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_reject_reserved_characters() {
        // R7RS reserves [ ] { } for future extensions
        let mut lexer = Lexer::new("[");
        assert!(matches!(
            lexer.next_token(),
            Err(LexError::ReservedCharacter('['))
        ));

        let mut lexer = Lexer::new("]");
        assert!(matches!(
            lexer.next_token(),
            Err(LexError::ReservedCharacter(']'))
        ));

        let mut lexer = Lexer::new("{");
        assert!(matches!(
            lexer.next_token(),
            Err(LexError::ReservedCharacter('{'))
        ));

        let mut lexer = Lexer::new("}");
        assert!(matches!(
            lexer.next_token(),
            Err(LexError::ReservedCharacter('}'))
        ));
    }

    #[test]
    fn test_vertical_bar_identifier_basic() {
        let mut lexer = Lexer::new("|hello world|");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("hello world".to_string())
        );
    }

    #[test]
    fn test_vertical_bar_identifier_empty() {
        // R7RS: || is a valid identifier
        let mut lexer = Lexer::new("||");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("".to_string())
        );
    }

    #[test]
    fn test_vertical_bar_identifier_with_special_chars() {
        let mut lexer = Lexer::new("|(hello world!)|");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("(hello world!)".to_string())
        );
    }

    #[test]
    fn test_vertical_bar_identifier_with_escapes() {
        // Test \| escape
        let mut lexer = Lexer::new("|foo\\|bar|");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("foo|bar".to_string())
        );

        // Test \t escape
        let mut lexer = Lexer::new("|\\t\\t|");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("\t\t".to_string())
        );

        // Test \n escape
        let mut lexer = Lexer::new("|foo\\nbar|");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("foo\nbar".to_string())
        );

        // Test \a (alarm) escape
        let mut lexer = Lexer::new("|\\a|");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("\u{0007}".to_string())
        );

        // Test \b (backspace) escape
        let mut lexer = Lexer::new("|\\b|");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("\u{0008}".to_string())
        );
    }

    #[test]
    fn test_vertical_bar_identifier_with_hex_escape() {
        // R7RS example: |H\x65;llo| == Hello
        let mut lexer = Lexer::new("|H\\x65;llo|");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("Hello".to_string())
        );

        // R7RS example: |\x3BB;| == λ
        let mut lexer = Lexer::new("|\\x3BB;|");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("λ".to_string())
        );

        // R7RS example: |\x9;\x9;| == two tabs
        let mut lexer = Lexer::new("|\\x9;\\x9;|");
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("\t\t".to_string())
        );
    }

    #[test]
    fn test_vertical_bar_identifier_unterminated() {
        let mut lexer = Lexer::new("|hello");
        assert!(matches!(
            lexer.next_token(),
            Err(LexError::UnterminatedVerticalBarIdentifier)
        ));
    }

    #[test]
    fn test_vertical_bar_identifier_invalid_escape() {
        let mut lexer = Lexer::new("|foo\\q|");
        assert!(matches!(
            lexer.next_token(),
            Err(LexError::InvalidEscapeInIdentifier(_))
        ));
    }

    #[test]
    fn test_vertical_bar_identifier_in_expression() {
        let mut lexer = Lexer::new("(|foo bar| |hello world|)");
        assert_eq!(lexer.next_token().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("foo bar".to_string())
        );
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("hello world".to_string())
        );
        assert_eq!(lexer.next_token().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_block_comment_basic() {
        let mut lexer = Lexer::new("#| this is a comment |# 42");
        assert_eq!(lexer.next_token().unwrap(), Token::Number("42".to_string()));
    }

    #[test]
    fn test_block_comment_nested() {
        let mut lexer = Lexer::new("#| outer #| inner |# outer |# 42");
        assert_eq!(lexer.next_token().unwrap(), Token::Number("42".to_string()));
    }

    #[test]
    fn test_block_comment_with_code() {
        let mut lexer = Lexer::new("(+ #| comment |# 1 2)");
        assert_eq!(lexer.next_token().unwrap(), Token::LeftParen);
        assert_eq!(
            lexer.next_token().unwrap(),
            Token::Identifier("+".to_string())
        );
        assert_eq!(lexer.next_token().unwrap(), Token::Number("1".to_string()));
        assert_eq!(lexer.next_token().unwrap(), Token::Number("2".to_string()));
        assert_eq!(lexer.next_token().unwrap(), Token::RightParen);
    }

    #[test]
    fn test_block_comment_multiline() {
        let mut lexer = Lexer::new("#|\nline 1\nline 2\n|# 42");
        assert_eq!(lexer.next_token().unwrap(), Token::Number("42".to_string()));
    }

    #[test]
    fn test_block_comment_unterminated() {
        let mut lexer = Lexer::new("#| this is unterminated");
        assert!(matches!(
            lexer.next_token(),
            Err(LexError::UnterminatedBlockComment)
        ));
    }

    #[test]
    fn test_block_comment_unterminated_nested() {
        let mut lexer = Lexer::new("#| outer #| inner |# outer");
        assert!(matches!(
            lexer.next_token(),
            Err(LexError::UnterminatedBlockComment)
        ));
    }
}
