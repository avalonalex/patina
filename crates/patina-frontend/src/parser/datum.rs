//! Token-driven datum construction. Pending containers and prefixes live on
//! the heap, not on the Rust call stack, including inside datum comments.

use super::{ParseError, Parser};
use crate::lexer::{Lexer, Token};
use patina_core::TaggedValue;

enum Tail {
    Elements,
    Needed,
    Complete(TaggedValue),
}

enum Frame {
    List {
        elements: Vec<TaggedValue>,
        tail: Tail,
        at: (u32, u32),
    },
    Vector {
        elements: Vec<TaggedValue>,
        at: (u32, u32),
    },
    Bytes(Vec<u8>, (u32, u32)),
    Prefix {
        name: &'static str,
        at: (u32, u32),
    },
    Label(usize),
    Comment {
        was_discarding: bool,
    },
    // Discarded datums need their structure checked, but allocate no Scheme
    // values and neither define nor reference labels in the surrounding datum.
    SkippedList(Tail, (u32, u32)),
    SkippedPrefix,
}

impl Frame {
    fn opening(&self) -> Option<(u32, u32)> {
        match self {
            Self::List { at, .. }
            | Self::Vector { at, .. }
            | Self::Bytes(_, at)
            | Self::SkippedList(_, at) => Some(*at),
            _ => None,
        }
    }
}

impl Parser {
    pub(super) fn parse_expr(&mut self) -> Result<TaggedValue, ParseError> {
        self.read_datum(false)
    }

    pub(super) fn skip_datum(&mut self) -> Result<(), ParseError> {
        self.read_datum(true).map(|_| ())
    }

    fn read_datum(&mut self, discarding: bool) -> Result<TaggedValue, ParseError> {
        let mut frames = Vec::new();
        self.read_with_frames(discarding, &mut frames)
            .map_err(|error| {
                error
                    .at(self.current_span())
                    .within(frames.iter().rev().find_map(Frame::opening))
            })
    }

