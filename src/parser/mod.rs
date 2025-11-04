use crate::lexer::{LexError, Lexer, Token};
use crate::value::Value;
use std::rc::Rc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Lexer error: {0}")]
    LexError(#[from] LexError),

    #[error("Unexpected EOF")]
    UnexpectedEof,

    #[error("Unexpected token: {0:?}")]
    UnexpectedToken(Token),

    #[error("Invalid syntax: {0}")]
    InvalidSyntax(String),
}

pub struct Parser {
    lexer: Lexer,
    current_token: Token,
}

impl Parser {
    pub fn new(input: &str) -> Result<Self, ParseError> {
        let mut lexer = Lexer::new(input);
        let current_token = lexer.next_token()?;
        Ok(Parser {
            lexer,
            current_token,
        })
    }

    fn advance(&mut self) -> Result<(), ParseError> {
        self.current_token = self.lexer.next_token()?;
        Ok(())
    }

    pub fn parse(&mut self) -> Result<Value, ParseError> {
        self.parse_expr()
    }

    fn parse_expr(&mut self) -> Result<Value, ParseError> {
        match &self.current_token.clone() {
            Token::Boolean(b) => {
                let val = Value::Boolean(*b);
                self.advance()?;
                Ok(val)
            }
            Token::Number(s) => {
                let val = self.parse_number(s)?;
                self.advance()?;
                Ok(val)
            }
            Token::Character(c) => {
                let val = Value::Character(*c);
                self.advance()?;
                Ok(val)
            }
            Token::String(s) => {
                let val = Value::String(Rc::new(s.clone()));
                self.advance()?;
                Ok(val)
            }
            Token::Identifier(s) => {
                let val = Value::Symbol(Rc::from(s.as_str()));
                self.advance()?;
                Ok(val)
            }
            Token::Quote => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                Ok(self.make_list(vec![Value::Symbol(Rc::from("quote")), quoted]))
            }
            Token::Quasiquote => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                Ok(self.make_list(vec![Value::Symbol(Rc::from("quasiquote")), quoted]))
            }
            Token::Unquote => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                Ok(self.make_list(vec![Value::Symbol(Rc::from("unquote")), quoted]))
            }
            Token::UnquoteSplicing => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                Ok(self.make_list(vec![Value::Symbol(Rc::from("unquote-splicing")), quoted]))
            }
            Token::LeftParen => self.parse_list(),
            Token::VectorOpen => self.parse_vector(),
            Token::BytevectorOpen => self.parse_bytevector(),
            Token::Eof => Err(ParseError::UnexpectedEof),
            token => Err(ParseError::UnexpectedToken(token.clone())),
        }
    }

    fn parse_list(&mut self) -> Result<Value, ParseError> {
        self.advance()?; // consume (

        let mut elements = Vec::new();
        let mut dotted_tail = None;

        while self.current_token != Token::RightParen {
            if self.current_token == Token::Eof {
                return Err(ParseError::UnexpectedEof);
            }

            if self.current_token == Token::Dot {
                self.advance()?;
                dotted_tail = Some(self.parse_expr()?);
                break;
            }

            elements.push(self.parse_expr()?);
        }

        if self.current_token != Token::RightParen {
            return Err(ParseError::UnexpectedToken(self.current_token.clone()));
        }
        self.advance()?; // consume )

        if let Some(tail) = dotted_tail {
            Ok(self.make_dotted_list(elements, tail))
        } else {
            Ok(self.make_list(elements))
        }
    }

    fn parse_vector(&mut self) -> Result<Value, ParseError> {
        self.advance()?; // consume #(

        let mut elements = Vec::new();

        while self.current_token != Token::RightParen {
            if self.current_token == Token::Eof {
                return Err(ParseError::UnexpectedEof);
            }
            elements.push(self.parse_expr()?);
        }

        self.advance()?; // consume )
        Ok(Value::Vector(Rc::new(elements)))
    }

    fn parse_bytevector(&mut self) -> Result<Value, ParseError> {
        self.advance()?; // consume #u8(

        let mut bytes = Vec::new();

        while self.current_token != Token::RightParen {
            if self.current_token == Token::Eof {
                return Err(ParseError::UnexpectedEof);
            }

            if let Token::Number(s) = &self.current_token.clone() {
                let byte: u8 = s
                    .parse()
                    .map_err(|_| ParseError::InvalidSyntax("Invalid byte value".to_string()))?;
                bytes.push(byte);
                self.advance()?;
            } else {
                return Err(ParseError::InvalidSyntax(
                    "Bytevector must contain only bytes (0-255)".to_string(),
                ));
            }
        }

        self.advance()?; // consume )
        Ok(Value::Bytevector(Rc::new(bytes)))
    }

    fn parse_number(&self, s: &str) -> Result<Value, ParseError> {
        // Simple number parsing - extend this for full R7RS numeric tower
        if let Ok(n) = s.parse::<i64>() {
            Ok(Value::Integer(n))
        } else if let Ok(f) = s.parse::<f64>() {
            Ok(Value::Real(f))
        } else {
            Err(ParseError::InvalidSyntax(format!("Invalid number: {}", s)))
        }
    }

    fn make_list(&self, elements: Vec<Value>) -> Value {
        elements
            .into_iter()
            .rev()
            .fold(Value::Null, |acc, elem| Value::Pair(Rc::new((elem, acc))))
    }

    fn make_dotted_list(&self, elements: Vec<Value>, tail: Value) -> Value {
        elements
            .into_iter()
            .rev()
            .fold(tail, |acc, elem| Value::Pair(Rc::new((elem, acc))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_atom() {
        let mut parser = Parser::new("42").unwrap();
        let result = parser.parse().unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_parse_list() {
        let mut parser = Parser::new("(+ 1 2)").unwrap();
        let result = parser.parse().unwrap();
        // Should be a list with three elements
        assert!(matches!(result, Value::Pair(_)));
    }
}
