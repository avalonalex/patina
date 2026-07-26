//! Port types for R7RS I/O operations
//!
//! Ports are Scheme objects representing input/output devices.
//! R7RS distinguishes:
//! - Input vs Output ports (direction)
//! - Textual vs Binary ports (kind)
//!
//! This module provides the infrastructure for string ports, stdio ports,
//! file ports, and (in the future) bytevector ports.

use crate::vfs::{FileSystem, ReadPort, WritePort};
use std::cell::RefCell;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::rc::Rc;

/// A Scheme port for I/O operations
#[derive(Debug, Clone)]
pub struct Port {
    /// Textual or binary
    pub kind: PortKind,
    /// Input or output
    pub direction: PortDirection,
    /// The actual port data (shared, mutable)
    pub data: Rc<RefCell<PortData>>,
    /// Text already read from the underlying source but not yet consumed
    /// (e.g. the rest of a line after `read` parses one datum from it).
    /// Textual input operations drain this before touching the source.
    /// Shared behind `Rc` so cloned ports stay in sync, like `data`.
    pushback: Rc<RefCell<String>>,
}

/// Whether a port operates on characters (textual) or bytes (binary)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    Textual,
    Binary,
}

/// Whether a port is for input or output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortDirection {
    Input,
    Output,
}

/// The underlying data for a port
pub enum PortData {
    /// String port for textual I/O (in-memory)
    String(StringPortData),
    /// Bytevector port for binary I/O (in-memory)
    Bytevector(BytevectorPortData),
    /// Standard I/O (stdin, stdout, stderr)
    Stdio(StdioKind),
    /// File port for file I/O
    File(FilePortData),
    /// Closed port - no further operations allowed
    Closed,
}

// Manual Debug impl because BufReader/BufWriter don't implement Debug well
impl std::fmt::Debug for PortData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortData::String(s) => f.debug_tuple("String").field(s).finish(),
            PortData::Bytevector(b) => f.debug_tuple("Bytevector").field(b).finish(),
            PortData::Stdio(k) => f.debug_tuple("Stdio").field(k).finish(),
            PortData::File(fp) => f.debug_struct("File").field("path", &fp.path).finish(),
            PortData::Closed => write!(f, "Closed"),
        }
    }
}

/// Data for a string-based port
#[derive(Debug)]
pub struct StringPortData {
    /// The string content
    pub content: String,
    /// Current read position (for input ports)
    pub position: usize,
}

/// Data for a bytevector-based port (binary I/O)
#[derive(Debug)]
pub struct BytevectorPortData {
    /// The bytevector content
    pub content: Vec<u8>,
    /// Current read position (for input ports)
    pub position: usize,
}

/// Data for a file-based port
pub struct FilePortData {
    /// The file path (for display/debugging)
    pub path: PathBuf,
    /// The file handle - either a buffered reader or writer
    pub handle: FileHandle,
}

/// File handle - either input or output.
/// Uses trait objects so the underlying stream can come from any `FileSystem` impl.
pub enum FileHandle {
    Input(Box<dyn ReadPort>),
    Output(Box<dyn WritePort>),
}

/// Which standard I/O stream
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdioKind {
    Stdin,
    Stdout,
    Stderr,
}

impl Port {
    fn new_port(kind: PortKind, direction: PortDirection, data: PortData) -> Rc<Port> {
        Rc::new(Port {
            kind,
            direction,
            data: Rc::new(RefCell::new(data)),
            pushback: Rc::new(RefCell::new(String::new())),
        })
    }

    /// Create a new input string port
    pub fn new_input_string(content: String) -> Rc<Port> {
        Self::new_port(
            PortKind::Textual,
            PortDirection::Input,
            PortData::String(StringPortData {
                content,
                position: 0,
            }),
        )
    }

    /// Create a new output string port
    pub fn new_output_string() -> Rc<Port> {
        Self::new_port(
            PortKind::Textual,
            PortDirection::Output,
            PortData::String(StringPortData {
                content: String::new(),
                position: 0,
            }),
        )
    }

    /// Create a new input bytevector port (binary)
    pub fn new_input_bytevector(content: Vec<u8>) -> Rc<Port> {
        Self::new_port(
            PortKind::Binary,
            PortDirection::Input,
            PortData::Bytevector(BytevectorPortData {
                content,
                position: 0,
            }),
        )
    }

    /// Create a new output bytevector port (binary)
    pub fn new_output_bytevector() -> Rc<Port> {
        Self::new_port(
            PortKind::Binary,
            PortDirection::Output,
            PortData::Bytevector(BytevectorPortData {
                content: Vec::new(),
                position: 0,
            }),
        )
    }

    /// Create a stdin port
    pub fn stdin() -> Rc<Port> {
        Self::new_port(
            PortKind::Textual,
            PortDirection::Input,
            PortData::Stdio(StdioKind::Stdin),
        )
    }