    fn read_with_frames(
        &mut self,
        mut discarding: bool,
        frames: &mut Vec<Frame>,
    ) -> Result<TaggedValue, ParseError> {
        'tokens: loop {
            if self.current_token == Token::DatumComment {
                frames.push(Frame::Comment {
                    was_discarding: discarding,
                });
                discarding = true;
                self.advance()?;
                continue;
            }
            if self.current_token == Token::Eof {
                return Err(self.incomplete_datum());
            }
            // A dotted tail is exactly one datum, followed by comments or ).
            if self.current_token != Token::RightParen
                && matches!(
                    frames.last(),
                    Some(
                        Frame::List {
                            tail: Tail::Complete(_),
                            ..
                        } | Frame::SkippedList(Tail::Complete(_), _)
                    )
                )
            {
                return Err(ParseError::UnexpectedToken(self.current_token.clone()));
            }
            // Bytevectors accept numeric tokens only, not arbitrary datums
            // that happen to produce numbers (labels or quote prefixes).
            if let Some(Frame::Bytes(bytes, _)) = frames.last_mut()
                && self.current_token != Token::RightParen
            {
                let Token::Number(number) = &self.current_token else {
                    return Err(ParseError::InvalidSyntax(
                        "Bytevector must contain only bytes (0-255)".to_string(),
                    ));
                };
                let value = self.parse_number(number)?;
                let Some(byte) = value.as_fixnum() else {
                    return Err(ParseError::InvalidSyntax(
                        "Bytevector must contain only integer bytes (0-255)".to_string(),
                    ));
                };
                if !(0..=255).contains(&byte) {
                    return Err(ParseError::InvalidSyntax(format!(
                        "Byte value out of range (0-255): {byte}"
                    )));
                }
                bytes.push(byte as u8);
                self.advance()?;
                continue;
            }

            let at = (self.current_token_line, self.current_token_column);
            let mut value = match self.current_token.clone() {
                Token::LeftParen | Token::VectorOpen | Token::BytevectorOpen => {
                    let frame = if discarding {
                        Frame::SkippedList(Tail::Elements, at)
                    } else {
                        match self.current_token {
                            Token::LeftParen => Frame::List {
                                elements: Vec::new(),
                                tail: Tail::Elements,
                                at,
                            },
                            Token::VectorOpen => Frame::Vector {
                                elements: Vec::new(),
                                at,
                            },
                            _ => Frame::Bytes(Vec::new(), at),
                        }
                    };
                    self.advance()?;
                    if !discarding {
                        self.nesting += 1;
                    }
                    frames.push(frame);
                    continue;
                }
                Token::RightParen => {
                    if matches!(
                        frames.last(),
                        Some(
                            Frame::List {
                                tail: Tail::Needed,
                                ..
                            } | Frame::SkippedList(Tail::Needed, _)
                        )
                    ) {
                        return Err(ParseError::UnexpectedToken(Token::RightParen));
                    }
                    let value = match frames.pop() {
                        Some(Frame::List { elements, tail, at }) => {
                            let tail = match tail {
                                Tail::Elements => TaggedValue::NULL,
                                Tail::Complete(tail) => tail,
                                Tail::Needed => {
                                    return Err(ParseError::UnexpectedToken(Token::RightParen));
                                }
                            };
                            let value = self
                                .heap
                                .borrow_mut()
                                .list_from_iter_with_tail(elements, tail);
                            self.record_source(value, at.0, at.1);
                            value
                        }
                        Some(Frame::Vector { elements, at }) => {
                            let value = self.heap.borrow_mut().alloc_vector(elements);
                            self.record_source(value, at.0, at.1);
                            value
                        }
                        Some(Frame::Bytes(bytes, _)) => {
                            self.heap.borrow_mut().alloc_bytevector(bytes)
                        }
                        Some(Frame::SkippedList(Tail::Elements | Tail::Complete(_), _)) => {
                            TaggedValue::UNSPECIFIED
                        }
                        _ => return Err(ParseError::UnexpectedToken(Token::RightParen)),
                    };
                    if !discarding {
                        self.nesting -= 1;
                    }
                    value
                }
                Token::Dot => {
                    match frames.last_mut() {
                        Some(Frame::List { elements, tail, .. })
                            if !elements.is_empty() && matches!(tail, Tail::Elements) =>
                        {
                            *tail = Tail::Needed;
                        }
                        // Keep the existing skip reader's structural-only
                        // treatment of lists, vectors and bytevectors.
                        Some(Frame::SkippedList(tail, _)) if matches!(tail, Tail::Elements) => {
                            *tail = Tail::Needed;
                        }
                        _ => return Err(ParseError::UnexpectedToken(Token::Dot)),
                    }
                    self.advance()?;
                    continue;
                }
                Token::Quote | Token::Quasiquote | Token::Unquote | Token::UnquoteSplicing => {
                    frames.push(if discarding {
                        Frame::SkippedPrefix
                    } else {
                        let name = match self.current_token {
                            Token::Quote => "quote",
                            Token::Quasiquote => "quasiquote",
                            Token::Unquote => "unquote",
                            _ => "unquote-splicing",
                        };
                        Frame::Prefix { name, at }
                    });
                    self.advance()?;
                    continue;
                }
                Token::DatumLabel(label) => {
                    let span = self.current_span();
                    self.advance()?;
                    if discarding {
                        frames.push(Frame::SkippedPrefix);
                    } else {
                        if self.labels.contains_key(&label) {
                            return Err(ParseError::DuplicateLabel(label).at(span));
                        }
                        frames.push(Frame::Label(label));
                    }
                    continue;
                }
                // Even numeric conversion is unnecessary when discarding.
                _ if discarding => TaggedValue::UNSPECIFIED,
                Token::Boolean(value) => {
                    if value {
                        TaggedValue::TRUE
                    } else {
                        TaggedValue::FALSE
                    }
                }
                Token::Number(number) => match self.parse_number(&number) {
                    Ok(value) => value,
                    Err(_) if Lexer::is_peculiar_identifier(&number) => {
                        let name = Lexer::identifier_name(number, self.current_token_fold_case);
                        self.heap.borrow_mut().intern_symbol(&name)
                    }
                    Err(error) => return Err(error),
                },
                Token::Character(value) => TaggedValue::character(value),
                Token::String(value) => self.heap.borrow_mut().alloc_string(value),
                Token::Identifier(value) => self.heap.borrow_mut().intern_symbol(&value),
                Token::DatumRef(label) => {
                    if let Some(value) = self.labels.get(&label) {
                        *value
                    } else {
                        self.pending_refs.push((label, self.current_span()));
                        self.heap.borrow_mut().alloc_label_placeholder(label)
                    }
                }
                token => return Err(ParseError::UnexpectedToken(token)),
            };
            if discarding {
                self.advance()?;
            } else {
                self.advance_past_datum()?;
            }

            // Hand a finished datum to its parent. Prefixes finish another
            // datum; a comment drops one and still leaves its parent waiting.
            loop {
                match frames.last_mut() {
                    None => return Ok(value),
                    Some(Frame::List { elements, tail, .. }) => {
                        if matches!(tail, Tail::Needed) {
                            *tail = Tail::Complete(value);
                        } else {
                            elements.push(value);
                        }
                        continue 'tokens;
                    }
                    Some(Frame::Vector { elements, .. }) => {
                        elements.push(value);
                        continue 'tokens;
                    }
                    Some(Frame::SkippedList(tail, _)) => {
                        if matches!(tail, Tail::Needed) {
                            *tail = Tail::Complete(value);
                        }
                        continue 'tokens;
                    }
                    _ => {}
                }
                match frames.pop().expect("a pending prefix") {
                    Frame::Prefix { name, at } => {
                        let symbol = self.heap.borrow_mut().intern_symbol(name);
                        value = self.make_list(vec![symbol, value]);
                        self.record_source(value, at.0, at.1);
                    }
                    Frame::Label(label) => {
                        self.labels.insert(label, value);
                    }
                    Frame::SkippedPrefix => {}
                    Frame::Comment { was_discarding } => {
                        discarding = was_discarding;
                        continue 'tokens;
                    }
                    _ => unreachable!("containers receive values above"),
                }
            }
        }
    }
}
