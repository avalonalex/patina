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

    #[error("Reserved character (R7RS): {0}. Square brackets [ ] and curly braces {{ }} are reserved for future extensions")]
    ReservedCharacter(char),

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
        self.skip_whitespace_and_comments();

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
            '#' => self.read_hash_syntax(),
            _ if ch.is_numeric()
                || (ch == '-' || ch == '+')
                    && (self.peek_is_numeric() || self.peek_is_imaginary()) =>
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

    fn skip_whitespace_and_comments(&mut self) {
        while !self.is_at_end() {
            match self.current_char() {
                ' ' | '\t' | '\n' | '\r' => self.advance(),
                ';' => {
                    while !self.is_at_end() && self.current_char() != '\n' {
                        self.advance();
                    }
                }
                _ => break,
            }
        }
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
            s if s.len() == 1 => s.chars().next().unwrap(),
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
}