    /// Create a stdout port
    pub fn stdout() -> Rc<Port> {
        Self::new_port(
            PortKind::Textual,
            PortDirection::Output,
            PortData::Stdio(StdioKind::Stdout),
        )
    }

    /// Create a stderr port
    pub fn stderr() -> Rc<Port> {
        Self::new_port(
            PortKind::Textual,
            PortDirection::Output,
            PortData::Stdio(StdioKind::Stderr),
        )
    }

    /// Open a file for reading via the given filesystem.
    pub fn open_input_file(path: &str, fs: &dyn FileSystem) -> io::Result<Rc<Port>> {
        let reader = fs.open_read(std::path::Path::new(path))?;
        Ok(Self::new_port(
            PortKind::Textual,
            PortDirection::Input,
            PortData::File(FilePortData {
                path: PathBuf::from(path),
                handle: FileHandle::Input(reader),
            }),
        ))
    }

    /// Open a file for writing (creates or truncates) via the given filesystem.
    pub fn open_output_file(path: &str, fs: &dyn FileSystem) -> io::Result<Rc<Port>> {
        let writer = fs.open_write(std::path::Path::new(path))?;
        Ok(Self::new_port(
            PortKind::Textual,
            PortDirection::Output,
            PortData::File(FilePortData {
                path: PathBuf::from(path),
                handle: FileHandle::Output(writer),
            }),
        ))
    }

    /// Open a binary file for reading via the given filesystem.
    pub fn open_binary_input_file(path: &str, fs: &dyn FileSystem) -> io::Result<Rc<Port>> {
        let reader = fs.open_read(std::path::Path::new(path))?;
        Ok(Self::new_port(
            PortKind::Binary,
            PortDirection::Input,
            PortData::File(FilePortData {
                path: PathBuf::from(path),
                handle: FileHandle::Input(reader),
            }),
        ))
    }

    /// Open a binary file for writing (creates or truncates) via the given filesystem.
    pub fn open_binary_output_file(path: &str, fs: &dyn FileSystem) -> io::Result<Rc<Port>> {
        let writer = fs.open_write(std::path::Path::new(path))?;
        Ok(Self::new_port(
            PortKind::Binary,
            PortDirection::Output,
            PortData::File(FilePortData {
                path: PathBuf::from(path),
                handle: FileHandle::Output(writer),
            }),
        ))
    }

    /// Take the buffered pushback text, leaving the buffer empty.
    /// Used by `read` to resume from text it previously buffered.
    pub fn take_pushback(&self) -> String {
        std::mem::take(&mut *self.pushback.borrow_mut())
    }

    /// Store text that was read from the underlying source but not
    /// consumed. Textual input operations deliver it before reading
    /// from the source again.
    pub fn set_pushback(&self, text: String) {
        *self.pushback.borrow_mut() = text;
    }

    /// Check if port is open
    pub fn is_open(&self) -> bool {
        !matches!(*self.data.borrow(), PortData::Closed)
    }

    /// Check if this is an input port
    pub fn is_input(&self) -> bool {
        self.direction == PortDirection::Input
    }

    /// Check if this is an output port
    pub fn is_output(&self) -> bool {
        self.direction == PortDirection::Output
    }

    /// Check if this is a textual port
    pub fn is_textual(&self) -> bool {
        self.kind == PortKind::Textual
    }

    /// Check if this is a binary port
    pub fn is_binary(&self) -> bool {
        self.kind == PortKind::Binary
    }

    /// Close the port. For file output ports, finalizes (flushes) the writer first.
    pub fn close(&self) {
        self.pushback.borrow_mut().clear();
        let mut data = self.data.borrow_mut();
        // Finalize write ports before closing
        if let PortData::File(ref mut fp) = *data {
            if let FileHandle::Output(ref mut writer) = fp.handle {
                let _ = writer.finalize();
            }
        }
        *data = PortData::Closed;
    }

