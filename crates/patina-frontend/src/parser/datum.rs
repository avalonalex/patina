//! Token-driven datum construction. Pending containers and prefixes live on
//! the heap, not on the Rust call stack, including inside datum comments.

use super::{Labels, ParseError, Parser};
use crate::lexer::{Lexer, Token};
use patina_core::{Cx, Value};

enum Tail<'gc> {
    Elements,
    Needed,
    Complete(Value<'gc>),
}

// Specialize the same grammar for syntax and ordinary Scheme data. The latter
// does no provenance work. Syntax records where each container, identifier
// and string begins; it no longer records each element's span (#643).
trait SourceMode {
    const TRACK: bool;
}

struct DatumMode;

impl SourceMode for DatumMode {
    const TRACK: bool = false;
}

struct ProgramMode;

impl SourceMode for ProgramMode {
    const TRACK: bool = true;
}

enum Frame<'gc> {
    List {
        elements: Vec<Value<'gc>>,
        tail: Tail<'gc>,
        at: (u32, u32),
    },
    Vector {
        elements: Vec<Value<'gc>>,
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
    SkippedList(Tail<'gc>, (u32, u32)),
    SkippedPrefix,
}

impl Frame<'_> {
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
    pub(super) fn parse_expr<'gc>(
        &mut self,
        cx: &mut Cx<'gc>,
        labels: &mut Labels<'gc>,
    ) -> Result<Value<'gc>, ParseError> {
        self.read_datum(cx, labels, false)
    }

    /// Read past a datum without building it: a discarded datum neither
    /// defines nor references the labels around it.
    pub(super) fn skip_datum(&mut self, cx: &mut Cx<'_>) -> Result<(), ParseError> {
        self.read_datum(cx, &mut Labels::default(), true)
            .map(|_| ())
    }

    fn read_datum<'gc>(
        &mut self,
        cx: &mut Cx<'gc>,
        labels: &mut Labels<'gc>,
        discarding: bool,
    ) -> Result<Value<'gc>, ParseError> {
        if self.source_map.is_some() {
            self.read_datum_in::<ProgramMode>(cx, labels, discarding)
        } else {
            self.read_datum_in::<DatumMode>(cx, labels, discarding)
        }
    }

    fn read_datum_in<'gc, M: SourceMode>(
        &mut self,
        cx: &mut Cx<'gc>,
        labels: &mut Labels<'gc>,
        discarding: bool,
    ) -> Result<Value<'gc>, ParseError> {
        let mut frames = Vec::new();
        self.read_with_frames::<M>(cx, labels, discarding, &mut frames)
            .map_err(|error| {
                error
                    .at(self.current_span())
                    .within(frames.iter().rev().find_map(Frame::opening))
            })
    }

    fn read_with_frames<'gc, M: SourceMode>(
        &mut self,
        cx: &mut Cx<'gc>,
        labels: &mut Labels<'gc>,
        mut discarding: bool,
        frames: &mut Vec<Frame<'gc>>,
    ) -> Result<Value<'gc>, ParseError> {
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
                let value = self.parse_number(cx, number)?;
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
            let mut location = if M::TRACK {
                self.source_location(at.0, at.1)
            } else {
                None
            };
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
                                Tail::Elements => Value::NULL,
                                Tail::Complete(tail) => tail,
                                Tail::Needed => {
                                    return Err(ParseError::UnexpectedToken(Token::RightParen));
                                }
                            };
                            if M::TRACK {
                                location = self.source_location(at.0, at.1);
                            }
                            cx.list_from_iter_with_tail(elements, tail)
                        }
                        Some(Frame::Vector { elements, at }) => {
                            if M::TRACK {
                                location = self.source_location(at.0, at.1);
                            }
                            cx.alloc_vector(elements)
                        }
                        Some(Frame::Bytes(bytes, at)) => {
                            if M::TRACK {
                                location = self.source_location(at.0, at.1);
                            }
                            cx.alloc_bytevector(bytes)
                        }
                        Some(Frame::SkippedList(Tail::Elements | Tail::Complete(_), _)) => {
                            Value::UNSPECIFIED
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
                        if labels.defined.contains_key(&label) {
                            return Err(ParseError::DuplicateLabel(label).at(span));
                        }
                        frames.push(Frame::Label(label));
                    }
                    continue;
                }
                // Even numeric conversion is unnecessary when discarding.
                _ if discarding => Value::UNSPECIFIED,
                Token::Boolean(value) => Value::boolean(value),
                Token::Number(number) => match self.parse_number(cx, &number) {
                    Ok(value) => value,
                    Err(_) if Lexer::is_peculiar_identifier(&number) => {
                        let name = Lexer::identifier_name(number, self.current_token_fold_case);
                        self.identifier(cx, &name)
                    }
                    Err(error) => return Err(error),
                },
                Token::Character(value) => Value::character(value),
                Token::String(value) => cx.alloc_string(value),
                Token::Identifier(value) => self.identifier(cx, &value),
                Token::DatumRef(label) => {
                    if let Some(value) = labels.defined.get(&label) {
                        *value
                    } else {
                        labels.pending.push((label, self.current_span()));
                        cx.alloc_label_placeholder(label)
                    }
                }
                token => return Err(ParseError::UnexpectedToken(token)),
            };
            if M::TRACK
                && !discarding
                && let Some(loc) = &location
            {
                // A label reference is another edge to existing syntax. Keep
                // the identifier/container's original spelling location.
                if !matches!(self.current_token, Token::DatumRef(_)) || cx.source(value).is_none() {
                    cx.record_source(value, loc.clone());
                }
            }
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
                        let symbol = cx.intern_symbol(name);
                        value = cx.list_from_iter([symbol, value]);
                        if M::TRACK
                            && let Some(loc) = &mut location
                        {
                            loc.line = at.0;
                            loc.column = at.1;
                            loc.length = loc.span.as_ref().map(|span| {
                                if span.end_line == at.0 {
                                    span.end_column.saturating_sub(at.1).max(1)
                                } else {
                                    1
                                }
                            });
                            cx.record_source(value, loc.clone());
                        }
                    }
                    Frame::Label(label) => {
                        labels.defined.insert(label, value);
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