    /// Read a single character from an input port
    /// Returns None if EOF or port is closed/not readable
    pub fn read_char(&self) -> io::Result<Option<char>> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }

        // Deliver buffered pushback text before reading the source
        {
            let mut pb = self.pushback.borrow_mut();
            if let Some(ch) = pb.chars().next() {
                pb.drain(..ch.len_utf8());
                return Ok(Some(ch));
            }
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::String(ref mut s) => {
                if s.position >= s.content.len() {
                    return Ok(None); // EOF
                }
                // Get character at position (handle UTF-8)
                let remaining = &s.content[s.position..];
                if let Some(ch) = remaining.chars().next() {
                    s.position += ch.len_utf8();
                    Ok(Some(ch))
                } else {
                    Ok(None)
                }
            }
            PortData::Stdio(StdioKind::Stdin) => {
                let stdin = io::stdin();
                let mut handle = stdin.lock();
                let mut buf = [0u8; 4]; // Max UTF-8 char size
                match handle.read(&mut buf[..1]) {
                    Ok(0) => Ok(None), // EOF
                    Ok(_) => {
                        // Try to read a complete UTF-8 character
                        let first_byte = buf[0];
                        let char_len = if first_byte & 0x80 == 0 {
                            1
                        } else if first_byte & 0xE0 == 0xC0 {
                            2
                        } else if first_byte & 0xF0 == 0xE0 {
                            3
                        } else {
                            4
                        };
                        if char_len > 1 {
                            handle.read_exact(&mut buf[1..char_len])?;
                        }
                        let s = std::str::from_utf8(&buf[..char_len])
                            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                        Ok(s.chars().next())
                    }
                    Err(e) => Err(e),
                }
            }
            PortData::Stdio(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    let mut buf = [0u8; 4]; // Max UTF-8 char size
                    match reader.read(&mut buf[..1]) {
                        Ok(0) => Ok(None), // EOF
                        Ok(_) => {
                            // Determine UTF-8 character length from first byte
                            let first_byte = buf[0];
                            let char_len = if first_byte & 0x80 == 0 {
                                1
                            } else if first_byte & 0xE0 == 0xC0 {
                                2
                            } else if first_byte & 0xF0 == 0xE0 {
                                3
                            } else {
                                4
                            };
                            if char_len > 1 {
                                reader.read_exact(&mut buf[1..char_len])?;
                            }
                            let s = std::str::from_utf8(&buf[..char_len])
                                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                            Ok(s.chars().next())
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an input file port",
                    ))
                }
            }
            PortData::Bytevector(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "read-char: not a textual port",
            )),
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Read a single byte from a binary input port
    /// Returns None if EOF or port is closed/not readable
    pub fn read_u8(&self) -> io::Result<Option<u8>> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::Bytevector(ref mut b) => {
                if b.position >= b.content.len() {
                    return Ok(None); // EOF
                }
                let byte = b.content[b.position];
                b.position += 1;
                Ok(Some(byte))
            }
            PortData::Stdio(StdioKind::Stdin) => {
                let stdin = io::stdin();
                let mut handle = stdin.lock();
                let mut buf = [0u8; 1];
                match handle.read(&mut buf) {
                    Ok(0) => Ok(None), // EOF
                    Ok(_) => Ok(Some(buf[0])),
                    Err(e) => Err(e),
                }
            }
            PortData::Stdio(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    let mut buf = [0u8; 1];
                    match reader.read(&mut buf) {
                        Ok(0) => Ok(None), // EOF
                        Ok(_) => Ok(Some(buf[0])),
                        Err(e) => Err(e),
                    }
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an input file port",
                    ))
                }
            }
            PortData::String(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "read-u8: not a binary port",
            )),
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Peek at the next byte without consuming it
    pub fn peek_u8(&self) -> io::Result<Option<u8>> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::Bytevector(b) => {
                if b.position >= b.content.len() {
                    return Ok(None); // EOF
                }
                Ok(Some(b.content[b.position]))
            }
            PortData::Stdio(StdioKind::Stdin) => {
                let stdin = io::stdin();
                let mut handle = stdin.lock();
                let buf = handle.fill_buf()?;
                if buf.is_empty() {
                    return Ok(None);
                }
                Ok(Some(buf[0]))
            }
            PortData::Stdio(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    let buf = reader.fill_buf()?;
                    if buf.is_empty() {
                        return Ok(None);
                    }
                    Ok(Some(buf[0]))
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an input file port",
                    ))
                }
            }
            PortData::String(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "peek-u8: not a binary port",
            )),
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Check if a byte is ready to be read (without blocking)
    pub fn u8_ready(&self) -> io::Result<bool> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::Bytevector(b) => Ok(b.position < b.content.len()),
            PortData::Stdio(StdioKind::Stdin) => {
                // For stdin, we'd need platform-specific non-blocking check
                // For now, assume always ready (conservative)
                Ok(true)
            }
            PortData::Stdio(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    let buf = reader.fill_buf()?;
                    Ok(!buf.is_empty())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an input file port",
                    ))
                }
            }
            PortData::String(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "u8-ready?: not a binary port",
            )),
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Write a single byte to a binary output port
    pub fn write_u8(&self, byte: u8) -> io::Result<()> {
        if self.direction != PortDirection::Output {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an output port",
            ));
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::Bytevector(ref mut b) => {
                b.content.push(byte);
                Ok(())
            }
            PortData::Stdio(StdioKind::Stdout) => {
                io::stdout().write_all(&[byte])?;
                io::stdout().flush()
            }
            PortData::Stdio(StdioKind::Stderr) => {
                io::stderr().write_all(&[byte])?;
                io::stderr().flush()
            }
            PortData::Stdio(StdioKind::Stdin) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an output port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Output(ref mut writer) = fp.handle {
                    writer.write_all(&[byte])
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an output file port",
                    ))
                }
            }
            PortData::String(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "write-u8: not a binary port",
            )),
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Peek at the next character without consuming it
    pub fn peek_char(&self) -> io::Result<Option<char>> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }

        // Buffered pushback text is delivered first, so peek there first
        if let Some(ch) = self.pushback.borrow().chars().next() {
            return Ok(Some(ch));
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::String(s) => {
                if s.position >= s.content.len() {
                    return Ok(None); // EOF
                }
                let remaining = &s.content[s.position..];
                Ok(remaining.chars().next())
            }
            PortData::Stdio(StdioKind::Stdin) => {
                // For stdin, we need to use fill_buf to peek
                let stdin = io::stdin();
                let mut handle = stdin.lock();
                let buf = handle.fill_buf()?;
                if buf.is_empty() {
                    return Ok(None);
                }
                // Try to decode first UTF-8 char from buffer
                let s = std::str::from_utf8(buf)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                Ok(s.chars().next())
            }
            PortData::Stdio(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    let buf = reader.fill_buf()?;
                    if buf.is_empty() {
                        return Ok(None);
                    }
                    // Try to decode first UTF-8 char from buffer
                    let s = std::str::from_utf8(buf)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                    Ok(s.chars().next())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an input file port",
                    ))
                }
            }
            PortData::Bytevector(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "peek-char: not a textual port",
            )),
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Check if a character is ready to be read (without blocking)
    pub fn char_ready(&self) -> io::Result<bool> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }

        if !self.pushback.borrow().is_empty() {
            return Ok(true);
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::String(s) => Ok(s.position < s.content.len()),
            PortData::Stdio(StdioKind::Stdin) => {
                // For string ports, always ready if not at EOF
                // For stdin, we'd need platform-specific non-blocking check
                // For now, assume always ready (conservative)
                Ok(true)
            }
            PortData::Stdio(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    // Check if buffer has data available
                    let buf = reader.fill_buf()?;
                    Ok(!buf.is_empty())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an input file port",
                    ))
                }
            }
            PortData::Bytevector(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "char-ready?: not a textual port",
            )),
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Write a string to an output port
    pub fn write_string(&self, s: &str) -> io::Result<()> {
        if self.direction != PortDirection::Output {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an output port",
            ));
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::String(ref mut port_data) => {
                port_data.content.push_str(s);
                Ok(())
            }
            PortData::Stdio(StdioKind::Stdout) => {
                print!("{}", s);
                io::stdout().flush()
            }
            PortData::Stdio(StdioKind::Stderr) => {
                eprint!("{}", s);
                io::stderr().flush()
            }
            PortData::Stdio(StdioKind::Stdin) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an output port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Output(ref mut writer) = fp.handle {
                    writer.write_all(s.as_bytes())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an output file port",
                    ))
                }
            }
            PortData::Bytevector(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "write-string: not a textual port",
            )),
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Write a single character to an output port
    pub fn write_char(&self, c: char) -> io::Result<()> {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        self.write_string(s)
    }

    /// Flush output port buffer
    pub fn flush(&self) -> io::Result<()> {
        if self.direction != PortDirection::Output {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an output port",
            ));
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::String(_) => Ok(()),     // String ports don't need flushing
            PortData::Bytevector(_) => Ok(()), // Bytevector ports don't need flushing
            PortData::Stdio(StdioKind::Stdout) => io::stdout().flush(),
            PortData::Stdio(StdioKind::Stderr) => io::stderr().flush(),
            PortData::Stdio(StdioKind::Stdin) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an output port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Output(ref mut writer) = fp.handle {
                    writer.flush()
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an output file port",
                    ))
                }
            }
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Get the accumulated string from an output string port
    pub fn get_output_string(&self) -> io::Result<String> {
        if self.direction != PortDirection::Output {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an output port",
            ));
        }

        let data = self.data.borrow();
        match &*data {
            PortData::String(s) => Ok(s.content.clone()),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a string port",
            )),
        }
    }

    /// Get the accumulated bytevector from an output bytevector port
    pub fn get_output_bytevector(&self) -> io::Result<Vec<u8>> {
        if self.direction != PortDirection::Output {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an output port",
            ));
        }

        let data = self.data.borrow();
        match &*data {
            PortData::Bytevector(b) => Ok(b.content.clone()),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a bytevector port",
            )),
        }
    }

    /// Read up to k bytes from a binary input port
    /// Returns a Vec<u8> of up to k bytes, or None if EOF before reading any
    pub fn read_bytevector(&self, k: usize) -> io::Result<Option<Vec<u8>>> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }

        if k == 0 {
            return Ok(Some(Vec::new()));
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::Bytevector(ref mut b) => {
                if b.position >= b.content.len() {
                    return Ok(None); // EOF
                }
                let available = b.content.len() - b.position;
                let to_read = std::cmp::min(k, available);
                let result = b.content[b.position..b.position + to_read].to_vec();
                b.position += to_read;
                Ok(Some(result))
            }
            PortData::Stdio(StdioKind::Stdin) => {
                let stdin = io::stdin();
                let mut handle = stdin.lock();
                let mut buf = vec![0u8; k];
                match handle.read(&mut buf) {
                    Ok(0) => Ok(None), // EOF
                    Ok(n) => {
                        buf.truncate(n);
                        Ok(Some(buf))
                    }
                    Err(e) => Err(e),
                }
            }
            PortData::Stdio(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    let mut buf = vec![0u8; k];
                    match reader.read(&mut buf) {
                        Ok(0) => Ok(None), // EOF
                        Ok(n) => {
                            buf.truncate(n);
                            Ok(Some(buf))
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an input file port",
                    ))
                }
            }
            PortData::String(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "read-bytevector: not a binary port",
            )),
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Read into a bytevector at specified start position
    /// Returns number of bytes read, or None if EOF before reading any
    pub fn read_bytevector_into(
        &self,
        buf: &mut [u8],
        start: usize,
        end: usize,
    ) -> io::Result<Option<usize>> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }

        if start >= end || start >= buf.len() {
            return Ok(Some(0));
        }

        let actual_end = std::cmp::min(end, buf.len());
        let target = &mut buf[start..actual_end];

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::Bytevector(ref mut b) => {
                if b.position >= b.content.len() {
                    return Ok(None); // EOF
                }
                let available = b.content.len() - b.position;
                let to_read = std::cmp::min(target.len(), available);
                target[..to_read].copy_from_slice(&b.content[b.position..b.position + to_read]);
                b.position += to_read;
                Ok(Some(to_read))
            }
            PortData::Stdio(StdioKind::Stdin) => {
                let stdin = io::stdin();
                let mut handle = stdin.lock();
                match handle.read(target) {
                    Ok(0) => Ok(None), // EOF
                    Ok(n) => Ok(Some(n)),
                    Err(e) => Err(e),
                }
            }
            PortData::Stdio(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    match reader.read(target) {
                        Ok(0) => Ok(None), // EOF
                        Ok(n) => Ok(Some(n)),
                        Err(e) => Err(e),
                    }
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an input file port",
                    ))
                }
            }
            PortData::String(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "read-bytevector!: not a binary port",
            )),
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Write bytes from a bytevector to an output port
    pub fn write_bytevector(&self, bytes: &[u8]) -> io::Result<()> {
        if self.direction != PortDirection::Output {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an output port",
            ));
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::Bytevector(ref mut b) => {
                b.content.extend_from_slice(bytes);
                Ok(())
            }
            PortData::Stdio(StdioKind::Stdout) => {
                io::stdout().write_all(bytes)?;
                io::stdout().flush()
            }
            PortData::Stdio(StdioKind::Stderr) => {
                io::stderr().write_all(bytes)?;
                io::stderr().flush()
            }
            PortData::Stdio(StdioKind::Stdin) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an output port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Output(ref mut writer) = fp.handle {
                    writer.write_all(bytes)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an output file port",
                    ))
                }
            }
            PortData::String(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "write-bytevector: not a binary port",
            )),
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Read a line from an input port (including newline if present)
    pub fn read_line(&self) -> io::Result<Option<String>> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }

        // Buffered pushback text comes first. If it holds a complete line,
        // return it; otherwise it is the start of a line whose remainder
        // still has to come from the source.
        let mut prefix = self.take_pushback();
        if let Some(newline_pos) = prefix.find('\n') {
            let rest = prefix.split_off(newline_pos + 1);
            self.set_pushback(rest);
            return Ok(Some(prefix));
        }
        match self.read_line_from_source()? {
            Some(line) => {
                prefix.push_str(&line);
                Ok(Some(prefix))
            }
            None if prefix.is_empty() => Ok(None),
            None => Ok(Some(prefix)),
        }
    }

    /// Read a line from the underlying source, bypassing the pushback buffer
    fn read_line_from_source(&self) -> io::Result<Option<String>> {
        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::String(ref mut s) => {
                if s.position >= s.content.len() {
                    return Ok(None); // EOF
                }
                let remaining = &s.content[s.position..];
                if let Some(newline_pos) = remaining.find('\n') {
                    let line = remaining[..=newline_pos].to_string();
                    s.position += line.len();
                    Ok(Some(line))
                } else {
                    // No newline, return rest of content
                    let line = remaining.to_string();
                    s.position = s.content.len();
                    Ok(Some(line))
                }
            }
            PortData::Stdio(StdioKind::Stdin) => {
                let stdin = io::stdin();
                let mut line = String::new();
                match stdin.lock().read_line(&mut line) {
                    Ok(0) => Ok(None), // EOF
                    Ok(_) => Ok(Some(line)),
                    Err(e) => Err(e),
                }
            }
            PortData::Stdio(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            )),
            PortData::File(ref mut fp) => {
                if let FileHandle::Input(ref mut reader) = fp.handle {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => Ok(None), // EOF
                        Ok(_) => Ok(Some(line)),
                        Err(e) => Err(e),
                    }
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "not an input file port",
                    ))
                }
            }
            PortData::Bytevector(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "read-line: not a textual port",
            )),
            PortData::Closed => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "port is closed",
            )),
        }
    }

    /// Get the remaining content from a string input port (for `read` procedure)
    pub fn remaining_content(&self) -> io::Result<String> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }

        let data = self.data.borrow();
        match &*data {
            PortData::String(s) => Ok(s.content[s.position..].to_string()),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a string port",
            )),
        }
    }

    /// Advance the position in a string input port (after `read` consumes characters)
    pub fn advance_position(&self, chars_consumed: usize) -> io::Result<()> {
        if self.direction != PortDirection::Input {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an input port",
            ));
        }

        let mut data = self.data.borrow_mut();
        match &mut *data {
            PortData::String(ref mut s) => {
                s.position += chars_consumed;
                Ok(())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a string port",
            )),
        }
    }
}

impl std::fmt::Display for Port {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dir = match self.direction {
            PortDirection::Input => "input",
            PortDirection::Output => "output",
        };
        let kind = match self.kind {
            PortKind::Textual => "textual",
            PortKind::Binary => "binary",
        };
        let data = self.data.borrow();
        let source = match &*data {
            PortData::String(_) => "string".to_string(),
            PortData::Bytevector(_) => "bytevector".to_string(),
            PortData::Stdio(StdioKind::Stdin) => "stdin".to_string(),
            PortData::Stdio(StdioKind::Stdout) => "stdout".to_string(),
            PortData::Stdio(StdioKind::Stderr) => "stderr".to_string(),
            PortData::File(fp) => fp.path.display().to_string(),
            PortData::Closed => "closed".to_string(),
        };
        write!(f, "#<{}-{}-port:{}>", kind, dir, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_string_port() {
        let port = Port::new_input_string("hello".to_string());
        assert!(port.is_input());
        assert!(port.is_textual());
        assert!(port.is_open());

        assert_eq!(port.read_char().unwrap(), Some('h'));
        assert_eq!(port.read_char().unwrap(), Some('e'));
        assert_eq!(port.read_char().unwrap(), Some('l'));
        assert_eq!(port.read_char().unwrap(), Some('l'));
        assert_eq!(port.read_char().unwrap(), Some('o'));
        assert_eq!(port.read_char().unwrap(), None); // EOF
    }

    #[test]
    fn test_output_string_port() {
        let port = Port::new_output_string();
        assert!(port.is_output());
        assert!(port.is_textual());

        port.write_string("hello").unwrap();
        port.write_string(" world").unwrap();
        assert_eq!(port.get_output_string().unwrap(), "hello world");
    }

    #[test]
    fn test_peek_char() {
        let port = Port::new_input_string("ab".to_string());

        assert_eq!(port.peek_char().unwrap(), Some('a'));
        assert_eq!(port.peek_char().unwrap(), Some('a')); // Still 'a'
        assert_eq!(port.read_char().unwrap(), Some('a'));
        assert_eq!(port.peek_char().unwrap(), Some('b'));
        assert_eq!(port.read_char().unwrap(), Some('b'));
        assert_eq!(port.peek_char().unwrap(), None); // EOF
    }

    #[test]
    fn test_pushback_drained_by_read_char() {
        let port = Port::new_input_string("xyz".to_string());
        port.set_pushback("ab".to_string());

        // Pushback text is delivered before the underlying source
        assert_eq!(port.peek_char().unwrap(), Some('a'));
        assert_eq!(port.read_char().unwrap(), Some('a'));
        assert_eq!(port.read_char().unwrap(), Some('b'));
        assert_eq!(port.peek_char().unwrap(), Some('x'));
        assert_eq!(port.read_char().unwrap(), Some('x'));
    }

    #[test]
    fn test_pushback_read_line_complete_line() {
        let port = Port::new_input_string("source\n".to_string());
        port.set_pushback("40\nrest".to_string());

        // A full line inside the pushback is returned without touching
        // the source; the remainder stays buffered
        assert_eq!(port.read_line().unwrap(), Some("40\n".to_string()));
        assert_eq!(port.read_line().unwrap(), Some("restsource\n".to_string()));
    }

    #[test]
    fn test_pushback_read_line_partial_line() {
        // Pushback without a newline is the start of a line whose
        // remainder comes from the source
        let port = Port::new_input_string("tail\n".to_string());
        port.set_pushback("head ".to_string());
        assert_eq!(port.read_line().unwrap(), Some("head tail\n".to_string()));

        // At source EOF the pushback alone is the final line
        let port = Port::new_input_string(String::new());
        port.set_pushback("last".to_string());
        assert_eq!(port.read_line().unwrap(), Some("last".to_string()));
        assert_eq!(port.read_line().unwrap(), None);
    }

    #[test]
    fn test_pushback_char_ready_and_take() {
        let port = Port::new_input_string(String::new());
        assert!(!port.char_ready().unwrap());
        port.set_pushback("z".to_string());
        assert!(port.char_ready().unwrap());
        assert_eq!(port.take_pushback(), "z".to_string());
        assert_eq!(port.take_pushback(), String::new());
        assert!(!port.char_ready().unwrap());
    }

    #[test]
    fn test_close_clears_pushback() {
        let port = Port::new_input_string("abc".to_string());
        port.set_pushback("leftover".to_string());
        port.close();
        assert_eq!(port.take_pushback(), String::new());
        assert!(port.read_char().is_err());
    }

    #[test]
    fn test_close_port() {
        let port = Port::new_input_string("test".to_string());
        assert!(port.is_open());
        port.close();
        assert!(!port.is_open());
        assert!(port.read_char().is_err());
    }

    #[test]
    fn test_unicode_reading() {
        let port = Port::new_input_string("λ→".to_string());
        assert_eq!(port.read_char().unwrap(), Some('λ'));
        assert_eq!(port.read_char().unwrap(), Some('→'));
        assert_eq!(port.read_char().unwrap(), None);
    }

    #[test]
    fn test_file_port_write_and_read() {
        use crate::vfs::NativeFs;
        use std::fs;

        let native = NativeFs;

        // Create a temp file path
        let temp_path = std::env::temp_dir().join("patina_test_port.txt");
        let temp_path = temp_path.to_str().unwrap();

        // Write to file
        {
            let port = Port::open_output_file(temp_path, &native).unwrap();
            assert!(port.is_output());
            assert!(port.is_textual());
            port.write_string("hello\nworld").unwrap();
            port.flush().unwrap();
            // Port is dropped here, which should close the file
        }

        // Read from file
        {
            let port = Port::open_input_file(temp_path, &native).unwrap();
            assert!(port.is_input());
            assert!(port.is_textual());

            assert_eq!(port.read_char().unwrap(), Some('h'));
            assert_eq!(port.read_char().unwrap(), Some('e'));
            assert_eq!(port.read_char().unwrap(), Some('l'));
            assert_eq!(port.read_char().unwrap(), Some('l'));
            assert_eq!(port.read_char().unwrap(), Some('o'));
            assert_eq!(port.read_char().unwrap(), Some('\n'));

            // Read the rest as a line
            let line = port.read_line().unwrap();
            assert_eq!(line, Some("world".to_string()));
        }

        // Cleanup
        fs::remove_file(temp_path).unwrap();
    }

    #[test]
    fn test_file_port_display() {
        use crate::vfs::NativeFs;
        use std::fs;

        let native = NativeFs;

        let temp_path = std::env::temp_dir().join("patina_test_display.txt");
        let temp_path = temp_path.to_str().unwrap();

        // Create the file
        let port = Port::open_output_file(temp_path, &native).unwrap();
        let display = format!("{}", port);
        assert!(display.contains("output"));
        assert!(display.contains("patina_test_display.txt"));

        // Cleanup
        drop(port);
        let _ = fs::remove_file(temp_path);
    }

    #[test]
    fn test_file_port_with_memory_fs() {
        use crate::vfs::MemoryFs;

        let fs = MemoryFs::new();

        // Write to memory file
        {
            let port = Port::open_output_file("/test.txt", &fs).unwrap();
            port.write_string("hello\nworld").unwrap();
            port.close();
        }

        // Read from memory file
        {
            let port = Port::open_input_file("/test.txt", &fs).unwrap();
            assert_eq!(port.read_char().unwrap(), Some('h'));
            assert_eq!(port.read_char().unwrap(), Some('e'));
            assert_eq!(port.read_char().unwrap(), Some('l'));
            assert_eq!(port.read_char().unwrap(), Some('l'));
            assert_eq!(port.read_char().unwrap(), Some('o'));
            assert_eq!(port.read_char().unwrap(), Some('\n'));

            let line = port.read_line().unwrap();
            assert_eq!(line, Some("world".to_string()));
        }
    }

    #[test]
    fn test_input_bytevector_port() {
        let port = Port::new_input_bytevector(vec![1, 2, 3, 4, 5]);
        assert!(port.is_input());
        assert!(port.is_binary());
        assert!(port.is_open());

        assert_eq!(port.read_u8().unwrap(), Some(1));
        assert_eq!(port.read_u8().unwrap(), Some(2));
        assert_eq!(port.read_u8().unwrap(), Some(3));
        assert_eq!(port.read_u8().unwrap(), Some(4));
        assert_eq!(port.read_u8().unwrap(), Some(5));
        assert_eq!(port.read_u8().unwrap(), None); // EOF
    }

    #[test]
    fn test_output_bytevector_port() {
        let port = Port::new_output_bytevector();
        assert!(port.is_output());
        assert!(port.is_binary());

        port.write_u8(10).unwrap();
        port.write_u8(20).unwrap();
        port.write_u8(30).unwrap();
        assert_eq!(port.get_output_bytevector().unwrap(), vec![10, 20, 30]);
    }

    #[test]
    fn test_peek_u8() {
        let port = Port::new_input_bytevector(vec![42, 43]);

        assert_eq!(port.peek_u8().unwrap(), Some(42));
        assert_eq!(port.peek_u8().unwrap(), Some(42)); // Still 42
        assert_eq!(port.read_u8().unwrap(), Some(42));
        assert_eq!(port.peek_u8().unwrap(), Some(43));
        assert_eq!(port.read_u8().unwrap(), Some(43));
        assert_eq!(port.peek_u8().unwrap(), None); // EOF
    }

    #[test]
    fn test_u8_ready() {
        let port = Port::new_input_bytevector(vec![1, 2]);
        assert!(port.u8_ready().unwrap());
        assert_eq!(port.read_u8().unwrap(), Some(1));
        assert!(port.u8_ready().unwrap());
        assert_eq!(port.read_u8().unwrap(), Some(2));
        assert!(!port.u8_ready().unwrap()); // EOF
    }

    #[test]
    fn test_binary_port_display() {
        let port = Port::new_input_bytevector(vec![1, 2, 3]);
        let display = format!("{}", port);
        assert!(display.contains("binary"));
        assert!(display.contains("input"));
        assert!(display.contains("bytevector"));
    }

    #[test]
    fn test_textual_operations_on_binary_port_fail() {
        let port = Port::new_input_bytevector(vec![65, 66, 67]);

        // Textual operations should fail on binary port
        assert!(port.read_char().is_err());
        assert!(port.peek_char().is_err());
        assert!(port.read_line().is_err());
        assert!(port.char_ready().is_err());
    }

    #[test]
    fn test_binary_operations_on_textual_port_fail() {
        let port = Port::new_input_string("hello".to_string());

        // Binary operations should fail on textual port
        assert!(port.read_u8().is_err());
        assert!(port.peek_u8().is_err());
        assert!(port.u8_ready().is_err());
    }

    #[test]
    fn test_read_bytevector() {
        let port = Port::new_input_bytevector(vec![1, 2, 3, 4, 5]);

        // Read 3 bytes
        let result = port.read_bytevector(3).unwrap();
        assert_eq!(result, Some(vec![1, 2, 3]));

        // Read more than remaining - should get partial
        let result = port.read_bytevector(10).unwrap();
        assert_eq!(result, Some(vec![4, 5]));

        // EOF
        let result = port.read_bytevector(1).unwrap();
        assert_eq!(result, None);

        // Read 0 bytes returns empty vec, not EOF
        let port2 = Port::new_input_bytevector(vec![1, 2, 3]);
        let result = port2.read_bytevector(0).unwrap();
        assert_eq!(result, Some(vec![]));
    }

    #[test]
    fn test_write_bytevector() {
        let port = Port::new_output_bytevector();

        port.write_bytevector(&[10, 20, 30]).unwrap();
        port.write_bytevector(&[40, 50]).unwrap();

        assert_eq!(
            port.get_output_bytevector().unwrap(),
            vec![10, 20, 30, 40, 50]
        );
    }

    #[test]
    fn test_read_bytevector_into() {
        let port = Port::new_input_bytevector(vec![1, 2, 3, 4, 5]);
        let mut buf = vec![0u8; 10];

        // Read into buffer starting at position 2
        let result = port.read_bytevector_into(&mut buf, 2, 7).unwrap();
        assert_eq!(result, Some(5));
        assert_eq!(buf, vec![0, 0, 1, 2, 3, 4, 5, 0, 0, 0]);

        // EOF
        let result = port.read_bytevector_into(&mut buf, 0, 5).unwrap();
        assert_eq!(result, None);
    }
}
